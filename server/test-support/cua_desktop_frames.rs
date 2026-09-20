//! Router-level tests for desktop Cua targets over the machine WebSocket (#17).
//!
//! Included from `app/router.rs`. The router is served on a loopback port and
//! a fake desktop connects to it the way the app does: authenticated with the
//! API token, registering first, then answering frames. Nothing here runs a
//! driver; the fake answers with canned protocol envelopes, so what these
//! tests prove is the wire: how a desktop becomes a typed target, how a
//! request and its answer travel and correlate, what a reconnect replaces,
//! what a disconnect drops, and that a desktop without a descriptor keeps the
//! legacy toolcalls exactly as before.

use super::*;
use crate::{
    config::Config,
    services::{
        machine_registry::AgentToolCall,
        tool::Tool as _,
        tools::computer::{ListMachinesArgs, ListMachinesTool},
    },
};
use axum::{
    body::Body,
    http::{Method, Request, StatusCode, header},
};
use cua_protocol::{
    ActionDelivery, ActionEffect, ActionOutcome, ActionRoute, AppsResult, Capability, CaptureScope,
    CheckedCuaAdapter, ClickAction, ClickActionResult, ClickArgs, CuaAction, CuaActionKind,
    CuaActionResult, CuaRegistrationEnvelope, CuaRequestEnvelope, CuaResponse, CuaResponseEnvelope,
    DeliveryMode, DriverVersion, ElementAddress, EmptyArgs, MachineDescriptor, MachineHealth,
    MachineId, MachineLocation, MouseButton, Permission, PermissionState, Platform,
    ProtocolVersion, RequestId, RuntimeErrorCode, SessionLabel, StartSessionArgs,
    StartSessionResult, WindowPoint, WindowTarget,
};
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message, client::IntoClientRequest},
};
use tower::ServiceExt;

const TOKEN: &str = "issue-17-machine-token";
const MAX_BODY: usize = 64 * 1024 * 1024;
const STUDIO: &str = "4f3c1c2e-9b5e-4d2b-8f0a-1c2d3e4f5a6b";
/// `[cua].call_timeout_secs` for these tests: long enough for a fake that
/// answers, short enough that a test proving "fails now, not at the
/// deadline" is meaningful.
const CALL_TIMEOUT_SECS: u64 = 4;
const WAIT: Duration = Duration::from_secs(5);

struct Harness {
    _workspace: tempfile::TempDir,
    state: AppState,
    addr: SocketAddr,
    server: tokio::task::JoinHandle<()>,
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.server.abort();
    }
}

/// The router on a loopback port, over a temporary workspace. Building the
/// state never starts a driver (#16), so the only Cua targets are the ones
/// these tests register.
async fn harness() -> Harness {
    let workspace = tempfile::tempdir().unwrap();
    let mut config = Config {
        auth_token: TOKEN.into(),
        ..Default::default()
    };
    config.cua.call_timeout_secs = CALL_TIMEOUT_SECS;
    let state = AppState::new_in(config, workspace.path().to_owned()).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = build_router(state.clone(), None);
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    Harness {
        _workspace: workspace,
        state,
        addr,
        server,
    }
}

