//! The lifecycle of the server-local Cua target (#16): started once by the
//! gateway, registered into the shared `CuaTargets` while the driver is up,
//! driven through bounded per-run sessions, and stopped by the shutdown hook.
//!
//! Nothing here consults the host on construction; `start` does, so tests
//! that build an `AppState` never spawn a driver. A headless host or a driver
//! that cannot start leaves the runtime idle and the server healthy.

use std::{
    collections::BTreeSet,
    future::Future,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use cua_protocol::{
    CuaAction, CuaRequestEnvelope, CuaResponseEnvelope, MachineDescriptor, MachineId, SessionLabel,
    ValidationError,
};

use super::{
    discovery::DriverLookupError,
    host::{HostProbe, Skip},
    transport::DriverTransport,
};
use crate::{config::CuaConfig, services::machine_registry::CuaTargets};

/// Where the runtime is in its life.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeStatus {
    /// `start` has not run.
    NotStarted,
    /// The host has no GUI target to offer; the server runs without one.
    Skipped(Skip),
    /// A driver was found but could not be started or described.
    Failed(String),
    /// The server-local target is registered under this id.
    Running(MachineId),
    /// The shutdown hook ran; the driver is gone.
    Stopped,
}

/// Why a run could not be executed.
#[derive(Debug)]
pub enum RunError {
    /// No server-local target is registered right now.
    Unavailable,
    /// The run outlived `[cua].run_timeout_secs`; its session was ended.
    Timeout(Duration),
    /// The run's body returned an error; its session was ended.
    Failed(anyhow::Error),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => f.write_str("no server-local computer-use target is available"),
            Self::Timeout(limit) => write!(f, "the run exceeded its {limit:?} limit"),
            Self::Failed(error) => write!(f, "the run failed: {error}"),
        }
    }
}

impl std::error::Error for RunError {}

/// Why one action inside a run was not executed.
#[derive(Debug)]
pub enum ExecError {
    /// The target does not advertise the capability, lacks the permission,
    /// is unavailable, or the action is one the run manages itself.
    Refused(String),
    /// The run's session could not be started, so the action never ran.
    SessionFailed(String),
    /// The driver answered, but the answer did not pass the protocol boundary.
    Protocol(ValidationError),
}

impl std::fmt::Display for ExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(reason) => write!(f, "refused: {reason}"),
            Self::SessionFailed(reason) => write!(f, "session could not start: {reason}"),
            Self::Protocol(error) => write!(f, "protocol: {error}"),
        }
    }
}

impl std::error::Error for ExecError {}

/// The registered target: the adapter that executes inside the checked
/// boundary and the transport that owns the driver child.
struct Target {
    machine_id: MachineId,
    adapter: Arc<cua_protocol::CheckedCuaAdapter>,
    transport: Arc<dyn DriverTransport>,
}

enum State {
    NotStarted,
    Skipped(Skip),
    Failed(String),
    Running(Target),
    Stopped,
}

struct Inner {
    config: CuaConfig,
    targets: CuaTargets,
    state: tokio::sync::Mutex<State>,
    /// Labels of the sessions runs hold open right now; `shutdown` ends every
    /// one and a run that finishes afterwards finds its label gone.
    open: Arc<Mutex<BTreeSet<SessionLabel>>>,
    next_run: AtomicU64,
}

/// A handle on the server-local runtime; clones share one runtime.
#[derive(Clone)]
pub struct CuaRuntime {
    inner: Arc<Inner>,
}

impl CuaRuntime {
    /// A runtime that has not looked at the host yet. Registers into
    /// `targets` once started.
    pub fn new(config: CuaConfig, targets: CuaTargets) -> Self {
        Self {
            inner: Arc::new(Inner {
                config,
                targets,
                state: tokio::sync::Mutex::new(State::NotStarted),
                open: Arc::new(Mutex::new(BTreeSet::new())),
                next_run: AtomicU64::new(1),
            }),
        }
    }

