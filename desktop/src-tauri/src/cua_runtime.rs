//! The desktop's Cua Driver runtime (#17): one persistent `cua-driver mcp`
//! child for the app lifetime, the descriptor it registers with over the
//! machine WebSocket, the local allowlist every inbound `cua_request` frame
//! passes before the driver sees it, and the sessions the desktop holds open
//! for the server, ended on disconnect and at exit.
//!
//! The wire mapping (action to tool call, payload to validated envelope,
//! health report to descriptor) is `cua_protocol::driver_mcp`, shared with
//! the server; nothing here re-encodes an action. Explicit one-shot capture
//! only: the driver snapshots a window when a `get_window_state` asks for
//! one, and nothing here records or streams.

use std::{
    collections::{BTreeSet, VecDeque},
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};

use cua_protocol::{
    driver_mcp::{
        descriptor_from_health, error_response, response_for, tool_call, DriverCallFailure,
    },
    CheckedCuaAdapter, CuaAction, CuaActionKind, CuaActionResult, CuaRegistrationEnvelope,
    CuaRequestEnvelope, CuaResponse, CuaResponseEnvelope, CuaRuntimeError, HealthReportArgs,
    HealthReportResult, MachineDescriptor, MachineId, MachineLocation, ProtocolVersion, RequestId,
    RuntimeErrorCode, SessionLabel, SessionRefArgs, MAX_ID_BYTES,
};
use futures_util::FutureExt as _;
use rmcp::{
    model::{
        CallToolRequest, CallToolRequestParams, CallToolResult, ClientInfo, ClientRequest,
        Implementation, RawContent, ServerResult,
    },
    service::{PeerRequestOptions, ServerSink, ServiceError},
    transport::TokioChildProcess,
    ServiceExt as _,
};
use serde_json::{json, Map, Value};
use tokio::io::AsyncBufReadExt as _;

// ---------------------------------------------------------------------------
// Transport: one driver process the runtime can call tools on
// ---------------------------------------------------------------------------

/// The structured payload of one tool call, or why there is none.
pub type CallOutcome = Result<Value, DriverCallFailure>;

pub type TransportFuture<'a> = Pin<Box<dyn Future<Output = CallOutcome> + Send + 'a>>;

pub type ExitFuture<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

/// One driver process the runtime can call tools on; the same shape as the
/// server's `services::cua::transport::DriverTransport`, so tests use an
/// in-memory fake and only the real child speaks MCP.
pub trait DriverTransport: Send + Sync {
    /// Call one MCP tool by its driver name and return its structured payload.
    fn call_tool(&self, name: &str, arguments: Map<String, Value>) -> TransportFuture<'_>;

    /// Stop the driver for good: later calls fail and the child, if any, is
    /// killed. Idempotent, so the exit hook and a drop can both call it.
    fn close(&self);

    /// Resolve once the driver is gone for any reason: the child exited or
    /// crashed on its own, or `close` ran. Resolves at once for a driver that
    /// is already gone, so a watcher can never miss the exit.
    fn exited(&self) -> ExitFuture<'_>;
}

/// The deadlines one driver process is held to: an executable that never
/// speaks MCP wedges the `initialize` handshake, and a driver blocked on a
/// permission prompt wedges every later tool call, silently and forever.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverTimeouts {
    /// How long `<driver> mcp` may take to answer the MCP `initialize`
    /// handshake before the child is killed.
    pub handshake: Duration,
    /// How long one tool call may take before the request is cancelled and
    /// the call fails as a retryable [`DriverCallFailure::Timeout`].
    pub call: Duration,
}

impl Default for DriverTimeouts {
    fn default() -> Self {
        Self {
            handshake: Duration::from_secs(10),
            // The longest legitimate single action is a `verify_state` wait
            // (at most 10 s by the protocol) or an app launch; anything past
            // this is a stuck driver, not a slow one.
            call: Duration::from_secs(30),
        }
    }
}

/// The structured payload of an MCP `tools/call` result: the driver puts the
/// typed result in `structuredContent` and a human rendering in the text
/// block, so the text is only consulted when it is itself JSON. An `isError`
/// result carries the driver's structured `code` beside its text.
fn payload_from_call_result(result: CallToolResult) -> CallOutcome {
    let _ = result;
    todo!("desktop cua runtime (#17)")
}

/// MCP over stdio to one persistent `cua-driver mcp` child process. The child
/// lives exactly as long as this transport: the keep-alive task owns the rmcp
/// service, and dropping the transport aborts that task, which drops the
/// service, closes the connection and kills the child.
pub struct StdioDriverTransport {
    sink: ServerSink,
    /// Aborting this ends the rmcp service and kills the child.
    keep_alive: tokio::task::AbortHandle,
    /// `true` once the keep-alive task has ended, whatever ended it.
    gone: tokio::sync::watch::Receiver<bool>,
    timeouts: DriverTimeouts,
}

impl Drop for StdioDriverTransport {
    fn drop(&mut self) {
        self.keep_alive.abort();
    }
}

impl StdioDriverTransport {
    /// Spawn `<driver> mcp` and complete the MCP handshake within
    /// `timeouts.handshake`; a child that has not answered by then is killed
    /// and the error names the driver and repeats what it said on stderr.
    pub async fn spawn_with(driver: &Path, timeouts: DriverTimeouts) -> Result<Self, String> {
        let _ = (driver, timeouts);
        todo!("desktop cua runtime (#17)")
    }
}