impl Harness {
    async fn json(&self, method: Method, uri: &str) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .body(Body::empty())
            .unwrap();
        let response = build_router(self.state.clone(), None)
            .oneshot(request)
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), MAX_BODY)
            .await
            .unwrap();
        let value = serde_json::from_slice(&bytes).unwrap_or_else(|error| {
            panic!(
                "{uri}: expected JSON body, got {error}: {}",
                String::from_utf8_lossy(&bytes)
            )
        });
        (status, value)
    }

    /// `GET /machines`, the one row for `machine_id`.
    async fn known_row(&self, machine_id: &str) -> Value {
        let (status, body) = self
            .json(Method::GET, "/api/instances/companion/machines")
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let rows = body["machines"].as_array().unwrap();
        let matching: Vec<&Value> = rows
            .iter()
            .filter(|row| row["machine_id"] == machine_id)
            .collect();
        assert_eq!(matching.len(), 1, "one row for {machine_id}: {rows:?}");
        matching[0].clone()
    }

    /// What the companion's `list_machines` tool prints.
    async fn list_machines(&self) -> String {
        ListMachinesTool::new(self.state.machine_registry.clone())
            .call(ListMachinesArgs {})
            .await
            .unwrap()
    }

    async fn target(&self, machine_id: &str) -> Arc<CheckedCuaAdapter> {
        self.state
            .machine_registry
            .cua()
            .select(Some(&MachineId::try_from(machine_id).unwrap()))
            .await
            .unwrap()
    }

    /// Execute one typed request through the registry, off this task so the
    /// test can play the desktop meanwhile.
    fn execute(
        &self,
        adapter: Arc<CheckedCuaAdapter>,
        request: CuaRequestEnvelope,
    ) -> tokio::task::JoinHandle<Result<CuaResponseEnvelope, cua_protocol::ValidationError>> {
        tokio::spawn(async move { adapter.execute(&request).await })
    }
}