    /// Probe this host, find the driver, and register the server-local
    /// target when there is one. Every outcome is logged once; none of them
    /// fails the server.
    pub async fn start(&self) {
        todo!("slice 3: runtime start")
    }

    /// `start` with the host and the driver lookup supplied, so tests decide
    /// what the host looks like without touching the process environment.
    pub(crate) async fn start_from(
        &self,
        host: HostProbe,
        driver: Result<Option<PathBuf>, DriverLookupError>,
    ) {
        let _ = (host, driver);
        todo!("slice 3: runtime start")
    }

    /// Register the target behind an already running transport: ask it for
    /// its health report, build the descriptor, and put the checked adapter
    /// into the shared registry. `hostname` labels the row the API lists.
    pub(crate) async fn attach(
        &self,
        transport: Arc<dyn DriverTransport>,
        machine_id: MachineId,
        hostname: &str,
    ) -> anyhow::Result<MachineDescriptor> {
        let _ = (transport, machine_id, hostname);
        todo!("slice 3: runtime attach")
    }

    pub async fn status(&self) -> RuntimeStatus {
        todo!("slice 3: runtime status")
    }

    /// The registered target's id, `None` unless running.
    pub async fn machine_id(&self) -> Option<MachineId> {
        match self.status().await {
            RuntimeStatus::Running(id) => Some(id),
            _ => None,
        }
    }

    /// Execute `body` as one run: the first action it executes opens a driver
    /// session, every action carries that session, and the session is ended
    /// when the body completes, fails, or exceeds the run timeout.
    pub async fn run<T, F, Fut>(&self, purpose: &str, body: F) -> Result<T, RunError>
    where
        F: FnOnce(Arc<RunSession>) -> Fut,
        Fut: Future<Output = anyhow::Result<T>>,
    {
        let _ = (purpose, body);
        todo!("slice 3: per-run sessions")
    }

    /// End every open session, unregister the target and stop the driver.
    /// Safe to call on a runtime that never started or already stopped.
    pub async fn shutdown(&self) {
        todo!("slice 3: shutdown hook")
    }
}

/// One run's view of the target: actions go through the checked adapter,
/// labelled with the run's session.
pub struct RunSession {
    label: SessionLabel,
    machine_id: MachineId,
    adapter: Arc<cua_protocol::CheckedCuaAdapter>,
    /// Set once `start_session` succeeded.
    started: tokio::sync::Mutex<bool>,
    open: Arc<Mutex<BTreeSet<SessionLabel>>>,
    next_request: AtomicU64,
}

impl RunSession {
    /// The driver session this run's actions belong to.
    pub fn label(&self) -> &SessionLabel {
        &self.label
    }

    pub fn machine_id(&self) -> &MachineId {
        &self.machine_id
    }

    /// Execute one action inside this run's session. The action is
    /// authorized against the target's descriptor before the session is
    /// opened, so a refused action never reaches the driver and never opens
    /// a session on its own.
    pub async fn execute(&self, action: CuaAction) -> Result<CuaResponseEnvelope, ExecError> {
        let _ = (action, &self.adapter, &self.started, &self.open);
        todo!("slice 3: per-run sessions")
    }