impl DriverTransport for StdioDriverTransport {
    fn call_tool(&self, name: &str, arguments: Map<String, Value>) -> TransportFuture<'_> {
        let _ = (name, arguments);
        todo!("desktop cua runtime (#17)")
    }

    fn close(&self) {
        todo!("desktop cua runtime (#17)")
    }

    fn exited(&self) -> ExitFuture<'_> {
        todo!("desktop cua runtime (#17)")
    }
}

// ---------------------------------------------------------------------------
// Discovery: which driver binary the desktop runs
// ---------------------------------------------------------------------------

/// Names the driver binary explicitly, as for the server.
pub const DRIVER_ENV: &str = "NOLUNE_CUA_DRIVER";
/// The manifest `nolune cua install` writes under the workspace (#20).
pub const INSTALL_MANIFEST: &str = "cua-driver/install.json";
const DRIVER_BINARY: &str = "cua-driver";

/// The driver this desktop runs: `NOLUNE_CUA_DRIVER` when set, else the
/// driver `nolune cua install` recorded under the workspace, else an
/// executable `cua-driver` on `path`. `None` when there is none.
pub fn locate_driver(
    workspace: &Path,
    env_override: Option<&std::ffi::OsStr>,
    path: Option<&std::ffi::OsStr>,
) -> Option<PathBuf> {
    let _ = (workspace, env_override, path);
    todo!("desktop cua runtime (#17)")
}

// ---------------------------------------------------------------------------
// Describing the machine
// ---------------------------------------------------------------------------

/// The driver's unfiltered health report, decoded through the protocol's
/// envelope checks like any other action result.
pub async fn health_report(
    transport: &dyn DriverTransport,
    machine_id: &MachineId,
) -> Result<HealthReportResult, String> {
    let _ = (transport, machine_id);
    todo!("desktop cua runtime (#17)")
}

/// The descriptor this desktop registers: the driver's health report under
/// this machine's stable id, at the desktop location.
pub async fn describe_machine(
    transport: &dyn DriverTransport,
    machine_id: MachineId,
) -> Result<MachineDescriptor, String> {
    let _ = (transport, machine_id);
    todo!("desktop cua runtime (#17)")
}

/// The `cua` field of the register message: the descriptor in the
/// registration envelope the server decodes.
pub fn registration_envelope(descriptor: &MachineDescriptor) -> Value {
    let _ = descriptor;
    todo!("desktop cua runtime (#17)")
}

// ---------------------------------------------------------------------------
// The local allowlist
// ---------------------------------------------------------------------------

/// The typed request inside a `cua_request` frame, or `None` for anything
/// else on the socket (a legacy toolcall, the registration ack).
pub fn typed_request(frame: &Value) -> Option<&Value> {
    let _ = frame;
    todo!("desktop cua runtime (#17)")
}

/// Why a `cua_request` frame never reached the driver, and what to answer.
#[derive(Debug)]
pub enum Refusal {
    /// A readable request the allowlist refused: the full error envelope.
    Denied(CuaResponseEnvelope),
    /// A frame the protocol cannot read (a tool it does not name, a shape it
    /// does not accept): the answer names what the frame said about itself,
    /// so the server fails the call at once instead of at its deadline.
    Unreadable {
        request_id: Option<String>,
        machine_id: Option<String>,
        action: Option<String>,
        message: String,
    },
}

impl Refusal {
    /// The typed verdict: every refusal is a capability denial.
    pub fn error(&self) -> CuaRuntimeError {
        todo!("desktop cua runtime (#17)")
    }

    /// The `cua_response` frame to send back.
    pub fn frame(&self) -> Value {
        todo!("desktop cua runtime (#17)")
    }
}

/// Decode one inbound request through the protocol's bounds (size, depth,
/// shape, a tool the protocol names). Nothing else is ever executed.
pub fn decode_request(request: &Value) -> Result<CuaRequestEnvelope, Refusal> {
    let _ = request;
    todo!("desktop cua runtime (#17)")
}

/// The local allowlist: the request must name this machine, and the
/// descriptor must advertise the action's capability with its permissions
/// granted. Refused here, a request never touches the driver, whatever the
/// server said.
pub fn authorize(
    descriptor: &MachineDescriptor,
    request: &CuaRequestEnvelope,
) -> Result<(), Refusal> {
    let _ = (descriptor, request);
    todo!("desktop cua runtime (#17)")
}

/// The `cua_response` frame carrying `response` whole.
pub fn response_frame(response: &CuaResponseEnvelope) -> Value {
    let _ = response;
    todo!("desktop cua runtime (#17)")
}

// ---------------------------------------------------------------------------
// The runtime: one driver for the app lifetime
// ---------------------------------------------------------------------------

type SpawnFuture = Pin<Box<dyn Future<Output = Result<Arc<dyn DriverTransport>, String>> + Send>>;
type Spawner = Arc<dyn Fn() -> SpawnFuture + Send + Sync>;

/// The running driver: its transport, the descriptor it was described with,
/// and the checked adapter every admitted request executes through.
struct Driver {
    transport: Arc<dyn DriverTransport>,
    machine_id: MachineId,
    descriptor: MachineDescriptor,
    adapter: CheckedCuaAdapter,
    /// Logs the exit of a driver nobody stopped; aborted before a stop.
    watch: tokio::task::AbortHandle,
}