/// A desktop app as the machine WebSocket sees it.
struct FakeDesktop {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl FakeDesktop {
    /// Open the socket with the API token and send `register`; returns the
    /// desktop and the server's first frame (the ack or a refusal).
    async fn connect(addr: SocketAddr, register: Value) -> (Self, Value) {
        let mut request = format!("ws://{addr}/api/agents/ws/machine")
            .into_client_request()
            .unwrap();
        request.headers_mut().insert(
            header::AUTHORIZATION,
            format!("Bearer {TOKEN}").parse().unwrap(),
        );
        let (ws, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        let mut desktop = Self { ws };
        desktop.send(register).await;
        let first = desktop.next_frame().await;
        (desktop, first)
    }

    /// Connect and require the registration ack.
    async fn registered(addr: SocketAddr, register: Value) -> Self {
        let machine_id = register_machine_id(&register).to_owned();
        let (desktop, ack) = Self::connect(addr, register).await;
        assert_eq!(ack["type"], "registered", "{ack}");
        assert_eq!(ack["machine_id"], machine_id);
        desktop
    }

    async fn send(&mut self, frame: Value) {
        self.ws
            .send(Message::text(frame.to_string()))
            .await
            .unwrap();
    }

    /// The next text frame from the server; pings are answered by the
    /// client library and skipped here.
    async fn next_frame(&mut self) -> Value {
        loop {
            let message = tokio::time::timeout(WAIT, self.ws.next())
                .await
                .expect("a frame within 5s")
                .expect("the socket is open")
                .unwrap();
            match message {
                Message::Text(text) => return serde_json::from_str(&text).unwrap(),
                Message::Close(frame) => panic!("the server closed the socket: {frame:?}"),
                _ => continue,
            }
        }
    }

    /// Whether the server closed the socket (or the connection dropped).
    async fn closed(&mut self) -> bool {
        loop {
            match tokio::time::timeout(WAIT, self.ws.next()).await {
                Ok(None) | Ok(Some(Err(_))) => return true,
                Ok(Some(Ok(Message::Close(_)))) => return true,
                Ok(Some(Ok(_))) => continue,
                Err(_) => return false,
            }
        }
    }

    /// Close the socket the way an app quitting does.
    async fn close(mut self) {
        let _ = self.ws.close(None).await;
    }

    /// Answer the next `cua_request` frame: decode it as the desktop would,
    /// hand it to `answer`, send the envelope back. Returns the request.
    async fn answer_next(
        &mut self,
        answer: impl FnOnce(&CuaRequestEnvelope) -> Value,
    ) -> CuaRequestEnvelope {
        let frame = self.next_frame().await;
        assert_eq!(frame["type"], "cua_request", "{frame}");
        let request = CuaRequestEnvelope::from_json(&frame["request"].to_string()).unwrap();
        let response = answer(&request);
        self.send(json!({"type": "cua_response", "response": response}))
            .await;
        request
    }
}

fn register_machine_id(register: &Value) -> &str {
    register["machine_id"].as_str().unwrap()
}

fn descriptor(machine_id: &str, location: MachineLocation) -> MachineDescriptor {
    MachineDescriptor {
        machine_id: MachineId::try_from(machine_id).unwrap(),
        location,
        platform: Platform::Macos,
        driver_version: DriverVersion::try_from("0.28.2").unwrap(),
        health: MachineHealth::Healthy,
        permissions: PermissionState {
            accessibility: Permission::Granted,
            screen_capture: Permission::Granted,
        },
        capabilities: vec![
            Capability::AppDiscovery,
            Capability::Pointer,
            Capability::SessionLifecycle,
            Capability::Health,
        ],
    }
}

fn cua_field(machine_id: &str, location: MachineLocation) -> Value {
    serde_json::to_value(CuaRegistrationEnvelope {
        version: ProtocolVersion::V1,
        machine: descriptor(machine_id, location),
    })
    .unwrap()
}

/// The register message a desktop sends, with or without a descriptor.
fn register(machine_id: &str, cua: Option<Value>) -> Value {
    let mut frame = json!({
        "type": "register",
        "machine_id": machine_id,
        "os": "macos",
        "hostname": "studio",
        "screen_width": 1440,
        "screen_height": 900,
        "permissions": {"accessibility": "granted", "screen_capture": "granted"},
        "capabilities": ["screenshot", "left_click", "bash"],
    });
    if let Some(cua) = cua {
        frame["cua"] = cua;
    }
    frame
}

fn target() -> WindowTarget {
    WindowTarget {
        pid: 42,
        window_id: 99,
    }
}

fn click() -> CuaAction {
    CuaAction::Click(ClickArgs {
        target: target(),
        session: None,
        delivery_mode: DeliveryMode::Background,
        address: ElementAddress::Point(WindowPoint { x: 1.0, y: 2.0 }),
        button: MouseButton::Left,
        action: ClickAction::Press,
        modifiers: vec![],
        count: None,
    })
}

fn clicked() -> CuaActionResult {
    CuaActionResult::Click(ClickActionResult {
        target: target(),
        session: None,
        address: ElementAddress::Point(WindowPoint { x: 1.0, y: 2.0 }),
        button: MouseButton::Left,
        action: ClickAction::Press,
        count: None,
        outcome: ActionOutcome {
            effect: ActionEffect::Confirmed,
            route: ActionRoute::Accessibility,
            delivery: Some(ActionDelivery {
                requested: DeliveryMode::Background,
                delivered_count: Some(1),
            }),
            evidence: vec![],
            escalation: None,
        },
    })
}

fn request(machine_id: &str, request_id: &str, action: CuaAction) -> CuaRequestEnvelope {
    CuaRequestEnvelope {
        version: ProtocolVersion::V1,
        request_id: RequestId::try_from(request_id).unwrap(),
        machine_id: MachineId::try_from(machine_id).unwrap(),
        action,
    }
}

/// A success envelope for `request` carrying `result`; `action` follows the
/// result, so a mismatched result makes a self-consistent, wrong answer.
fn success(request: &CuaRequestEnvelope, result: CuaActionResult) -> Value {
    serde_json::to_value(CuaResponseEnvelope {
        version: request.version,
        request_id: request.request_id.clone(),
        machine_id: request.machine_id.clone(),
        action: result.kind(),
        response: CuaResponse::Success {
            result: Box::new(result),
        },
    })
    .unwrap()
}

fn error_of(envelope: CuaResponseEnvelope) -> cua_protocol::CuaRuntimeError {
    match envelope.response {
        CuaResponse::Error { error } => error,
        CuaResponse::Success { .. } => panic!("expected an error envelope"),
    }
}

#[tokio::test]
async fn a_desktop_with_a_cua_descriptor_is_a_typed_target_and_a_known_machine() {
    let h = harness().await;
    let (desktop, ack) = FakeDesktop::connect(
        h.addr,
        register(STUDIO, Some(cua_field(STUDIO, MachineLocation::Desktop))),
    )
    .await;
    assert_eq!(ack["type"], "registered");
    assert_eq!(ack["machine_id"], STUDIO);
    assert_eq!(
        ack["cua"], true,
        "the ack says the typed target is registered: {ack}"
    );

    // The registry holds the descriptor as advertised.
    assert_eq!(
        h.state.machine_registry.cua().list().await,
        vec![descriptor(STUDIO, MachineLocation::Desktop)]
    );

    // The companion's tool lists it as a desktop Cua target.
    let listed: Vec<Value> = serde_json::from_str(&h.list_machines().await).unwrap();
    let typed = listed
        .iter()
        .find(|entry| entry["machine_id"] == STUDIO && entry.get("health").is_some())
        .unwrap_or_else(|| panic!("no typed entry for {STUDIO}: {listed:?}"));
    assert_eq!(typed["location"], "desktop");
    assert_eq!(typed["os"], "macos");
    assert_eq!(typed["driver_version"], "0.28.2");
    assert_eq!(typed["health"], "healthy");
    assert_eq!(typed["permissions"]["accessibility"], "granted");
    assert_eq!(typed["permissions"]["screen_capture"], "granted");
    assert_eq!(
        typed["capabilities"],
        json!(["app_discovery", "pointer", "session_lifecycle", "health"])
    );

    // The known row is the desktop's record with the driver's live fields.
    let row = h.known_row(STUDIO).await;
    assert_eq!(row["location"], "desktop");
    assert_eq!(row["online"], true);
    assert_eq!(row["health"], "healthy");
    assert_eq!(row["hostname"], "studio");
    assert_eq!(row["permissions"]["accessibility"], "granted");
    assert_eq!(
        row["capabilities"],
        json!(["screenshot", "left_click", "bash"])
    );
    assert_eq!(row["driver_version"], "0.28.2");
    assert_eq!(row["cua_health"], "healthy");

    desktop.close().await;
}

#[tokio::test]
async fn a_click_travels_as_one_cua_request_frame_and_its_answer_resolves_the_call() {
    let h = harness().await;
    let mut desktop = FakeDesktop::registered(
        h.addr,
        register(STUDIO, Some(cua_field(STUDIO, MachineLocation::Desktop))),
    )
    .await;

    let adapter = h.target(STUDIO).await;
    let sent = request(STUDIO, "req-1", click());
    let call = h.execute(adapter, sent.clone());

    // The desktop reads exactly the envelope the registry executed.
    let mut answered = None;
    let received = desktop
        .answer_next(|request| {
            let answer = success(request, clicked());
            answered = Some(answer.clone());
            answer
        })
        .await;
    assert_eq!(received, sent, "the request survives the proxy unchanged");

    let response = call.await.unwrap().unwrap();
    assert_eq!(
        serde_json::to_value(&response).unwrap(),
        answered.unwrap(),
        "the structured result survives the proxy unchanged"
    );
    assert_eq!(response.action, CuaActionKind::Click);
    assert!(matches!(response.response, CuaResponse::Success { .. }));
    desktop.close().await;
}

#[tokio::test]
async fn an_answer_that_does_not_match_its_request_is_rejected() {
    let h = harness().await;
    let mut desktop = FakeDesktop::registered(
        h.addr,
        register(STUDIO, Some(cua_field(STUDIO, MachineLocation::Desktop))),
    )
    .await;
    let adapter = h.target(STUDIO).await;

    // Another request id: dropped, the call keeps waiting for its own answer.
    let sent = request(STUDIO, "req-1", click());
    let call = h.execute(adapter.clone(), sent.clone());
    let received = desktop
        .answer_next(|_| success(&request(STUDIO, "req-9", click()), clicked()))
        .await;
    assert_eq!(received.request_id.as_str(), "req-1");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!call.is_finished(), "a stray answer resolves nothing");
    desktop
        .send(json!({"type": "cua_response", "response": success(&sent, clicked())}))
        .await;
    assert!(matches!(
        call.await.unwrap().unwrap().response,
        CuaResponse::Success { .. }
    ));