    fn envelope(&self, action: CuaAction) -> CuaRequestEnvelope {
        let n = self.next_request.fetch_add(1, Ordering::Relaxed);
        CuaRequestEnvelope {
            version: cua_protocol::ProtocolVersion::V1,
            request_id: cua_protocol::RequestId::try_from(format!("{}-{n}", self.label.as_str()))
                .expect("a run label plus a counter is an identifier"),
            machine_id: self.machine_id.clone(),
            action,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{
        cua::{host::server_local_machine_id, transport::fake::FakeTransport},
        machine_registry::{MachineInfo, MachineRegistry},
        tool::Tool as _,
        tools::computer::{ListMachinesArgs, ListMachinesTool},
    };
    use cua_protocol::*;
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

    fn label(n: u64) -> Value {
        json!(format!("nolune-run-{n}"))
    }

    fn started(n: u64) -> Value {
        json!({
            "active": true, "capture_scope": "auto", "effective_scope": "window",
            "desktop_capture_authorized": true, "desktop_unlocked": true,
            "revived": false, "session": label(n)
        })
    }

    fn ended(n: u64) -> Value {
        json!({"session": label(n), "active": false})
    }

    fn target() -> WindowTarget {
        WindowTarget {
            pid: 42,
            window_id: 99,
        }
    }

    fn list_apps() -> CuaAction {
        CuaAction::ListApps(EmptyArgs {})
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

    fn gui_host() -> HostProbe {
        HostProbe {
            os: "macos",
            display: true,
            container: false,
            hostname: "studio".into(),
        }
    }

    fn config() -> CuaConfig {
        CuaConfig::default()
    }

    fn harness() -> (MachineRegistry, CuaRuntime) {
        let registry = MachineRegistry::new();
        let runtime = CuaRuntime::new(config(), registry.cua().clone());
        (registry, runtime)
    }

    async fn attached(
        registry: &MachineRegistry,
        report: &str,
        answers: Vec<crate::services::cua::transport::CallOutcome>,
    ) -> (CuaRuntime, Arc<FakeTransport>) {
        let mut outcomes = vec![Ok(payload(report))];
        outcomes.extend(answers);
        let transport = Arc::new(FakeTransport::answering(outcomes));
        let runtime = CuaRuntime::new(config(), registry.cua().clone());
        runtime
            .attach(
                transport.clone(),
                server_local_machine_id("studio"),
                "studio",
            )
            .await
            .unwrap();
        (runtime, transport)
    }

    async fn listed_by_tool(registry: &MachineRegistry) -> String {
        ListMachinesTool::new(registry.clone())
            .call(ListMachinesArgs {})
            .await
            .unwrap()
    }

    fn desktop(machine_id: &str, hostname: &str) -> MachineInfo {
        MachineInfo {
            machine_id: machine_id.into(),
            os: "macos".into(),
            hostname: hostname.into(),
            screen_width: 1440,
            screen_height: 900,
            last_seen: 1_700_000_000,
            instance_slug: None,
            platform: Some(Platform::Macos),
            location: MachineLocation::Desktop,
            permissions: None,
            capabilities: Vec::new(),
        }
    }

    #[tokio::test]
    async fn a_headless_host_registers_nothing_and_the_server_stays_healthy() {
        for (host, driver, expected) in [
            (
                HostProbe {
                    os: "linux",
                    display: false,
                    ..gui_host()
                },
                Ok(Some(PathBuf::from("/usr/bin/cua-driver"))),
                Skip::NoDisplay,
            ),
            (
                HostProbe {
                    container: true,
                    os: "linux",
                    ..gui_host()
                },
                Ok(Some(PathBuf::from("/usr/bin/cua-driver"))),
                Skip::Container,
            ),
            (gui_host(), Ok(None), Skip::NoDriver),
        ] {
            let (registry, runtime) = harness();
            assert_eq!(runtime.status().await, RuntimeStatus::NotStarted);
            runtime.start_from(host, driver).await;
            assert_eq!(runtime.status().await, RuntimeStatus::Skipped(expected));
            assert_eq!(runtime.machine_id().await, None);
            assert!(registry.cua().list().await.is_empty());
            assert!(registry.known_at(1_700_000_000).await.unwrap().is_empty());
            assert!(
                listed_by_tool(&registry)
                    .await
                    .starts_with("No machines connected")
            );
            let refused = runtime.run("check", |_| async { Ok(()) }).await;
            assert!(matches!(refused, Err(RunError::Unavailable)), "{refused:?}");
            // Shutting down a runtime that never started is a no-op.
            runtime.shutdown().await;
            assert_eq!(runtime.status().await, RuntimeStatus::Stopped);
        }
    }

    #[tokio::test]
    async fn a_misconfigured_driver_path_is_reported_and_registers_nothing() {
        let (registry, runtime) = harness();
        let error = DriverLookupError::NotExecutable {
            source: "[cua].driver_path",
            path: PathBuf::from("/typo/cua-driver"),
        };
        runtime.start_from(gui_host(), Err(error.clone())).await;
        assert_eq!(
            runtime.status().await,
            RuntimeStatus::Skipped(Skip::Misconfigured(error))
        );
        assert!(registry.cua().list().await.is_empty());
    }

    /// A driver that never speaks MCP: the runtime gives up within the
    /// handshake deadline and leaves no target behind.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_driver_that_fails_the_handshake_leaves_no_target_behind() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let silent = dir.path().join("silent-driver");
        std::fs::write(
            &silent,
            "#!/bin/sh\n[ \"$1\" = warm-up ] && exit 0\nexec sleep 60\n",
        )
        .unwrap();
        std::fs::set_permissions(&silent, std::fs::Permissions::from_mode(0o755)).unwrap();
        // macOS scans a fresh executable on first run; keep that out of the deadline.
        assert!(
            std::process::Command::new(&silent)
                .arg("warm-up")
                .status()
                .unwrap()
                .success()
        );

        let registry = MachineRegistry::new();
        let runtime = CuaRuntime::new(
            CuaConfig {
                handshake_timeout_secs: 1,
                ..CuaConfig::default()
            },
            registry.cua().clone(),
        );
        let started = std::time::Instant::now();
        runtime.start_from(gui_host(), Ok(Some(silent))).await;
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the handshake deadline bounds start, took {:?}",
            started.elapsed()
        );
        match runtime.status().await {
            RuntimeStatus::Failed(message) => {
                assert!(message.contains("handshake"), "{message}");
                assert!(message.contains("silent-driver"), "{message}");
            }
            other => panic!("a driver that never answers is a failure, got {other:?}"),
        }
        assert!(registry.cua().list().await.is_empty());
        assert!(matches!(
            runtime.run("check", |_| async { Ok(()) }).await,
            Err(RunError::Unavailable)
        ));
    }

    #[tokio::test]
    async fn on_a_gui_host_the_server_local_target_is_listed_everywhere() {
        let registry = MachineRegistry::new();
        let (runtime, transport) = attached(&registry, HEALTHY, vec![]).await;
        assert_eq!(
            transport.tools_called(),
            vec!["health_report".to_owned()],
            "attach asks for one health report and nothing else"
        );

        let id = server_local_machine_id("studio");
        assert_eq!(runtime.status().await, RuntimeStatus::Running(id.clone()));
        assert_eq!(runtime.machine_id().await, Some(id.clone()));

        // The typed registry.
        let targets = registry.cua().list().await;
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].machine_id, id);
        assert_eq!(targets[0].location, MachineLocation::ServerLocal);
        assert_eq!(targets[0].health, MachineHealth::Healthy);
        assert!(targets[0].capabilities.contains(&Capability::Pointer));

