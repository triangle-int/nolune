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
    BoundedText, CheckedCuaAdapter, CuaAction, CuaActionKind, CuaActionResult,
    CuaRegistrationEnvelope, CuaRequestEnvelope, CuaResponse, CuaResponseEnvelope, CuaRuntimeError,
    ElementAddress, HealthReportArgs, HealthReportResult, MachineDescriptor, MachineId,
    MachineLocation, ProtocolVersion, RequestId, RuntimeErrorCode, SessionLabel, SessionRefArgs,
    ValidationError, WindowTarget, MAX_ID_BYTES,
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
    let text = result
        .content
        .iter()
        .filter_map(|content| match &content.raw {
            RawContent::Text(raw) => Some(raw.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    if result.is_error == Some(true) {
        let code = result
            .structured_content
            .as_ref()
            .and_then(|structured| structured.get("code"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        return Err(DriverCallFailure::Tool {
            code,
            message: text,
        });
    }
    if let Some(structured) = result.structured_content {
        return Ok(structured);
    }
    if text.trim_start().starts_with(['{', '[']) {
        if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
            return Ok(parsed);
        }
    }
    Err(DriverCallFailure::Malformed(
        "driver returned no structured payload".to_owned(),
    ))
}

/// How many of the driver's stderr lines are kept for an error message.
const STDERR_LINES_KEPT: usize = 16;
/// How long a stderr line may be in an error message.
const STDERR_LINE_LIMIT: usize = 400;
/// How long the stderr reader gets to drain after the child failed.
const STDERR_DRAIN: Duration = Duration::from_millis(300);

/// The last lines a driver child wrote to stderr, read as they arrive. The
/// driver explains itself there (a missing daemon, a permission to grant),
/// and only a message that repeats it is actionable.
struct DriverStderr {
    lines: Arc<Mutex<VecDeque<String>>>,
    reader: Option<tokio::task::JoinHandle<()>>,
}

impl DriverStderr {
    fn capture(stderr: Option<tokio::process::ChildStderr>) -> Self {
        let lines = Arc::new(Mutex::new(VecDeque::new()));
        let reader = stderr.map(|stderr| {
            let lines = lines.clone();
            tokio::spawn(async move {
                let mut reader = tokio::io::BufReader::new(stderr).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    let line = line.trim().to_owned();
                    if line.is_empty() {
                        continue;
                    }
                    eprintln!("[cua] driver: {line}");
                    let mut kept = lines.lock().unwrap();
                    if kept.len() == STDERR_LINES_KEPT {
                        kept.pop_front();
                    }
                    kept.push_back(line.chars().take(STDERR_LINE_LIMIT).collect());
                }
            })
        });
        Self { lines, reader }
    }

    /// `; the driver said: "..."` once the child is gone and its stderr is
    /// drained, or nothing when it said nothing.
    async fn suffix(mut self) -> String {
        if let Some(mut reader) = self.reader.take() {
            // The child is dead or dying by now; give the reader a moment to
            // reach end of file, then report what arrived so far.
            if tokio::time::timeout(STDERR_DRAIN, &mut reader)
                .await
                .is_err()
            {
                reader.abort();
            }
        }
        let lines = self.lines.lock().unwrap();
        if lines.is_empty() {
            String::new()
        } else {
            format!(
                "; the driver said: \"{}\"",
                lines.iter().cloned().collect::<Vec<_>>().join(" | ")
            )
        }
    }
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

fn client_info() -> ClientInfo {
    let mut info = ClientInfo::default();
    info.client_info = Implementation::new("nolune-desktop", env!("CARGO_PKG_VERSION"));
    info
}

impl StdioDriverTransport {
    /// Spawn `<driver> mcp` and complete the MCP handshake within
    /// `timeouts.handshake`; a child that has not answered by then is killed
    /// and the error names the driver and repeats what it said on stderr.
    pub async fn spawn_with(driver: &Path, timeouts: DriverTimeouts) -> Result<Self, String> {
        let mut command = tokio::process::Command::new(driver);
        command.arg("mcp");
        let (process, stderr) = TokioChildProcess::builder(command)
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("could not start {} mcp: {error}", driver.display()))?;
        let said = DriverStderr::capture(stderr);
        // On expiry the connect future is dropped with the child process
        // still inside it, which kills the child.
        let connected =
            tokio::time::timeout(timeouts.handshake, client_info().serve(process)).await;
        let running = match connected {
            Ok(Ok(running)) => running,
            Ok(Err(error)) => {
                return Err(format!(
                    "could not start {} mcp: {error}{}",
                    driver.display(),
                    said.suffix().await
                ));
            }
            Err(_elapsed) => {
                return Err(format!(
                    "{} mcp did not complete the MCP handshake within {:?}{}",
                    driver.display(),
                    timeouts.handshake,
                    said.suffix().await
                ));
            }
        };
        let sink = running.peer().clone();
        let keep_alive = tokio::spawn(async move {
            let _ = running.waiting().await;
        });
        eprintln!("[cua] driver started: {} mcp", driver.display());
        // The keep-alive task ends when the child exits or when it is
        // aborted; either way the flag flips exactly once.
        let abort = keep_alive.abort_handle();
        let (flag, gone) = tokio::sync::watch::channel(false);
        tokio::spawn(async move {
            let _ = keep_alive.await;
            flag.send_replace(true);
        });
        Ok(Self {
            sink,
            keep_alive: abort,
            gone,
            timeouts,
        })
    }
}

/// One `tools/call` that rmcp cancels (with a `notifications/cancelled` to
/// the driver) when `deadline` passes.
async fn call_tool_within(
    sink: &ServerSink,
    params: CallToolRequestParams,
    deadline: Duration,
) -> Result<CallToolResult, ServiceError> {
    let request = ClientRequest::CallToolRequest(CallToolRequest::new(params));
    let options = PeerRequestOptions::with_timeout(deadline);
    let handle = sink.send_request_with_option(request, options).await?;
    match handle.await_response().await? {
        ServerResult::CallToolResult(result) => Ok(result),
        _ => Err(ServiceError::UnexpectedResponse),
    }
}

impl DriverTransport for StdioDriverTransport {
    fn call_tool(&self, name: &str, arguments: Map<String, Value>) -> TransportFuture<'_> {
        let mut params = CallToolRequestParams::new(name.to_owned());
        params.arguments = Some(arguments);
        let name = name.to_owned();
        Box::pin(async move {
            let result = call_tool_within(&self.sink, params, self.timeouts.call)
                .await
                .map_err(|error| match error {
                    ServiceError::Timeout { timeout } => DriverCallFailure::Timeout(format!(
                        "{name} did not answer within {timeout:?}; the request was cancelled"
                    )),
                    other => DriverCallFailure::Transport(other.to_string()),
                })?;
            payload_from_call_result(result)
        })
    }

    /// Abort the keep-alive task: the rmcp service drops, the connection
    /// closes and the child is killed. Aborting twice is harmless.
    fn close(&self) {
        if !self.keep_alive.is_finished() {
            eprintln!("[cua] driver stopped");
        }
        self.keep_alive.abort();
    }

    fn exited(&self) -> ExitFuture<'_> {
        let mut gone = self.gone.clone();
        Box::pin(async move {
            // A closed channel means the flag task is over, which it only is
            // after it flipped the flag: gone either way.
            let _ = gone.wait_for(|gone| *gone).await;
        })
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

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        metadata.is_file()
    }
}

