//! One supervised `codex app-server` child.
//!
//! [`AppServer`] owns at most one child at a time. It completes the
//! `initialize` handshake within a deadline, correlates answers to requests
//! by id (the app-server answers out of order), hands notifications and the
//! app-server's own requests to every subscriber (and refuses a request
//! nobody is listening for, so a turn never waits on an answer that cannot
//! come), and when the child dies it fails what was pending with a typed
//! error and starts a fresh child on the next request, after a bounded
//! exponential backoff. Every exchange is held to one deadline that covers
//! the write as well as the wait: a child that keeps stdout open but stops
//! reading stdin is killed, not waited for. `close` kills the child and
//! refuses restarts; dropping the last handle kills it too, so no child
//! outlives the gateway.
//!
//! The child is owned by its pump task, which reads stdout until end of
//! file, kills the child when told to stop or when the last handle is
//! dropped, and reaps it. Nothing else touches the process, so there is
//! exactly one place where a child dies and one place that says why.

use std::{
    collections::{HashMap, VecDeque},
    ffi::OsString,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt as _, AsyncWriteExt as _},
    sync::{broadcast, oneshot, watch},
    time::Instant,
};

use super::{
    APP_SERVER_ARGS, AppServerError,
    protocol::{self, Frame, FrameError, INITIALIZE, INITIALIZED, RequestId, RpcError},
};

/// How many events a subscriber may fall behind before it is told so.
const EVENT_BUFFER: usize = 1024;
/// How many of the child's stderr lines are kept for an error message.
const STDERR_LINES_KEPT: usize = 16;
/// How long a stderr line may be in an error message.
const STDERR_LINE_LIMIT: usize = 400;
/// How long the stderr reader gets to drain after the child died.
const STDERR_DRAIN: Duration = Duration::from_millis(300);
/// How long a child that closed stdout gets to exit before it is killed,
/// and how long a failed write waits for the pump to say why.
const EXIT_GRACE: Duration = Duration::from_secs(2);

/// The deadlines one child is held to.
///
/// Without them a binary that never speaks the protocol (a wrapper script,
/// the wrong build) wedges the handshake, and a request the app-server
/// never answers, or never reads, wedges its caller, silently and forever.
/// Each deadline covers the write too: a frame larger than the pipe buffer
/// to a child that stopped reading stdin would otherwise block its writer,
/// and behind it every other writer, for as long as stdout stays open.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Timeouts {
    /// How long a fresh child may take to read and answer `initialize` and
    /// read `initialized`.
    pub handshake: Duration,
    /// How long one request may take to be written and answered. Turn
    /// output streams as notifications, so a request is only ever a short
    /// exchange. A notification or an answer to the app-server gets the
    /// same bound on its write.
    pub request: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            handshake: Duration::from_secs(15),
            request: Duration::from_secs(30),
        }
    }
}

/// How restarts are paced after a child dies or fails to start.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RestartPolicy {
    /// The wait before the first restart; each consecutive failure doubles it.
    pub base: Duration,
    /// The longest wait, however many failures in a row.
    pub max: Duration,
    /// A child that served this long before dying resets the failure count.
    pub stable_after: Duration,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self {
            base: Duration::from_millis(250),
            max: Duration::from_secs(10),
            stable_after: Duration::from_secs(30),
        }
    }
}

impl RestartPolicy {
    /// The wait before the next start after `failures` consecutive failures.
    pub fn delay(&self, failures: u32) -> Duration {
        if failures == 0 {
            return Duration::ZERO;
        }
        let doublings = (failures - 1).min(30);
        self.base.saturating_mul(1u32 << doublings).min(self.max)
    }
}

/// How to start the child.
#[derive(Clone, Debug)]
pub struct Launch {
    /// The verified binary, from [`discovery`](super::discovery).
    pub binary: PathBuf,
    /// Its arguments; [`APP_SERVER_ARGS`] unless a test says otherwise.
    pub args: Vec<OsString>,
    /// Extra environment for the child.
    pub env: Vec<(OsString, OsString)>,
    pub timeouts: Timeouts,
    pub restart: RestartPolicy,
}

impl Launch {
    pub fn new(binary: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
            args: APP_SERVER_ARGS.iter().map(OsString::from).collect(),
            env: Vec::new(),
            timeouts: Timeouts::default(),
            restart: RestartPolicy::default(),
        }
    }

    /// `codex app-server`, for messages.
    fn command_line(&self) -> String {
        std::iter::once(self.binary.display().to_string())
            .chain(
                self.args
                    .iter()
                    .map(|arg| arg.to_string_lossy().into_owned()),
            )
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// What subscribers hear from the child, in the order it happened.
#[derive(Clone, Debug, PartialEq)]
pub enum Incoming {
    /// A child completed the handshake; `generation` counts children.
    Started { generation: u64, pid: u32 },
    /// A streamed event.
    Notification { method: String, params: Value },
    /// A request the app-server sent; answer it with
    /// [`AppServer::respond`] and the same `id`. The app-server waits for
    /// that answer for as long as it takes, so a subscriber that fell
    /// behind (`RecvError::Lagged`) may have missed one and must not carry
    /// on as if the turn were healthy: interrupt the turn or close the
    /// supervisor. When nobody is subscribed at all, the supervisor refuses
    /// the request itself (error `-32601`) rather than let the turn wait.
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    /// The child is gone, by a crash, an exit or a close. Requests that
    /// were pending failed with [`AppServerError::Exited`]; the next
    /// request starts a new child unless the supervisor is closed.
    Exited { generation: u64, reason: String },
}

/// The `initialize` params: who is asking, and that the experimental
/// surface (`dynamicTools`, `item/tool/call`) is wanted.
fn initialize_params() -> Value {
    json!({
        "clientInfo": {
            "name": "nolune",
            "title": "Nolune",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "capabilities": {"experimentalApi": true},
    })
}

/// The last lines a child wrote to stderr, read as they arrive.
///
/// The app-server explains itself there (a missing login, a bad config
/// value), and only a message that repeats it is actionable.
struct StderrTail {
    lines: Arc<Mutex<VecDeque<String>>>,
    reader: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl StderrTail {
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
                    log::info!("[codex] app-server: {line}");
                    let mut kept = lines.lock().unwrap();
                    if kept.len() == STDERR_LINES_KEPT {
                        kept.pop_front();
                    }
                    kept.push_back(line.chars().take(STDERR_LINE_LIMIT).collect());
                }
            })
        });
        Self {
            lines,
            reader: Mutex::new(reader),
        }
    }

    /// `; it said: "..."` once the child is gone and its stderr is drained,
    /// or nothing when it said nothing.
    async fn suffix(&self) -> String {
        let reader = self.reader.lock().unwrap().take();
        if let Some(mut reader) = reader {
            // The child is dead by now; give the reader a moment to reach
            // end of file. Past that the pipe is held open by a grandchild:
            // report what arrived so far and stop reading.
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
                "; it said: \"{}\"",
                lines.iter().cloned().collect::<Vec<_>>().join(" | ")
            )
        }
    }
}

