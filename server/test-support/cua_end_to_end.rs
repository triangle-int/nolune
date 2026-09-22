//! End-to-end scenarios for Cua computer use (#21), the last child of #12.
//!
//! Included from `app/router.rs`. Every scenario runs the way a chat turn
//! does: the typed machine tools (`server/src/services/tools/cua.rs`) are
//! built over the state's registry and called through `ToolDyn` with JSON
//! arguments, so argument decoding, target resolution, the orchestrator's
//! ledger and gate, the transports and the rendering are all on the path.
//! Nothing here runs a driver. The server-local target is a
//! `FakeTransport` attached to the state's runtime, and a desktop target is
//! a fake app on the machine WebSocket answering `cua_request` frames with
//! canned driver payloads. One test per bullet of the issue, each proving
//! the refusal as well as the success.

use super::*;
use crate::{
    config::Config,
    services::{
        cua::{
            host::{DisplaySession, HostProbe, PlatformSupport, Skip, server_local_machine_id},
            runtime::{CuaRuntime, RuntimeStatus},
            transport::{CallOutcome, fake::FakeTransport},
        },
        tool::ToolDyn,
        tools::{
            computer::{ListMachinesTool, MachineTarget, SERVER_HOME_TARGET},
            cua::{
                ActTool, CaptureStore, CuaTools, DiscoverWindowsTool, GetWindowStateTool,
                VerifyStateTool,
            },
            tool_trail_line,
        },
    },
};
use axum::{
    body::Body,
    http::{Method, Request, StatusCode, header},
};
use cua_protocol::{
    Capability, CuaAction, CuaRegistrationEnvelope, CuaRequestEnvelope, DriverVersion,
    MachineDescriptor, MachineHealth, MachineId, MachineLocation, Permission, PermissionState,
    Platform, ProtocolVersion,
    driver_mcp::{DriverCallFailure, response_for},
};
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{collections::BTreeSet, net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message, client::IntoClientRequest},
};
use tower::ServiceExt;

const TOKEN: &str = "issue-21-machine-token";
const MAX_BODY: usize = 64 * 1024 * 1024;
/// The desktop's stable id.
const STUDIO: &str = "4f3c1c2e-9b5e-4d2b-8f0a-1c2d3e4f5a6b";
/// The server machine's hostname; its target is `server-local:home-mini`.
const HOST: &str = "home-mini";
/// `[cua].call_timeout_secs`: long enough for a fake that answers, short
/// enough that "fails now, not at the deadline" means something.
const CALL_TIMEOUT_SECS: u64 = 4;
const WAIT: Duration = Duration::from_secs(5);
/// The default install: captures are inlined, never fetched by URL.
const PUBLIC_URL: &str = "http://localhost:26559";
const PID: u32 = 42;
const WINDOW: u64 = 99;

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

// ═══════════════════════════════════════════════════════════════════════════
// The server
// ═══════════════════════════════════════════════════════════════════════════

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

/// The router on a loopback port over a temporary workspace. Building the
/// state never starts a driver, so the only Cua targets are the ones a test
/// attaches or registers.
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

    /// Every row of `GET /machines`; the server answers whatever the
    /// targets are, which is what a headless server has to keep doing.
    async fn known_rows(&self) -> Vec<Value> {
        let (status, body) = self
            .json(Method::GET, "/api/instances/companion/machines")
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body["machines"].as_array().unwrap().clone()
    }

    /// The one row for `machine_id`.
    async fn known_row(&self, machine_id: &str) -> Value {
        let rows = self.known_rows().await;
        let matching: Vec<&Value> = rows
            .iter()
            .filter(|row| row["machine_id"] == machine_id)
            .collect();
        assert_eq!(matching.len(), 1, "one row for {machine_id}: {rows:?}");
        matching[0].clone()
    }

    /// What `list_machines` prints, through the tool layer.
    async fn list_machines(&self) -> String {
        let tool: Box<dyn ToolDyn> =
            Box::new(ListMachinesTool::new(self.state.machine_registry.clone()));
        let raw = tool.call("{}".into()).await.unwrap();
        serde_json::from_str(&raw).unwrap()
    }

    /// The `list_machines` entries, or none when nothing is connected.
    async fn listed(&self) -> Vec<Value> {
        let listing = self.list_machines().await;
        serde_json::from_str(&listing).unwrap_or_default()
    }

    /// The entry for `machine_id` that carries driver fields, if any.
    async fn typed_entry(&self, machine_id: &str) -> Option<Value> {
        self.listed()
            .await
            .into_iter()
            .find(|entry| entry["machine_id"] == machine_id && entry.get("health").is_some())
    }

    /// The typed tools of one chat turn with `chosen` as the composer's
    /// choice: the same construction `build_tools` performs, over the
    /// state's registry, with the turn's own orchestrator.
    async fn turn(&self, chosen: Option<&str>) -> Tools {
        let registry = self.state.machine_registry.clone();
        let target = MachineTarget::resolve(&registry, chosen).await;
        let shared = CuaTools::new(
            registry,
            target.clone(),
            CaptureStore::new(
                &self.state.workspace_dir,
                "companion",
                PUBLIC_URL,
                &self.state.resources,
            ),
        );
        Tools {
            discover: Arc::new(DiscoverWindowsTool::new(shared.clone())),
            state: Arc::new(GetWindowStateTool::new(shared.clone())),
            act: Arc::new(ActTool::new(shared.clone())),
            verify: Arc::new(VerifyStateTool::new(shared)),
            target,
        }
    }

    /// A server-local runtime over the state's targets, attached to a fake
    /// driver that answers `report` to the registration's health report and
    /// then `answers`, in order. The state's own runtime is used for the
    /// discovery scenario; later attachments need a runtime of their own,
    /// as the registry keeps a handle on the latest.
    async fn attach_server_local(
        &self,
        report: &str,
        answers: Vec<Value>,
    ) -> (CuaRuntime, Arc<FakeTransport>) {
        let runtime = CuaRuntime::new(
            self.state.config.read().await.cua.clone(),
            self.state.machine_registry.cua().clone(),
            self.state.workspace_dir.clone(),
        );
        let transport = attach(&runtime, report, answers).await;
        (runtime, transport)
    }
}

/// Attach a fake driver to `runtime` as the server-local target.
async fn attach(runtime: &CuaRuntime, report: &str, answers: Vec<Value>) -> Arc<FakeTransport> {
    let mut outcomes: Vec<CallOutcome> = vec![Ok(serde_json::from_str(report).unwrap())];
    outcomes.extend(answers.into_iter().map(Ok));
    let transport = Arc::new(FakeTransport::answering(outcomes));
    runtime
        .attach(transport.clone(), server_local_machine_id(HOST), HOST, None)
        .await
        .unwrap();
    transport
}

fn server_local_id() -> String {
    server_local_machine_id(HOST).as_str().to_owned()
}

// ═══════════════════════════════════════════════════════════════════════════
// The tools of a turn
// ═══════════════════════════════════════════════════════════════════════════

struct Tools {
    discover: Arc<dyn ToolDyn>,
    state: Arc<dyn ToolDyn>,
    act: Arc<dyn ToolDyn>,
    verify: Arc<dyn ToolDyn>,
    /// The turn's resolved target, for the trail line the loop persists.
    target: MachineTarget,
}