/// The driver this desktop runs: `NOLUNE_CUA_DRIVER` when set, else the
/// driver `nolune cua install` recorded under the workspace, else an
/// executable `cua-driver` on `path`. `None` when there is none.
pub fn locate_driver(
    workspace: &Path,
    env_override: Option<&std::ffi::OsStr>,
    path: Option<&std::ffi::OsStr>,
) -> Option<PathBuf> {
    if let Some(named) = env_override.filter(|named| !named.is_empty()) {
        let named = PathBuf::from(named);
        if is_executable(&named) {
            return Some(named);
        }
        // An explicitly named driver that cannot run is a misconfiguration,
        // never silently "no driver": say so and register legacy-only.
        eprintln!(
            "[cua] {DRIVER_ENV} names {} which is not an executable file",
            named.display()
        );
        return None;
    }
    let manifest = workspace.join(INSTALL_MANIFEST);
    if let Ok(raw) = std::fs::read_to_string(&manifest) {
        let installed = serde_json::from_str::<Value>(&raw)
            .ok()
            .and_then(|manifest| manifest.get("driver")?.as_str().map(PathBuf::from));
        match installed {
            Some(driver) if is_executable(&driver) => return Some(driver),
            Some(driver) => eprintln!(
                "[cua] {} names {} which is gone; run nolune cua install",
                manifest.display(),
                driver.display()
            ),
            None => eprintln!("[cua] {} is not a driver manifest", manifest.display()),
        }
    }
    path.and_then(|path| {
        std::env::split_paths(path)
            .map(|dir| dir.join(DRIVER_BINARY))
            .find(|candidate| is_executable(candidate))
    })
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
    let request = CuaRequestEnvelope {
        version: ProtocolVersion::V1,
        request_id: RequestId::try_from("health").expect("static id"),
        machine_id: machine_id.clone(),
        action: CuaAction::HealthReport(HealthReportArgs {
            include: vec![],
            skip: vec![],
        }),
    };
    let call = tool_call(&request.action).map_err(|error| error.to_string())?;
    let outcome = transport.call_tool(call.name, call.arguments).await;
    match response_for(&request, outcome).response {
        CuaResponse::Success { result } => match *result {
            CuaActionResult::HealthReport(report) => Ok(report),
            other => Err(format!("health_report answered with {:?}", other.kind())),
        },
        CuaResponse::Error { error } => Err(format!(
            "health_report failed ({:?}): {}",
            error.code,
            error.message.as_str()
        )),
    }
}

/// The descriptor this desktop registers: the driver's health report under
/// this machine's stable id, at the desktop location. The driver acts under
/// its own bundle's grants, so its report decides the permissions and the
/// capabilities; the app's own grants stay on the legacy `permissions`.
pub async fn describe_machine(
    transport: &dyn DriverTransport,
    machine_id: MachineId,
) -> Result<MachineDescriptor, String> {
    let report = health_report(transport, &machine_id).await?;
    Ok(descriptor_from_health(
        machine_id,
        MachineLocation::Desktop,
        &report,
    ))
}

/// The `cua` field of the register message: the descriptor in the
/// registration envelope the server decodes.
pub fn registration_envelope(descriptor: &MachineDescriptor) -> Value {
    serde_json::to_value(CuaRegistrationEnvelope {
        version: ProtocolVersion::V1,
        machine: descriptor.clone(),
    })
    .expect("a valid descriptor serializes")
}

// ---------------------------------------------------------------------------
// The local allowlist
// ---------------------------------------------------------------------------