/// What the pump task hands to a request: the answer, the error object, or
/// an answer that could not be read.
type Answer = Result<Value, AppServerError>;

/// One child's state, shared by its handle and its pump task. The task
/// holds no handle, so dropping the last handle stops the task.
struct Shared {
    generation: u64,
    pid: u32,
    started_at: Instant,
    /// Requests waiting for their answer, by id.
    pending: Mutex<HashMap<RequestId, oneshot::Sender<Answer>>>,
    /// `Some(reason)` once the child is gone.
    exit: watch::Sender<Option<String>>,
    stderr: StderrTail,
}

impl Shared {
    fn exit_reason(&self) -> Option<String> {
        self.exit.borrow().clone()
    }

    #[allow(dead_code)]
    fn is_gone(&self) -> bool {
        self.exit.borrow().is_some()
    }

    /// How long ago the child started. Measured when the next child is
    /// wanted, it tells a hot crash loop (short) from a rare death (long):
    /// a child that died long ago but was only missed now was not looping.
    fn age(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// The exit reason, waiting briefly for the pump to establish it when a
    /// write just failed.
    async fn exit_reason_soon(&self) -> Option<String> {
        let mut exit = self.exit.subscribe();
        let gone = tokio::time::timeout(EXIT_GRACE, exit.wait_for(|reason| reason.is_some())).await;
        match gone {
            Ok(Ok(reason)) => reason.clone(),
            _ => self.exit_reason(),
        }
    }

    /// Record the exit and fail every pending request: the receivers see a
    /// dropped sender and read the reason. The reason is set first, so a
    /// request that registers after this sees it too.
    fn exited(&self, reason: String) {
        self.exit.send_replace(Some(reason));
        self.pending.lock().unwrap().clear();
    }

    /// `codex app-server (pid N) <what happened>`: every reason a child is
    /// gone is phrased the same way, whoever established it.
    fn describe(&self, what_happened: &str) -> String {
        format!("codex app-server (pid {}) {what_happened}", self.pid)
    }
}

/// One live child: where to write, and what the pump shares.
struct Connection {
    shared: Arc<Shared>,
    stdin: tokio::sync::Mutex<tokio::process::ChildStdin>,
    /// `Some(why)`, or dropped, makes the pump kill the child; the first
    /// reason given is the one reported.
    stop: watch::Sender<Option<String>>,
}

/// A request the app-server sent that no subscriber received.
struct Unheard {
    id: Value,
    method: String,
}

impl Connection {
    /// Spawn the child and complete the handshake within
    /// `launch.timeouts.handshake`; a child that has not answered by then is
    /// killed and the error repeats what it said on stderr.
    async fn spawn(
        launch: &Launch,
        generation: u64,
        next_id: &AtomicU64,
        events: &broadcast::Sender<Incoming>,
    ) -> Result<Arc<Self>, AppServerError> {
        let mut command = tokio::process::Command::new(&launch.binary);
        command
            .args(&launch.args)
            .envs(launch.env.iter().map(|(key, value)| (key, value)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|error| {
            AppServerError::Handshake(format!(
                "could not start {}: {error}",
                launch.command_line()
            ))
        })?;
        let pid = child.id().ok_or_else(|| {
            AppServerError::Handshake(format!(
                "{} exited before it was seen",
                launch.command_line()
            ))
        })?;
        let stdin = child.stdin.take().expect("stdin is piped");
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = StderrTail::capture(child.stderr.take());
        let shared = Arc::new(Shared {
            generation,
            pid,
            started_at: Instant::now(),
            pending: Mutex::new(HashMap::new()),
            exit: watch::Sender::new(None),
            stderr,
        });
        let (stop, stopped) = watch::channel(None);
        let connection = Arc::new(Self {
            shared: shared.clone(),
            stdin: tokio::sync::Mutex::new(stdin),
            stop,
        });
        tokio::spawn(pump(
            child,
            stdout,
            shared,
            Arc::downgrade(&connection),
            events.clone(),
            stopped,
            launch.timeouts.request,
        ));

        // One deadline for the whole handshake: the `initialize` exchange
        // and the `initialized` notification that completes it.
        let handshake = launch.timeouts.handshake;
        let due = Instant::now() + handshake;
        let hello = connection
            .request_by(next_id, INITIALIZE, initialize_params(), handshake, due)
            .await;
        let hello = match hello {
            Ok(hello) => hello,
            Err(AppServerError::Timeout { .. }) => {
                connection.stop("was killed after the handshake failed");
                return Err(AppServerError::Handshake(format!(
                    "{} did not answer initialize within {handshake:?}{}",
                    launch.command_line(),
                    connection.shared.stderr.suffix().await
                )));
            }
            Err(error) => {
                connection.stop("was killed after the handshake failed");
                return Err(AppServerError::Handshake(error.to_string()));
            }
        };
        if let Err(error) = connection
            .notify(INITIALIZED, Value::Object(Default::default()), due)
            .await
        {
            connection.stop("was killed after the handshake failed");
            return Err(AppServerError::Handshake(error.to_string()));
        }
        log::info!(
            "[codex] app-server started: pid {pid}, {}",
            hello
                .get("userAgent")
                .and_then(Value::as_str)
                .unwrap_or("no userAgent")
        );
        Ok(connection)
    }

    /// Ask the pump to kill the child, for this reason. Harmless when it
    /// already ended; a second reason does not replace the first.
    fn stop(&self, why: &str) {
        self.stop.send_if_modified(|current| {
            if current.is_some() {
                return false;
            }
            *current = Some(why.to_owned());
            true
        });
    }

    /// Why nothing may be sent to this child any more: the pump's reason
    /// once it reaped the child, or the kill that is on its way to it.
    fn gone_reason(&self) -> Option<String> {
        self.shared.exit_reason().or_else(|| {
            self.stop
                .borrow()
                .as_deref()
                .map(|why| self.shared.describe(why))
        })
    }

    /// Write one line by `due`. A child that is not reading is gone: the
    /// pump says why when it reaped the child (the write failed because it
    /// died), and otherwise the child is killed. Whatever was half-written
    /// is no frame, and a child that keeps stdout open while it ignores
    /// stdin would hold every writer behind this one for good.
    async fn send(&self, line: String, what: &str, due: Instant) -> Result<(), AppServerError> {
        let written = tokio::time::timeout_at(due, async {
            let mut stdin = self.stdin.lock().await;
            stdin.write_all(line.as_bytes()).await?;
            stdin.flush().await
        })
        .await;
        let reason = match written {
            Ok(Ok(())) => return Ok(()),
            Ok(Err(error)) => match self.shared.exit_reason_soon().await {
                Some(reason) => reason,
                None => self.kill(&format!(
                    "was killed because it stopped reading stdin: {error}"
                )),
            },
            Err(_elapsed) => match self.shared.exit_reason() {
                Some(reason) => reason,
                None => self.kill(&format!(
                    "was killed because it did not read {what} in time"
                )),
            },
        };
        Err(AppServerError::Exited(reason))
    }

    /// Kill the child for this reason and return the reason as reported.
    fn kill(&self, why: &str) -> String {
        log::warn!("[codex] {}", self.shared.describe(why));
        self.stop(why);
        self.gone_reason()
            .unwrap_or_else(|| self.shared.describe(why))
    }

    /// Send `method` and wait for its answer, all within `deadline`.
    async fn request(
        &self,
        next_id: &AtomicU64,
        method: &str,
        params: Value,
        deadline: Duration,
    ) -> Result<Value, AppServerError> {
        self.request_by(next_id, method, params, deadline, Instant::now() + deadline)
            .await
    }

    /// Send `method` and wait for its answer, both by `due`; `deadline` is
    /// how long that was, for the error.
    async fn request_by(
        &self,
        next_id: &AtomicU64,
        method: &str,
        params: Value,
        deadline: Duration,
        due: Instant,
    ) -> Result<Value, AppServerError> {
        if let Some(reason) = self.gone_reason() {
            return Err(AppServerError::Exited(reason));
        }
        let id = next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let (answer, waiting) = oneshot::channel();
        self.shared.pending.lock().unwrap().insert(id, answer);
        // The pump sets the reason before it fails the pending requests, so
        // a request registered after that would wait forever: check again.
        if let Some(reason) = self.shared.exit_reason() {
            self.shared.pending.lock().unwrap().remove(&id);
            return Err(AppServerError::Exited(reason));
        }
        if let Err(error) = self
            .send(protocol::request_line(id, method, params), method, due)
            .await
        {
            self.shared.pending.lock().unwrap().remove(&id);
            return Err(error);
        }
        match tokio::time::timeout_at(due, waiting).await {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(_dropped)) => Err(AppServerError::Exited(
                self.shared
                    .exit_reason()
                    .unwrap_or_else(|| "codex app-server went away".to_owned()),
            )),
            Err(_elapsed) => {
                self.shared.pending.lock().unwrap().remove(&id);
                Err(AppServerError::Timeout {
                    method: method.to_owned(),
                    after: deadline,
                })
            }
        }
    }