        // GET /machines.
        let known = registry.known_at(1_700_000_000).await.unwrap();
        assert_eq!(known.len(), 1, "{known:?}");
        let row = &known[0];
        assert_eq!(row.machine_id, "server-local:studio");
        assert_eq!(row.location, MachineLocation::ServerLocal);
        assert_eq!(row.hostname, "studio");
        assert_eq!(row.display_name, "studio");
        assert_eq!(row.custom_name, None);
        assert_eq!(row.os, "macos");
        assert_eq!(row.platform, Some(Platform::Macos));
        assert!(row.online);
        assert_eq!(row.health, MachineHealth::Healthy);
        assert_eq!(row.cua_health, Some(MachineHealth::Healthy));
        assert_eq!(row.driver_version.as_deref(), Some("0.28.2"));
        assert_eq!(
            row.permissions,
            Some(PermissionState {
                accessibility: Permission::Granted,
                screen_capture: Permission::Granted,
            })
        );
        assert!(row.capabilities.contains(&"pointer".to_owned()));
        assert!(row.capabilities.contains(&"session_lifecycle".to_owned()));
        assert_eq!(
            row.last_seen, 1_700_000_000,
            "a running target was seen now"
        );
        assert!(row.first_seen <= row.last_seen);
        assert_eq!(
            row.instance_slug.as_deref(),
            Some(crate::domain::companion::CANONICAL_SLUG)
        );