    // The right id but another action's answer: refused by the correlation
    // check, the call fails as a driver failure.
    let sent = request(STUDIO, "req-2", click());
    let call = h.execute(adapter.clone(), sent.clone());
    desktop
        .answer_next(|request| {
            success(
                request,
                CuaActionResult::ListApps(AppsResult { apps: vec![] }),
            )
        })
        .await;
    let response = call.await.unwrap().unwrap();
    assert_eq!(response.request_id.as_str(), "req-2");
    assert_eq!(response.action, CuaActionKind::Click);
    let error = error_of(response);
    assert_eq!(error.code, RuntimeErrorCode::DriverFailure);
    assert!(!error.retryable);
    assert!(
        error.message.as_str().contains("does not match"),
        "{}",
        error.message.as_str()
    );

    // An unreadable answer naming the request fails it now, not at the deadline.
    let sent = request(STUDIO, "req-3", click());
    let call = h.execute(adapter, sent.clone());
    let started = tokio::time::Instant::now();
    desktop
        .answer_next(|request| {
            let mut broken = success(request, clicked());
            broken["response"]["result"]["result"]["unknown_field"] = json!(1);
            broken
        })
        .await;
    let error = error_of(call.await.unwrap().unwrap());
    assert_eq!(error.code, RuntimeErrorCode::DriverFailure);
    assert!(error.message.as_str().contains("unknown field"));
    assert!(
        started.elapsed() < Duration::from_secs(CALL_TIMEOUT_SECS),
        "failed at once, not at the {CALL_TIMEOUT_SECS}s deadline"
    );
    desktop.close().await;
}

#[tokio::test]
async fn a_registration_without_cua_keeps_the_legacy_toolcalls_working() {
    let h = harness().await;
    let (mut desktop, ack) = FakeDesktop::connect(h.addr, register(STUDIO, None)).await;
    assert_eq!(ack["type"], "registered");
    assert_eq!(ack["cua"], false, "{ack}");

    // No typed target, a plain desktop entry, a plain known row.
    assert!(h.state.machine_registry.cua().list().await.is_empty());
    let listed: Vec<Value> = serde_json::from_str(&h.list_machines().await).unwrap();
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0]["machine_id"], STUDIO);
    assert_eq!(listed[0]["location"], "desktop");
    assert!(listed[0].get("health").is_none());
    let row = h.known_row(STUDIO).await;
    assert_eq!(row["online"], true);
    assert_eq!(row["driver_version"], Value::Null);
    assert_eq!(row["cua_health"], Value::Null);

    // The legacy toolcall is the flat message it always was, and its
    // `action_result` resolves it.
    let registry = h.state.machine_registry.clone();
    let call = tokio::spawn(async move {
        registry
            .execute(
                STUDIO,
                AgentToolCall {
                    request_id: "legacy-1".into(),
                    action: "screenshot".into(),
                    params: json!({}),
                },
            )
            .await
    });
    let frame = desktop.next_frame().await;
    assert_eq!(
        frame,
        json!({"request_id": "legacy-1", "action": "screenshot"})
    );
    desktop
        .send(json!({
            "type": "action_result", "request_id": "legacy-1",
            "result_type": "screenshot", "image": "aGk=", "width": 1, "height": 1, "scale": 1.0
        }))
        .await;
    let result = call.await.unwrap().unwrap();
    assert_eq!(result.result_type, "screenshot");
    assert_eq!(result.image.as_deref(), Some("aGk="));

    // A typed answer from a legacy-only desktop is dropped, not a crash.
    desktop
        .send(json!({"type": "cua_response", "response": {"anything": true}}))
        .await;
    let registry = h.state.machine_registry.clone();
    let call = tokio::spawn(async move {
        registry
            .execute(
                STUDIO,
                AgentToolCall {
                    request_id: "legacy-2".into(),
                    action: "screenshot".into(),
                    params: json!({}),
                },
            )
            .await
    });
    assert_eq!(desktop.next_frame().await["request_id"], "legacy-2");
    desktop
        .send(json!({
            "type": "action_result", "request_id": "legacy-2",
            "result_type": "action", "success": true
        }))
        .await;
    assert_eq!(call.await.unwrap().unwrap().success, Some(true));
    desktop.close().await;
}