    async fn notify(
        &self,
        method: &str,
        params: Value,
        due: Instant,
    ) -> Result<(), AppServerError> {
        self.send(protocol::notification_line(method, params), method, due)
            .await
    }

    async fn respond(
        &self,
        id: &Value,
        outcome: Result<Value, RpcError>,
        due: Instant,
    ) -> Result<(), AppServerError> {
        let what = format!("the answer to its request {id}");
        self.send(protocol::response_line(id, outcome), &what, due)
            .await
    }
}

/// Route one line the child wrote: answers to their requests, everything
/// else to the subscribers. A line that is not JSON is chatter (the test
/// harness prints some; codex prints none), a JSON object that is not a
/// frame fails the request it was for, when it names one. A request from
/// the app-server that no subscriber received is returned, for the caller
/// to refuse: the app-server would wait for its answer forever.
fn dispatch(line: &str, shared: &Shared, events: &broadcast::Sender<Incoming>) -> Option<Unheard> {
    match protocol::parse_frame(line) {
        Ok(Frame::Response { id, outcome }) => match shared.pending.lock().unwrap().remove(&id) {
            Some(waiting) => {
                let _ = waiting.send(outcome.map_err(AppServerError::Rpc));
            }
            None => log::debug!("[codex] answer to request {id} arrived after its deadline"),
        },
        Ok(Frame::Notification { method, params }) => {
            let _ = events.send(Incoming::Notification { method, params });
        }
        Ok(Frame::Request { id, method, params }) => {
            let heard = events.send(Incoming::Request {
                id: id.clone(),
                method: method.clone(),
                params,
            });
            if heard.is_err() {
                return Some(Unheard { id, method });
            }
        }
        Err(FrameError::NotJson(_)) => log::debug!("[codex] app-server stdout: {line}"),
        Err(FrameError::Shape(why)) => {
            let addressed = serde_json::from_str::<Value>(line)
                .ok()
                .and_then(|frame| frame.get("id")?.as_u64())
                .and_then(|id| shared.pending.lock().unwrap().remove(&id));
            match addressed {
                Some(waiting) => {
                    let _ = waiting.send(Err(AppServerError::Protocol(why)));
                }
                None => log::warn!("[codex] app-server wrote an unreadable frame: {why}"),
            }
        }
    }
    None
}

/// Own the child: read its stdout until end of file or a stop, then reap
/// it, fail what was pending and tell the subscribers. A request from the
/// app-server that nobody received is refused from a task of its own: the
/// pump must keep reading stdout while that answer is written, or a child
/// blocked on a full stdout and a pump blocked on a full stdin would wait
/// for each other.
async fn pump(
    mut child: tokio::process::Child,
    stdout: tokio::process::ChildStdout,
    shared: Arc<Shared>,
    writer: Weak<Connection>,
    events: broadcast::Sender<Incoming>,
    mut stopped: watch::Receiver<Option<String>>,
    write_deadline: Duration,
) {
    let mut lines = tokio::io::BufReader::new(stdout).lines();
    let mut killed = None;
    loop {
        tokio::select! {
            line = lines.next_line() => match line {
                Ok(Some(line)) => {
                    if let Some(unheard) = dispatch(&line, &shared, &events)
                        && let Some(connection) = writer.upgrade()
                    {
                        tokio::spawn(refuse(connection, unheard, write_deadline));
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    log::warn!("[codex] app-server stdout: {error}");
                    break;
                }
            },
            changed = stopped.changed() => {
                // A stop, or the last handle dropped (`changed` is Err then).
                let why = match changed {
                    Ok(()) => stopped.borrow_and_update().clone(),
                    Err(_dropped) => Some("was stopped".to_owned()),
                };
                if let Some(why) = why {
                    killed = Some(why);
                    let _ = child.start_kill();
                    break;
                }
            }
        }
    }
    // Stdout closed: the child is exiting. One that lingers is killed.
    let status = match tokio::time::timeout(EXIT_GRACE, child.wait()).await {
        Ok(Ok(status)) => Some(status),
        Ok(Err(_)) => None,
        Err(_lingering) => {
            let _ = child.start_kill();
            child.wait().await.ok()
        }
    };
    let reason = match killed {
        Some(why) => {
            let reason = shared.describe(&why);
            log::info!("[codex] {reason}");
            reason
        }
        None => {
            let status = status.map_or("an unknown status".to_owned(), |status| status.to_string());
            let reason = shared.describe(&format!(
                "exited with {status}{}",
                shared.stderr.suffix().await
            ));
            log::warn!("[codex] {reason}");
            reason
        }
    };
    shared.exited(reason.clone());
    let _ = events.send(Incoming::Exited {
        generation: shared.generation,
        reason,
    });
}

/// Answer a request from the app-server that no subscriber received with
/// an error, so the turn it belongs to fails instead of waiting for an
/// answer that cannot come.
async fn refuse(connection: Arc<Connection>, unheard: Unheard, write_deadline: Duration) {
    let Unheard { id, method } = unheard;
    log::warn!(
        "[codex] nobody is subscribed to answer the app-server's {method} request {id}: refusing it"
    );
    let refusal = RpcError {
        code: -32601,
        message: format!("nolune has no handler listening for {method}"),
        data: None,
    };
    let due = Instant::now() + write_deadline;
    if let Err(error) = connection.respond(&id, Err(refusal), due).await {
        log::warn!("[codex] could not refuse the app-server's {method} request {id}: {error}");
    }
}

struct State {
    connection: Option<Arc<Connection>>,
    closed: bool,
    /// How many children completed the handshake.
    generation: u64,
    /// Consecutive short-lived children and failed starts.
    failures: u32,
}

struct Supervisor {
    launch: Launch,
    state: Mutex<State>,
    /// Held while a child is being started, so concurrent callers wait for
    /// that one instead of each starting their own.
    starting: tokio::sync::Mutex<()>,
    next_id: AtomicU64,
    events: broadcast::Sender<Incoming>,
}

/// The supervised child. Cheap to clone; every clone is the same process.
#[derive(Clone)]
pub struct AppServer {
    inner: Arc<Supervisor>,
}

impl AppServer {
    /// Start the child and complete the handshake now, so a binary that
    /// cannot serve is reported at start rather than at the first turn.
    pub async fn start(launch: Launch) -> Result<Self, AppServerError> {
        let (events, _) = broadcast::channel(EVENT_BUFFER);
        let server = Self {
            inner: Arc::new(Supervisor {
                launch,
                state: Mutex::new(State {
                    connection: None,
                    closed: false,
                    generation: 0,
                    failures: 0,
                }),
                starting: tokio::sync::Mutex::new(()),
                next_id: AtomicU64::new(0),
                events,
            }),
        };
        server.connection().await?;
        Ok(server)
    }

    /// The live child, when there is one; `Closed` after `close`.
    fn live(&self) -> Result<Option<Arc<Connection>>, AppServerError> {
        let state = self.inner.state.lock().unwrap();
        if state.closed {
            return Err(AppServerError::Closed);
        }
        Ok(state
            .connection
            .clone()
            .filter(|connection| connection.gone_reason().is_none()))
    }

    /// The live child, or why there is none right now. Nothing is started:
    /// an answer or a notification is addressed to the child that asked,
    /// and a fresh child never asked anything.
    fn current(&self) -> Result<Arc<Connection>, AppServerError> {
        let state = self.inner.state.lock().unwrap();
        if state.closed {
            return Err(AppServerError::Closed);
        }
        match &state.connection {
            Some(connection) => match connection.gone_reason() {
                None => Ok(connection.clone()),
                Some(reason) => Err(AppServerError::Exited(reason)),
            },
            None => Err(AppServerError::Exited(
                "codex app-server is not running".to_owned(),
            )),
        }
    }

    /// The live child, or a fresh one: the previous child's death is
    /// accounted for, the backoff waited out, and exactly one child is
    /// started however many callers arrive at once.
    async fn connection(&self) -> Result<Arc<Connection>, AppServerError> {
        if let Some(live) = self.live()? {
            return Ok(live);
        }
        let _starting = self.inner.starting.lock().await;
        if let Some(live) = self.live()? {
            return Ok(live);
        }
        let restart = self.inner.launch.restart;
        let (delay, generation) = {
            let mut state = self.inner.state.lock().unwrap();
            if let Some(dead) = state.connection.take() {
                if dead.shared.age() >= restart.stable_after {
                    state.failures = 0;
                }
                state.failures += 1;
            }
            (restart.delay(state.failures), state.generation + 1)
        };
        if !delay.is_zero() {
            log::info!("[codex] starting the app-server again in {delay:?}");
            tokio::time::sleep(delay).await;
        }
        if self.inner.state.lock().unwrap().closed {
            return Err(AppServerError::Closed);
        }
        let started = Connection::spawn(
            &self.inner.launch,
            generation,
            &self.inner.next_id,
            &self.inner.events,
        )
        .await;
        let connection = match started {
            Ok(connection) => connection,
            Err(error) => {
                self.inner.state.lock().unwrap().failures += 1;
                return Err(error);
            }
        };
        let pid = connection.shared.pid;
        {
            let mut state = self.inner.state.lock().unwrap();
            // Closed while the child was starting: it must not outlive that.
            if state.closed {
                connection.stop("was stopped");
                return Err(AppServerError::Closed);
            }
            state.generation = generation;
            state.connection = Some(connection.clone());
        }
        let _ = self
            .inner
            .events
            .send(Incoming::Started { generation, pid });
        Ok(connection)
    }

    /// Send a request and wait for its answer, starting a child first when
    /// the previous one is gone.
    pub async fn request(&self, method: &str, params: Value) -> Result<Value, AppServerError> {
        let connection = self.connection().await?;
        connection
            .request(
                &self.inner.next_id,
                method,
                params,
                self.inner.launch.timeouts.request,
            )
            .await
    }

    /// Send a notification to the live child; there is no child to start
    /// for one. The write is held to the request deadline.
    #[cfg_attr(not(test), allow(dead_code))]
    pub async fn notify(&self, method: &str, params: Value) -> Result<(), AppServerError> {
        self.current()?.notify(method, params, self.due()).await
    }

    /// Answer a request the app-server sent ([`Incoming::Request`]). A
    /// child that died since it asked is reported, never replaced: its
    /// question died with it.
    pub async fn respond(
        &self,
        id: &Value,
        outcome: Result<Value, RpcError>,
    ) -> Result<(), AppServerError> {
        self.current()?.respond(id, outcome, self.due()).await
    }

    /// When a write started now must be done by.
    fn due(&self) -> Instant {
        Instant::now() + self.inner.launch.timeouts.request
    }

    /// Listen to what the child says from now on. The channel keeps the
    /// last [`EVENT_BUFFER`] events for a subscriber that falls behind; one
    /// that is told it lagged may have missed an [`Incoming::Request`] the
    /// app-server is still waiting on, and must interrupt the turn or close
    /// the supervisor rather than carry on.
    pub fn subscribe(&self) -> broadcast::Receiver<Incoming> {
        self.inner.events.subscribe()
    }

    /// The live child's pid, if there is one.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn pid(&self) -> Option<u32> {
        self.live()
            .ok()
            .flatten()
            .map(|connection| connection.shared.pid)
    }

    /// How many children completed the handshake so far.
    pub fn generation(&self) -> u64 {
        self.inner.state.lock().unwrap().generation
    }

    /// The binary this supervisor starts; the status route (27d) reports it.
    #[allow(dead_code)]
    pub fn binary(&self) -> &Path {
        &self.inner.launch.binary
    }

    /// Kill the child and refuse restarts. Idempotent.
    pub fn close(&self) {
        let connection = {
            let mut state = self.inner.state.lock().unwrap();
            state.closed = true;
            state.connection.take()
        };
        if let Some(connection) = connection {
            connection.stop("was stopped");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{CODEX_VERSION, discovery, fake, protocol::TURN_INTERRUPT};
    use super::*;
    use crate::services::llm::contract::LlmError;
    use serde_json::json;
    use std::{
        fs,
        time::{Duration, Instant},
    };

    /// Whether a process with this id still exists (a reaped one does not).
    #[cfg(unix)]
    fn process_exists(pid: u32) -> bool {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    /// Wait for the process to be gone, or fail.
    #[cfg(unix)]
    async fn assert_gone(pid: u32) {
        let gone_by = Instant::now() + Duration::from_secs(5);
        while process_exists(pid) && Instant::now() < gone_by {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(!process_exists(pid), "child {pid} was left running");
    }

    /// The pids the fake recorded, one per start.
    fn recorded_pids(pid_file: &Path) -> Vec<u32> {
        fs::read_to_string(pid_file)
            .unwrap_or_default()
            .lines()
            .map(|line| line.trim().parse().unwrap())
            .collect()
    }

    /// Start the fake with a generous handshake: it is a whole test binary
    /// starting under whatever load the rest of the suite puts on the host.
    async fn start(pid_file: Option<&Path>) -> AppServer {
        AppServer::start(fake::launch(pid_file))
            .await
            .expect("the fake completes the handshake")
    }

    async fn next_event(events: &mut broadcast::Receiver<Incoming>) -> Incoming {
        tokio::time::timeout(Duration::from_secs(10), events.recv())
            .await
            .expect("an event arrives")
            .expect("the subscription is live")
    }

    #[test]
    fn the_default_deadlines_are_generous_but_finite() {
        let defaults = Timeouts::default();
        assert!(defaults.handshake >= Duration::from_secs(5));
        assert!(defaults.handshake <= Duration::from_secs(60));
        assert!(defaults.request >= Duration::from_secs(15));
        assert!(defaults.request <= Duration::from_secs(120));
    }

    #[test]
    fn restart_delay_is_zero_then_exponential_then_capped() {
        let policy = RestartPolicy {
            base: Duration::from_millis(250),
            max: Duration::from_secs(10),
            stable_after: Duration::from_secs(30),
        };
        assert_eq!(policy.delay(0), Duration::ZERO);
        assert_eq!(policy.delay(1), Duration::from_millis(250));
        assert_eq!(policy.delay(2), Duration::from_millis(500));
        assert_eq!(policy.delay(3), Duration::from_secs(1));
        assert_eq!(policy.delay(6), Duration::from_secs(8));
        assert_eq!(policy.delay(7), Duration::from_secs(10));
        assert_eq!(policy.delay(100), Duration::from_secs(10));
        let defaults = RestartPolicy::default();
        assert!(defaults.base >= Duration::from_millis(100));
        assert!(defaults.max <= Duration::from_secs(60));
        assert!(defaults.stable_after > defaults.max);
    }

    #[test]
    fn a_launch_runs_the_app_server_subcommand_by_default() {
        let launch = Launch::new("/opt/bin/codex");
        assert_eq!(launch.args, vec![OsString::from("app-server")]);
        assert!(launch.env.is_empty());
        assert_eq!(launch.timeouts, Timeouts::default());
        assert_eq!(launch.restart, RestartPolicy::default());
    }

    #[test]
    fn the_fixture_is_pinned_to_the_supported_version() {
        let path = fake::fixture_path();
        let name = path.file_name().unwrap().to_str().unwrap();
        assert_eq!(name, format!("codex-{CODEX_VERSION}.jsonl"));
        let text = fs::read_to_string(&path).unwrap();
        let mut lines = text.lines();
        let header: Value = serde_json::from_str(lines.next().unwrap()).unwrap();
        assert_eq!(header["pin"], CODEX_VERSION);
        let mut methods = Vec::new();
        for line in lines {
            let entry: Value = serde_json::from_str(line).expect(line);
            let method = entry["on"].as_str().expect("every entry names its method");
            methods.push(method.to_owned());
            if method == "initialize" {
                let agent = entry["reply"]["userAgent"].as_str().unwrap();
                assert!(agent.contains(CODEX_VERSION), "{agent}");
            }
        }
        for required in [
            "initialize",
            "model/list",
            "account/read",
            "thread/start",
            "thread/resume",
            "turn/start",
            TURN_INTERRUPT,
        ] {
            assert!(methods.iter().any(|m| m == required), "missing {required}");
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_handshake_completes_and_requests_are_answered() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pids");
        let server = start(Some(&pid_file)).await;
        let pid = server.pid().expect("a live child");
        assert!(process_exists(pid), "the child runs after the handshake");
        assert_eq!(server.generation(), 1);
        assert_eq!(recorded_pids(&pid_file), vec![pid]);

        let models = server.request("model/list", json!({})).await.unwrap();
        assert_eq!(models["data"][0]["id"], "gpt-6-astra");
        assert_eq!(models["data"][0]["isDefault"], true);
        let account = server.request("account/read", json!({})).await.unwrap();
        assert_eq!(account["account"]["type"], "chatgpt");
        let thread = server
            .request(
                "thread/start",
                json!({"approvalPolicy": "never", "sandbox": "read-only"}),
            )
            .await
            .unwrap();
        assert_eq!(thread["thread"]["id"], "thr_fixture_1");

        server.close();
        assert_gone(pid).await;
        assert_eq!(recorded_pids(&pid_file).len(), 1, "no restart after close");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn an_unknown_method_is_an_rpc_error_not_a_dead_child() {
        let server = start(None).await;
        let error = server
            .request("no/such/method", json!({}))
            .await
            .unwrap_err();
        assert!(
            matches!(&error, AppServerError::Rpc(rpc) if rpc.code == -32600 && rpc.message.contains("no/such/method")),
            "{error:?}"
        );
        assert_eq!(server.generation(), 1, "the child is still the same");
        assert_eq!(
            server
                .request("fake/echo", json!({"after": true}))
                .await
                .unwrap(),
            json!({"after": true})
        );
        server.close();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn an_unreadable_answer_fails_its_request_at_once_as_out_of_protocol() {
        let server = start(None).await;
        let started = Instant::now();
        let error = server.request("fake/garble", json!({})).await.unwrap_err();
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "not held until the request deadline, took {:?}",
            started.elapsed()
        );
        assert!(
            matches!(&error, AppServerError::Protocol(why) if why.contains("error")),
            "{error:?}"
        );
        assert!(matches!(
            LlmError::from(error),
            LlmError::InvalidResponse(_)
        ));
        // One bad answer does not cost the child.
        assert_eq!(server.generation(), 1);
        assert_eq!(
            server.request("fake/echo", json!({"ok": 1})).await.unwrap(),
            json!({"ok": 1})
        );
        server.close();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_child_that_never_answers_initialize_fails_the_handshake_and_is_killed() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pids");
        let mut launch = fake::launch(Some(&pid_file));
        launch.env.push((fake::MODE_ENV.into(), "silent".into()));
        launch.timeouts.handshake = Duration::from_secs(1);

        let started = Instant::now();
        let error = AppServer::start(launch)
            .await
            .err()
            .expect("a child that never answers initialize is not an app-server");
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "the handshake deadline bounds start, took {:?}",
            started.elapsed()
        );
        assert!(
            matches!(&error, AppServerError::Handshake(reason) if reason.contains("initialize") && reason.contains("1s")),
            "{error:?}"
        );
        assert!(matches!(LlmError::from(error), LlmError::Transport(_)));

        // The child does not outlive the failed handshake.
        let pids = recorded_pids(&pid_file);
        assert_eq!(pids.len(), 1, "the silent fake started exactly once");
        assert_gone(pids[0]).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_child_that_exits_at_once_reports_what_it_said_on_stderr() {
        let mut launch = fake::launch(None);
        launch.env.push((fake::MODE_ENV.into(), "exit:3".into()));
        launch.env.push((
            fake::STDERR_ENV.into(),
            "error: run `codex login` first".into(),
        ));
        let error = AppServer::start(launch)
            .await
            .err()
            .expect("a child that exits is not an app-server");
        let message = error.to_string();
        assert!(message.contains("handshake"), "{message}");
        assert!(message.contains("run `codex login` first"), "{message}");
        assert!(
            message.contains("exit status: 3"),
            "the exit status is named: {message}"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn concurrent_requests_are_correlated_by_id_not_by_order() {
        let server = start(None).await;
        let (slow, echo) = tokio::join!(
            async {
                let outcome = server.request("fake/slow", json!({})).await;
                (outcome, Instant::now())
            },
            async {
                let outcome = server.request("fake/echo", json!({"n": 7})).await;
                (outcome, Instant::now())
            }
        );
        assert_eq!(echo.0.unwrap(), json!({"n": 7}));
        assert_eq!(slow.0.unwrap(), json!({"slow": true}));
        assert!(
            echo.1 < slow.1,
            "the quick answer is not held behind the slow one"
        );

        // A burst of look-alike requests each gets its own answer back.
        let answers = futures::future::join_all(
            (0..24).map(|n| server.request("fake/echo", json!({"n": n}))),
        )
        .await;
        for (n, answer) in answers.into_iter().enumerate() {
            assert_eq!(answer.unwrap(), json!({"n": n}));
        }
        server.close();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn notifications_reach_every_subscriber_in_order() {
        let server = start(None).await;
        let mut first = server.subscribe();
        let mut second = server.subscribe();
        let turn = server
            .request(
                "turn/start",
                json!({"threadId": "thr_fixture_1", "input": [{"type": "text", "text": "hi"}]}),
            )
            .await
            .unwrap();
        assert_eq!(turn["turn"]["id"], "turn_fixture_1");

        let expected = [
            "turn/started",
            "item/started",
            "item/agentMessage/delta",
            "item/agentMessage/delta",
            "item/completed",
            "thread/tokenUsage/updated",
            "turn/completed",
        ];
        for events in [&mut first, &mut second] {
            let mut text = String::new();
            for method in expected {
                let event = next_event(events).await;
                match event {
                    Incoming::Notification {
                        method: heard,
                        params,
                    } => {
                        assert_eq!(heard, method);
                        if heard == "item/agentMessage/delta" {
                            text.push_str(params["delta"].as_str().unwrap());
                        }
                        if heard == "turn/completed" {
                            assert_eq!(params["turn"]["status"], "completed");
                        }
                    }
                    other => panic!("expected {method}, got {other:?}"),
                }
            }
            assert_eq!(text, "Hello from the fixture");
        }
        server.close();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_request_from_the_app_server_is_delivered_and_answered() {
        let server = start(None).await;
        let mut events = server.subscribe();
        let asked = tokio::spawn({
            let server = server.clone();
            async move { server.request("fake/ask", json!({})).await }
        });
        let event = next_event(&mut events).await;
        let Incoming::Request { id, method, params } = event else {
            panic!("expected the app-server's request, got {event:?}");
        };
        assert_eq!(method, "item/tool/call");
        assert_eq!(params["tool"], "read_memory");
        assert_eq!(params["callId"], "call_fixture_1");
        server
            .respond(
                &id,
                Ok(json!({"contentItems": [{"type": "inputText", "text": "remembered"}], "success": true})),
            )
            .await
            .unwrap();
        let answer = asked.await.unwrap().unwrap();
        assert_eq!(answer["answered"]["success"], true);
        assert_eq!(answer["answered"]["contentItems"][0]["text"], "remembered");
        server.close();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_request_from_the_app_server_nobody_listens_for_is_refused_not_left_hanging() {
        // No subscriber at all: the app-server's question would otherwise
        // wait forever for an answer (the fake gives up after 10 s, codex
        // never does).
        let server = start(None).await;
        let started = Instant::now();
        let answer = server.request("fake/ask", json!({})).await.unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "refused at once, not after the fake gave up: took {:?}",
            started.elapsed()
        );
        assert_eq!(answer["refused"]["code"], -32601, "{answer}");
        let message = answer["refused"]["message"].as_str().unwrap();
        assert!(message.contains("item/tool/call"), "{message}");
        // The child served the refusal and still serves.
        assert_eq!(server.generation(), 1);
        assert_eq!(
            server
                .request("fake/echo", json!({"after": true}))
                .await
                .unwrap(),
            json!({"after": true})
        );
        server.close();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_crash_fails_the_pending_request_and_the_next_request_restarts_one_child() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pids");
        let server = start(Some(&pid_file)).await;
        let first = server.pid().unwrap();
        let mut events = server.subscribe();

        let error = server.request("fake/crash", json!({})).await.unwrap_err();
        assert!(
            matches!(&error, AppServerError::Exited(reason) if reason.contains("exit status: 3")),
            "the exit status is named: {error:?}"
        );
        assert!(matches!(LlmError::from(error), LlmError::Transport(_)));
        assert_gone(first).await;
        assert!(
            matches!(
                next_event(&mut events).await,
                Incoming::Exited { generation: 1, .. }
            ),
            "subscribers hear about the crash"
        );
        assert_eq!(
            server.pid(),
            None,
            "no child between the crash and the next request"
        );

        // Several callers race to be first after the crash: one child.
        let answers =
            futures::future::join_all((0..4).map(|n| server.request("fake/echo", json!({"n": n}))))
                .await;
        for (n, answer) in answers.into_iter().enumerate() {
            assert_eq!(answer.unwrap(), json!({"n": n}));
        }
        let second = server.pid().expect("a fresh child");
        assert_ne!(first, second);
        assert_eq!(server.generation(), 2);
        assert!(matches!(
            next_event(&mut events).await,
            Incoming::Started { generation: 2, pid } if pid == second
        ));
        assert_eq!(recorded_pids(&pid_file), vec![first, second]);

        server.close();
        assert_gone(second).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn an_answer_to_a_dead_child_is_refused_rather_than_starting_a_new_one() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pids");
        let server = start(Some(&pid_file)).await;
        let first = server.pid().unwrap();
        server.request("fake/crash", json!({})).await.unwrap_err();
        assert_gone(first).await;

        let refused = server
            .respond(&json!(0), Ok(json!({"success": true})))
            .await
            .unwrap_err();
        assert!(
            matches!(&refused, AppServerError::Exited(reason) if reason.contains("exit status: 3")),
            "the exit status is named: {refused:?}"
        );
        let refused = server.notify("initialized", json!({})).await.unwrap_err();
        assert!(matches!(refused, AppServerError::Exited(_)), "{refused:?}");
        assert_eq!(server.pid(), None);
        assert_eq!(server.generation(), 1);
        assert_eq!(recorded_pids(&pid_file), vec![first], "nothing was started");

        // A request is what starts the next child.
        server.request("model/list", json!({})).await.unwrap();
        assert_eq!(server.generation(), 2);
        server.close();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn restarts_back_off_per_consecutive_crash() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pids");
        let mut launch = fake::launch(Some(&pid_file));
        launch.restart = RestartPolicy {
            base: Duration::from_millis(300),
            max: Duration::from_secs(5),
            stable_after: Duration::from_secs(60),
        };
        let server = AppServer::start(launch).await.unwrap();

        server.request("fake/crash", json!({})).await.unwrap_err();
        let started = Instant::now();
        server.request("model/list", json!({})).await.unwrap();
        assert!(
            started.elapsed() >= Duration::from_millis(300),
            "the first restart waits the base delay, took {:?}",
            started.elapsed()
        );

        server.request("fake/crash", json!({})).await.unwrap_err();
        let started = Instant::now();
        server.request("model/list", json!({})).await.unwrap();
        assert!(
            started.elapsed() >= Duration::from_millis(600),
            "the second consecutive restart waits twice as long, took {:?}",
            started.elapsed()
        );
        assert_eq!(server.generation(), 3);
        assert_eq!(recorded_pids(&pid_file).len(), 3);
        server.close();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_request_the_app_server_never_answers_times_out_without_wedging_the_next() {
        let mut launch = fake::launch(None);
        launch.timeouts.request = Duration::from_millis(500);
        let server = AppServer::start(launch).await.unwrap();

        let started = Instant::now();
        let error = server.request("fake/hang", json!({})).await.unwrap_err();
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the request deadline bounds request, took {:?}",
            started.elapsed()
        );
        assert_eq!(
            error,
            AppServerError::Timeout {
                method: "fake/hang".into(),
                after: Duration::from_millis(500),
            }
        );
        assert!(matches!(LlmError::from(error), LlmError::Timeout));

        // The child is still the same one, and the next request gets its
        // own deadline instead of waiting behind the first.
        assert_eq!(server.generation(), 1);
        assert_eq!(
            server.request("fake/echo", json!({"ok": 1})).await.unwrap(),
            json!({"ok": 1})
        );
        server.close();
    }

    /// A frame no pipe buffer holds, so a write to a child that is not
    /// reading cannot complete: a `turn/start` with a long history and the
    /// tool definitions is this large.
    fn oversized_params() -> Value {
        json!({"blob": "x".repeat(1 << 20)})
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_child_that_stops_reading_stdin_is_killed_within_the_deadline_and_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pids");
        let mut launch = fake::launch(Some(&pid_file));
        launch.timeouts.request = Duration::from_millis(500);
        let server = AppServer::start(launch).await.unwrap();
        let first = server.pid().unwrap();
        let mut events = server.subscribe();
        // After this answer the fake never reads stdin again; stdout stays
        // open, so the pump sees nothing wrong.
        server.request("fake/deaf", json!({})).await.unwrap();

        // The oversized write cannot complete; the interrupt queues behind
        // it for the shared stdin. Neither may wait past its deadline.
        let started = Instant::now();
        let (stuck, interrupt) =
            tokio::join!(server.request("fake/echo", oversized_params()), async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                server
                    .request(
                        TURN_INTERRUPT,
                        json!({"threadId": "thr_fixture_1", "turnId": "turn_fixture_1"}),
                    )
                    .await
            });
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the deadline bounds the write, not only the wait: took {:?}",
            started.elapsed()
        );
        let error = stuck.unwrap_err();
        assert!(
            matches!(&error, AppServerError::Exited(reason) if reason.contains("fake/echo") && reason.contains("killed")),
            "{error:?}"
        );
        assert!(matches!(LlmError::from(error), LlmError::Transport(_)));
        let error = interrupt.unwrap_err();
        assert!(
            matches!(&error, AppServerError::Exited(_)),
            "the interrupt is not wedged behind the stuck write: {error:?}"
        );

        // The wedged child is killed, subscribers hear it, and the next
        // request is served by a fresh one.
        assert_gone(first).await;
        assert!(matches!(
            next_event(&mut events).await,
            Incoming::Exited { generation: 1, reason } if reason.contains("killed")
        ));
        assert_eq!(
            server.request("fake/echo", json!({"ok": 1})).await.unwrap(),
            json!({"ok": 1})
        );
        let second = server.pid().expect("a fresh child");
        assert_ne!(first, second);
        assert_eq!(server.generation(), 2);
        assert_eq!(recorded_pids(&pid_file), vec![first, second]);
        server.close();
        assert_gone(second).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_child_that_closes_stdin_but_keeps_stdout_open_is_killed_and_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pids");
        let server = start(Some(&pid_file)).await;
        let first = server.pid().unwrap();
        let mut events = server.subscribe();
        // The fake closes its end of stdin after this answer: the next
        // write fails at once, while the pump has no exit to report.
        server.request("fake/close_stdin", json!({})).await.unwrap();

        let started = Instant::now();
        let error = server
            .request("fake/echo", json!({"ok": 1}))
            .await
            .unwrap_err();
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "a failed write is not held to the request deadline: took {:?}",
            started.elapsed()
        );
        assert!(
            matches!(&error, AppServerError::Exited(reason) if reason.contains("killed")),
            "{error:?}"
        );

        // A child that refuses to read is not handed out again.
        assert_gone(first).await;
        assert!(matches!(
            next_event(&mut events).await,
            Incoming::Exited { generation: 1, .. }
        ));
        assert_eq!(
            server.request("fake/echo", json!({"ok": 2})).await.unwrap(),
            json!({"ok": 2})
        );
        assert_eq!(server.generation(), 2);
        assert_eq!(recorded_pids(&pid_file).len(), 2);
        server.close();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn close_kills_the_child_and_refuses_restarts() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pids");
        let server = start(Some(&pid_file)).await;
        let pid = server.pid().unwrap();
        let mut events = server.subscribe();

        server.close();
        server.close(); // idempotent
        assert_gone(pid).await;
        assert_eq!(server.pid(), None);
        assert!(matches!(
            next_event(&mut events).await,
            Incoming::Exited { generation: 1, .. }
        ));
        assert_eq!(
            server.request("model/list", json!({})).await.unwrap_err(),
            AppServerError::Closed
        );
        assert_eq!(
            server.notify("initialized", json!({})).await.unwrap_err(),
            AppServerError::Closed
        );
        assert_eq!(recorded_pids(&pid_file).len(), 1, "nothing restarted");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn dropping_the_last_handle_kills_the_child() {
        let server = start(None).await;
        let pid = server.pid().unwrap();
        let other = server.clone();
        drop(server);
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(
            process_exists(pid),
            "a clone keeps the child alive; only the last handle kills it"
        );
        drop(other);
        assert_gone(pid).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn turn_interrupt_is_forwarded_and_ends_the_turn_as_interrupted() {
        let server = start(None).await;
        let mut events = server.subscribe();
        server
            .request(
                "turn/start",
                json!({"threadId": "thr_fixture_1", "input": [{"type": "text", "text": "hi"}]}),
            )
            .await
            .unwrap();
        let reply = server
            .request(
                TURN_INTERRUPT,
                json!({"threadId": "thr_fixture_1", "turnId": "turn_fixture_1"}),
            )
            .await
            .unwrap();
        assert_eq!(reply, json!({}));

        let mut statuses = Vec::new();
        loop {
            if let Incoming::Notification { method, params } = next_event(&mut events).await
                && method == "turn/completed"
            {
                let status = params["turn"]["status"].as_str().unwrap().to_owned();
                let done = status == "interrupted";
                statuses.push(status);
                if done {
                    break;
                }
            }
        }
        assert_eq!(statuses, ["completed", "interrupted"]);
        server.close();
    }

    /// Needs the real binary on `PATH` at the pinned version; run with
    /// `--ignored`. Only reads: `model/list` writes nothing to `~/.codex`.
    #[cfg(unix)]
    #[tokio::test]
    #[ignore]
    async fn live_app_server_answers_model_list_and_dies_with_the_supervisor() {
        let found = discovery::discover().await.expect("codex at the pin");
        let server = AppServer::start(Launch::new(found.path)).await.unwrap();
        let pid = server.pid().unwrap();
        let models = server.request("model/list", json!({})).await.unwrap();
        assert!(
            models["data"].as_array().is_some_and(|m| !m.is_empty()),
            "{models}"
        );
        server.close();
        assert_gone(pid).await;
    }
}
