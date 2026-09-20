//! The driver client: what the server asks a Cua Driver for, expressed in
//! protocol types and executed through a [`DriverTransport`].

use std::sync::Arc;

use cua_protocol::{
    CheckedCuaAdapter, CuaAction, CuaActionResult, CuaRequestEnvelope, CuaResponse,
    HealthReportArgs, HealthReportResult, MachineDescriptor, MachineId, MachineLocation,
    ProtocolVersion, RequestId, ValidationError,
    driver_mcp::{
        DriverCallFailure, descriptor_from_health, error_response, response_for, tool_call,
    },
};

use super::transport::DriverTransport;

/// Ask the driver for its health report and build the descriptor the
/// server-local target advertises from it.
pub async fn describe_machine(
    transport: &dyn DriverTransport,
    machine_id: MachineId,
) -> anyhow::Result<MachineDescriptor> {
    let report = health_report(transport, &machine_id).await?;
    Ok(descriptor_from_health(
        machine_id,
        MachineLocation::ServerLocal,
        &report,
    ))
}

/// The driver's unfiltered health report, decoded through the same envelope
/// checks as any other action result.
pub async fn health_report(
    transport: &dyn DriverTransport,
    machine_id: &MachineId,
) -> anyhow::Result<HealthReportResult> {
    let request = CuaRequestEnvelope {
        version: ProtocolVersion::V1,
        request_id: RequestId::try_from("health").expect("static id"),
        machine_id: machine_id.clone(),
        action: CuaAction::HealthReport(HealthReportArgs {
            include: vec![],
            skip: vec![],
        }),
    };
    let call = tool_call(&request.action)?;
    let outcome = transport.call_tool(call.name, call.arguments).await;
    match response_for(&request, outcome).response {
        CuaResponse::Success { result } => match *result {
            CuaActionResult::HealthReport(report) => Ok(report),
            other => anyhow::bail!("health_report answered with {:?}", other.kind()),
        },
        CuaResponse::Error { error } => anyhow::bail!(
            "health_report failed ({:?}): {}",
            error.code,
            error.message.as_str()
        ),
    }
}