type Call = tokio::task::JoinHandle<Result<String, String>>;

impl Tools {
    /// Call a tool the way the agent loop does, with JSON arguments, off
    /// this task so the test can play the desktop meanwhile.
    fn call(&self, tool: &Arc<dyn ToolDyn>, args: Value) -> Call {
        let tool = tool.clone();
        tokio::spawn(async move {
            tool.call(args.to_string())
                .await
                .map_err(|error| error.to_string())
        })
    }

    fn discover(&self, args: Value) -> Call {
        self.call(&self.discover, args)
    }

    fn observe(&self, args: Value) -> Call {
        self.call(&self.state, args)
    }

    fn act(&self, args: Value) -> Call {
        self.call(&self.act, args)
    }

    fn verify(&self, args: Value) -> Call {
        self.call(&self.verify, args)
    }

    /// The trail line the loop persists with a call of `tool`.
    fn trail(&self, tool: &str, args: &Value) -> String {
        tool_trail_line(tool, &args.to_string(), &self.target).unwrap()
    }
}

/// A finished call's rendered result: the tool's JSON document (or its
/// blocks), decoded from the string the tool layer hands the model.
async fn rendered(call: Call) -> Value {
    let raw = call
        .await
        .unwrap()
        .unwrap_or_else(|error| panic!("{error}"));
    let text: String = serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{e}: {raw}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text}"))
}

/// A finished call's error, which must start with `code`.
async fn refused(call: Call, code: &str) -> String {
    let error = match call.await.unwrap() {
        Err(error) => error,
        Ok(output) => panic!("expected a {code} refusal, got {output}"),
    };
    let message = error
        .strip_prefix("ToolCallError: ")
        .unwrap_or(&error)
        .to_owned();
    assert!(
        message.starts_with(&format!("{code}: ")),
        "expected a {code} refusal, got {message}"
    );
    message
}

/// A call refused before its arguments made sense to the tool: the JSON
/// never decoded, so nothing was resolved and nothing was sent.
async fn unreadable(call: Call) -> String {
    let error = match call.await.unwrap() {
        Err(error) => error,
        Ok(output) => panic!("expected an argument error, got {output}"),
    };
    assert!(error.starts_with("JsonError: "), "{error}");
    error
}

fn target_args() -> Value {
    json!({"pid": PID, "window_id": WINDOW})
}

fn observe_args(machine_id: Option<&str>, screenshot: bool) -> Value {
    let mut args = json!({"target": target_args(), "include_screenshot": screenshot});
    if let Some(id) = machine_id {
        args["machine_id"] = json!(id);
    }
    args
}

fn click_token(token: &str) -> Value {
    json!({
        "target": target_args(),
        "action": {"kind": "click", "address": {"kind": "element_token", "element_token": token}},
    })
}

fn click_point(x: f64, y: f64) -> Value {
    json!({
        "target": target_args(),
        "action": {"kind": "click", "address": {"kind": "point", "x": x, "y": y}},
    })
}

fn window_exists() -> Value {
    json!({"expect": [{"predicate": "window_exists", "value": true}]})
}

fn with_verify(mut act: Value) -> Value {
    act["verify"] = window_exists();
    act
}

// ═══════════════════════════════════════════════════════════════════════════
// Driver payloads
// ═══════════════════════════════════════════════════════════════════════════

fn element(index: u32, token: &str, role: &str, label: &str) -> Value {
    json!({
        "element_index": index, "element_token": token, "role": role, "label": label,
        "value": "", "actions": ["AXPress"], "enabled": true,
        "frame": {"x": 10.0, "y": 20.0, "width": 80.0, "height": 24.0}, "depth": 0
    })
}

fn tiny_png() -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .encode(include_bytes!("../../cua-protocol/tests/fixtures/tiny.png"))
}

/// A healthy window state issuing `elements` under `snapshot`.
fn state(snapshot: &str, elements: Vec<Value>, screenshot: bool) -> Value {
    let mut state = json!({
        "target": target_args(),
        "snapshot_id": snapshot,
        "elements": elements,
        "truncated": false,
        "app_name": "Notes",
        "window_title": "Groceries",
        "element_count": elements.len(),
    });
    if screenshot {
        state["screenshot"] = json!({
            "media_type": "png", "base64": tiny_png(), "width": 1, "height": 1,
        });
    }
    state
}

/// The driver's own degraded observation: the accessibility surface of the
/// window could not be resolved, so no snapshot and no tokens.
fn degraded(screenshot: bool) -> Value {
    let fixture = include_str!("../../cua-protocol/tests/fixtures/degraded-window-state.json");
    let mut state: Value = serde_json::from_str(fixture).unwrap();
    state["target"] = target_args();
    state["background_input"]["exact_window"]["pid"] = json!(PID);
    state["background_input"]["exact_window"]["window_id"] = json!(WINDOW);
    if screenshot {
        state["screenshot"] = json!({
            "media_type": "png", "base64": tiny_png(), "width": 1, "height": 1,
        });
    }
    state
}

fn confirmed() -> Value {
    json!({
        "effect": "confirmed", "route": "accessibility",
        "delivery": {"requested": "background", "delivered_count": 1},
        "evidence": ["accessibility_readback"],
    })
}

fn verification(overall: &str) -> Value {
    json!({"overall": overall, "predicates": [{"predicate_index": 0, "status": overall}]})
}

/// An action result in the driver's flat shape: the request's own
/// arguments echoed back with `outcome`, the way the driver answers.
fn echo(request: &CuaRequestEnvelope, outcome: Value) -> Value {
    let mut value = serde_json::to_value(&request.action).unwrap();
    let mut args = value["args"].take();
    let object = args.as_object_mut().unwrap();
    object.remove("delivery_mode");
    object.remove("modifiers");
    object.insert("outcome".into(), outcome);
    args
}