/// One driver for the app lifetime, started on the first connection and
/// kept across reconnects; restarted on the next request after a crash;
/// stopped when the app exits.
pub struct CuaRuntime {
    spawner: Spawner,
    driver: tokio::sync::Mutex<Option<Arc<Driver>>>,
    /// The labelled sessions the desktop confirmed open for the server and
    /// has not ended: what a disconnect or an exit has to end.
    open: Mutex<BTreeSet<SessionLabel>>,
}

impl CuaRuntime {
    /// A runtime that starts its driver through `spawner`.
    pub fn with_spawner(spawner: Spawner) -> Self {
        let _ = spawner;
        todo!("desktop cua runtime (#17)")
    }

    /// Whether a driver is running (spawned and not gone).
    pub async fn is_running(&self) -> bool {
        todo!("desktop cua runtime (#17)")
    }

    /// The descriptor to register with: starts the driver when none runs,
    /// re-reads the health of the one that does. An error means this desktop
    /// registers without a typed target.
    pub async fn start(&self, machine_id: &str) -> Result<MachineDescriptor, String> {
        let _ = machine_id;
        todo!("desktop cua runtime (#17)")
    }

    /// One inbound `cua_request` frame's `request`, answered with the
    /// `cua_response` frame to send back: refused locally, or executed
    /// through the checked adapter with its typed result forwarded unchanged.
    pub async fn handle(&self, request: &Value) -> Value {
        let _ = request;
        todo!("desktop cua runtime (#17)")
    }

    /// End every session the desktop holds open for the server: the socket
    /// is gone, so nobody will. The driver keeps running for the reconnect.
    pub async fn end_sessions(&self) {
        todo!("desktop cua runtime (#17)")
    }

    /// End the open sessions and stop the driver: the app is exiting.
    /// Idempotent and a no-op for a runtime that never started.
    pub async fn shutdown(&self) {
        todo!("desktop cua runtime (#17)")
    }

    /// The labelled sessions still open, in label order.
    #[cfg(test)]
    pub fn open_sessions(&self) -> Vec<SessionLabel> {
        todo!("desktop cua runtime (#17)")
    }
}

/// The app's one runtime: the driver from [`locate_driver`] under the
/// workspace `local_server::nolune_home` names, with the default deadlines.
pub fn runtime() -> &'static CuaRuntime {
    todo!("desktop cua runtime (#17)")
}

/// Stop the driver as the app exits, bounded so a stuck driver never holds
/// the exit: the open sessions are ended and the child is killed.
pub fn shutdown_blocking() {
    todo!("desktop cua runtime (#17)")
}

// ---------------------------------------------------------------------------
// Overlay copy
// ---------------------------------------------------------------------------

/// The action name the overlay is told: the protocol's own spelling of the
/// kind (`click`, `type_text`, `get_window_state`).
pub fn action_name(kind: CuaActionKind) -> String {
    let _ = kind;
    todo!("desktop cua runtime (#17)")
}

/// A short human detail for the overlay: what a pointer action targets, a
/// bounded preview of typed text, the app being launched. Never a secret
/// beyond what the user sees the companion do.
pub fn action_detail(action: &CuaAction) -> String {
    let _ = action;
    todo!("desktop cua runtime (#17)")
}

#[cfg(test)]
pub(crate) mod fake {
    //! An in-memory driver for tests: records every call and answers from a
    //! queue of canned outcomes.

    use super::*;

    pub struct FakeTransport {
        calls: Mutex<Vec<(String, Map<String, Value>)>>,
        outcomes: Mutex<VecDeque<CallOutcome>>,
        closed: std::sync::atomic::AtomicBool,
        /// Flipped by `close` and by `crash`: the stand-in for a child that
        /// is no longer there.
        gone: tokio::sync::watch::Sender<bool>,
    }

    impl FakeTransport {
        pub fn answering(outcomes: impl IntoIterator<Item = CallOutcome>) -> Arc<Self> {
            Arc::new(Self {
                calls: Mutex::new(Vec::new()),
                outcomes: Mutex::new(outcomes.into_iter().collect()),
                closed: std::sync::atomic::AtomicBool::new(false),
                gone: tokio::sync::watch::Sender::new(false),
            })
        }

        /// Queue another answer behind the ones already waiting.
        pub fn answer(&self, outcome: CallOutcome) {
            self.outcomes.lock().unwrap().push_back(outcome);
        }

        /// The driver child died on its own: `exited` resolves and every
        /// later call fails, while `closed` stays false.
        pub fn crash(&self) {
            self.gone.send_replace(true);
        }

        pub fn gone(&self) -> bool {
            *self.gone.borrow()
        }

        /// Every `(tool, arguments)` pair in the order it was called.
        pub fn calls(&self) -> Vec<(String, Map<String, Value>)> {
            self.calls.lock().unwrap().clone()
        }

        /// The tool names called, in order.
        pub fn tools_called(&self) -> Vec<String> {
            self.calls().into_iter().map(|(name, _)| name).collect()
        }

        /// Whether `close` was called: the stand-in for a killed child.
        pub fn closed(&self) -> bool {
            self.closed.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    impl DriverTransport for FakeTransport {
        fn call_tool(&self, name: &str, arguments: Map<String, Value>) -> TransportFuture<'_> {
            if self.gone() {
                let why = if self.closed() { "closed" } else { "exited" };
                return Box::pin(async move {
                    Err(DriverCallFailure::Transport(format!(
                        "fake driver is {why}"
                    )))
                });
            }
            self.calls
                .lock()
                .unwrap()
                .push((name.to_owned(), arguments));
            let outcome = self
                .outcomes
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| {
                    Err(DriverCallFailure::Transport(format!(
                        "fake driver has no answer for {name}"
                    )))
                });
            Box::pin(async move { outcome })
        }