        // The list_machines tool.
        let output = listed_by_tool(&registry).await;
        let entries: Vec<Value> = serde_json::from_str(&output).unwrap();
        let entry = entries
            .iter()
            .find(|entry| entry["machine_id"] == "server-local:studio")
            .expect("list_machines names the server-local target");
        assert_eq!(entry["location"], "server_local");
        assert_eq!(entry["health"], "healthy");
        assert_eq!(entry["driver_version"], "0.28.2");
    }

    #[tokio::test]
    async fn the_server_local_id_never_collides_with_a_desktop_on_the_same_host() {
        let registry = MachineRegistry::new();
        // A desktop from before #80 registered under its bare hostname; a
        // current one under a UUID, still reporting the same hostname.
        for machine_id in ["studio", "4f3c1c2e-9b5e-4d2b-8f0a-1c2d3e4f5a6b"] {
            let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
            registry.register(desktop(machine_id, "studio"), tx).await;
        }
        let (_runtime, _transport) = attached(&registry, HEALTHY, vec![]).await;

        let known = registry.known_at(1_700_000_000).await.unwrap();
        let mut ids: Vec<&str> = known.iter().map(|m| m.machine_id.as_str()).collect();
        ids.sort_unstable();
        assert_eq!(
            ids,
            vec![
                "4f3c1c2e-9b5e-4d2b-8f0a-1c2d3e4f5a6b",
                "server-local:studio",
                "studio"
            ]
        );
        assert!(
            known
                .iter()
                .all(|m| m.hostname == "studio" && m.display_name == "studio"),
            "three rows, one hostname, three ids: {known:?}"
        );
        let entries: Vec<Value> = serde_json::from_str(&listed_by_tool(&registry).await).unwrap();
        assert_eq!(entries.len(), 3);

        // A desktop that claims the server-local id (the WebSocket grammar
        // allows it) does not produce a second row for that id.
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        registry
            .register(desktop("server-local:studio", "impostor"), tx)
            .await;
        let known = registry.known_at(1_700_000_000).await.unwrap();
        let rows: Vec<_> = known
            .iter()
            .filter(|m| m.machine_id == "server-local:studio")
            .collect();
        assert_eq!(rows.len(), 1, "{known:?}");
        assert_eq!(rows[0].location, MachineLocation::ServerLocal);
        assert_eq!(rows[0].hostname, "studio");
    }

    #[tokio::test]
    async fn a_run_opens_its_session_before_the_first_action_and_ends_it_after() {
        let registry = MachineRegistry::new();
        let (runtime, transport) = attached(
            &registry,
            HEALTHY,
            vec![Ok(started(1)), Ok(json!({"apps": []})), Ok(ended(1))],
        )
        .await;

        // A run that executes nothing opens nothing.
        let idle: Result<u8, _> = runtime.run("idle", |_| async { Ok(7) }).await;
        assert_eq!(idle.unwrap(), 7);
        assert_eq!(transport.tools_called(), vec!["health_report".to_owned()]);

        let apps = runtime
            .run("list the apps", |run| async move {
                assert_eq!(run.label().as_str(), "nolune-run-1");
                let response = run.execute(list_apps()).await?;
                assert_eq!(response.action, CuaActionKind::ListApps);
                assert!(matches!(response.response, CuaResponse::Success { .. }));
                Ok(response)
            })
            .await
            .unwrap();
        assert_eq!(apps.machine_id.as_str(), "server-local:studio");

        let calls = transport.calls();
        assert_eq!(
            transport.tools_called(),
            vec!["health_report", "start_session", "list_apps", "end_session"]
        );
        assert_eq!(calls[1].1["session"], label(1));
        assert_eq!(calls[3].1["session"], label(1));
        assert!(
            registry.cua().list().await.len() == 1,
            "the target stays registered between runs"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_failing_or_timed_out_run_still_ends_its_session() {
        let registry = MachineRegistry::new();
        let (runtime, transport) = attached(
            &registry,
            HEALTHY,
            vec![
                Ok(started(1)),
                Ok(json!({"apps": []})),
                Ok(ended(1)),
                Ok(started(2)),
                Ok(json!({"apps": []})),
                Ok(ended(2)),
            ],
        )
        .await;

        let failed: Result<(), RunError> = runtime
            .run("fails", |run| async move {
                run.execute(list_apps()).await?;
                anyhow::bail!("the model gave up")
            })
            .await;
        match failed {
            Err(RunError::Failed(error)) => assert!(error.to_string().contains("gave up")),
            other => panic!("expected the body's error, got {other:?}"),
        }
        assert_eq!(
            transport.tools_called(),
            vec!["health_report", "start_session", "list_apps", "end_session"]
        );

        let stuck: Result<(), RunError> = runtime
            .run("hangs", |run| async move {
                run.execute(list_apps()).await?;
                std::future::pending::<()>().await;
                Ok(())
            })
            .await;
        match stuck {
            Err(RunError::Timeout(limit)) => assert_eq!(limit, config().run_timeout()),
            other => panic!("expected a timeout, got {other:?}"),
        }
        let calls = transport.calls();
        assert_eq!(calls.len(), 7, "{calls:?}");
        assert_eq!(calls[6].0, "end_session");
        assert_eq!(calls[6].1["session"], label(2));
    }

    #[tokio::test]
    async fn shutdown_ends_open_sessions_unregisters_the_target_and_closes_the_driver() {
        let registry = MachineRegistry::new();
        let (runtime, transport) = attached(
            &registry,
            HEALTHY,
            vec![
                Ok(started(1)),
                Ok(json!({"apps": []})),
                Ok(started(2)),
                Ok(json!({"apps": []})),
                Ok(ended(1)),
                Ok(ended(2)),
            ],
        )
        .await;

        // Two runs park after their first action, holding their sessions open.
        let release = Arc::new(tokio::sync::Notify::new());
        let mut parked = Vec::new();
        for purpose in ["first", "second"] {
            let runtime = runtime.clone();
            let release = release.clone();
            parked.push(tokio::spawn(async move {
                runtime
                    .run(purpose, |run| async move {
                        run.execute(list_apps()).await?;
                        release.notified().await;
                        Ok(())
                    })
                    .await
            }));
        }
        while transport.tools_called().len() < 5 {
            tokio::task::yield_now().await;
        }

        runtime.shutdown().await;
        assert_eq!(runtime.status().await, RuntimeStatus::Stopped);
        assert!(
            registry.cua().list().await.is_empty(),
            "the target is gone from the registry"
        );
        assert!(
            registry.known_at(1_700_000_000).await.unwrap().is_empty(),
            "and from GET /machines"
        );
        assert!(transport.closed(), "the driver child is killed");
        let ended_labels: Vec<Value> = transport
            .calls()
            .into_iter()
            .filter(|(name, _)| name == "end_session")
            .map(|(_, args)| args["session"].clone())
            .collect();
        assert_eq!(ended_labels, vec![label(1), label(2)]);

        // The parked runs finish afterwards without ending anything twice.
        release.notify_waiters();
        release.notify_one();
        release.notify_one();
        for run in parked {
            run.await.unwrap().unwrap();
        }
        assert_eq!(
            transport
                .tools_called()
                .iter()
                .filter(|name| *name == "end_session")
                .count(),
            2
        );
        assert!(matches!(
            runtime.run("late", |_| async { Ok(()) }).await,
            Err(RunError::Unavailable)
        ));
        // Idempotent.
        runtime.shutdown().await;
        assert_eq!(runtime.status().await, RuntimeStatus::Stopped);
    }

    #[tokio::test]
    async fn a_refused_capability_never_reaches_the_driver() {
        let registry = MachineRegistry::new();
        let (runtime, transport) = attached(
            &registry,
            ACCESSIBILITY_DENIED,
            vec![Ok(started(1)), Ok(json!({"apps": []})), Ok(ended(1))],
        )
        .await;
        let descriptor = &registry.cua().list().await[0];
        assert_eq!(descriptor.health, MachineHealth::Degraded);
        assert!(!descriptor.capabilities.contains(&Capability::Pointer));

        let inner = transport.clone();
        runtime
            .run("click without accessibility", |run| async move {
                let transport = inner;
                let refused = run.execute(click()).await;
                match refused {
                    Err(ExecError::Refused(reason)) => {
                        assert!(reason.contains("capability"), "{reason}");
                    }
                    other => panic!("a dropped capability is refused, got {other:?}"),
                }
                // Sessions are the runtime's job.
                let managed = run
                    .execute(CuaAction::StartSession(StartSessionArgs {
                        session: Some(SessionLabel::try_from("mine").unwrap()),
                    }))
                    .await;
                assert!(matches!(managed, Err(ExecError::Refused(_))), "{managed:?}");
                assert_eq!(
                    transport.tools_called(),
                    vec!["health_report".to_owned()],
                    "no session was opened for refused actions"
                );
                // An advertised action still works in the same run.
                run.execute(list_apps()).await?;
                Ok(())
            })
            .await
            .unwrap();
        assert_eq!(
            transport.tools_called(),
            vec!["health_report", "start_session", "list_apps", "end_session"]
        );
    }

    #[tokio::test]
    async fn a_snapshot_action_verification_round_trip_runs_inside_one_session() {
        let fixture =
            include_str!("../../../../cua-protocol/tests/fixtures/degraded-window-state.json");
        let mut snapshot: Value = serde_json::from_str(fixture).unwrap();
        snapshot["target"] = json!({"pid": 42, "window_id": 99});
        snapshot["background_input"]["exact_window"]["pid"] = json!(42);
        snapshot["background_input"]["exact_window"]["window_id"] = json!(99);
        let clicked = json!({
            "target": {"pid": 42, "window_id": 99},
            "session": label(1),
            "address": {"kind": "point", "x": 1.0, "y": 2.0},
            "button": "left", "action": "press",
            "outcome": {
                "effect": "confirmed", "route": "accessibility",
                "delivery": {"requested": "background", "delivered_count": 1},
                "evidence": ["delivery_receipt"]
            }
        });
        let verified = json!({
            "overall": "satisfied",
            "predicates": [{"predicate_index": 0, "status": "satisfied"}]
        });

        let registry = MachineRegistry::new();
        let (runtime, transport) = attached(
            &registry,
            HEALTHY,
            vec![
                Ok(started(1)),
                Ok(snapshot),
                Ok(clicked),
                Ok(verified),
                Ok(ended(1)),
            ],
        )
        .await;

        let outcome = runtime
            .run("save the document", |run| async move {
                let state = run
                    .execute(CuaAction::GetWindowState(GetWindowStateArgs {
                        target: target(),
                        session: None,
                        include_accessibility_tree: true,
                        include_screenshot: false,
                        max_elements: None,
                        max_depth: None,
                        max_dimension: None,
                        query: None,
                    }))
                    .await?;
                let CuaResponse::Success { result } = state.response else {
                    anyhow::bail!("snapshot failed: {state:?}");
                };
                let CuaActionResult::GetWindowState(window) = *result else {
                    anyhow::bail!("not a window state");
                };
                assert!(window.degraded, "the fixture is the degraded snapshot");

                let click = run.execute(click()).await?;
                let CuaResponse::Success { result } = click.response else {
                    anyhow::bail!("click failed: {click:?}");
                };
                let CuaActionResult::Click(click) = *result else {
                    anyhow::bail!("not a click result");
                };
                assert_eq!(click.outcome.effect, ActionEffect::Confirmed);
                assert_eq!(
                    click.session.as_ref().map(SessionLabel::as_str),
                    Some("nolune-run-1")
                );

                let verify = run
                    .execute(CuaAction::VerifyState(VerifyStateArgs {
                        target: target(),
                        session: None,
                        expect: vec![VerifyPredicate::WindowExists(true)],
                        include_screenshot: false,
                        stable_samples: 1,
                        timeout_ms: 500,
                    }))
                    .await?;
                let CuaResponse::Success { result } = verify.response else {
                    anyhow::bail!("verification failed: {verify:?}");
                };
                let CuaActionResult::VerifyState(verification) = *result else {
                    anyhow::bail!("not a verification");
                };
                Ok(verification.overall)
            })
            .await
            .unwrap();
        assert_eq!(outcome, PredicateStatus::Satisfied);

        let calls = transport.calls();
        assert_eq!(
            transport.tools_called(),
            vec![
                "health_report",
                "start_session",
                "get_window_state",
                "click",
                "verify_state",
                "end_session"
            ]
        );
        for index in 2..=4 {
            assert_eq!(
                calls[index].1["session"],
                label(1),
                "{} carries the run's session",
                calls[index].0
            );
        }
        assert_eq!(calls[3].1["x"], 1.0);
        assert_eq!(calls[3].1["y"], 2.0);
    }

    #[tokio::test]
    async fn a_session_that_will_not_start_fails_the_action_and_the_run_cleanly() {
        let registry = MachineRegistry::new();
        let (runtime, transport) = attached(
            &registry,
            HEALTHY,
            vec![Err(cua_protocol::driver_mcp::DriverCallFailure::Tool {
                code: Some("session_limit".into()),
                message: "too many sessions".into(),
            })],
        )
        .await;
        let outcome: Result<(), RunError> = runtime
            .run("no session", |run| async move {
                let failed = run.execute(list_apps()).await;
                match failed {
                    Err(ExecError::SessionFailed(reason)) => {
                        assert!(reason.contains("too many sessions"), "{reason}");
                    }
                    other => panic!("expected SessionFailed, got {other:?}"),
                }
                Ok(())
            })
            .await;
        outcome.unwrap();
        assert_eq!(
            transport.tools_called(),
            vec!["health_report", "start_session"],
            "nothing was executed and nothing is ended for a session that never opened"
        );
    }

    /// Against a real `cua-driver mcp` child; run by hand with `--ignored`
    /// on a host that has the driver. It only reads and ends its own session.
    #[tokio::test]
    #[ignore = "needs an installed cua-driver binary"]
    async fn live_server_local_target_round_trips_a_session() {
        let registry = MachineRegistry::new();
        let runtime = CuaRuntime::new(config(), registry.cua().clone());
        runtime.start().await;
        let RuntimeStatus::Running(id) = runtime.status().await else {
            eprintln!("no server-local target here: {:?}", runtime.status().await);
            return;
        };
        eprintln!("live target: {id:?}");
        let sessions = runtime
            .run("live", |run| async move {
                let response = run
                    .execute(CuaAction::ListSessions(ListSessionsArgs {
                        cursor: None,
                        limit: None,
                    }))
                    .await?;
                Ok(response)
            })
            .await
            .unwrap();
        eprintln!("live sessions: {sessions:?}");
        runtime.shutdown().await;
        assert!(registry.cua().list().await.is_empty());
    }
}