#[tokio::test]
async fn a_reconnect_under_the_same_id_replaces_the_target_instead_of_duplicating_it() {
    let h = harness().await;
    let mut first = FakeDesktop::registered(
        h.addr,
        register(STUDIO, Some(cua_field(STUDIO, MachineLocation::Desktop))),
    )
    .await;
    let mut second = FakeDesktop::registered(
        h.addr,
        register(STUDIO, Some(cua_field(STUDIO, MachineLocation::Desktop))),
    )
    .await;

    // One target, one row, one legacy entry.
    assert_eq!(h.state.machine_registry.cua().list().await.len(), 1);
    h.known_row(STUDIO).await;
    assert_eq!(h.state.machine_registry.list().await.len(), 1);

    // Requests go to the socket that registered last.
    let adapter = h.target(STUDIO).await;
    let sent = request(STUDIO, "req-1", CuaAction::ListApps(EmptyArgs {}));
    let call = h.execute(adapter, sent.clone());
    second
        .answer_next(|request| {
            success(
                request,
                CuaActionResult::ListApps(AppsResult { apps: vec![] }),
            )
        })
        .await;
    assert!(matches!(
        call.await.unwrap().unwrap().response,
        CuaResponse::Success { .. }
    ));

    // The stale socket closing leaves the live target alone.
    first.close().await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(h.state.machine_registry.cua().list().await.len(), 1);
    assert_eq!(h.known_row(STUDIO).await["online"], true);
    let adapter = h.target(STUDIO).await;
    let sent = request(STUDIO, "req-2", CuaAction::ListApps(EmptyArgs {}));
    let call = h.execute(adapter, sent.clone());
    second
        .answer_next(|request| {
            success(
                request,
                CuaActionResult::ListApps(AppsResult { apps: vec![] }),
            )
        })
        .await;
    assert!(matches!(
        call.await.unwrap().unwrap().response,
        CuaResponse::Success { .. }
    ));

    // The live socket closing removes it.
    second.close().await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(h.state.machine_registry.cua().list().await.is_empty());
    assert_eq!(h.known_row(STUDIO).await["online"], false);
}