// ═══════════════════════════════════════════════════════════════════════════
// The desktop
// ═══════════════════════════════════════════════════════════════════════════

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

    /// A desktop registered with `descriptor` as its typed target.
    async fn registered(addr: SocketAddr, descriptor: MachineDescriptor) -> Self {
        let (desktop, ack) = Self::connect(addr, register(Some(cua_field(descriptor)))).await;
        assert_eq!(ack["type"], "registered", "{ack}");
        assert_eq!(ack["cua"], true, "{ack}");
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

    /// The next `cua_request` frame, decoded as the desktop would.
    async fn next_request(&mut self) -> CuaRequestEnvelope {
        let frame = self.next_frame().await;
        assert_eq!(frame["type"], "cua_request", "{frame}");
        CuaRequestEnvelope::from_json(&frame["request"].to_string()).unwrap()
    }

    /// Answer the next `cua_request` frame with the driver payload `answer`
    /// builds for it (through the same decoding a desktop applies), and
    /// return the request.
    async fn answer_next(
        &mut self,
        answer: impl FnOnce(&CuaRequestEnvelope) -> Value,
    ) -> CuaRequestEnvelope {
        let request = self.next_request().await;
        let response = response_for(&request, Ok(answer(&request)));
        self.send(json!({"type": "cua_response", "response": response}))
            .await;
        request
    }

    /// Answer the next `cua_request` with the driver's own error.
    async fn fail_next(&mut self, failure: DriverCallFailure) -> CuaRequestEnvelope {
        let request = self.next_request().await;
        let response = response_for(&request, Err(failure));
        self.send(json!({"type": "cua_response", "response": response}))
            .await;
        request
    }

    /// Nothing reached the desktop: a refusal happened before the wire.
    async fn saw_nothing(&mut self) {
        match tokio::time::timeout(Duration::from_millis(300), self.ws.next()).await {
            Err(_) => {}
            Ok(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => {}
            Ok(other) => panic!("the desktop was reached: {other:?}"),
        }
    }
}

/// A descriptor advertising every capability with both grants.
fn descriptor(machine_id: &str) -> MachineDescriptor {
    MachineDescriptor {
        machine_id: MachineId::try_from(machine_id).unwrap(),
        location: MachineLocation::Desktop,
        platform: Platform::Macos,
        driver_version: DriverVersion::try_from("0.28.2").unwrap(),
        health: MachineHealth::Healthy,
        permissions: PermissionState {
            accessibility: Permission::Granted,
            screen_capture: Permission::Granted,
        },
        capabilities: vec![
            Capability::AppDiscovery,
            Capability::AppLaunch,
            Capability::WindowDiscovery,
            Capability::WindowObservation,
            Capability::WindowManagement,
            Capability::Pointer,
            Capability::Keyboard,
            Capability::ElementValue,
            Capability::Menu,
            Capability::Verification,
            Capability::SessionLifecycle,
            Capability::Health,
        ],
    }
}

/// The descriptor a driver advertises with `accessibility` and
/// `screen_capture` as given: the capabilities a permission that is not
/// granted would refuse anyway are left out, as `descriptor_from_health`
/// leaves them out.
fn descriptor_with(accessibility: Permission, screen_capture: Permission) -> MachineDescriptor {
    let mut descriptor = descriptor(STUDIO);
    descriptor.permissions = PermissionState {
        accessibility,
        screen_capture,
    };
    if accessibility != Permission::Granted {
        descriptor.health = MachineHealth::Degraded;
        descriptor.capabilities.retain(|capability| {
            !matches!(
                capability,
                Capability::WindowManagement
                    | Capability::Pointer
                    | Capability::Keyboard
                    | Capability::ElementValue
                    | Capability::Menu
                    | Capability::Verification
            )
        });
    }
    if screen_capture != Permission::Granted {
        descriptor.health = MachineHealth::Degraded;
    }
    descriptor
}

fn cua_field(descriptor: MachineDescriptor) -> Value {
    serde_json::to_value(CuaRegistrationEnvelope {
        version: ProtocolVersion::V1,
        machine: descriptor,
    })
    .unwrap()
}

/// The register message a desktop from this release sends.
fn register(cua: Option<Value>) -> Value {
    let mut frame = json!({
        "type": "register",
        "machine_id": STUDIO,
        "os": "macos",
        "hostname": "studio",
        "capabilities": crate::domain::machine::DESKTOP_TOOLCALLS,
    });
    if let Some(cua) = cua {
        frame["cua"] = cua;
    }
    frame
}

fn started(n: u64) -> Value {
    json!({
        "active": true, "capture_scope": "auto", "effective_scope": "window",
        "desktop_capture_authorized": true, "desktop_unlocked": true,
        "revived": false, "session": format!("nolune-run-{n}")
    })
}

fn ended(n: u64) -> Value {
    json!({"session": format!("nolune-run-{n}"), "active": false})
}

/// Yield until `condition` holds, or fail after a bounded number of turns.
async fn eventually<F, Fut>(what: &str, mut condition: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..2000 {
        if condition().await {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("{what} never happened");
}

// ═══════════════════════════════════════════════════════════════════════════
// The scenarios
// ═══════════════════════════════════════════════════════════════════════════

/// Server-local machine discovery: the state's own runtime registers the
/// server machine from the driver's health report, the listings and the
/// typed tools see it as the server home, and a driver that cannot be
/// described registers nothing while the server keeps serving.
#[tokio::test]
async fn server_local_machine_discovery() {
    let h = harness().await;
    let local = server_local_id();

    // Nothing registered: the listings are empty, the server answers, and
    // the typed tools say there is nothing to drive.
    assert_eq!(h.state.cua.status().await, RuntimeStatus::NotStarted);
    assert!(h.list_machines().await.contains("No machines connected"));
    assert!(h.known_rows().await.is_empty());
    let tools = h.turn(None).await;
    let message = refused(
        tools.discover(json!({"mode": "list_apps"})),
        "no_cua_target",
    )
    .await;
    assert!(message.contains("driver"), "{message}");

    // A driver whose health report cannot be read is closed and reported;
    // nothing is listed and the server stays up.
    let broken = Arc::new(FakeTransport::answering([Err(
        DriverCallFailure::Transport("the driver closed its pipe".into()),
    )]));
    assert!(
        h.state
            .cua
            .attach(broken.clone(), server_local_machine_id(HOST), HOST, None)
            .await
            .is_err()
    );
    assert!(broken.closed(), "an undescribable driver is stopped");
    assert!(matches!(
        h.state.cua.status().await,
        RuntimeStatus::Failed(_)
    ));
    assert!(h.known_rows().await.is_empty());
    assert!(h.list_machines().await.contains("No machines connected"));

    // A healthy driver registers the server machine under its reserved id.
    let transport = attach(
        &h.state.cua,
        HEALTHY,
        vec![started(1), json!({"apps": []}), ended(1)],
    )
    .await;
    assert_eq!(
        h.state.cua.status().await,
        RuntimeStatus::Running(server_local_machine_id(HOST))
    );
    let entry = h
        .typed_entry(&local)
        .await
        .unwrap_or_else(|| panic!("no server-local entry: {}", local));
    assert_eq!(entry["location"], "server_local");
    assert_eq!(entry["os"], "macos");
    assert_eq!(entry["driver_version"], "0.28.2");
    assert_eq!(entry["health"], "healthy");
    assert_eq!(
        entry["permissions"],
        json!({"accessibility": "granted", "screen_capture": "granted"})
    );
    let capabilities: BTreeSet<String> =
        serde_json::from_value(entry["capabilities"].clone()).unwrap();
    for needed in [
        "app_discovery",
        "window_observation",
        "pointer",
        "verification",
    ] {
        assert!(capabilities.contains(needed), "{capabilities:?}");
    }
    let row = h.known_row(&local).await;
    assert_eq!(row["location"], "server_local");
    assert_eq!(row["online"], true);
    assert_eq!(row["hostname"], HOST);
    assert_eq!(row["driver_version"], "0.28.2");
    assert_eq!(row["cua_health"], "healthy");

    // With nothing chosen the server machine is the only Cua target, so the
    // typed tools drive it as one run each: session opened, the action,
    // session ended. The result and the trail call it the server home.
    let tools = h.turn(None).await;
    let args = json!({"mode": "list_apps"});
    let apps = rendered(tools.discover(args.clone())).await;
    assert_eq!(apps["apps"], json!([]));
    assert_eq!(apps["machine"], "the server home", "{apps}");
    assert_eq!(
        transport.tools_called(),
        ["health_report", "start_session", "list_apps", "end_session"]
    );
    assert_eq!(
        tools.trail("discover_windows", &args),
        "listing apps on the server home"
    );
    assert_eq!(
        tools.trail(
            "get_window_state",
            &json!({"machine_id": local, "target": target_args()})
        ),
        "observing a window on the server home"
    );

    // Choosing the home, or naming the server-local id, is the same target.
    let home = h.turn(Some(SERVER_HOME_TARGET)).await;
    assert_eq!(
        home.trail(
            "verify_state",
            &json!({"target": target_args(), "expect": []})
        ),
        "verifying a window on the server home"
    );
    let by_id = h.turn(Some(&local)).await;
    assert_eq!(
        by_id.trail("act", &json!({"action": {"kind": "click"}})),
        "click on the server home"
    );

    // Shutdown ends the run's driver and drops the target: nothing is
    // listed and the tools say the home has no driver now.
    h.state.cua.shutdown().await;
    assert!(transport.closed());
    assert_eq!(h.state.cua.status().await, RuntimeStatus::Stopped);
    assert!(h.typed_entry(&local).await.is_none());
    assert!(h.known_rows().await.is_empty());
    let message = refused(
        home.discover(json!({"mode": "list_apps"})),
        "no_server_local_target",
    )
    .await;
    assert!(message.contains("run_command"), "{message}");
}

/// Remote desktop machine discovery: a desktop registers its typed target
/// over the machine WebSocket and is driven through it, a desktop without
/// a descriptor is a legacy-only computer the typed tools refuse by name,
/// and a descriptor for another machine or the server location refuses the
/// whole registration.
#[tokio::test]
async fn remote_desktop_machine_discovery() {
    let h = harness().await;

    // No descriptor: listed without driver fields, and the typed tools
    // refuse the chosen desktop by name without touching the socket.
    let (mut legacy, ack) = FakeDesktop::connect(h.addr, register(None)).await;
    assert_eq!(ack["type"], "registered");
    assert_eq!(ack["cua"], false, "{ack}");
    assert!(h.typed_entry(STUDIO).await.is_none());
    assert_eq!(h.listed().await.len(), 1);
    let tools = h.turn(Some(STUDIO)).await;
    let message = refused(
        tools.discover(json!({"mode": "list_apps"})),
        "no_cua_driver",
    )
    .await;
    assert!(
        message.contains("studio")
            && message.contains("Install driver")
            // A desktop has no `nolune` on its PATH, so the refusal must
            // never answer with one (#231).
            && !message.contains("nolune cua"),
        "{message}"
    );
    legacy.saw_nothing().await;
    legacy.close().await;
    eventually("the legacy desktop leaves the listing", || async {
        h.state.machine_registry.list().await.is_empty()
    })
    .await;

    // A descriptor that names another machine or claims the server
    // location refuses the registration and closes the socket.
    let mut elsewhere = descriptor("elsewhere");
    elsewhere.machine_id = MachineId::try_from("elsewhere").unwrap();
    let mut local = descriptor(STUDIO);
    local.location = MachineLocation::ServerLocal;
    for bad in [elsewhere, local] {
        let (mut desktop, first) =
            FakeDesktop::connect(h.addr, register(Some(cua_field(bad)))).await;
        assert_eq!(first["type"], "error", "{first}");
        assert_eq!(first["error"], "invalid_cua_registration");
        assert!(desktop.closed().await);
    }
    assert!(h.state.machine_registry.cua().list().await.is_empty());
    assert!(h.list_machines().await.contains("No machines connected"));

    // The typed registration: one entry with the driver's fields, one
    // online row, and a request that travels as a `cua_request` frame.
    let mut desktop = FakeDesktop::registered(h.addr, descriptor(STUDIO)).await;
    let entry = h.typed_entry(STUDIO).await.expect("a typed entry");
    assert_eq!(entry["location"], "desktop");
    assert_eq!(entry["hostname"], "studio");
    assert_eq!(entry["driver_version"], "0.28.2");
    assert_eq!(entry["health"], "healthy");
    assert_eq!(entry["permissions"]["accessibility"], "granted");
    let row = h.known_row(STUDIO).await;
    assert_eq!(row["location"], "desktop");
    assert_eq!(row["online"], true);
    assert_eq!(row["driver_version"], "0.28.2");
    assert_eq!(row["cua_health"], "healthy");

    let tools = h.turn(None).await;
    let args = json!({"mode": "list_apps"});
    let call = tools.discover(args.clone());
    let request = desktop.answer_next(|_| json!({"apps": []})).await;
    assert_eq!(request.machine_id.as_str(), STUDIO);
    assert!(matches!(request.action, CuaAction::ListApps(_)));
    let apps = rendered(call).await;
    assert_eq!(apps["machine"], "studio", "named by its hostname: {apps}");
    assert_eq!(
        tools.trail("discover_windows", &args),
        "listing apps on studio"
    );

    // Naming a computer nobody has is refused by that id; the desktop is
    // not asked.
    let message = refused(
        tools.discover(
            json!({"mode": "list_apps", "machine_id": "9a8b7c6d-5e4f-4a3b-9c2d-1e0f9a8b7c6d"}),
        ),
        "machine_unavailable",
    )
    .await;
    assert!(message.contains("9a8b7c6d"), "{message}");
    desktop.saw_nothing().await;
    desktop.close().await;
}

/// Snapshot, element action, verification on a background window: the
/// observation issues tokens, the click carries one in the background, the
/// verification the model asked for runs right after it, and the result is
/// reported only when it is satisfied. Acting before observing, acting
/// twice on one snapshot, and an unsatisfied verification are refusals.
#[tokio::test]
async fn snapshot_element_action_and_verification_on_a_background_window() {
    let h = harness().await;
    let mut desktop = FakeDesktop::registered(h.addr, descriptor(STUDIO)).await;
    let tools = h.turn(None).await;

    // Nothing observed yet: the click never reaches the desktop.
    let message = refused(tools.act(click_token("tok/a")), "snapshot_required").await;
    assert!(message.contains("get_window_state"), "{message}");
    desktop.saw_nothing().await;

    // Observe: a tree, no capture unless asked for.
    let call = tools.observe(observe_args(None, false));
    let request = desktop
        .answer_next(|_| {
            state(
                "s00000001",
                vec![element(0, "tok/a", "AXButton", "Add item")],
                false,
            )
        })
        .await;
    let CuaAction::GetWindowState(observation) = request.action else {
        panic!("expected get_window_state, got {:?}", request.action.kind());
    };
    assert!(observation.include_accessibility_tree);
    assert!(!observation.include_screenshot, "no capture unless asked");
    let observed = rendered(call).await;
    assert_eq!(observed["snapshot_id"], "s00000001");
    assert_eq!(observed["elements"][0][1], "tok/a");
    assert_eq!(observed["pixel_addresses"]["allowed"], false);
    assert!(observed.get("screenshot").is_none(), "{observed}");
    assert_eq!(
        tools.trail("get_window_state", &observe_args(None, false)),
        "observing a window on studio"
    );

    // Act with predicates: the click goes out in the background addressed
    // by the token, then the verification, and the report says verified.
    let call = tools.act(with_verify(click_token("tok/a")));
    let click = desktop
        .answer_next(|request| echo(request, confirmed()))
        .await;
    let CuaAction::Click(args) = &click.action else {
        panic!("expected click, got {:?}", click.action.kind());
    };
    assert_eq!(args.delivery_mode, cua_protocol::DeliveryMode::Background);
    assert!(matches!(
        &args.address,
        cua_protocol::ElementAddress::ElementToken { element_token } if element_token.as_str() == "tok/a"
    ));
    assert_eq!(args.target.pid, PID);
    let verify = desktop.answer_next(|_| verification("satisfied")).await;
    assert!(matches!(verify.action, CuaAction::VerifyState(_)));
    let acted = rendered(call).await;
    assert_eq!(acted["verified"], true, "{acted}");
    assert_eq!(acted["outcome"]["effect"], "confirmed");
    assert_eq!(acted["outcome"]["delivery"], "background");
    assert_eq!(acted["verification"]["overall"], "satisfied");
    assert_eq!(tools.trail("act", &click_token("tok/a")), "click on studio");

    // The snapshot is consumed by the action: the next one needs a fresh
    // observation, and the desktop is not asked.
    let message = refused(tools.act(click_token("tok/a")), "snapshot_consumed").await;
    assert!(message.contains("get_window_state"), "{message}");
    desktop.saw_nothing().await;

    // Observe again; an unsatisfied verification fails the action, and the
    // standalone verification fails the same way.
    let call = tools.observe(observe_args(None, false));
    desktop
        .answer_next(|_| {
            state(
                "s00000002",
                vec![element(0, "tok/b", "AXButton", "Add item")],
                false,
            )
        })
        .await;
    rendered(call).await;
    let call = tools.act(with_verify(click_token("tok/b")));
    desktop
        .answer_next(|request| echo(request, confirmed()))
        .await;
    desktop.answer_next(|_| verification("unsatisfied")).await;
    let message = refused(call, "verification_failed").await;
    assert!(message.contains("not reported as done"), "{message}");

    let mut verify_args = window_exists();
    verify_args["target"] = target_args();
    let call = tools.verify(verify_args.clone());
    desktop.answer_next(|_| verification("unsatisfied")).await;
    refused(call, "verification_failed").await;
    let call = tools.verify(verify_args);
    desktop.answer_next(|_| verification("satisfied")).await;
    let verified = rendered(call).await;
    assert_eq!(verified["overall"], "satisfied");
    desktop.close().await;
}

/// Pixel fallback only after a real signal: a point address is refused
/// while the window's accessibility route works, refused without a capture
/// to read it from, and forwarded only once the driver reported the surface
/// unavailable, or a verification there failed, and the latest observation
/// carried the screenshot.
#[tokio::test]
async fn pixel_fallback_only_after_a_degraded_or_failed_verification_signal() {
    let h = harness().await;
    let mut desktop = FakeDesktop::registered(h.addr, descriptor(STUDIO)).await;
    let tools = h.turn(None).await;

    // A healthy window with a capture on record still refuses a point.
    let call = tools.observe(observe_args(None, true));
    desktop
        .answer_next(|_| {
            state(
                "s00000001",
                vec![element(0, "tok/a", "AXButton", "Add item")],
                true,
            )
        })
        .await;
    let observed = rendered(call).await;
    let text = observed
        .as_array()
        .and_then(|blocks| blocks.iter().find(|block| block["type"] == "text").cloned())
        .map(|block| serde_json::from_str::<Value>(block["text"].as_str().unwrap()).unwrap())
        .unwrap_or(observed);
    assert_eq!(text["pixel_addresses"]["allowed"], false);
    assert_eq!(text["screenshot"]["shown"], true, "{text}");
    let message = refused(tools.act(click_point(5.0, 5.0)), "pixel_refused").await;
    assert!(message.contains("element_token"), "{message}");
    desktop.saw_nothing().await;

    // The driver reports the surface unavailable, but without a capture
    // there is nothing to read a point from.
    let call = tools.observe(observe_args(None, false));
    desktop.answer_next(|_| degraded(false)).await;
    let observed = rendered(call).await;
    assert_eq!(observed["degraded"], true);
    assert_eq!(observed["pixel_addresses"]["allowed"], false);
    assert!(
        observed["pixel_addresses"]["reason"]
            .as_str()
            .unwrap()
            .contains("include_screenshot")
    );
    let message = refused(tools.act(click_point(5.0, 5.0)), "pixel_refused").await;
    assert!(message.contains("include_screenshot"), "{message}");
    desktop.saw_nothing().await;

    // Degraded and captured: the point is forwarded, in the background.
    let call = tools.observe(observe_args(None, true));
    desktop.answer_next(|_| degraded(true)).await;
    let observed = rendered(call).await;
    let text = observed
        .as_array()
        .and_then(|blocks| blocks.iter().find(|block| block["type"] == "text").cloned())
        .map(|block| serde_json::from_str::<Value>(block["text"].as_str().unwrap()).unwrap())
        .expect("the capture beside the text");
    assert_eq!(text["pixel_addresses"]["allowed"], true, "{text}");
    let escalation = text["escalation"].as_str().unwrap();
    assert!(
        escalation.contains("foreground") && escalation.contains("never"),
        "{escalation}"
    );
    let call = tools.act(click_point(5.0, 5.0));
    let click = desktop
        .answer_next(|request| echo(request, confirmed()))
        .await;
    let CuaAction::Click(args) = &click.action else {
        panic!("expected click");
    };
    assert!(matches!(
        args.address,
        cua_protocol::ElementAddress::Point(_)
    ));
    assert_eq!(args.delivery_mode, cua_protocol::DeliveryMode::Background);
    let acted = rendered(call).await;
    assert_eq!(acted["verified"], true);

    // A verified action closes the route: the healthy window observed with
    // a capture refuses a point again, until a verification there fails.
    let call = tools.observe(observe_args(None, true));
    desktop
        .answer_next(|_| {
            state(
                "s00000002",
                vec![element(0, "tok/b", "AXButton", "Add item")],
                true,
            )
        })
        .await;
    rendered(call).await;
    refused(tools.act(click_point(5.0, 5.0)), "pixel_refused").await;
    desktop.saw_nothing().await;
    let mut verify_args = window_exists();
    verify_args["target"] = target_args();
    let call = tools.verify(verify_args);
    desktop.answer_next(|_| verification("unsatisfied")).await;
    refused(call, "verification_failed").await;
    let call = tools.observe(observe_args(None, true));
    desktop
        .answer_next(|_| {
            state(
                "s00000003",
                vec![element(0, "tok/c", "AXButton", "Add item")],
                true,
            )
        })
        .await;
    rendered(call).await;
    let call = tools.act(click_point(5.0, 5.0));
    let click = desktop
        .answer_next(|request| echo(request, confirmed()))
        .await;
    assert!(matches!(click.action, CuaAction::Click(_)));
    rendered(call).await;
    desktop.close().await;
}

/// Stale element-token refusal: a token no snapshot issued, or one from a
/// snapshot a newer one replaced, is refused before anything is sent; the
/// driver's own stale verdict drops the window from the ledger; a token
/// from the latest snapshot is forwarded.
#[tokio::test]
async fn stale_element_token_refusal() {
    let h = harness().await;
    let mut desktop = FakeDesktop::registered(h.addr, descriptor(STUDIO)).await;
    let tools = h.turn(None).await;

    let call = tools.observe(observe_args(None, false));
    desktop
        .answer_next(|_| {
            state(
                "s00000001",
                vec![element(0, "tok/a", "AXButton", "Add item")],
                false,
            )
        })
        .await;
    rendered(call).await;
    let message = refused(tools.act(click_token("tok/zzz")), "stale_snapshot").await;
    assert!(
        message.contains("s00000001") && message.contains("no snapshot of this window issued it"),
        "{message}"
    );
    desktop.saw_nothing().await;

    // A newer snapshot voids the old tokens and names the one it replaced.
    let call = tools.observe(observe_args(None, false));
    desktop
        .answer_next(|_| {
            state(
                "s00000002",
                vec![element(0, "tok/b", "AXButton", "Add item")],
                false,
            )
        })
        .await;
    rendered(call).await;
    let message = refused(tools.act(click_token("tok/a")), "stale_snapshot").await;
    assert!(
        message.contains("s00000002") && message.contains("s00000001 was replaced"),
        "{message}"
    );
    desktop.saw_nothing().await;
    let message = refused(
        tools.act(json!({
            "target": target_args(),
            "action": {"kind": "click", "address": {
                "kind": "element_index", "element_index": 0, "snapshot_id": "s00000001"
            }},
        })),
        "stale_snapshot",
    )
    .await;
    assert!(message.contains("s00000002"), "{message}");
    desktop.saw_nothing().await;

    // The latest token is forwarded.
    let call = tools.act(click_token("tok/b"));
    let click = desktop
        .answer_next(|request| echo(request, confirmed()))
        .await;
    assert!(matches!(click.action, CuaAction::Click(_)));
    rendered(call).await;

    // The driver calling a snapshot stale is believed: the window is
    // forgotten and the next address there needs a fresh observation.
    let call = tools.observe(observe_args(None, false));
    desktop
        .answer_next(|_| {
            state(
                "s00000003",
                vec![element(0, "tok/c", "AXButton", "Add item")],
                false,
            )
        })
        .await;
    rendered(call).await;
    let call = tools.act(click_token("tok/c"));
    desktop
        .fail_next(DriverCallFailure::Tool {
            code: Some("stale_snapshot".into()),
            message: "snapshot s00000003 is stale".into(),
        })
        .await;
    let message = refused(call, "stale_snapshot").await;
    assert!(message.contains("get_window_state again"), "{message}");
    refused(tools.act(click_token("tok/c")), "snapshot_required").await;
    desktop.saw_nothing().await;
    desktop.close().await;
}

/// Denied permissions: an action whose permission the driver does not hold
/// is refused from the descriptor, before a frame is sent or a run opens,
/// on a desktop and on the server machine alike; what the remaining grant
/// allows still reaches the driver.
#[tokio::test]
async fn denied_permissions_refuse_before_the_driver() {
    let h = harness().await;

    // Accessibility denied on a desktop: no tree, no click; a capture-only
    // observation, which needs screen recording alone, goes through.
    let mut desktop = FakeDesktop::registered(
        h.addr,
        descriptor_with(Permission::Denied, Permission::Granted),
    )
    .await;
    let entry = h.typed_entry(STUDIO).await.unwrap();
    assert_eq!(entry["health"], "degraded");
    assert_eq!(entry["permissions"]["accessibility"], "denied");
    let tools = h.turn(None).await;
    let message = refused(
        tools.observe(observe_args(None, false)),
        "permission_denied",
    )
    .await;
    assert!(
        message.contains("Accessibility is denied") && message.contains("Computers page"),
        "{message}"
    );
    desktop.saw_nothing().await;
    let message = refused(tools.act(click_token("tok/a")), "permission_denied").await;
    assert!(message.contains("Accessibility"), "{message}");
    desktop.saw_nothing().await;
    let call = tools.observe(json!({
        "target": target_args(), "include_accessibility_tree": false, "include_screenshot": true,
    }));
    let request = desktop
        .answer_next(|_| {
            let mut captured = state("s00000001", vec![], true);
            captured.as_object_mut().unwrap().remove("snapshot_id");
            captured
        })
        .await;
    assert!(matches!(request.action, CuaAction::GetWindowState(_)));
    rendered(call).await;
    desktop.close().await;
    eventually("the desktop leaves the listing", || async {
        h.state.machine_registry.cua().list().await.is_empty()
    })
    .await;

    // Screen recording denied: the capture is refused, the tree is not.
    let mut desktop = FakeDesktop::registered(
        h.addr,
        descriptor_with(Permission::Granted, Permission::Denied),
    )
    .await;
    let tools = h.turn(None).await;
    let message = refused(tools.observe(observe_args(None, true)), "permission_denied").await;
    assert!(message.contains("Screen recording is denied"), "{message}");
    desktop.saw_nothing().await;
    let call = tools.observe(observe_args(None, false));
    desktop
        .answer_next(|_| {
            state(
                "s00000001",
                vec![element(0, "tok/a", "AXButton", "Add item")],
                false,
            )
        })
        .await;
    assert_eq!(rendered(call).await["snapshot_id"], "s00000001");
    desktop.close().await;
    eventually("the desktop leaves the listing", || async {
        h.state.machine_registry.cua().list().await.is_empty()
    })
    .await;

    // The server machine: the driver's report says accessibility is not
    // granted, so the target advertises it and a click is refused without
    // opening a run; the driver saw the health report and nothing else.
    let (runtime, transport) = h.attach_server_local(ACCESSIBILITY_DENIED, vec![]).await;
    let local = server_local_id();
    let entry = h.typed_entry(&local).await.unwrap();
    assert_eq!(entry["health"], "degraded");
    assert_eq!(entry["permissions"]["accessibility"], "denied");
    assert!(
        !entry["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|capability| capability == "pointer"),
        "{entry}"
    );
    let home = h.turn(Some(SERVER_HOME_TARGET)).await;
    let message = refused(home.act(click_token("tok/a")), "permission_denied").await;
    assert!(message.contains("nolune cua status"), "{message}");
    refused(home.observe(observe_args(None, false)), "permission_denied").await;
    assert_eq!(transport.tools_called(), ["health_report"]);
    runtime.shutdown().await;
}

/// Desktop disconnect and session cleanup: a desktop that quits fails the
/// call waiting on it at once, loses its target and the sessions it held,
/// and the tools refuse it by name afterwards; a server-local driver that
/// exits is unregistered, and the runtime's shutdown ends what it opened.
#[tokio::test]
async fn desktop_disconnect_and_session_cleanup() {
    let h = harness().await;
    let mut desktop = FakeDesktop::registered(h.addr, descriptor(STUDIO)).await;
    let tools = h.turn(None).await;

    let call = tools.observe(observe_args(None, false));
    desktop
        .answer_next(|_| {
            state(
                "s00000001",
                vec![element(0, "tok/a", "AXButton", "Add item")],
                false,
            )
        })
        .await;
    rendered(call).await;

    // The click the desktop never answers, then the app quits.
    let waiting = tools.act(click_token("tok/a"));
    let request = desktop.next_request().await;
    assert!(matches!(request.action, CuaAction::Click(_)));
    let closed_at = tokio::time::Instant::now();
    desktop.close().await;
    let message = refused(waiting, "runtime_unavailable").await;
    assert!(message.contains("retryable"), "{message}");
    assert!(
        closed_at.elapsed() < Duration::from_secs(CALL_TIMEOUT_SECS),
        "failed with the socket, not at the deadline"
    );
    eventually("the target is dropped", || async {
        h.state.machine_registry.cua().list().await.is_empty()
    })
    .await;
    assert!(
        h.state
            .machine_registry
            .desktop_cua_link(STUDIO)
            .await
            .is_none(),
        "the link and its sessions go with the socket"
    );
    assert!(h.typed_entry(STUDIO).await.is_none());
    let row = h.known_row(STUDIO).await;
    assert_eq!(row["online"], false);
    assert_eq!(row["driver_version"], Value::Null);
    assert_eq!(row["cua_health"], Value::Null);
    assert!(h.list_machines().await.contains("No machines connected"));
    refused(tools.observe(observe_args(None, false)), "no_cua_target").await;
    let chosen = h.turn(Some(STUDIO)).await;
    let message = refused(chosen.act(click_token("tok/a")), "machine_unavailable").await;
    assert!(message.contains("studio"), "{message}");

    // The server machine: a driver that exits drops the target and the
    // tools say so; a shutdown ends the driver cleanly.
    let (runtime, transport) = h
        .attach_server_local(HEALTHY, vec![started(1), json!({"apps": []}), ended(1)])
        .await;
    let local = server_local_id();
    let home = h.turn(Some(SERVER_HOME_TARGET)).await;
    rendered(home.discover(json!({"mode": "list_apps"}))).await;
    assert_eq!(
        transport.tools_called(),
        ["health_report", "start_session", "list_apps", "end_session"],
        "every run ends its own session"
    );
    transport.crash();
    eventually("the exited driver is unregistered", || async {
        h.state.machine_registry.cua().list().await.is_empty()
    })
    .await;
    assert!(matches!(runtime.status().await, RuntimeStatus::Failed(_)));
    assert!(h.typed_entry(&local).await.is_none());
    refused(
        home.discover(json!({"mode": "list_apps"})),
        "no_server_local_target",
    )
    .await;
    runtime.shutdown().await;

    let (runtime, transport) = h.attach_server_local(HEALTHY, vec![]).await;
    assert!(h.typed_entry(&local).await.is_some());
    runtime.shutdown().await;
    assert!(transport.closed(), "shutdown stops the driver");
    assert!(h.typed_entry(&local).await.is_none());
}

/// Headless server behaviour: a host without a graphical session, a
/// container, an unsupported platform or `[cua].enabled = false` registers
/// nothing, the server keeps serving, the typed tools explain the absence,
/// and a remote desktop can still register and be driven.
#[tokio::test]
async fn headless_server_behaviour() {
    let h = harness().await;
    let probe = |os: &'static str, session: DisplaySession, container: bool| HostProbe {
        os,
        support: match os {
            "macos" => PlatformSupport::Supported,
            _ => PlatformSupport::Unsupported(format!(
                "Nolune drives computers on macOS only for now; on {os} the pinned driver still installs"
            )),
        },
        session,
        container,
        hostname: HOST.into(),
    };
    let driver = || Ok(Some(PathBuf::from("/opt/cua/cua-driver")));
    let runtime = || {
        CuaRuntime::new(
            crate::config::CuaConfig::default(),
            h.state.machine_registry.cua().clone(),
            h.state.workspace_dir.clone(),
        )
    };

    // Linux without a display, macOS outside an Aqua login, Windows without
    // a session: headless, nothing started.
    for (os, session) in [
        (
            "linux",
            DisplaySession::Headless(
                "no display session (DISPLAY and WAYLAND_DISPLAY are unset)".into(),
            ),
        ),
        (
            "macos",
            DisplaySession::Headless(
                "no graphical session (launchctl managername reports Background)".into(),
            ),
        ),
        (
            "windows",
            DisplaySession::Headless("no interactive session (SESSIONNAME is unset)".into()),
        ),
    ] {
        let runtime = runtime();
        runtime
            .start_from(probe(os, session, false), driver())
            .await;
        match runtime.status().await {
            RuntimeStatus::Skipped(Skip::Headless(reason)) if os == "macos" => {
                assert!(reason.contains("Background"), "{reason}")
            }
            RuntimeStatus::Skipped(Skip::Unsupported(reason)) => {
                assert!(reason.contains("macOS only"), "{os}: {reason}")
            }
            other => panic!("{os}: expected a skip, got {other:?}"),
        }
        assert!(h.state.machine_registry.cua().list().await.is_empty());
    }
    let runtime_in_container = runtime();
    runtime_in_container
        .start_from(
            probe(
                "macos",
                DisplaySession::Present("launchd session Aqua".into()),
                true,
            ),
            driver(),
        )
        .await;
    assert_eq!(
        runtime_in_container.status().await,
        RuntimeStatus::Skipped(Skip::Container)
    );
    let disabled = CuaRuntime::new(
        crate::config::CuaConfig {
            enabled: false,
            ..Default::default()
        },
        h.state.machine_registry.cua().clone(),
        h.state.workspace_dir.clone(),
    );
    disabled
        .start_from(
            probe(
                "macos",
                DisplaySession::Present("launchd session Aqua".into()),
                false,
            ),
            driver(),
        )
        .await;
    assert_eq!(
        disabled.status().await,
        RuntimeStatus::Skipped(Skip::Disabled)
    );
    let unsupported = runtime();
    unsupported
        .start_from(
            probe("linux", DisplaySession::Present("DISPLAY=:0".into()), false),
            driver(),
        )
        .await;
    assert!(matches!(
        unsupported.status().await,
        RuntimeStatus::Skipped(Skip::Unsupported(_))
    ));

    // The server serves and the tools explain: nothing to drive, the home
    // has no driver, a desktop nobody connected is unavailable.
    assert!(h.known_rows().await.is_empty());
    assert!(h.list_machines().await.contains("No machines connected"));
    let open = h.turn(None).await;
    let message = refused(open.discover(json!({"mode": "list_apps"})), "no_cua_target").await;
    assert!(
        message.contains("server machine") && message.contains("desktop"),
        "{message}"
    );
    assert_eq!(
        open.trail("discover_windows", &json!({"mode": "list_apps"})),
        "listing apps on the connected computer",
        "the generic wording while there is nothing to name"
    );
    let home = h.turn(Some(SERVER_HOME_TARGET)).await;
    let message = refused(
        home.observe(observe_args(None, false)),
        "no_server_local_target",
    )
    .await;
    assert!(message.contains("run_command"), "{message}");
    let chosen = h.turn(Some(STUDIO)).await;
    refused(chosen.act(click_token("tok/a")), "machine_unavailable").await;

    // A remote desktop is unaffected by the server being headless.
    let mut desktop = FakeDesktop::registered(h.addr, descriptor(STUDIO)).await;
    let tools = h.turn(None).await;
    let call = tools.discover(json!({"mode": "list_windows"}));
    desktop
        .answer_next(|_| json!({"windows": [], "current_space_id": 1}))
        .await;
    let windows = rendered(call).await;
    assert_eq!(windows["machine"], "studio");
    assert_eq!(windows["windows_total"], 0);
    desktop.close().await;
}

/// Capability-manifest refusal, end to end through the tool layer: the
/// tools offer no recording, replay, session or management action and
/// refuse one before anything is resolved or sent; a descriptor that names
/// a capability the protocol does not have is refused at registration; a
/// target that does not advertise an action's capability refuses it before
/// the wire, while one that does receives it.
#[tokio::test]
async fn capability_manifest_refuses_recording_replay_and_management_end_to_end() {
    let h = harness().await;

    // A descriptor advertising a capability outside the manifest never
    // registers, so no target can claim to record.
    let mut recording = cua_field(descriptor(STUDIO));
    recording["machine"]["capabilities"]
        .as_array_mut()
        .unwrap()
        .push(json!("screen_recording"));
    let (mut desktop, first) = FakeDesktop::connect(h.addr, register(Some(recording))).await;
    assert_eq!(first["type"], "error", "{first}");
    assert_eq!(first["error"], "invalid_cua_registration");
    assert!(desktop.closed().await);
    assert!(h.state.machine_registry.cua().list().await.is_empty());

    // The tools' own definitions: `act` offers exactly the typed window
    // actions, and no definition names a recorder, a replay, a session or
    // a management surface.
    let mut desktop = FakeDesktop::registered(h.addr, descriptor(STUDIO)).await;
    let tools = h.turn(None).await;
    let act = tools.act.definition(String::new()).await;
    let action = &act.parameters["properties"]["action"];
    let branches = action["oneOf"]
        .as_array()
        .or_else(|| action["anyOf"].as_array())
        .unwrap_or_else(|| panic!("{}", act.parameters));
    let kinds: BTreeSet<String> = branches
        .iter()
        .flat_map(|variant| {
            let kind = &variant["properties"]["kind"];
            let mut literals = Vec::new();
            if let Some(constant) = kind["const"].as_str() {
                literals.push(constant.to_owned());
            }
            if let Some(values) = kind["enum"].as_array() {
                literals.extend(values.iter().filter_map(Value::as_str).map(str::to_owned));
            }
            literals
        })
        .collect();
    let expected: BTreeSet<String> = [
        "click",
        "double_click",
        "right_click",
        "drag",
        "scroll",
        "type_text",
        "press_key",
        "hotkey",
        "set_value",
        "invoke_menu",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert_eq!(kinds, expected);
    for tool in [&tools.discover, &tools.state, &tools.act, &tools.verify] {
        let definition = serde_json::to_string(&tool.definition(String::new()).await).unwrap();
        for absent in [
            "record",
            "replay",
            "stream",
            "start_session",
            "end_session",
            "set_window_frame",
            "kill",
            "clipboard",
            "set_config",
            "bring_to_front",
            "get_desktop_state",
        ] {
            assert!(
                !definition.contains(absent),
                "{} offers {absent:?}: {definition}",
                tool.name()
            );
        }
    }

    // Asked for anyway, the call never decodes: nothing is resolved, nothing
    // reaches the desktop.
    for kind in [
        "start_recording",
        "stop_recording",
        "replay",
        "start_session",
        "end_session",
        "set_window_frame",
        "kill_app",
        "clipboard_read",
        "set_config",
        "bring_to_front",
    ] {
        let error = unreadable(tools.act(json!({
            "target": target_args(),
            "action": {"kind": kind, "address": {"kind": "element_token", "element_token": "tok/a"}},
        })))
        .await;
        assert!(error.contains("unknown variant"), "{kind}: {error}");
    }
    for mode in ["get_desktop_state", "record", "kill_app"] {
        unreadable(tools.discover(json!({"mode": mode}))).await;
    }
    let error = unreadable(tools.act(json!({
        "target": target_args(),
        "action": {"kind": "click", "address": {"kind": "element_token", "element_token": "tok/a"}},
        "delivery_mode": "foreground",
    })))
    .await;
    assert!(error.contains("unknown variant"), "{error}");
    desktop.saw_nothing().await;
    desktop.close().await;
    eventually("the desktop leaves the listing", || async {
        h.state.machine_registry.cua().list().await.is_empty()
    })
    .await;

    // A target without the pointer capability refuses a click from its
    // descriptor, before the ledger and before the wire; with it, the click
    // is the frame the desktop receives.
    let mut observer = descriptor(STUDIO);
    observer
        .capabilities
        .retain(|capability| !matches!(capability, Capability::Pointer | Capability::Keyboard));
    let mut desktop = FakeDesktop::registered(h.addr, observer).await;
    let tools = h.turn(None).await;
    let call = tools.observe(observe_args(None, false));
    desktop
        .answer_next(|_| {
            state(
                "s00000001",
                vec![element(0, "tok/a", "AXButton", "Add item")],
                false,
            )
        })
        .await;
    rendered(call).await;
    let message = refused(tools.act(click_token("tok/a")), "action_refused").await;
    assert!(message.contains("capability not advertised"), "{message}");
    let message = refused(
        tools.act(json!({
            "target": target_args(),
            "action": {"kind": "type_text", "text": "milk",
                       "address": {"kind": "element_token", "element_token": "tok/a"}},
        })),
        "action_refused",
    )
    .await;
    assert!(message.contains("capability not advertised"), "{message}");
    desktop.saw_nothing().await;
    desktop.close().await;
    eventually("the desktop leaves the listing", || async {
        h.state.machine_registry.cua().list().await.is_empty()
    })
    .await;

    let mut desktop = FakeDesktop::registered(h.addr, descriptor(STUDIO)).await;
    let tools = h.turn(None).await;
    let call = tools.observe(observe_args(None, false));
    desktop
        .answer_next(|_| {
            state(
                "s00000001",
                vec![element(0, "tok/a", "AXButton", "Add item")],
                false,
            )
        })
        .await;
    rendered(call).await;
    let call = tools.act(click_token("tok/a"));
    let click = desktop
        .answer_next(|request| echo(request, confirmed()))
        .await;
    assert!(matches!(click.action, CuaAction::Click(_)));
    rendered(call).await;
    // The frames a desktop ever sees from the tools are observations,
    // actions and verifications: no session or health call, which are the
    // runtime's and the registration's.
    desktop.saw_nothing().await;
    desktop.close().await;
}

/// The recorded calls of a fake driver never include a recorder: what a
/// run sends the server-local driver is the session bracket around the
/// typed action, and the health report the registration asked for.
#[tokio::test]
async fn the_server_local_driver_only_ever_sees_typed_actions_and_its_session() {
    let h = harness().await;
    let (runtime, transport) = h
        .attach_server_local(
            HEALTHY,
            vec![
                started(1),
                state(
                    "s00000001",
                    vec![element(0, "tok/a", "AXButton", "Add item")],
                    true,
                ),
                ended(1),
            ],
        )
        .await;
    let tools = h.turn(None).await;
    let observed = rendered(tools.observe(observe_args(None, true))).await;
    let blocks = observed.as_array().expect("the capture beside the text");
    assert_eq!(blocks[0]["type"], "image");
    let calls = transport.calls();
    let names: Vec<&str> = calls.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "health_report",
            "start_session",
            "get_window_state",
            "end_session"
        ]
    );
    let (_, observation) = &calls[2];
    assert_eq!(observation["include_screenshot"], json!(true));
    assert_eq!(observation["session"], json!("nolune-run-1"));
    for name in names {
        assert!(
            !name.contains("record") && !name.contains("stream") && !name.contains("watch"),
            "{name}"
        );
    }
    runtime.shutdown().await;
}