/// A checked adapter that executes every authorized request against the
/// driver: action to tool call, payload to correlated envelope.
///
/// The adapter validates and authorizes before this callback runs and checks
/// the correlation after it, so the callback only encodes, calls and decodes;
/// a driver failure becomes a typed runtime error, never a panic.
pub fn checked_adapter(
    transport: Arc<dyn DriverTransport>,
    descriptor: MachineDescriptor,
) -> Result<CheckedCuaAdapter, ValidationError> {
    CheckedCuaAdapter::new(descriptor, move |request| {
        let transport = transport.clone();
        Box::pin(async move {
            let call = match tool_call(&request.action) {
                Ok(call) => call,
                Err(error) => {
                    return error_response(
                        &request,
                        &DriverCallFailure::Malformed(error.to_string()),
                    );
                }
            };
            let outcome = transport.call_tool(call.name, call.arguments).await;
            response_for(&request, outcome)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::cua::transport::fake::FakeTransport;
    use cua_protocol::{
        Capability, CuaAction, CuaActionKind, CuaRequestEnvelope, CuaResponse, DriverVersion,
        EmptyArgs, ListWindowsArgs, MachineHealth, MachineLocation, Permission, PermissionState,
        Platform, ProtocolVersion, RequestId, RuntimeErrorCode,
        driver_mcp::{DriverCallFailure, HEALTH_REPORT_TOOL},
    };
    use serde_json::{Value, json};

    const HEALTHY: &str = r#"{
        "schema_version":"1","platform":"darwin","driver_version":"0.28.2","overall":"ok",
        "checks":[
            {"name":"binary_version","status":"pass","message":"cua-driver 0.28.2"},
            {"name":"platform_supported","status":"pass","message":"macOS 27.0 (arm64)","data":{"architecture":"arm64","os_version":"27.0"}},
            {"name":"session_active","status":"pass","message":"MCP session is active."},
            {"name":"bundle_identity","status":"pass","message":"Bundle is com.trycua.driver.","data":{"bundle_identifier":"com.trycua.driver","executable_path":"/Applications/CuaDriver.app/Contents/MacOS/cua-driver","identity_source":"current_process"}},
            {"name":"tcc_accessibility","status":"pass","message":"Accessibility is granted.","data":{"bundle_identifier":"com.trycua.driver"}},
            {"name":"tcc_screen_recording","status":"pass","message":"Screen Recording is granted.","data":{"bundle_identifier":"com.trycua.driver"}},
            {"name":"ax_capability","status":"pass","message":"AX is trusted and reachable."},
            {"name":"screen_capture_capability","status":"skip","message":"Direct capture was not probed."}
        ]
    }"#;

    const ACCESSIBILITY_DENIED: &str = r#"{
        "schema_version":"1","platform":"darwin","driver_version":"0.28.2","overall":"degraded",
        "checks":[
            {"name":"binary_version","status":"pass","message":"cua-driver 0.28.2"},
            {"name":"platform_supported","status":"pass","message":"macOS 27.0 (arm64)","data":{"architecture":"arm64","os_version":"27.0"}},
            {"name":"session_active","status":"pass","message":"MCP session is active."},
            {"name":"bundle_identity","status":"pass","message":"Bundle is com.trycua.driver.","data":{"bundle_identifier":"com.trycua.driver","executable_path":"/Applications/CuaDriver.app/Contents/MacOS/cua-driver","identity_source":"current_process"}},
            {"name":"tcc_accessibility","status":"fail","message":"Accessibility is not granted.","hint":"Run cua-driver permissions grant.","data":{"bundle_identifier":"com.trycua.driver"}},
            {"name":"tcc_screen_recording","status":"pass","message":"Screen Recording is granted.","data":{"bundle_identifier":"com.trycua.driver"}},
            {"name":"ax_capability","status":"fail","message":"AX is not trusted.","hint":"Grant Accessibility."},
            {"name":"screen_capture_capability","status":"skip","message":"Direct capture was not probed."}
        ]
    }"#;

    fn payload(json: &str) -> Value {
        serde_json::from_str(json).unwrap()
    }

    fn id() -> MachineId {
        MachineId::try_from("server-local:studio").unwrap()
    }

    fn request(action: CuaAction) -> CuaRequestEnvelope {
        CuaRequestEnvelope {
            version: ProtocolVersion::V1,
            request_id: RequestId::try_from("req-1").unwrap(),
            machine_id: id(),
            action,
        }
    }

    fn descriptor(capabilities: Vec<Capability>) -> MachineDescriptor {
        MachineDescriptor {
            machine_id: id(),
            location: MachineLocation::ServerLocal,
            platform: Platform::Macos,
            driver_version: DriverVersion::try_from("0.28.2").unwrap(),
            health: MachineHealth::Healthy,
            permissions: PermissionState {
                accessibility: Permission::Granted,
                screen_capture: Permission::Granted,
            },
            capabilities,
        }
    }

    #[tokio::test]
    async fn describe_machine_asks_for_the_health_report_and_builds_the_descriptor() {
        let transport = FakeTransport::answering([Ok(payload(HEALTHY))]);
        let machine = describe_machine(&transport, id()).await.unwrap();
        assert_eq!(
            transport.calls(),
            vec![(HEALTH_REPORT_TOOL.to_owned(), serde_json::Map::new())],
            "one unfiltered health_report call"
        );
        assert_eq!(machine.machine_id, id());
        assert_eq!(machine.location, MachineLocation::ServerLocal);
        assert_eq!(machine.platform, Platform::Macos);
        assert_eq!(machine.health, MachineHealth::Healthy);
        assert_eq!(machine.permissions.accessibility, Permission::Granted);
        assert!(machine.capabilities.contains(&Capability::Pointer));
        machine.validate().unwrap();
    }

    #[tokio::test]
    async fn describe_machine_reports_a_denied_permission_as_degraded() {
        let transport = FakeTransport::answering([Ok(payload(ACCESSIBILITY_DENIED))]);
        let machine = describe_machine(&transport, id()).await.unwrap();
        assert_eq!(machine.health, MachineHealth::Degraded);
        assert_eq!(machine.permissions.accessibility, Permission::Denied);
        assert!(!machine.capabilities.contains(&Capability::Pointer));
        assert!(!machine.capabilities.contains(&Capability::Keyboard));
        assert!(machine.capabilities.contains(&Capability::Health));
    }

    #[tokio::test]
    async fn describe_machine_fails_when_the_driver_cannot_answer() {
        let unreachable =
            FakeTransport::answering([Err(DriverCallFailure::Transport("broken pipe".into()))]);
        let error = describe_machine(&unreachable, id()).await.unwrap_err();
        assert!(error.to_string().contains("broken pipe"), "{error}");

        let nonsense = FakeTransport::answering([Ok(json!({"overall": "fine"}))]);
        assert!(describe_machine(&nonsense, id()).await.is_err());

        let report = FakeTransport::answering([Ok(payload(HEALTHY))]);
        assert_eq!(
            health_report(&report, &id())
                .await
                .unwrap()
                .driver_version
                .as_str(),
            "0.28.2"
        );
    }

    #[tokio::test]
    async fn the_adapter_encodes_actions_and_decodes_payloads_inside_the_checked_boundary() {
        let transport = Arc::new(FakeTransport::answering([
            Ok(json!({"apps": []})),
            Err(DriverCallFailure::Tool {
                code: Some("window_id_not_found".into()),
                message: "window_id 1 is not a live window".into(),
            }),
            Ok(json!({"windows": [], "unexpected": true})),
        ]));
        let adapter = checked_adapter(
            transport.clone(),
            descriptor(vec![Capability::AppDiscovery, Capability::WindowDiscovery]),
        )
        .unwrap();

        let apps = request(CuaAction::ListApps(EmptyArgs {}));
        let response = adapter.execute(&apps).await.unwrap();
        assert_eq!(response.request_id, apps.request_id);
        assert_eq!(response.action, CuaActionKind::ListApps);
        assert!(matches!(response.response, CuaResponse::Success { .. }));

        let windows = request(CuaAction::ListWindows(ListWindowsArgs {
            pid: None,
            on_screen_only: false,
        }));
        let refused = adapter.execute(&windows).await.unwrap();
        match refused.response {
            CuaResponse::Error { error } => {
                assert_eq!(error.code, RuntimeErrorCode::TargetUnavailable);
            }
            other => panic!("driver errors surface as typed runtime errors, got {other:?}"),
        }

        let undecodable = adapter.execute(&windows).await.unwrap();
        match undecodable.response {
            CuaResponse::Error { error } => {
                assert_eq!(error.code, RuntimeErrorCode::DriverFailure);
            }
            other => panic!("unreadable payloads surface as driver failures, got {other:?}"),
        }

        let calls = transport.calls();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0], ("list_apps".to_owned(), serde_json::Map::new()));
        assert_eq!(calls[1].0, "list_windows");
        assert_eq!(calls[1].1["on_screen_only"], false);
    }

    #[tokio::test]
    async fn a_call_the_transport_cancelled_surfaces_as_a_retryable_timeout() {
        let transport = Arc::new(FakeTransport::answering([Err(DriverCallFailure::Timeout(
            "list_apps did not answer within 30s".into(),
        ))]));
        let adapter = checked_adapter(
            transport.clone(),
            descriptor(vec![Capability::AppDiscovery]),
        )
        .unwrap();

        let apps = request(CuaAction::ListApps(EmptyArgs {}));
        let response = adapter.execute(&apps).await.unwrap();
        assert_eq!(response.request_id, apps.request_id);
        match response.response {
            CuaResponse::Error { error } => {
                assert_eq!(error.code, RuntimeErrorCode::Timeout);
                assert!(error.retryable, "a slow driver is worth another try");
                assert!(error.message.as_str().contains("30s"));
            }
            other => panic!("a cancelled call is a typed timeout, got {other:?}"),
        }
        assert_eq!(transport.calls().len(), 1);
    }

    /// Round-trips through a real `cua-driver mcp` child. Run it by hand with
    /// `--ignored` on a host that has the driver installed; it only reads.
    #[tokio::test]
    #[ignore = "needs an installed cua-driver binary"]
    async fn live_driver_reports_health_and_lists_sessions_through_the_checked_boundary() {
        use crate::services::cua::{
            discovery,
            transport::{DriverTimeouts, StdioDriverTransport},
        };

        let Some(driver) = discovery::discover(None).unwrap() else {
            eprintln!("no cua-driver on this host; nothing to check");
            return;
        };
        let transport = Arc::new(
            StdioDriverTransport::spawn_with(&driver, DriverTimeouts::default())
                .await
                .unwrap(),
        );
        let machine = describe_machine(transport.as_ref(), id()).await.unwrap();
        eprintln!("live descriptor: {machine:?}");
        machine.validate().unwrap();
        assert_ne!(machine.health, MachineHealth::Unavailable);

        let adapter = checked_adapter(transport.clone(), machine).unwrap();
        let response = adapter
            .execute(&request(CuaAction::ListSessions(
                cua_protocol::ListSessionsArgs {
                    cursor: None,
                    limit: None,
                },
            )))
            .await
            .unwrap();
        match response.response {
            CuaResponse::Success { result } => match *result {
                cua_protocol::CuaActionResult::ListSessions(list) => {
                    eprintln!("live sessions: {}", list.sessions.len());
                }
                other => panic!("unexpected result {other:?}"),
            },
            CuaResponse::Error { error } => panic!("live list_sessions failed: {error:?}"),
        }
        drop(adapter);
        transport.close();
        // The child is killed asynchronously once the connection drops.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    #[tokio::test]
    async fn the_adapter_refuses_unadvertised_actions_before_touching_the_driver() {
        let transport = Arc::new(FakeTransport::answering([Ok(json!({"apps": []}))]));
        let adapter = checked_adapter(
            transport.clone(),
            descriptor(vec![Capability::AppDiscovery]),
        )
        .unwrap();

        let windows = request(CuaAction::ListWindows(ListWindowsArgs {
            pid: None,
            on_screen_only: false,
        }));
        assert!(adapter.execute(&windows).await.is_err());
        assert!(transport.calls().is_empty(), "the driver never saw it");

        let other_machine = CuaRequestEnvelope {
            machine_id: MachineId::try_from("elsewhere").unwrap(),
            ..request(CuaAction::ListApps(EmptyArgs {}))
        };
        assert!(adapter.execute(&other_machine).await.is_err());
        assert!(transport.calls().is_empty());
    }
}