#[tokio::test]
async fn a_disconnect_fails_the_waiting_calls_and_drops_the_target_and_its_sessions() {
    let h = harness().await;
    let mut desktop = FakeDesktop::registered(
        h.addr,
        register(STUDIO, Some(cua_field(STUDIO, MachineLocation::Desktop))),
    )
    .await;
    let adapter = h.target(STUDIO).await;

    // A session the desktop confirms open.
    let label = SessionLabel::try_from("nolune-run-1").unwrap();
    let start = request(
        STUDIO,
        "run-1-start",
        CuaAction::StartSession(StartSessionArgs {
            session: Some(label.clone()),
        }),
    );
    let call = h.execute(adapter.clone(), start);
    desktop
        .answer_next(|request| {
            success(
                request,
                CuaActionResult::StartSession(StartSessionResult {
                    active: true,
                    capture_scope: CaptureScope::Auto,
                    effective_scope: CaptureScope::Window,
                    desktop_capture_authorized: true,
                    desktop_unlocked: true,
                    escalation_detail: None,
                    escalation_reason: None,
                    revived: false,
                    session: Some(SessionLabel::try_from("nolune-run-1").unwrap()),
                }),
            )
        })
        .await;
    call.await.unwrap().unwrap();
    let link = h
        .state
        .machine_registry
        .desktop_cua_link(STUDIO)
        .await
        .expect("the desktop has a typed link");
    assert_eq!(link.open_sessions(), vec![label]);

    // A click the desktop never answers, then the app quits.
    let waiting = h.execute(adapter, request(STUDIO, "run-1-1", click()));
    let frame = desktop.next_frame().await;
    assert_eq!(frame["type"], "cua_request");
    let started = tokio::time::Instant::now();
    desktop.close().await;

    let error = error_of(waiting.await.unwrap().unwrap());
    assert_eq!(error.code, RuntimeErrorCode::RuntimeUnavailable);
    assert!(error.retryable);
    assert!(
        started.elapsed() < Duration::from_secs(CALL_TIMEOUT_SECS),
        "failed with the socket, not at the {CALL_TIMEOUT_SECS}s deadline"
    );
    assert!(link.open_sessions().is_empty(), "lost with the socket");
    assert!(h.state.machine_registry.cua().list().await.is_empty());
    assert!(
        h.state
            .machine_registry
            .desktop_cua_link(STUDIO)
            .await
            .is_none()
    );
    let row = h.known_row(STUDIO).await;
    assert_eq!(row["online"], false);
    assert_eq!(row["driver_version"], Value::Null);
    assert_eq!(row["cua_health"], Value::Null);
    assert!(
        h.list_machines().await.contains("No machines connected"),
        "nothing left to drive"
    );
}