/// The typed request inside a `cua_request` frame, or `None` for anything
/// else on the socket (a legacy toolcall, the registration ack).
pub fn typed_request(frame: &Value) -> Option<&Value> {
    if frame.get("type").and_then(Value::as_str) != Some("cua_request") {
        return None;
    }
    frame.get("request").filter(|request| request.is_object())
}

/// Longest message a locally built error envelope carries.
const MAX_LOCAL_MESSAGE_BYTES: usize = 1_024;

/// A message as bounded protocol text: control characters dropped, length
/// capped, never empty, so the error envelope always validates.
fn bounded_message(raw: &str) -> BoundedText {
    let mut message: String = raw
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
        .collect();
    if message.len() > MAX_LOCAL_MESSAGE_BYTES {
        let mut end = MAX_LOCAL_MESSAGE_BYTES;
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        message.truncate(end);
    }
    if message.is_empty() {
        message.push_str("refused by this desktop");
    }
    BoundedText::try_from(message).expect("filtered text within bounds is valid")
}

/// What a frame said about itself, cut to the protocol's id bound.
fn bounded_id(raw: &str) -> String {
    raw.chars().take(MAX_ID_BYTES).collect()
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
        match self {
            Self::Denied(envelope) => match &envelope.response {
                CuaResponse::Error { error } => error.clone(),
                CuaResponse::Success { .. } => denial(format!(
                    "{} is not allowed on this desktop",
                    action_name(envelope.action)
                )),
            },
            Self::Unreadable { message, .. } => denial(message.clone()),
        }
    }

    /// The `cua_response` frame to send back.
    pub fn frame(&self) -> Value {
        match self {
            Self::Denied(envelope) => response_frame(envelope),
            Self::Unreadable {
                request_id,
                machine_id,
                action,
                ..
            } => json!({
                "type": "cua_response",
                "response": {
                    "version": ProtocolVersion::V1,
                    "request_id": request_id,
                    "machine_id": machine_id,
                    "action": action,
                    "response": {"status": "error", "error": self.error()},
                },
            }),
        }
    }
}

/// A capability denial: never retryable, the server has to change what it asks.
fn denial(message: String) -> CuaRuntimeError {
    CuaRuntimeError {
        code: RuntimeErrorCode::CapabilityDenied,
        message: bounded_message(&message),
        retryable: false,
    }
}

/// Decode one inbound request through the protocol's bounds (size, depth,
/// shape, a tool the protocol names). Nothing else is ever executed.
pub fn decode_request(request: &Value) -> Result<CuaRequestEnvelope, Refusal> {
    CuaRequestEnvelope::from_json(&request.to_string()).map_err(|error| {
        let field = |key: &str| request.get(key).and_then(Value::as_str).map(bounded_id);
        Refusal::Unreadable {
            request_id: field("request_id"),
            machine_id: field("machine_id"),
            action: request
                .get("action")
                .and_then(|action| action.get("tool"))
                .and_then(Value::as_str)
                .map(bounded_id),
            message: format!("request is not on this desktop's allowlist: {error}"),
        }
    })
}

/// The local allowlist: the request must name this machine, and the
/// descriptor must advertise the action's capability with its permissions
/// granted. Refused here, a request never touches the driver, whatever the
/// server said.
pub fn authorize(
    descriptor: &MachineDescriptor,
    request: &CuaRequestEnvelope,
) -> Result<(), Refusal> {
    let refuse = |message: String| {
        Refusal::Denied(CuaResponseEnvelope {
            version: request.version,
            request_id: request.request_id.clone(),
            machine_id: request.machine_id.clone(),
            action: request.action.kind(),
            response: CuaResponse::Error {
                error: denial(message),
            },
        })
    };
    if request.machine_id != descriptor.machine_id {
        return Err(refuse(format!(
            "request names machine '{}' but this desktop is '{}'",
            request.machine_id.as_str(),
            descriptor.machine_id.as_str()
        )));
    }
    descriptor.authorize(&request.action).map_err(|error| {
        refuse(format!(
            "{} is not allowed on this desktop: {error}",
            action_name(request.action.kind())
        ))
    })
}

/// The `cua_response` frame carrying `response` whole.
pub fn response_frame(response: &CuaResponseEnvelope) -> Value {
    json!({"type": "cua_response", "response": response})
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
}

impl Driver {
    /// A driver described from `transport`'s health report; a transport that
    /// cannot be described is closed.
    async fn describe(
        transport: Arc<dyn DriverTransport>,
        machine_id: MachineId,
    ) -> Result<Arc<Self>, String> {
        let descriptor = match describe_machine(&*transport, machine_id.clone()).await {
            Ok(descriptor) => descriptor,
            Err(error) => {
                transport.close();
                return Err(error);
            }
        };
        let adapter = checked_adapter(transport.clone(), descriptor.clone())
            .map_err(|error| format!("descriptor: {error}"))?;
        Ok(Arc::new(Self {
            transport,
            machine_id,
            descriptor,
            adapter,
        }))
    }

    /// Whether the child is gone (exited on its own or closed).
    fn gone(&self) -> bool {
        self.transport.exited().now_or_never().is_some()
    }
}