        fn close(&self) {
            self.closed.store(true, std::sync::atomic::Ordering::SeqCst);
            self.gone.send_replace(true);
        }

        fn exited(&self) -> ExitFuture<'_> {
            let mut gone = self.gone.subscribe();
            Box::pin(async move {
                let _ = gone.wait_for(|gone| *gone).await;
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeTransport;
    use super::*;
    use cua_protocol::{
        driver_mcp::HEALTH_REPORT_TOOL, ActionDelivery, ActionEffect, ActionOutcome, ActionRoute,
        Capability, CaptureScope, ClickAction, ClickActionResult, ClickArgs, DeliveryMode,
        ElementAddress, EmptyArgs, EndSessionResult, LaunchAppArgs, MachineHealth, MouseButton,
        Permission, StartSessionArgs, StartSessionResult, TypeTextArgs, WindowPoint, WindowTarget,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    const STUDIO: &str = "4f3c1c2e-9b5e-4d2b-8f0a-1c2d3e4f5a6b";

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
        MachineId::try_from(STUDIO).unwrap()
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

    fn list_apps() -> CuaAction {
        CuaAction::ListApps(EmptyArgs {})
    }

    /// The driver's payload for a confirmed click, as it answers `tools/call`.
    fn clicked() -> Value {
        serde_json::to_value(ClickActionResult {
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
        .unwrap()
    }

    fn started(label: &str) -> Value {
        serde_json::to_value(StartSessionResult {
            active: true,
            capture_scope: CaptureScope::Auto,
            effective_scope: CaptureScope::Window,
            desktop_capture_authorized: true,
            desktop_unlocked: true,
            escalation_detail: None,
            escalation_reason: None,
            revived: false,
            session: Some(SessionLabel::try_from(label).unwrap()),
        })
        .unwrap()
    }

    fn ended(label: &str) -> Value {
        serde_json::to_value(EndSessionResult {
            session: Some(SessionLabel::try_from(label).unwrap()),
            active: false,
        })
        .unwrap()
    }

    fn request(request_id: &str, machine_id: &str, action: CuaAction) -> Value {
        serde_json::to_value(CuaRequestEnvelope {
            version: ProtocolVersion::V1,
            request_id: RequestId::try_from(request_id).unwrap(),
            machine_id: MachineId::try_from(machine_id).unwrap(),
            action,
        })
        .unwrap()
    }

    /// The typed answer inside a `cua_response` frame.
    fn answer(frame: &Value) -> CuaResponseEnvelope {
        assert_eq!(frame["type"], "cua_response", "{frame}");
        CuaResponseEnvelope::from_json(&frame["response"].to_string())
            .unwrap_or_else(|error| panic!("{error}: {frame}"))
    }

    fn error_of(frame: &Value) -> CuaRuntimeError {
        match answer(frame).response {
            CuaResponse::Error { error } => error,
            CuaResponse::Success { .. } => panic!("expected an error: {frame}"),
        }
    }

    /// A runtime whose spawner hands out `fakes` in order and counts spawns.
    fn runtime_over(fakes: Vec<Arc<FakeTransport>>) -> (CuaRuntime, Arc<AtomicUsize>) {
        let spawns = Arc::new(AtomicUsize::new(0));
        let queue = Arc::new(Mutex::new(VecDeque::from(fakes)));
        let counter = spawns.clone();
        let runtime = CuaRuntime::with_spawner(Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            let next = queue.lock().unwrap().pop_front();
            Box::pin(async move {
                next.map(|fake| fake as Arc<dyn DriverTransport>)
                    .ok_or_else(|| "no driver installed".to_owned())
            })
        }));
        (runtime, spawns)
    }

    #[tokio::test]
    async fn the_registration_descriptor_comes_from_the_drivers_health_report() {
        let fake = FakeTransport::answering([Ok(payload(HEALTHY))]);
        let (runtime, spawns) = runtime_over(vec![fake.clone()]);
        assert!(!runtime.is_running().await);

        let descriptor = runtime.start(STUDIO).await.unwrap();
        assert_eq!(spawns.load(Ordering::SeqCst), 1);
        assert!(runtime.is_running().await);
        assert_eq!(
            fake.calls(),
            vec![(HEALTH_REPORT_TOOL.to_owned(), Map::new())],
            "one unfiltered health_report call describes the machine"
        );
        assert_eq!(descriptor.machine_id, id());
        assert_eq!(descriptor.location, MachineLocation::Desktop);
        assert_eq!(descriptor.health, MachineHealth::Healthy);
        assert_eq!(descriptor.driver_version.as_str(), "0.28.2");
        assert_eq!(descriptor.permissions.accessibility, Permission::Granted);
        assert!(descriptor.capabilities.contains(&Capability::Pointer));
        descriptor.validate().unwrap();

        // The register message carries it the way the server decodes it.
        let envelope = registration_envelope(&descriptor);
        let decoded = CuaRegistrationEnvelope::from_json(&envelope.to_string()).unwrap();
        assert_eq!(decoded.version, ProtocolVersion::V1);
        assert_eq!(decoded.machine, descriptor);

        // A driver that cannot report leaves the desktop legacy-only.
        let silent = FakeTransport::answering([Err(DriverCallFailure::Timeout(
            "health_report did not answer".into(),
        ))]);
        let (runtime, _) = runtime_over(vec![silent.clone()]);
        let error = runtime.start(STUDIO).await.unwrap_err();
        assert!(error.contains("health_report"), "{error}");
        assert!(
            silent.closed(),
            "a driver that cannot be described is stopped"
        );
        assert!(!runtime.is_running().await);
    }

    #[tokio::test]
    async fn a_forged_tool_is_refused_locally_without_touching_the_driver() {
        let fake = FakeTransport::answering([Ok(payload(HEALTHY))]);
        let (runtime, _) = runtime_over(vec![fake.clone()]);
        runtime.start(STUDIO).await.unwrap();

        for tool in ["clipboard_read", "kill_app", "record", "screenshot"] {
            let forged = json!({
                "version": "v1", "request_id": "forged-1", "machine_id": STUDIO,
                "action": {"tool": tool, "args": {}},
            });
            let frame = runtime.handle(&forged).await;
            assert_eq!(frame["type"], "cua_response", "{tool}: {frame}");
            let response = &frame["response"];
            assert_eq!(response["request_id"], "forged-1", "{tool}: {frame}");
            assert_eq!(response["machine_id"], STUDIO, "{tool}: {frame}");
            assert_eq!(response["action"], tool, "{tool}: {frame}");
            assert_eq!(response["response"]["status"], "error", "{tool}: {frame}");
            assert_eq!(
                response["response"]["error"]["code"], "capability_denied",
                "{tool}: {frame}"
            );
            assert_eq!(response["response"]["error"]["retryable"], false);
        }

        // A readable request for another machine is refused the same way.
        let elsewhere = request("forged-2", "elsewhere", list_apps());
        let error = error_of(&runtime.handle(&elsewhere).await);
        assert_eq!(error.code, RuntimeErrorCode::CapabilityDenied);
        assert!(error.message.as_str().contains("elsewhere"), "{error:?}");

        // Garbage that names no request still gets a bounded refusal.
        let garbage = json!({"tool": "clipboard_read"});
        let frame = runtime.handle(&garbage).await;
        assert_eq!(frame["type"], "cua_response");
        assert_eq!(
            frame["response"]["response"]["error"]["code"],
            "capability_denied"
        );

        assert_eq!(
            fake.tools_called(),
            vec![HEALTH_REPORT_TOOL.to_owned()],
            "nothing refused locally reaches the driver"
        );
        assert!(runtime.is_running().await);
    }

    #[tokio::test]
    async fn a_click_while_accessibility_is_denied_is_refused_locally() {
        let fake =
            FakeTransport::answering([Ok(payload(ACCESSIBILITY_DENIED)), Ok(json!({"apps": []}))]);
        let (runtime, _) = runtime_over(vec![fake.clone()]);
        let descriptor = runtime.start(STUDIO).await.unwrap();
        assert_eq!(descriptor.health, MachineHealth::Degraded);
        assert_eq!(descriptor.permissions.accessibility, Permission::Denied);
        assert!(!descriptor.capabilities.contains(&Capability::Pointer));

        let refused = runtime.handle(&request("req-1", STUDIO, click())).await;
        let envelope = answer(&refused);
        assert_eq!(envelope.request_id.as_str(), "req-1");
        assert_eq!(envelope.action, CuaActionKind::Click);
        let error = error_of(&refused);
        assert_eq!(error.code, RuntimeErrorCode::CapabilityDenied);
        assert!(!error.retryable);
        assert_eq!(
            fake.tools_called(),
            vec![HEALTH_REPORT_TOOL.to_owned()],
            "the click never reached the driver"
        );

        // What the descriptor still allows goes through.
        let listed = runtime.handle(&request("req-2", STUDIO, list_apps())).await;
        match answer(&listed).response {
            CuaResponse::Success { result } => {
                assert!(matches!(*result, CuaActionResult::ListApps(_)))
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            fake.tools_called(),
            vec![HEALTH_REPORT_TOOL.to_owned(), "list_apps".to_owned()]
        );
    }

    #[tokio::test]
    async fn admitted_requests_use_the_shared_mapping_and_forward_results_unchanged() {
        let fake = FakeTransport::answering([
            Ok(payload(HEALTHY)),
            Ok(clicked()),
            Err(DriverCallFailure::Tool {
                code: Some("window_id_not_found".into()),
                message: "window_id 99 is not a live window".into(),
            }),
            Ok(json!({"windows": [], "unexpected": true})),
        ]);
        let (runtime, _) = runtime_over(vec![fake.clone()]);
        runtime.start(STUDIO).await.unwrap();

        // The call the driver sees is exactly what cua_protocol::driver_mcp
        // encodes: one copy of the mapping, shared with the server.
        let frame = runtime.handle(&request("req-1", STUDIO, click())).await;
        let expected = tool_call(&click()).unwrap();
        assert_eq!(
            fake.calls()[1],
            (expected.name.to_owned(), expected.arguments)
        );
        // The structured result survives untouched, correlated to the request.
        assert_eq!(frame["response"]["request_id"], "req-1");
        assert_eq!(frame["response"]["action"], "click");
        assert_eq!(frame["response"]["response"]["status"], "success");
        assert_eq!(frame["response"]["response"]["result"]["result"], clicked());
        let envelope = answer(&frame);
        let sent =
            CuaRequestEnvelope::from_json(&request("req-1", STUDIO, click()).to_string()).unwrap();
        envelope.validate_response_for(&sent).unwrap();

        // A driver error is the typed runtime error, never a panic.
        let gone = error_of(&runtime.handle(&request("req-2", STUDIO, click())).await);
        assert_eq!(gone.code, RuntimeErrorCode::TargetUnavailable);
        assert!(gone.message.as_str().contains("window_id 99"));

        // An unreadable payload fails as a driver failure.
        let windows = CuaAction::ListWindows(cua_protocol::ListWindowsArgs {
            pid: None,
            on_screen_only: true,
        });
        let unreadable = error_of(&runtime.handle(&request("req-3", STUDIO, windows)).await);
        assert_eq!(unreadable.code, RuntimeErrorCode::DriverFailure);
    }

    #[tokio::test]
    async fn sessions_the_desktop_confirmed_are_ended_on_disconnect_and_at_exit() {
        let fake = FakeTransport::answering([
            Ok(payload(HEALTHY)),
            Ok(started("nolune-run-1")),
            Ok(started("nolune-run-2")),
            Ok(ended("nolune-run-2")),
            Ok(ended("nolune-run-1")),
            Ok(payload(HEALTHY)),
            Ok(started("nolune-run-3")),
            Ok(ended("nolune-run-3")),
        ]);
        let (runtime, spawns) = runtime_over(vec![fake.clone()]);
        runtime.start(STUDIO).await.unwrap();

        let start = |label: &str| {
            CuaAction::StartSession(StartSessionArgs {
                session: Some(SessionLabel::try_from(label).unwrap()),
            })
        };
        let labels = |labels: &[&str]| -> Vec<SessionLabel> {
            labels
                .iter()
                .map(|label| SessionLabel::try_from(*label).unwrap())
                .collect()
        };
        runtime
            .handle(&request("s-1", STUDIO, start("nolune-run-1")))
            .await;
        runtime
            .handle(&request("s-2", STUDIO, start("nolune-run-2")))
            .await;
        assert_eq!(
            runtime.open_sessions(),
            labels(&["nolune-run-1", "nolune-run-2"])
        );
        // An end the server asked for is forgotten as it is confirmed.
        let end = CuaAction::EndSession(SessionRefArgs {
            session: Some(SessionLabel::try_from("nolune-run-2").unwrap()),
        });
        runtime.handle(&request("e-2", STUDIO, end)).await;
        assert_eq!(runtime.open_sessions(), labels(&["nolune-run-1"]));

        // The socket closed: what is still open is ended, the driver stays.
        runtime.end_sessions().await;
        assert_eq!(runtime.open_sessions(), Vec::<SessionLabel>::new());
        let calls = fake.calls();
        assert_eq!(calls[4].0, "end_session");
        assert_eq!(
            calls[4].1,
            json!({"session": "nolune-run-1"})
                .as_object()
                .unwrap()
                .clone()
        );
        assert!(
            !fake.closed(),
            "a disconnect keeps the driver for the reconnect"
        );
        assert!(runtime.is_running().await);
        runtime.end_sessions().await; // nothing open: no call
        assert_eq!(fake.calls().len(), 5);

        // The reconnect re-reads the health of the same driver.
        runtime.start(STUDIO).await.unwrap();
        assert_eq!(spawns.load(Ordering::SeqCst), 1, "no second driver");
        assert_eq!(fake.tools_called()[5], HEALTH_REPORT_TOOL);

        // App exit: the open session is ended and the driver is stopped.
        runtime
            .handle(&request("s-3", STUDIO, start("nolune-run-3")))
            .await;
        runtime.shutdown().await;
        assert_eq!(fake.tools_called()[7], "end_session");
        assert!(fake.closed(), "exit kills the driver");
        assert!(!runtime.is_running().await);
        assert_eq!(runtime.open_sessions(), Vec::<SessionLabel>::new());
        runtime.shutdown().await; // idempotent
        assert_eq!(fake.calls().len(), 8);

        // Nothing runs after the exit: a late frame is answered, not executed.
        let late = error_of(&runtime.handle(&request("late", STUDIO, list_apps())).await);
        assert_eq!(late.code, RuntimeErrorCode::RuntimeUnavailable);
        assert!(late.retryable);
    }

    #[tokio::test]
    async fn a_crashed_driver_is_restarted_on_the_next_request() {
        let first = FakeTransport::answering([Ok(payload(HEALTHY)), Ok(started("nolune-run-1"))]);
        let second = FakeTransport::answering([Ok(payload(HEALTHY)), Ok(json!({"apps": []}))]);
        let (runtime, spawns) = runtime_over(vec![first.clone(), second.clone()]);
        runtime.start(STUDIO).await.unwrap();
        runtime
            .handle(&request(
                "s-1",
                STUDIO,
                CuaAction::StartSession(StartSessionArgs {
                    session: Some(SessionLabel::try_from("nolune-run-1").unwrap()),
                }),
            ))
            .await;
        assert_eq!(runtime.open_sessions().len(), 1);

        first.crash();
        tokio::time::timeout(Duration::from_secs(2), async {
            while runtime.is_running().await {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("a crashed driver is noticed");
        assert!(
            runtime.open_sessions().is_empty(),
            "sessions die with the driver; nobody is left to end them"
        );

        // The next request restarts the driver under the same id and runs.
        let frame = runtime.handle(&request("req-1", STUDIO, list_apps())).await;
        assert_eq!(
            frame["response"]["response"]["status"], "success",
            "{frame}"
        );
        assert_eq!(spawns.load(Ordering::SeqCst), 2);
        assert_eq!(
            second.tools_called(),
            vec![HEALTH_REPORT_TOOL.to_owned(), "list_apps".to_owned()],
            "the restarted driver is described before it executes"
        );
        assert!(
            first.tools_called().len() == 2,
            "the dead driver saw nothing more"
        );
        assert!(runtime.is_running().await);

        // With no driver to restart, the request is answered as unavailable.
        second.crash();
        let frame = runtime.handle(&request("req-2", STUDIO, list_apps())).await;
        let error = error_of(&frame);
        assert_eq!(error.code, RuntimeErrorCode::RuntimeUnavailable);
        assert!(error.retryable);
        assert_eq!(spawns.load(Ordering::SeqCst), 3, "a restart was attempted");
        assert!(!runtime.is_running().await);
    }

    #[test]
    fn typed_frames_are_told_apart_from_legacy_toolcalls() {
        let typed = json!({"type": "cua_request", "request": {"request_id": "r-1"}});
        assert_eq!(typed_request(&typed), Some(&json!({"request_id": "r-1"})));
        for other in [
            json!({"request_id": "abc", "action": "screenshot"}),
            json!({"type": "registered", "machine_id": STUDIO, "cua": true}),
            json!({"type": "cua_request"}),
            json!({"type": "cua_request", "request": "click"}),
            json!("cua_request"),
        ] {
            assert_eq!(typed_request(&other), None, "{other}");
        }
    }

    #[test]
    fn overlay_copy_names_the_kind_and_a_bounded_detail() {
        assert_eq!(action_name(CuaActionKind::Click), "click");
        assert_eq!(action_name(CuaActionKind::TypeText), "type_text");
        assert_eq!(
            action_name(CuaActionKind::GetWindowState),
            "get_window_state"
        );
        for kind in CuaActionKind::ALL {
            let name = action_name(kind);
            assert!(
                name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'),
                "{name}"
            );
        }

        assert_eq!(action_detail(&click()), "window 99 of pid 42 at 1, 2");
        let typed = CuaAction::TypeText(TypeTextArgs {
            target: target(),
            session: None,
            delivery_mode: DeliveryMode::Background,
            address: ElementAddress::Point(WindowPoint { x: 1.0, y: 2.0 }),
            text: cua_protocol::BoundedText::try_from(
                "a rather long line of text that the overlay must cut short",
            )
            .unwrap(),
            delay_ms: 0,
        });
        let detail = action_detail(&typed);
        assert!(detail.ends_with("..."), "{detail}");
        assert!(detail.chars().count() <= 33, "{detail}");
        let launch = CuaAction::LaunchApp(LaunchAppArgs {
            bundle_id: Some(cua_protocol::AppBundleId::try_from("com.apple.Notes").unwrap()),
            name: None,
            creates_new_application_instance: false,
        });
        assert_eq!(action_detail(&launch), "com.apple.Notes");
        let named = CuaAction::LaunchApp(LaunchAppArgs {
            bundle_id: None,
            name: Some(cua_protocol::AppDisplayName::try_from("Notes").unwrap()),
            creates_new_application_instance: false,
        });
        assert_eq!(action_detail(&named), "Notes");
        assert_eq!(action_detail(&list_apps()), "");
    }

    #[test]
    fn the_driver_is_located_by_override_then_workspace_install_then_path() {
        use std::ffi::OsStr;

        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("home");
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let executable = |path: &Path| {
            std::fs::write(path, "#!/bin/sh\nexit 0\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        };

        assert_eq!(
            locate_driver(&workspace, None, None),
            None,
            "nothing anywhere"
        );

        let on_path = bin.join(DRIVER_BINARY);
        executable(&on_path);
        assert_eq!(
            locate_driver(&workspace, None, Some(OsStr::new(bin.to_str().unwrap()))),
            Some(on_path.clone())
        );

        // The workspace install beats PATH; a stale manifest is skipped.
        let install = workspace.join(INSTALL_MANIFEST);
        std::fs::create_dir_all(install.parent().unwrap()).unwrap();
        let installed = workspace.join("cua-driver/releases/0.28.2/cua-driver");
        std::fs::write(
            &install,
            json!({"version": "0.28.2", "driver": installed}).to_string(),
        )
        .unwrap();
        assert_eq!(
            locate_driver(&workspace, None, Some(OsStr::new(bin.to_str().unwrap()))),
            Some(on_path.clone()),
            "a manifest naming a missing binary is skipped"
        );
        std::fs::create_dir_all(installed.parent().unwrap()).unwrap();
        executable(&installed);
        assert_eq!(
            locate_driver(&workspace, None, Some(OsStr::new(bin.to_str().unwrap()))),
            Some(installed.clone())
        );
        assert_eq!(
            locate_driver(&workspace, None, None),
            Some(installed.clone())
        );

        // The explicit override wins, even over the install.
        let named = dir.path().join("my-driver");
        executable(&named);
        assert_eq!(
            locate_driver(&workspace, Some(named.as_os_str()), None),
            Some(named.clone())
        );
        assert_eq!(
            locate_driver(&workspace, Some(OsStr::new("")), None),
            Some(installed),
            "an empty override is no override"
        );
    }

    /// An executable shell script standing in for a driver binary, run once
    /// before it is returned: macOS scans a freshly written executable on
    /// its first run, which must not count against the deadlines under test.
    #[cfg(unix)]
    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(
            &path,
            format!("#!/bin/sh\n[ \"$1\" = warm-up ] && exit 0\n{body}"),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let warmed = loop {
            match std::process::Command::new(&path).arg("warm-up").status() {
                Ok(status) => break status,
                Err(error)
                    if error.kind() == std::io::ErrorKind::ExecutableFileBusy
                        && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(error) => panic!("{}: {error}", path.display()),
            }
        };
        assert!(warmed.success(), "{}", path.display());
        path
    }

    /// A stdio MCP server that completes the `initialize` handshake and
    /// answers every `tools/call` with the healthy report: enough of a
    /// driver to be described, started and stopped.
    #[cfg(unix)]
    fn reporting_server(report: &Path) -> String {
        format!(
            r#"
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"protocolVersion":"2025-03-26","capabilities":{{"tools":{{}}}},"serverInfo":{{"name":"stub","version":"0"}}}}}}\n' "$id"
      ;;
    *'"method":"tools/call"'*)
      id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"content":[{{"type":"text","text":"ok"}}],"structuredContent":' "$id"
      cat '{}'
      printf '}}}}\n'
      ;;
  esac
done
"#,
            report.display()
        )
    }

    /// Whether a process with this id still exists (a reaped one does not).
    #[cfg(unix)]
    fn process_exists(pid: &str) -> bool {
        std::process::Command::new("kill")
            .args(["-0", pid])
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    #[cfg(unix)]
    async fn wait_until_gone(pid: &str) {
        let gone_by = std::time::Instant::now() + Duration::from_secs(5);
        while process_exists(pid) && std::time::Instant::now() < gone_by {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn reconnects_reuse_one_driver_child_and_exit_kills_it() {
        let dir = tempfile::tempdir().unwrap();
        let report = dir.path().join("health.json");
        std::fs::write(&report, HEALTHY).unwrap();
        let pid_file = dir.path().join("pid");
        let stub = script(
            dir.path(),
            "reporting-driver",
            &format!(
                "echo $$ > '{}'\n{}",
                pid_file.display(),
                reporting_server(&report)
            ),
        );
        let timeouts = DriverTimeouts {
            handshake: Duration::from_secs(10),
            call: Duration::from_secs(5),
        };
        let runtime = CuaRuntime::with_spawner(Arc::new(move || {
            let stub = stub.clone();
            Box::pin(async move {
                StdioDriverTransport::spawn_with(&stub, timeouts)
                    .await
                    .map(|transport| Arc::new(transport) as Arc<dyn DriverTransport>)
            })
        }));

        let descriptor = runtime.start(STUDIO).await.unwrap();
        assert_eq!(descriptor.location, MachineLocation::Desktop);
        assert_eq!(descriptor.health, MachineHealth::Healthy);
        let pid = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .to_owned();
        assert!(process_exists(&pid), "the child runs after the handshake");

        // A disconnect and two reconnects: the same child, no second one.
        runtime.end_sessions().await;
        runtime.start(STUDIO).await.unwrap();
        runtime.start(STUDIO).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(&pid_file).unwrap().trim(),
            pid,
            "reconnecting never spawns a second driver"
        );
        assert!(process_exists(&pid));

        // App exit: the child is gone, and a later start would spawn anew.
        runtime.shutdown().await;
        wait_until_gone(&pid).await;
        assert!(!process_exists(&pid), "child {pid} outlived shutdown()");
        assert!(!runtime.is_running().await);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn deadlines_bound_the_handshake_and_every_call() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        let silent = script(
            dir.path(),
            "silent-driver",
            &format!("echo $$ > '{}'\nexec sleep 60\n", pid_file.display()),
        );
        let timeouts = DriverTimeouts {
            handshake: Duration::from_millis(500),
            call: Duration::from_millis(500),
        };
        let started = std::time::Instant::now();
        let error = StdioDriverTransport::spawn_with(&silent, timeouts)
            .await
            .err()
            .expect("a child that never answers initialize is not a driver");
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(error.contains("handshake"), "{error}");
        assert!(error.contains("silent-driver"), "names the driver: {error}");
        let pid = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .to_owned();
        wait_until_gone(&pid).await;
        assert!(
            !process_exists(&pid),
            "child {pid} outlived the failed handshake"
        );

        // A driver that answers the handshake and nothing else: the health
        // report times out, the runtime reports it and stops the child.
        let handshake_only = script(
            dir.path(),
            "stalled-driver",
            r#"
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"stub","version":"0"}}}\n' "$id"
      ;;
  esac
done
"#,
        );
        let transport = StdioDriverTransport::spawn_with(&handshake_only, timeouts)
            .await
            .expect("the stub completes the handshake");
        let started = std::time::Instant::now();
        let outcome = transport.call_tool("health_report", Map::new()).await;
        assert!(started.elapsed() < Duration::from_secs(5));
        match outcome {
            Err(DriverCallFailure::Timeout(message)) => {
                assert!(message.contains("health_report"), "{message}");
            }
            other => panic!("expected a timeout, got {other:?}"),
        }
        let still_running =
            tokio::time::timeout(Duration::from_millis(100), transport.exited()).await;
        assert!(still_running.is_err(), "a slow driver is still there");
        transport.close();
        tokio::time::timeout(Duration::from_secs(5), transport.exited())
            .await
            .expect("close is an exit as well");
        let after = transport.call_tool("health_report", Map::new()).await;
        assert!(after.is_err(), "a closed transport answers nothing");
    }
}