#[tokio::test]
async fn a_descriptor_for_another_machine_or_the_server_location_refuses_the_registration() {
    let h = harness().await;
    for cua in [
        cua_field("elsewhere", MachineLocation::Desktop),
        cua_field(STUDIO, MachineLocation::ServerLocal),
        json!({"version": "v1"}),
    ] {
        let (mut desktop, first) = FakeDesktop::connect(h.addr, register(STUDIO, Some(cua))).await;
        assert_eq!(first["type"], "error", "{first}");
        assert_eq!(first["error"], "invalid_cua_registration");
        assert!(
            desktop.closed().await,
            "refused registrations close the socket"
        );
    }
    assert!(h.state.machine_registry.cua().list().await.is_empty());
    assert!(h.state.machine_registry.list().await.is_empty());
    assert!(h.list_machines().await.contains("No machines connected"));
}

#[tokio::test]
async fn a_desktop_claiming_the_server_local_id_keeps_the_legacy_registration_only() {
    let h = harness().await;
    let local = descriptor("server-local:studio", MachineLocation::ServerLocal);
    let adapter = CheckedCuaAdapter::new(local.clone(), |request| {
        let response = CuaResponseEnvelope {
            version: request.version,
            request_id: request.request_id,
            machine_id: request.machine_id,
            action: request.action.kind(),
            response: CuaResponse::Success {
                result: Box::new(CuaActionResult::ListApps(AppsResult { apps: vec![] })),
            },
        };
        Box::pin(async move { response })
    })
    .unwrap();
    h.state
        .machine_registry
        .cua()
        .register_server_local(adapter, "studio", 1_700_000_000)
        .await
        .unwrap();

    let (desktop, ack) = FakeDesktop::connect(
        h.addr,
        register(
            "server-local:studio",
            Some(cua_field("server-local:studio", MachineLocation::Desktop)),
        ),
    )
    .await;
    assert_eq!(ack["type"], "registered");
    assert_eq!(ack["cua"], false, "the typed target was refused: {ack}");
    assert_eq!(
        h.state.machine_registry.cua().list().await,
        vec![local],
        "the server-local target is never shadowed"
    );
    assert_eq!(
        h.state.machine_registry.list().await.len(),
        1,
        "the legacy registration stands"
    );
    desktop.close().await;
}