/// A checked adapter that executes every authorized request against the
/// driver: action to tool call, payload to correlated envelope. The adapter
/// validates and authorizes before the callback runs and checks the
/// correlation after it, so the callback only encodes, calls and decodes.
fn checked_adapter(
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

/// Where the runtime stands with its driver.
enum Slot {
    /// No driver was ever started, or the app is exiting.
    None,
    Running(Arc<Driver>),
    /// The driver exited on its own; the next request restarts it under
    /// the same id.
    Crashed(MachineId),
}

/// One driver for the app lifetime, started on the first connection and
/// kept across reconnects; restarted on the next request after a crash;
/// stopped when the app exits.
pub struct CuaRuntime {
    spawner: Spawner,
    driver: tokio::sync::Mutex<Slot>,
    /// The labelled sessions the desktop confirmed open for the server and
    /// has not ended: what a disconnect or an exit has to end.
    open: Mutex<BTreeSet<SessionLabel>>,
}

impl CuaRuntime {
    /// A runtime that starts its driver through `spawner`.
    pub fn with_spawner(spawner: Spawner) -> Self {
        Self {
            spawner,
            driver: tokio::sync::Mutex::new(Slot::None),
            open: Mutex::new(BTreeSet::new()),
        }
    }

    /// Whether a driver is running (spawned and not gone).
    #[cfg(test)]
    pub async fn is_running(&self) -> bool {
        let mut slot = self.driver.lock().await;
        self.live(&mut slot).is_some()
    }

    /// The descriptor to register with: starts the driver when none runs,
    /// re-reads the health of the one that does. An error means this desktop
    /// registers without a typed target.
    pub async fn start(&self, machine_id: &str) -> Result<MachineDescriptor, String> {
        let machine_id = MachineId::try_from(machine_id).map_err(|error| error.to_string())?;
        let mut slot = self.driver.lock().await;
        let driver = match self.live(&mut slot) {
            // A reconnect: the same child, its health read again so the
            // descriptor the server binds is current.
            Some(running) => {
                let described = Driver::describe(running.transport.clone(), machine_id).await;
                if described.is_err() {
                    // A driver that no longer reports is stopped; the next
                    // start spawns a new one.
                    *slot = Slot::None;
                }
                described?
            }
            None => {
                let transport = (self.spawner)().await?;
                Driver::describe(transport, machine_id).await?
            }
        };
        let descriptor = driver.descriptor.clone();
        *slot = Slot::Running(driver);
        Ok(descriptor)
    }

    /// One inbound `cua_request` frame's `request`, answered with the
    /// `cua_response` frame to send back: refused locally, or executed
    /// through the checked adapter with its typed result forwarded unchanged.
    pub async fn handle(&self, request: &Value) -> Value {
        let request = match decode_request(request) {
            Ok(request) => request,
            Err(refusal) => return refusal.frame(),
        };
        let driver = match self.driver_for_request().await {
            Ok(driver) => driver,
            Err(why) => {
                return response_frame(&error_response(
                    &request,
                    &DriverCallFailure::Transport(why),
                ));
            }
        };
        if let Err(refusal) = authorize(&driver.descriptor, &request) {
            return refusal.frame();
        }
        let response = match driver.adapter.execute(&request).await {
            Ok(response) => response,
            Err(error) => error_response(
                &request,
                &DriverCallFailure::Malformed(format!(
                    "driver answered request {} with a response that does not match it: {error}",
                    request.request_id.as_str()
                )),
            ),
        };
        self.note_session(&request.action, &response.response);
        response_frame(&response)
    }

    /// End every session the desktop holds open for the server: the socket
    /// is gone, so nobody will. The driver keeps running for the reconnect.
    pub async fn end_sessions(&self) {
        let driver = {
            let mut slot = self.driver.lock().await;
            self.live(&mut slot)
        };
        let open = self.take_open();
        let Some(driver) = driver else {
            return;
        };
        for label in &open {
            end_session(&driver, label).await;
        }
        if !open.is_empty() {
            eprintln!("[cua] {} open session(s) ended on disconnect", open.len());
        }
    }

    /// End the open sessions and stop the driver: the app is exiting.
    /// Idempotent and a no-op for a runtime that never started.
    pub async fn shutdown(&self) {
        let driver = {
            let mut slot = self.driver.lock().await;
            match std::mem::replace(&mut *slot, Slot::None) {
                Slot::Running(driver) if !driver.gone() => Some(driver),
                _ => None,
            }
        };
        let open = self.take_open();
        let Some(driver) = driver else {
            return;
        };
        for label in &open {
            end_session(&driver, label).await;
        }
        driver.transport.close();
        // The child is killed when the aborted keep-alive task drops the
        // service; wait for that so an exiting app never leaves it behind.
        let _ = tokio::time::timeout(Duration::from_secs(3), driver.transport.exited()).await;
        eprintln!(
            "[cua] runtime stopped with the app; {} open session(s) ended",
            open.len()
        );
    }

    /// Kill the driver without ending anything: the exit's last resort
    /// when a graceful `shutdown` was cut short. Terminal like `shutdown`,
    /// closes the driver synchronously, whatever else is in flight, and
    /// hands its transport back for the exit to wait on; `None` when no
    /// driver was ever spawned. Idempotent.
    pub fn kill_driver(&self) -> Option<Arc<dyn DriverTransport>> {
        todo!("a stop that is terminal and reachable without the driver lock")
    }

    /// The labelled sessions still open, in label order.
    #[cfg(test)]
    pub fn open_sessions(&self) -> Vec<SessionLabel> {
        self.lock_open().iter().cloned().collect()
    }

    /// The running driver, or none: a driver that exited on its own is
    /// noticed here, with the sessions it held (nobody is left to end them),
    /// and remembered for a restart.
    fn live(&self, slot: &mut Slot) -> Option<Arc<Driver>> {
        let crashed = match &*slot {
            Slot::Running(driver) if driver.gone() => driver.machine_id.clone(),
            Slot::Running(driver) => return Some(driver.clone()),
            Slot::None | Slot::Crashed(_) => return None,
        };
        let lost = self.take_open();
        eprintln!(
            "[cua] driver exited on its own; {} open session(s) lost with it",
            lost.len()
        );
        *slot = Slot::Crashed(crashed);
        None
    }

    /// The driver a request executes on: the running one, or the one
    /// restarted under the same id after a crash.
    async fn driver_for_request(&self) -> Result<Arc<Driver>, String> {
        let mut slot = self.driver.lock().await;
        if let Some(driver) = self.live(&mut slot) {
            return Ok(driver);
        }
        let Slot::Crashed(machine_id) = &*slot else {
            return Err("no driver is running on this desktop".to_owned());
        };
        eprintln!("[cua] restarting the driver");
        let transport = (self.spawner)()
            .await
            .map_err(|error| format!("the driver could not be restarted: {error}"))?;
        let driver = Driver::describe(transport, machine_id.clone())
            .await
            .map_err(|error| format!("the restarted driver could not be described: {error}"))?;
        *slot = Slot::Running(driver.clone());
        Ok(driver)
    }

    /// The bookkeeping for a confirmed answer: a `start_session` that came
    /// back active opens its label, an `end_session` closes it. Implicit
    /// sessions carry no label and are the driver's own to end.
    fn note_session(&self, action: &CuaAction, response: &CuaResponse) {
        let CuaResponse::Success { result } = response else {
            return;
        };
        match (action, &**result) {
            (CuaAction::StartSession(args), CuaActionResult::StartSession(started)) => {
                if !started.active {
                    return;
                }
                if let Some(label) = started.session.clone().or_else(|| args.session.clone()) {
                    self.lock_open().insert(label);
                }
            }
            (CuaAction::EndSession(args), CuaActionResult::EndSession(ended)) => {
                if let Some(label) = ended.session.clone().or_else(|| args.session.clone()) {
                    self.lock_open().remove(&label);
                }
            }
            _ => {}
        }
    }

    /// Every open label in label order, leaving none.
    fn take_open(&self) -> Vec<SessionLabel> {
        std::mem::take(&mut *self.lock_open()).into_iter().collect()
    }

    /// A poisoned lock only means a task panicked mid-update; the set
    /// itself is still consistent.
    fn lock_open(&self) -> std::sync::MutexGuard<'_, BTreeSet<SessionLabel>> {
        self.open
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// End one driver session; a failure is logged, never propagated, because
/// the socket is gone either way.
async fn end_session(driver: &Driver, label: &SessionLabel) {
    let request = CuaRequestEnvelope {
        version: ProtocolVersion::V1,
        request_id: RequestId::try_from(format!("{}-end", label.as_str()))
            .expect("a session label plus a suffix is an identifier"),
        machine_id: driver.machine_id.clone(),
        action: CuaAction::EndSession(SessionRefArgs {
            session: Some(label.clone()),
        }),
    };
    match driver.adapter.execute(&request).await {
        Ok(envelope) => match envelope.response {
            CuaResponse::Success { .. } => eprintln!("[cua] session {} ended", label.as_str()),
            CuaResponse::Error { error } => eprintln!(
                "[cua] session {} did not end cleanly ({:?}): {}",
                label.as_str(),
                error.code,
                error.message.as_str()
            ),
        },
        Err(error) => eprintln!(
            "[cua] session {} did not end cleanly: {error}",
            label.as_str()
        ),
    }
}

static RUNTIME: std::sync::OnceLock<CuaRuntime> = std::sync::OnceLock::new();

/// The driver this desktop runs, from [`locate_driver`] under the workspace
/// `local_server::nolune_home` names, started with the default deadlines.
async fn spawn_installed_driver() -> Result<Arc<dyn DriverTransport>, String> {
    let workspace = crate::local_server::nolune_home();
    let driver = locate_driver(
        &workspace,
        std::env::var_os(DRIVER_ENV).as_deref(),
        std::env::var_os("PATH").as_deref(),
    )
    .ok_or_else(|| {
        format!(
            "no cua-driver installed: run `nolune cua install` (looked at {DRIVER_ENV}, {} and PATH)",
            workspace.join(INSTALL_MANIFEST).display()
        )
    })?;
    let transport = StdioDriverTransport::spawn_with(&driver, DriverTimeouts::default()).await?;
    Ok(Arc::new(transport))
}

/// The app's one runtime.
pub fn runtime() -> &'static CuaRuntime {
    RUNTIME
        .get_or_init(|| CuaRuntime::with_spawner(Arc::new(|| Box::pin(spawn_installed_driver()))))
}

/// How long the exit hook waits for the sessions to end and the child to die.
const EXIT_GRACE: Duration = Duration::from_secs(5);

/// Stop the driver as the app exits, bounded so a stuck driver never holds
/// the exit: the open sessions are ended and the child is killed.
pub fn shutdown_blocking() {
    let Some(runtime) = RUNTIME.get() else {
        return;
    };
    tauri::async_runtime::block_on(async {
        if tokio::time::timeout(EXIT_GRACE, runtime.shutdown())
            .await
            .is_err()
        {
            eprintln!("[cua] the driver did not stop within {EXIT_GRACE:?}");
        }
    });
}

// ---------------------------------------------------------------------------
// Overlay copy
// ---------------------------------------------------------------------------

/// The action name the overlay is told: the protocol's own spelling of the
/// kind (`click`, `type_text`, `get_window_state`).
pub fn action_name(kind: CuaActionKind) -> String {
    match serde_json::to_value(kind) {
        Ok(Value::String(name)) => name,
        _ => format!("{kind:?}").to_ascii_lowercase(),
    }
}

/// How many characters of typed text the overlay shows.
const DETAIL_PREVIEW_CHARS: usize = 30;

/// A short human detail for the overlay: what a pointer action targets, a
/// bounded preview of typed text, the app being launched. Never more than
/// what the user sees the companion do.
pub fn action_detail(action: &CuaAction) -> String {
    fn window(target: &WindowTarget) -> String {
        format!("window {} of pid {}", target.window_id, target.pid)
    }
    fn addressed(target: &WindowTarget, address: &ElementAddress) -> String {
        match address {
            ElementAddress::Point(point) => {
                format!("{} at {}, {}", window(target), point.x, point.y)
            }
            ElementAddress::ElementToken { .. } => format!("{} element", window(target)),
            ElementAddress::ElementIndex { element_index, .. } => {
                format!("{} element #{element_index}", window(target))
            }
        }
    }
    fn preview(text: &str) -> String {
        let short: String = text.chars().take(DETAIL_PREVIEW_CHARS).collect();
        if text.chars().count() > DETAIL_PREVIEW_CHARS {
            format!("{short}...")
        } else {
            short
        }
    }
    fn session(label: Option<&SessionLabel>) -> String {
        label
            .map(|label| label.as_str().to_owned())
            .unwrap_or_default()
    }
    match action {
        CuaAction::ListApps(_) | CuaAction::ListSessions(_) | CuaAction::HealthReport(_) => {
            String::new()
        }
        CuaAction::LaunchApp(args) => args
            .bundle_id
            .as_ref()
            .map(|bundle| bundle.as_str().to_owned())
            .or_else(|| args.name.as_ref().map(|name| name.as_str().to_owned()))
            .unwrap_or_default(),
        CuaAction::ListWindows(args) => {
            args.pid.map(|pid| format!("pid {pid}")).unwrap_or_default()
        }
        CuaAction::GetWindowState(args) => window(&args.target),
        CuaAction::SetWindowFrame(args) => window(&args.target),
        CuaAction::Click(args) => addressed(&args.target, &args.address),
        CuaAction::DoubleClick(args) => addressed(&args.target, &args.address),
        CuaAction::RightClick(args) => addressed(&args.target, &args.address),
        CuaAction::MoveCursor(args) => {
            format!(
                "{} to {}, {}",
                window(&args.target),
                args.point.x,
                args.point.y
            )
        }
        CuaAction::Drag(args) => format!(
            "{} from {}, {} to {}, {}",
            window(&args.target),
            args.from.x,
            args.from.y,
            args.to.x,
            args.to.y
        ),
        CuaAction::Scroll(args) => format!("{:?}", args.direction).to_ascii_lowercase(),
        CuaAction::TypeText(args) => preview(args.text.as_str()),
        CuaAction::PressKey(args) => args.key.as_str().to_owned(),
        CuaAction::Hotkey(args) => args
            .keys
            .iter()
            .map(|key| key.as_str())
            .collect::<Vec<_>>()
            .join("+"),
        CuaAction::SetValue(args) => window(&args.target),
        CuaAction::InvokeMenu(args) => args
            .path
            .iter()
            .map(|item| item.as_str())
            .collect::<Vec<_>>()
            .join(" > "),
        CuaAction::VerifyState(args) => window(&args.target),
        CuaAction::StartSession(args) => session(args.session.as_ref()),
        CuaAction::GetSession(args) | CuaAction::EndSession(args) => session(args.session.as_ref()),
    }
}

#[cfg(test)]
pub(crate) mod fake {
    //! An in-memory driver for tests: records every call and answers from a
    //! queue of canned outcomes.

    use super::*;

    /// What the fake does with one call.
    enum Canned {
        Answer(CallOutcome),
        /// Held until the fake is gone: a driver wedged on a permission
        /// prompt, which only a deadline or a kill releases.
        Stall,
    }

    pub struct FakeTransport {
        calls: Mutex<Vec<(String, Map<String, Value>)>>,
        outcomes: Mutex<VecDeque<Canned>>,
        closed: std::sync::atomic::AtomicBool,
        /// Flipped by `close` and by `crash`: the stand-in for a child that
        /// is no longer there.
        gone: tokio::sync::watch::Sender<bool>,
    }

    impl FakeTransport {
        pub fn answering(outcomes: impl IntoIterator<Item = CallOutcome>) -> Arc<Self> {
            Arc::new(Self {
                calls: Mutex::new(Vec::new()),
                outcomes: Mutex::new(outcomes.into_iter().map(Canned::Answer).collect()),
                closed: std::sync::atomic::AtomicBool::new(false),
                gone: tokio::sync::watch::Sender::new(false),
            })
        }

        /// After the answers queued so far, hold the next call for as long
        /// as the fake lives.
        pub fn then_stall(&self) {
            self.outcomes.lock().unwrap().push_back(Canned::Stall);
        }

        /// Queue more answers after the ones already there.
        pub fn also_answering(&self, outcomes: impl IntoIterator<Item = CallOutcome>) {
            self.outcomes
                .lock()
                .unwrap()
                .extend(outcomes.into_iter().map(Canned::Answer));
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
            let name = name.to_owned();
            match self.outcomes.lock().unwrap().pop_front() {
                Some(Canned::Answer(outcome)) => Box::pin(async move { outcome }),
                Some(Canned::Stall) => {
                    let mut gone = self.gone.subscribe();
                    Box::pin(async move {
                        let _ = gone.wait_for(|gone| *gone).await;
                        Err(DriverCallFailure::Transport(format!(
                            "fake driver went away while {name} was pending"
                        )))
                    })
                }
                None => Box::pin(async move {
                    Err(DriverCallFailure::Transport(format!(
                        "fake driver has no answer for {name}"
                    )))
                }),
            }
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

    fn start_session(label: &str) -> CuaAction {
        CuaAction::StartSession(StartSessionArgs {
            session: Some(SessionLabel::try_from(label).unwrap()),
        })
    }

    fn labels(labels: &[&str]) -> Vec<SessionLabel> {
        labels
            .iter()
            .map(|label| SessionLabel::try_from(*label).unwrap())
            .collect()
    }

    /// Flips a flag when dropped: proves a future was dropped, not left
    /// hanging.
    struct DropFlag(Arc<std::sync::atomic::AtomicBool>);

    impl Drop for DropFlag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
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

        runtime
            .handle(&request("s-1", STUDIO, start_session("nolune-run-1")))
            .await;
        runtime
            .handle(&request("s-2", STUDIO, start_session("nolune-run-2")))
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
            .handle(&request("s-3", STUDIO, start_session("nolune-run-3")))
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
    async fn a_disconnect_cut_short_keeps_the_sessions_it_did_not_end() {
        let fake = FakeTransport::answering([
            Ok(payload(HEALTHY)),
            Ok(started("nolune-run-1")),
            Ok(started("nolune-run-2")),
            Ok(ended("nolune-run-1")),
        ]);
        fake.then_stall();
        let (runtime, _) = runtime_over(vec![fake.clone()]);
        runtime.start(STUDIO).await.unwrap();
        runtime
            .handle(&request("s-1", STUDIO, start_session("nolune-run-1")))
            .await;
        runtime
            .handle(&request("s-2", STUDIO, start_session("nolune-run-2")))
            .await;
        assert_eq!(
            runtime.open_sessions(),
            labels(&["nolune-run-1", "nolune-run-2"])
        );

        // The bridge drops a closed socket's `end_sessions` when the user
        // disconnects explicitly meanwhile: the first session was ended,
        // the driver still holds the second.
        let cut_short =
            tokio::time::timeout(Duration::from_millis(100), runtime.end_sessions()).await;
        assert!(cut_short.is_err(), "the fake holds the second end_session");
        assert_eq!(
            &fake.tools_called()[3..],
            ["end_session", "end_session"],
            "the sessions are ended one at a time"
        );
        assert_eq!(
            runtime.open_sessions(),
            labels(&["nolune-run-2"]),
            "a session the driver still holds is not forgotten"
        );

        // The disconnect's own `end_sessions` ends what is left.
        fake.also_answering([Ok(ended("nolune-run-2"))]);
        runtime.end_sessions().await;
        assert_eq!(runtime.open_sessions(), Vec::<SessionLabel>::new());
        let calls = fake.calls();
        assert_eq!(calls.len(), 6);
        assert_eq!(calls[5].0, "end_session");
        assert_eq!(
            calls[5].1,
            json!({"session": "nolune-run-2"})
                .as_object()
                .unwrap()
                .clone()
        );
        assert!(
            runtime.is_running().await,
            "the driver stays for the reconnect"
        );
    }

    #[tokio::test]
    async fn an_exit_while_the_driver_is_still_reporting_stops_it_for_good() {
        // The first health_report wedges (a permission prompt): `start`
        // holds the driver lock for as long as the driver takes.
        let wedged = FakeTransport::answering([]);
        wedged.then_stall();
        let spare = FakeTransport::answering([Ok(payload(HEALTHY))]);
        let (runtime, spawns) = runtime_over(vec![wedged.clone(), spare.clone()]);

        let (started, stopped) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(runtime.start(STUDIO), async {
                // Once the driver holds the report, the app exits.
                while wedged.tools_called().is_empty() {
                    tokio::task::yield_now().await;
                }
                tokio::time::timeout(Duration::from_secs(1), runtime.shutdown()).await
            })
        })
        .await
        .expect("start lets go of the driver once the app exits");
        stopped.expect("shutdown is not starved by a description in flight");
        let error = started.unwrap_err();
        assert!(error.contains("exiting"), "{error}");
        assert!(wedged.closed(), "the wedged driver is killed");

        // Nothing starts again: the reconnect loop's next `start` spawns
        // nothing and a late request restarts nothing.
        let error = runtime.start(STUDIO).await.unwrap_err();
        assert!(error.contains("exiting"), "{error}");
        let late = error_of(&runtime.handle(&request("late", STUDIO, list_apps())).await);
        assert_eq!(late.code, RuntimeErrorCode::RuntimeUnavailable);
        assert_eq!(spawns.load(Ordering::SeqCst), 1, "no driver after the exit");
        assert!(spare.tools_called().is_empty());
        assert!(!runtime.is_running().await);
    }

    #[tokio::test]
    async fn an_exit_during_the_handshake_drops_the_spawn_and_nothing_starts_again() {
        // A driver still in its MCP handshake past the exit grace: the spawn
        // is dropped, which kills the child, instead of holding the exit.
        let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let spawns = Arc::new(AtomicUsize::new(0));
        let runtime = CuaRuntime::with_spawner(Arc::new({
            let dropped = dropped.clone();
            let spawns = spawns.clone();
            move || {
                spawns.fetch_add(1, Ordering::SeqCst);
                let flag = DropFlag(dropped.clone());
                Box::pin(async move {
                    let _flag = flag;
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    Err("the handshake never completes".to_owned())
                })
            }
        }));

        let (started, stopped) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(runtime.start(STUDIO), async {
                while spawns.load(Ordering::SeqCst) == 0 {
                    tokio::task::yield_now().await;
                }
                tokio::time::timeout(Duration::from_secs(1), runtime.shutdown()).await
            })
        })
        .await
        .expect("start lets go of the spawn once the app exits");
        stopped.expect("shutdown is not starved by a handshake in flight");
        let error = started.unwrap_err();
        assert!(error.contains("exiting"), "{error}");
        assert!(
            dropped.load(Ordering::SeqCst),
            "the spawn is dropped, and its child with it"
        );

        let error = runtime.start(STUDIO).await.unwrap_err();
        assert!(error.contains("exiting"), "{error}");
        assert_eq!(spawns.load(Ordering::SeqCst), 1, "nothing spawns again");
        assert!(!runtime.is_running().await);
    }

    #[tokio::test]
    async fn a_shutdown_cut_short_by_a_wedged_session_end_still_kills_the_driver() {
        let fake = FakeTransport::answering([
            Ok(payload(HEALTHY)),
            Ok(started("nolune-run-1")),
            Ok(started("nolune-run-2")),
        ]);
        fake.then_stall();
        let (runtime, spawns) = runtime_over(vec![fake.clone()]);
        runtime.start(STUDIO).await.unwrap();
        runtime
            .handle(&request("s-1", STUDIO, start_session("nolune-run-1")))
            .await;
        runtime
            .handle(&request("s-2", STUDIO, start_session("nolune-run-2")))
            .await;

        // The exit grace runs out while the driver holds the first
        // end_session: `shutdown` is dropped mid-way.
        let cut_short = tokio::time::timeout(Duration::from_millis(100), runtime.shutdown()).await;
        assert!(cut_short.is_err(), "the fake holds the end_session");
        assert_eq!(fake.tools_called().len(), 4);
        assert_eq!(fake.tools_called()[3], "end_session");
        assert_eq!(
            runtime.open_sessions(),
            labels(&["nolune-run-1", "nolune-run-2"]),
            "nothing the driver did not confirm ended is forgotten"
        );
        assert!(fake.closed(), "a shutdown cut short still kills the driver");

        // The exit hook still has the driver to wait for.
        let transport = runtime
            .kill_driver()
            .expect("the killed driver is there to wait for");
        tokio::time::timeout(Duration::from_secs(1), transport.exited())
            .await
            .expect("the killed driver is gone");

        // Nothing starts again.
        let error = runtime.start(STUDIO).await.unwrap_err();
        assert!(error.contains("exiting"), "{error}");
        assert_eq!(spawns.load(Ordering::SeqCst), 1);
        assert!(!runtime.is_running().await);
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
        // One line: the stub answers over line-delimited JSON-RPC.
        let report = dir.path().join("health.json");
        std::fs::write(&report, payload(HEALTHY).to_string()).unwrap();
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

    /// Against the installed driver (needs `cua-driver`: `NOLUNE_CUA_DRIVER`,
    /// a workspace install or `PATH`). Read-only: the driver is described,
    /// asked for its sessions, and stopped without a child left behind.
    #[cfg(unix)]
    #[tokio::test]
    #[ignore]
    async fn live_driver_describes_this_machine_and_leaves_no_child() {
        let children = || -> BTreeSet<String> {
            let listed = std::process::Command::new("pgrep")
                .args(["-f", "cua-driver mcp"])
                .output()
                .expect("pgrep runs");
            String::from_utf8_lossy(&listed.stdout)
                .lines()
                .map(str::to_owned)
                .collect()
        };
        let before = children();
        let runtime = CuaRuntime::with_spawner(Arc::new(|| Box::pin(spawn_installed_driver())));
        let descriptor = runtime
            .start(STUDIO)
            .await
            .expect("the installed driver reports");
        assert_eq!(descriptor.location, MachineLocation::Desktop);
        assert_eq!(descriptor.machine_id, id());
        assert_eq!(descriptor.platform, cua_protocol::Platform::Macos);
        assert_eq!(
            descriptor.driver_version.as_str(),
            cua_protocol::cua_driver_pin::PINNED_VERSION
        );
        eprintln!(
            "live: {:?}, accessibility {:?}, screen capture {:?}, {} capabilities",
            descriptor.health,
            descriptor.permissions.accessibility,
            descriptor.permissions.screen_capture,
            descriptor.capabilities.len()
        );
        assert!(
            children().len() > before.len(),
            "the runtime's own child is running"
        );

        let sessions = CuaAction::ListSessions(cua_protocol::ListSessionsArgs {
            cursor: None,
            limit: None,
        });
        let frame = runtime.handle(&request("live-1", STUDIO, sessions)).await;
        match answer(&frame).response {
            CuaResponse::Success { result } => {
                assert!(matches!(*result, CuaActionResult::ListSessions(_)))
            }
            CuaResponse::Error { error } => panic!("{error:?}"),
        }
        // Reconnecting keeps the one child; a forged frame still never runs.
        runtime.end_sessions().await;
        runtime.start(STUDIO).await.unwrap();
        let forged = json!({"version": "v1", "request_id": "live-2", "machine_id": STUDIO,
            "action": {"tool": "clipboard_read", "args": {}}});
        assert_eq!(
            runtime.handle(&forged).await["response"]["response"]["error"]["code"],
            "capability_denied"
        );

        runtime.shutdown().await;
        let gone_by = std::time::Instant::now() + Duration::from_secs(5);
        while children() != before && std::time::Instant::now() < gone_by {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(
            children(),
            before,
            "shutdown leaves no cua-driver mcp child"
        );
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
