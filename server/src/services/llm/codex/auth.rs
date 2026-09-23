//! The login behind the codex provider (#27): whether the pinned binary is
//! there, whether codex holds a ChatGPT login, and the login in flight.
//!
//! Everything here is asked of the app-server over its protocol
//! (`account/read`, `account/login/start`, `account/login/cancel`,
//! `account/logout`, the `account/login/completed` event). The tokens
//! themselves stay in codex's own home directory: no type in this module
//! has a field for one, nothing reads `~/.codex`, and what a status or a
//! login carries is the label of the account (its email and plan) and what
//! a person needs to finish a login (a URL to open, or a URL plus the code
//! to type there). The one supervised app-server child is started here on
//! first use and shared with the provider adapter through
//! [`Auth::app_server`].

use std::{
    ffi::OsString,
    fmt,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::broadcast::{self, error::RecvError};

use super::{
    AppServerError, CODEX_VERSION,
    discovery::{self, BinaryLookup, BinarySource, LocatedBinary},
    process::{AppServer, Incoming, Launch},
    protocol::RpcError,
};
use crate::services::cua::host::DisplaySession;

/// `account/read`: who codex is logged in as, or nobody.
pub const ACCOUNT_READ: &str = "account/read";
/// `account/login/start`: begin a login; the answer says how to finish it.
pub const ACCOUNT_LOGIN_START: &str = "account/login/start";
/// `account/login/cancel`: give up on a login that was started.
pub const ACCOUNT_LOGIN_CANCEL: &str = "account/login/cancel";
/// `account/logout`: forget the login.
pub const ACCOUNT_LOGOUT: &str = "account/logout";
/// The event that ends a login, with its `loginId`, `success` and `error`.
pub const ACCOUNT_LOGIN_COMPLETED: &str = "account/login/completed";

/// How long a login may stay pending before it is given up on: the person
/// has to open a URL and, for a device code, type the code, which the
/// app-server's own flows allow about this long for.
pub const LOGIN_DEADLINE: Duration = Duration::from_secs(15 * 60);

/// Which login a request asks for.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LoginChoice {
    /// The browser flow where this host has a display, else a device code.
    #[default]
    Auto,
    Browser,
    DeviceCode,
}

impl LoginChoice {
    /// The method for this host: the browser flow needs a browser on this
    /// very machine, because codex listens for the callback on its own
    /// `localhost`; a headless host gets a device code to finish elsewhere.
    pub fn resolve(self, session: &DisplaySession) -> LoginMethod {
        match self {
            Self::Browser => LoginMethod::Browser,
            Self::DeviceCode => LoginMethod::DeviceCode,
            Self::Auto if session.is_headless() => LoginMethod::DeviceCode,
            Self::Auto => LoginMethod::Browser,
        }
    }
}

/// How a login is finished.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginMethod {
    /// The ChatGPT managed login: open `auth_url` in a browser on this host;
    /// codex takes the callback on its own `localhost` port.
    Browser,
    /// The device code: open `verification_url` on any device and type
    /// `user_code` there.
    DeviceCode,
}

impl fmt::Display for LoginMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Browser => "browser",
            Self::DeviceCode => "device code",
        })
    }
}

/// Where a login is.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginState {
    /// Started; waiting for the person to finish it.
    Pending,
    /// The app-server reported success; `account/read` now names the account.
    Completed,
    /// The app-server reported failure, the child died, or nobody finished
    /// it within [`LOGIN_DEADLINE`]; `error` says which.
    Failed,
}

/// One login, from `account/login/start` to its completion. The poll
/// handle is `id`; a status names the login until the next one or a logout
/// replaces it. Once it is no longer pending, the URL and code are gone.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LoginStatus {
    pub id: String,
    pub method: LoginMethod,
    pub state: LoginState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Whether the pinned binary is there.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BinaryState {
    /// Found, and it is [`CODEX_VERSION`].
    Ready,
    /// No `codex` on `PATH` and none named by the environment.
    NotInstalled,
    /// Found, but another release; `version` says which.
    Incompatible,
    /// Found, but it could not say which version it is.
    Unusable,
}

/// The binary a status reports: its state, the pin, and for a binary that
/// was found, where and which version; `message` says what to do when it
/// is not ready.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BinaryStatus {
    pub state: BinaryState,
    pub pinned_version: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl BinaryStatus {
    fn ready(located: &LocatedBinary) -> Self {
        Self {
            state: BinaryState::Ready,
            pinned_version: CODEX_VERSION,
            path: Some(located.path.display().to_string()),
            version: Some(located.version.clone()),
            message: None,
        }
    }

    /// The discovery failure as a status; anything else is not about the
    /// binary and reads as not installed with the failure's text.
    fn from_error(error: &AppServerError) -> Self {
        let (state, path, version) = match error {
            AppServerError::NotInstalled => (BinaryState::NotInstalled, None, None),
            AppServerError::Incompatible { path, found } => (
                BinaryState::Incompatible,
                Some(path.display().to_string()),
                Some(found.clone()),
            ),
            AppServerError::Unusable { path, .. } => (
                BinaryState::Unusable,
                Some(path.display().to_string()),
                None,
            ),
            _ => (BinaryState::NotInstalled, None, None),
        };
        Self {
            state,
            pinned_version: CODEX_VERSION,
            path,
            version,
            message: Some(error.to_string()),
        }
    }

    pub fn is_ready(&self) -> bool {
        self.state == BinaryState::Ready
    }
}

/// The account codex is logged in as: its kind (`chatgpt`, `api_key`, or
/// whatever else the app-server reports) and, for a ChatGPT login, the
/// email and plan that label it. Never a token.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Account {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
}

impl Account {
    /// The `account` member of an `account/read` answer: `None` when it is
    /// `null` (nobody is logged in); an answer without it, or with one of
    /// another shape, is out of protocol.
    fn from_read(reply: &Value) -> Result<Option<Self>, AppServerError> {
        let out_of_protocol =
            |why: &str| AppServerError::Protocol(format!("{ACCOUNT_READ} answered {why}"));
        let account = match reply.get("account") {
            None => return Err(out_of_protocol("without account")),
            Some(Value::Null) => return Ok(None),
            Some(Value::Object(account)) => account,
            Some(_) => return Err(out_of_protocol("with an account of another shape")),
        };
        let kind = match account.get("type").and_then(Value::as_str) {
            Some("apiKey") => "api_key".to_owned(),
            Some(other) => other.to_owned(),
            None => return Err(out_of_protocol("with an account without a type")),
        };
        Ok(Some(Self {
            kind,
            email: account
                .get("email")
                .and_then(Value::as_str)
                .map(str::to_owned),
            plan: account
                .get("planType")
                .and_then(Value::as_str)
                .map(str::to_owned),
        }))
    }
}

/// What `GET /api/config/codex/status` answers.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Status {
    pub binary: BinaryStatus,
    /// The binary was found, whatever its version.
    pub installed: bool,
    /// The binary is the pinned release.
    pub compatible: bool,
    /// `account` is some: codex holds a login.
    pub logged_in: bool,
    pub account: Option<Account>,
    /// The login in flight, or the last one's outcome.
    pub login: Option<LoginStatus>,
    /// Why the account could not be read when the binary is ready: the
    /// app-server did not start, died, or answered out of protocol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Status {
    fn from_binary(binary: BinaryStatus, login: Option<LoginStatus>) -> Self {
        Self {
            installed: binary.state != BinaryState::NotInstalled,
            compatible: binary.is_ready(),
            binary,
            logged_in: false,
            account: None,
            login,
            error: None,
        }
    }
}

/// How the binary is found.
enum Binary {
    /// `NOLUNE_CODEX_BIN` and `PATH` of this process.
    Environment,
    /// A fixed lookup, for tests of what discovery says.
    Lookup {
        env_override: Option<OsString>,
        path: Option<OsString>,
    },
    /// A launch taken as verified, for tests with the fake app-server.
    Launch(Launch),
}

/// The login on record and the task watching for its completion.
struct Login {
    status: LoginStatus,
    watcher: Option<tokio::task::AbortHandle>,
}

struct Inner {
    binary: Binary,
    login_deadline: Duration,
    /// The binary once discovery succeeded; a failure is asked again next
    /// time, so installing codex after a status is noticed.
    located: Mutex<Option<LocatedBinary>>,
    /// The one app-server child, started on first use.
    server: tokio::sync::Mutex<Option<AppServer>>,
    login: Mutex<Option<Login>>,
    closed: AtomicBool,
}

impl Inner {
    /// The pending login `id` ended this way. A login that was replaced,
    /// or already ended, is left as it is.
    fn finish(&self, id: &str, outcome: Result<(), String>) {
        let mut login = self.login.lock().unwrap();
        let Some(login) = login
            .as_mut()
            .filter(|login| login.status.id == id && login.status.state == LoginState::Pending)
        else {
            return;
        };
        match &outcome {
            Ok(()) => log::info!("[codex] login completed ({})", login.status.method),
            Err(why) => log::warn!("[codex] login failed ({}): {why}", login.status.method),
        }
        login.status.finish(outcome);
        login.watcher = None;
    }

    /// The pending login, if there is one, is over for this reason; its
    /// watcher is stopped and its id handed back so the app-server can be
    /// told.
    fn give_up_pending(&self, why: &str) -> Option<String> {
        let mut login = self.login.lock().unwrap();
        let login = login
            .as_mut()
            .filter(|login| login.status.state == LoginState::Pending)?;
        if let Some(watcher) = login.watcher.take() {
            watcher.abort();
        }
        login.status.finish(Err(why.to_owned()));
        Some(login.status.id.clone())
    }
}

/// The login state and the shared app-server; clones share one state.
#[derive(Clone)]
pub struct Auth {
    inner: Arc<Inner>,
}

impl Auth {
    /// Finds the binary from this process's environment on first use;
    /// building this starts nothing and reads nothing.
    pub fn new() -> Self {
        Self::from_binary(Binary::Environment)
    }

    /// Finds the binary through this lookup instead of the environment.
    pub fn with_lookup(env_override: Option<OsString>, path: Option<OsString>) -> Self {
        Self::from_binary(Binary::Lookup { env_override, path })
    }

    /// Starts this launch, taken as the pinned binary, instead of one found
    /// by discovery.
    pub fn with_launch(launch: Launch) -> Self {
        Self::from_binary(Binary::Launch(launch))
    }

    /// Whether `other` is a handle on this very state (and child).
    #[cfg(test)]
    pub(crate) fn is_same(&self, other: &Auth) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// How long a login may stay pending; [`LOGIN_DEADLINE`] by default.
    /// Set before the handle is shared.
    pub fn login_deadline(mut self, deadline: Duration) -> Self {
        Arc::get_mut(&mut self.inner)
            .expect("the deadline is set before the handle is shared")
            .login_deadline = deadline;
        self
    }

    fn from_binary(binary: Binary) -> Self {
        Self {
            inner: Arc::new(Inner {
                binary,
                login_deadline: LOGIN_DEADLINE,
                located: Mutex::new(None),
                server: tokio::sync::Mutex::new(None),
                login: Mutex::new(None),
                closed: AtomicBool::new(false),
            }),
        }
    }

    /// The binary, found and verified; cached once it was.
    async fn locate(&self) -> Result<LocatedBinary, AppServerError> {
        if let Some(located) = self.inner.located.lock().unwrap().clone() {
            return Ok(located);
        }
        let located = match &self.inner.binary {
            Binary::Environment => discovery::discover().await?,
            Binary::Lookup { env_override, path } => {
                discovery::discover_with(BinaryLookup {
                    env_override: env_override.as_deref(),
                    path: path.as_deref(),
                })
                .await?
            }
            Binary::Launch(launch) => LocatedBinary {
                path: launch.binary.clone(),
                source: BinarySource::Environment,
                version: CODEX_VERSION.to_owned(),
            },
        };
        *self.inner.located.lock().unwrap() = Some(located.clone());
        Ok(located)
    }

    /// The one app-server child, started now when it was not yet: the
    /// handle the provider adapter shares, so a login and a turn speak to
    /// the same process. A binary that is missing or another release is a
    /// discovery error, never a start.
    pub async fn app_server(&self) -> Result<AppServer, AppServerError> {
        if self.inner.closed.load(Ordering::SeqCst) {
            return Err(AppServerError::Closed);
        }
        let mut server = self.inner.server.lock().await;
        if let Some(server) = server.as_ref() {
            return Ok(server.clone());
        }
        let located = self.locate().await?;
        let launch = match &self.inner.binary {
            Binary::Launch(launch) => launch.clone(),
            _ => Launch::new(located.path),
        };
        log::info!(
            "[codex] starting the app-server: {}",
            launch.binary.display()
        );
        let started = AppServer::start(launch).await?;
        // Shut down while the child was starting: it must not outlive that.
        if self.inner.closed.load(Ordering::SeqCst) {
            started.close();
            return Err(AppServerError::Closed);
        }
        *server = Some(started.clone());
        Ok(started)
    }

    /// Whether the binary is there, who codex is logged in as, and the
    /// login in flight. Never fails: a binary that is not there, or an
    /// app-server that could not answer, is what the status says.
    pub async fn status(&self) -> Status {
        let binary = match self.locate().await {
            Ok(located) => BinaryStatus::ready(&located),
            Err(error) => {
                return Status::from_binary(BinaryStatus::from_error(&error), self.login_status());
            }
        };
        let mut status = Status::from_binary(binary, self.login_status());
        match self.read_account().await {
            Ok(account) => {
                status.logged_in = account.is_some();
                status.account = account;
            }
            Err(error) => status.error = Some(error.to_string()),
        }
        status
    }

    /// `account/read`: who codex is logged in as, or nobody.
    async fn read_account(&self) -> Result<Option<Account>, AppServerError> {
        let reply = self
            .app_server()
            .await?
            .request(ACCOUNT_READ, json!({}))
            .await?;
        Account::from_read(&reply)
    }

    /// Start a login. A login that was still pending is cancelled first.
    /// The answer is what a person needs to finish it and the id to poll
    /// [`status`](Self::status) with; the completion arrives as an event,
    /// which a task watches for until [`LOGIN_DEADLINE`].
    pub async fn login(&self, method: LoginMethod) -> Result<LoginStatus, AppServerError> {
        let server = self.app_server().await?;
        // Subscribed before asking, so the completion cannot slip past.
        let events = server.subscribe();
        self.cancel_pending(&server, "replaced by a new login")
            .await;
        let reply = server.request(ACCOUNT_LOGIN_START, method.params()).await?;
        let status = LoginStatus::from_reply(method, &reply)?;
        let id = status.id.clone();
        *self.inner.login.lock().unwrap() = Some(Login {
            status: status.clone(),
            watcher: None,
        });
        let watcher = tokio::spawn(watch(
            Arc::downgrade(&self.inner),
            server,
            events,
            id.clone(),
            self.inner.login_deadline,
        ));
        if let Some(login) = self
            .inner
            .login
            .lock()
            .unwrap()
            .as_mut()
            .filter(|login| login.status.id == id)
        {
            login.watcher = Some(watcher.abort_handle());
        }
        log::info!("[codex] login started ({method}); waiting for the person to finish it");
        Ok(status)
    }

    /// A pending login is given up on for this reason, and the app-server
    /// told so it stops waiting; that it already ended is no failure.
    async fn cancel_pending(&self, server: &AppServer, why: &str) {
        let Some(id) = self.inner.give_up_pending(why) else {
            return;
        };
        if let Err(error) = server
            .request(ACCOUNT_LOGIN_CANCEL, json!({"loginId": id}))
            .await
        {
            log::warn!("[codex] could not cancel the pending login: {error}");
        }
    }

    /// Forget the login: a pending login is cancelled, `account/logout` is
    /// sent, and the status afterwards is answered.
    pub async fn logout(&self) -> Result<Status, AppServerError> {
        let server = self.app_server().await?;
        self.cancel_pending(&server, "logged out").await;
        server.request(ACCOUNT_LOGOUT, Value::Null).await?;
        *self.inner.login.lock().unwrap() = None;
        log::info!("[codex] logged out");
        Ok(self.status().await)
    }

    /// Stop the app-server child now and refuse to start another, without
    /// waiting on anything: [`shutdown`](Self::shutdown) for a caller that
    /// cannot await (a runtime built for a test, closed when the test is
    /// done with it). The start lock is held only while a child starts,
    /// and a start that sees the flag afterwards closes its own child.
    pub fn close(&self) {
        self.inner.closed.store(true, Ordering::SeqCst);
        self.inner.give_up_pending("codex app-server was shut down");
        let server = match self.inner.server.try_lock() {
            Ok(mut server) => server.take(),
            Err(_starting) => None,
        };
        if let Some(server) = server {
            server.close();
        }
    }

    /// Stop the app-server child and refuse to start another; a pending
    /// login ends with it.
    pub async fn shutdown(&self) {
        self.inner.closed.store(true, Ordering::SeqCst);
        self.inner.give_up_pending("codex app-server was shut down");
        if let Some(server) = self.inner.server.lock().await.take() {
            server.close();
            log::info!("[codex] app-server stopped");
        }
    }

    /// The login on record, as a status reports it.
    fn login_status(&self) -> Option<LoginStatus> {
        self.inner
            .login
            .lock()
            .unwrap()
            .as_ref()
            .map(|login| login.status.clone())
    }
}

impl Default for Auth {
    fn default() -> Self {
        Self::new()
    }
}

impl LoginMethod {
    /// The `account/login/start` params.
    fn params(self) -> Value {
        json!({"type": self.wire_type()})
    }

    /// The `type` of the login on the wire, in the params and the answer.
    fn wire_type(self) -> &'static str {
        match self {
            Self::Browser => "chatgpt",
            Self::DeviceCode => "chatgptDeviceCode",
        }
    }
}

impl LoginStatus {
    /// The answer to `account/login/start` for `method`: its `loginId` and
    /// either the `authUrl` to open or the `verificationUrl` and `userCode`
    /// to type there. An answer of another type or without them is out of
    /// protocol.
    fn from_reply(method: LoginMethod, reply: &Value) -> Result<Self, AppServerError> {
        let member = |name: &str| {
            reply
                .get(name)
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| {
                    AppServerError::Protocol(format!(
                        "{ACCOUNT_LOGIN_START} answered without {name}"
                    ))
                })
        };
        let kind = reply.get("type").and_then(Value::as_str).unwrap_or("");
        if kind != method.wire_type() {
            return Err(AppServerError::Protocol(format!(
                "{ACCOUNT_LOGIN_START} answered a {kind:?} login to a {method} login"
            )));
        }
        let id = member("loginId")?;
        let (auth_url, verification_url, user_code) = match method {
            LoginMethod::Browser => (Some(member("authUrl")?), None, None),
            LoginMethod::DeviceCode => (
                None,
                Some(member("verificationUrl")?),
                Some(member("userCode")?),
            ),
        };
        Ok(Self {
            id,
            method,
            state: LoginState::Pending,
            auth_url,
            verification_url,
            user_code,
            error: None,
        })
    }

    /// The login ended: what a person needed to finish it is of no use now.
    fn finish(&mut self, outcome: Result<(), String>) {
        self.state = match outcome {
            Ok(()) => LoginState::Completed,
            Err(_) => LoginState::Failed,
        };
        self.error = outcome.err();
        self.auth_url = None;
        self.verification_url = None;
        self.user_code = None;
    }
}

/// Wait for the login `id` to end, then record how. Past `deadline`
/// nobody is going to finish it: the app-server is told to stop waiting
/// and the login fails.
async fn watch(
    inner: Weak<Inner>,
    server: AppServer,
    mut events: broadcast::Receiver<Incoming>,
    id: String,
    deadline: Duration,
) {
    let outcome = match tokio::time::timeout(deadline, ended(&server, &mut events, &id)).await {
        Ok(outcome) => outcome,
        Err(_) => {
            if let Err(error) = server
                .request(ACCOUNT_LOGIN_CANCEL, json!({"loginId": &id}))
                .await
            {
                log::warn!("[codex] could not cancel the expired login: {error}");
            }
            Err(format!(
                "nobody finished the login within {deadline:?}; start it again"
            ))
        }
    };
    if let Some(inner) = inner.upgrade() {
        inner.finish(&id, outcome);
    }
}

/// How the login `id` ended, from the events. A subscriber told it lagged
/// may have missed the completion among a turn's events; the account then
/// says whether the login went through. A request the app-server sends
/// meanwhile is refused when this watcher is the only one who heard it.
async fn ended(
    server: &AppServer,
    events: &mut broadcast::Receiver<Incoming>,
    id: &str,
) -> Result<(), String> {
    loop {
        match events.recv().await {
            Ok(Incoming::Request {
                id: request,
                method,
                ..
            }) => refuse_when_alone(server, request, method),
            Ok(event) => {
                if let Some(outcome) = login_outcome(&event, id) {
                    return outcome;
                }
            }
            Err(RecvError::Lagged(_)) => match server.request(ACCOUNT_READ, json!({})).await {
                Ok(reply)
                    if reply
                        .get("account")
                        .is_some_and(|account| !account.is_null()) =>
                {
                    return Ok(());
                }
                Ok(_) => continue,
                Err(error) => return Err(error.to_string()),
            },
            Err(RecvError::Closed) => {
                return Err("the app-server supervisor was dropped".to_owned());
            }
        }
    }
}

/// A request the app-server sent while a login is pending. The supervisor
/// refuses a request nobody is subscribed for, so the app-server never
/// waits for an answer that cannot come; the watcher is subscribed for the
/// whole login, which would turn that net off, so it refuses in the
/// supervisor's stead when it is the only subscriber. Another subscriber
/// is a turn's, which answers the app-server's requests itself: a refusal
/// on top of its answer would fail a live tool call and hand the
/// app-server two answers, so the request is left to it. The answer is
/// written from a task of its own, so the watcher keeps hearing events.
fn refuse_when_alone(server: &AppServer, id: Value, method: String) {
    if server.subscribers() > 1 {
        return;
    }
    log::warn!(
        "[codex] nobody but the login watcher is subscribed to answer the app-server's {method} request {id}: refusing it"
    );
    let server = server.clone();
    tokio::spawn(async move {
        let refusal = RpcError {
            code: -32601,
            message: format!("nolune has no handler listening for {method}"),
            data: None,
        };
        if let Err(error) = server.respond(&id, Err(refusal)).await {
            log::warn!("[codex] could not refuse the app-server's {method} request {id}: {error}");
        }
    });
}

/// What an event says about the login `id`: nothing, or how it ended. A
/// completion that names another login is another login's; one that names
/// none is about the only login there is.
fn login_outcome(event: &Incoming, id: &str) -> Option<Result<(), String>> {
    match event {
        Incoming::Notification { method, params } if method == ACCOUNT_LOGIN_COMPLETED => {
            let named = params.get("loginId").and_then(Value::as_str);
            if named.is_some_and(|named| named != id) {
                return None;
            }
            let success = params
                .get("success")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if success {
                return Some(Ok(()));
            }
            let error = params
                .get("error")
                .and_then(Value::as_str)
                .filter(|error| !error.is_empty())
                .unwrap_or("the app-server reported the login failed without saying why");
            Some(Err(error.to_owned()))
        }
        Incoming::Exited { reason, .. } => {
            Some(Err(format!("{reason} before the login completed")))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::{CODEX_ENV, fake};
    use super::*;
    use crate::services::cua::host::DisplaySession;
    use std::{
        collections::BTreeSet,
        path::{Path, PathBuf},
        sync::OnceLock,
        time::Instant,
    };

    /// Every member name in `value`, at any depth.
    fn keys(value: &Value, out: &mut BTreeSet<String>) {
        match value {
            Value::Object(map) => {
                for (key, value) in map {
                    out.insert(key.clone());
                    keys(value, out);
                }
            }
            Value::Array(items) => items.iter().for_each(|item| keys(item, out)),
            _ => {}
        }
    }

    /// The members a status, a login or an account may carry: what a
    /// person needs, and nothing that could stand in for a credential.
    const ALLOWED_KEYS: &[&str] = &[
        "binary",
        "state",
        "pinned_version",
        "path",
        "version",
        "message",
        "installed",
        "compatible",
        "logged_in",
        "account",
        "kind",
        "email",
        "plan",
        "login",
        "id",
        "method",
        "auth_url",
        "verification_url",
        "user_code",
        "error",
    ];

    fn assert_only_allowed_keys(value: &Value, label: &str) {
        let mut found = BTreeSet::new();
        keys(value, &mut found);
        let stray: Vec<_> = found
            .iter()
            .filter(|key| !ALLOWED_KEYS.contains(&key.as_str()))
            .collect();
        assert!(stray.is_empty(), "{label} carries {stray:?}: {value}");
        for key in &found {
            let lowered = key.to_lowercase();
            assert!(
                !lowered.contains("token") && !lowered.contains("secret"),
                "{label} carries {key}"
            );
        }
    }

    /// An executable script named `codex` under `dir` that prints
    /// `version` for `--version`.
    fn fake_binary(dir: &Path, version: &str) -> PathBuf {
        let path = dir.join("codex");
        fake::codex_script(&path, &format!("#!/bin/sh\necho 'codex-cli {version}'\n"));
        path
    }

    /// An auth over the fake app-server with this initial state.
    fn auth_with_state(state: Value) -> Auth {
        Auth::with_launch(fake::launch_with_state(None, Some(state)))
    }

    /// The fake's account label.
    const FIXTURE_EMAIL: &str = "companion@example.test";
    /// The fake's device code and the URLs it hands out.
    const FIXTURE_CODE: &str = "NLNE-FXTR";
    const FIXTURE_VERIFICATION_URL: &str = "https://auth.openai.com/codex/device";
    const FIXTURE_AUTH_URL_MARK: &str = "code_challenge=fixture_challenge";

    /// Poll the status until `done` says so, or fail after ten seconds.
    async fn status_when(auth: &Auth, what: &str, done: impl Fn(&Status) -> bool) -> Status {
        let give_up = Instant::now() + Duration::from_secs(10);
        loop {
            let status = auth.status().await;
            if done(&status) {
                return status;
            }
            assert!(Instant::now() < give_up, "waiting for {what}: {status:?}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    fn completed(status: &Status) -> bool {
        status
            .login
            .as_ref()
            .is_some_and(|login| login.state != LoginState::Pending)
    }

    /// Every log record, so a test can prove the flow wrote no code, URL
    /// or address. One logger serves the whole test binary; records from
    /// other tests only make the assertion stricter.
    struct Capture(Mutex<Vec<String>>);

    impl log::Log for Capture {
        fn enabled(&self, _: &log::Metadata<'_>) -> bool {
            true
        }
        fn log(&self, record: &log::Record<'_>) {
            self.0
                .lock()
                .unwrap()
                .push(format!("{} {}", record.target(), record.args()));
        }
        fn flush(&self) {}
    }

    static CAPTURE: Capture = Capture(Mutex::new(Vec::new()));

    fn captured_logs() -> &'static Capture {
        static INSTALLED: OnceLock<()> = OnceLock::new();
        INSTALLED.get_or_init(|| {
            log::set_logger(&CAPTURE).expect("no other logger in the test binary");
            log::set_max_level(log::LevelFilter::Trace);
        });
        &CAPTURE
    }

    #[test]
    fn auto_is_a_device_code_on_a_headless_host_and_the_browser_flow_otherwise() {
        let headless = DisplaySession::Headless("no display".into());
        let display = DisplaySession::Present("DISPLAY=:0".into());
        assert_eq!(
            LoginChoice::Auto.resolve(&headless),
            LoginMethod::DeviceCode
        );
        assert_eq!(LoginChoice::Auto.resolve(&display), LoginMethod::Browser);
        assert_eq!(
            LoginChoice::Auto.resolve(&DisplaySession::NotChecked),
            LoginMethod::Browser
        );
        // An explicit choice is kept whatever the host looks like.
        assert_eq!(
            LoginChoice::Browser.resolve(&headless),
            LoginMethod::Browser
        );
        assert_eq!(
            LoginChoice::DeviceCode.resolve(&display),
            LoginMethod::DeviceCode
        );
        assert_eq!(
            serde_json::from_str::<LoginChoice>("\"device_code\"").unwrap(),
            LoginChoice::DeviceCode
        );
    }

    #[test]
    fn the_login_deadline_is_long_enough_to_type_a_code_but_finite() {
        assert!(LOGIN_DEADLINE >= Duration::from_secs(5 * 60));
        assert!(LOGIN_DEADLINE <= Duration::from_secs(60 * 60));
    }

    #[tokio::test]
    async fn a_missing_binary_is_not_installed_and_starts_nothing() {
        let empty = tempfile::tempdir().unwrap();
        let auth = Auth::with_lookup(None, Some(empty.path().as_os_str().to_owned()));
        let status = auth.status().await;
        assert_eq!(status.binary.state, BinaryState::NotInstalled);
        assert_eq!(status.binary.pinned_version, CODEX_VERSION);
        assert!(!status.installed && !status.compatible && !status.logged_in);
        assert_eq!(status.account, None);
        assert_eq!(status.login, None);
        let message = status.binary.message.as_deref().unwrap();
        assert!(message.contains(CODEX_ENV), "actionable: {message}");
        assert_eq!(
            auth.login(LoginMethod::DeviceCode).await.unwrap_err(),
            AppServerError::NotInstalled
        );
        assert_eq!(
            auth.logout().await.unwrap_err(),
            AppServerError::NotInstalled
        );
        assert!(auth.inner.server.lock().await.is_none(), "nothing started");
    }

    #[tokio::test]
    async fn another_release_is_incompatible_naming_both_versions_and_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let binary = fake_binary(dir.path(), "0.154.0");
        let auth = Auth::with_lookup(Some(binary.as_os_str().to_owned()), None);
        let status = auth.status().await;
        assert_eq!(status.binary.state, BinaryState::Incompatible);
        assert!(status.installed && !status.compatible && !status.logged_in);
        assert_eq!(status.binary.version.as_deref(), Some("0.154.0"));
        assert_eq!(
            status.binary.path.as_deref(),
            Some(binary.to_str().unwrap())
        );
        let message = status.binary.message.as_deref().unwrap();
        assert!(message.contains("0.154.0"), "{message}");
        assert!(message.contains(CODEX_VERSION), "{message}");
        assert!(message.contains(binary.to_str().unwrap()), "{message}");
        assert!(matches!(
            auth.login(LoginMethod::Browser).await.unwrap_err(),
            AppServerError::Incompatible { .. }
        ));
    }

    #[tokio::test]
    async fn a_binary_that_cannot_report_its_version_is_unusable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("codex");
        std::fs::write(&path, "not a program").unwrap();
        let auth = Auth::with_lookup(Some(path.as_os_str().to_owned()), None);
        let status = auth.status().await;
        assert_eq!(status.binary.state, BinaryState::Unusable);
        assert!(status.installed && !status.compatible);
        assert!(
            status
                .binary
                .message
                .as_deref()
                .unwrap()
                .contains(path.to_str().unwrap())
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn status_reads_the_account_from_the_app_server() {
        let auth = Auth::with_launch(fake::launch(None));
        let status = auth.status().await;
        assert_eq!(status.binary.state, BinaryState::Ready, "{status:?}");
        assert_eq!(status.binary.version.as_deref(), Some(CODEX_VERSION));
        assert!(status.installed && status.compatible && status.logged_in);
        assert_eq!(
            status.account,
            Some(Account {
                kind: "chatgpt".into(),
                email: Some(FIXTURE_EMAIL.into()),
                plan: Some("plus".into()),
            })
        );
        assert_eq!(status.login, None);
        assert_eq!(status.error, None);
        assert_only_allowed_keys(&serde_json::to_value(&status).unwrap(), "status");
        auth.shutdown().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_app_server_is_started_once_and_shared() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pids");
        let auth = Auth::with_launch(fake::launch(Some(&pid_file)));
        let first = auth.app_server().await.unwrap();
        let second = auth.clone().app_server().await.unwrap();
        assert_eq!(first.pid(), second.pid());
        auth.status().await;
        let pids = std::fs::read_to_string(&pid_file).unwrap();
        assert_eq!(pids.lines().count(), 1, "one child for everything: {pids}");
        auth.shutdown().await;
        let Err(refused) = auth.app_server().await else {
            panic!("no app-server after shutdown");
        };
        assert_eq!(refused, AppServerError::Closed);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn shutdown_kills_the_child_and_says_so_in_the_status() {
        let auth = Auth::with_launch(fake::launch(None));
        let pid = auth.app_server().await.unwrap().pid().unwrap();
        auth.shutdown().await;
        let gone_by = Instant::now() + Duration::from_secs(5);
        while Instant::now() < gone_by
            && std::process::Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(Instant::now() < gone_by, "child {pid} was left running");
        let status = auth.status().await;
        assert!(!status.logged_in);
        assert!(status.error.is_some(), "{status:?}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_device_code_login_hands_out_the_url_and_code_and_status_reflects_completion() {
        let auth = auth_with_state(json!({"session": "none"}));
        let before = auth.status().await;
        assert!(before.compatible && !before.logged_in, "{before:?}");
        assert_eq!(before.account, None);

        let login = auth.login(LoginMethod::DeviceCode).await.unwrap();
        assert_eq!(login.method, LoginMethod::DeviceCode);
        assert_eq!(login.state, LoginState::Pending);
        assert_eq!(
            login.verification_url.as_deref(),
            Some(FIXTURE_VERIFICATION_URL)
        );
        assert_eq!(login.user_code.as_deref(), Some(FIXTURE_CODE));
        assert_eq!(login.auth_url, None);
        assert_eq!(login.error, None);
        assert!(!login.id.is_empty());
        assert_only_allowed_keys(&serde_json::to_value(&login).unwrap(), "login");

        let pending = auth.status().await;
        assert_eq!(
            pending.login.as_ref(),
            Some(&login),
            "the poll sees the same login"
        );

        let after = status_when(&auth, "the login to complete", completed).await;
        let done = after.login.as_ref().unwrap();
        assert_eq!(done.id, login.id);
        assert_eq!(done.state, LoginState::Completed, "{done:?}");
        assert_eq!(done.error, None);
        assert_eq!(done.user_code, None, "the code is gone once it was used");
        assert_eq!(done.verification_url, None);
        assert!(after.logged_in, "{after:?}");
        assert_eq!(
            after.account.as_ref().and_then(|a| a.email.as_deref()),
            Some(FIXTURE_EMAIL)
        );
        auth.shutdown().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_browser_login_hands_out_the_auth_url_and_no_code() {
        let auth = auth_with_state(json!({"session": "none"}));
        let login = auth.login(LoginMethod::Browser).await.unwrap();
        assert_eq!(login.method, LoginMethod::Browser);
        assert_eq!(login.state, LoginState::Pending);
        let url = login.auth_url.as_deref().expect("the URL to open");
        assert!(url.contains(FIXTURE_AUTH_URL_MARK), "{url}");
        assert_eq!(login.user_code, None);
        assert_eq!(login.verification_url, None);

        let after = status_when(&auth, "the login to complete", completed).await;
        let done = after.login.as_ref().unwrap();
        assert_eq!(done.state, LoginState::Completed, "{done:?}");
        assert_eq!(done.auth_url, None, "the URL is gone once it was used");
        assert!(after.logged_in);
        auth.shutdown().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_login_the_app_server_reports_failed_is_failed_with_its_reason() {
        let auth = auth_with_state(json!({"session": "none", "login": "refused"}));
        let login = auth.login(LoginMethod::DeviceCode).await.unwrap();
        assert_eq!(login.state, LoginState::Pending);
        let after = status_when(&auth, "the login to fail", completed).await;
        let done = after.login.as_ref().unwrap();
        assert_eq!(done.state, LoginState::Failed);
        assert_eq!(done.id, login.id);
        assert!(
            done.error.as_deref().unwrap().contains("expired"),
            "the app-server's reason: {done:?}"
        );
        assert_eq!(done.user_code, None);
        assert!(!after.logged_in, "{after:?}");
        auth.shutdown().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_login_nobody_finishes_fails_at_the_deadline() {
        let auth = auth_with_state(json!({"session": "none", "login": "silent"}))
            .login_deadline(Duration::from_millis(300));
        let started = Instant::now();
        let login = auth.login(LoginMethod::DeviceCode).await.unwrap();
        assert_eq!(login.state, LoginState::Pending);
        let after = status_when(&auth, "the deadline", completed).await;
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "took {:?}",
            started.elapsed()
        );
        let done = after.login.as_ref().unwrap();
        assert_eq!(done.state, LoginState::Failed);
        assert!(
            done.error.as_deref().unwrap().contains("within"),
            "{done:?}"
        );
        assert!(!after.logged_in);
        auth.shutdown().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_request_from_the_app_server_during_a_pending_login_is_refused_not_left_waiting() {
        // The supervisor refuses a request from the app-server when nobody
        // is subscribed; the watcher of a pending login is subscribed for
        // up to the deadline, and must not turn that net off. "silent":
        // the login never completes, so the watcher stays.
        let auth = auth_with_state(json!({"session": "none", "login": "silent"}));
        let login = auth.login(LoginMethod::DeviceCode).await.unwrap();
        assert_eq!(login.state, LoginState::Pending);
        let server = auth.app_server().await.unwrap();
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
        // The refusal is not the login's end: it still waits for the person.
        let status = auth.status().await;
        assert_eq!(
            status.login.as_ref().map(|login| login.state),
            Some(LoginState::Pending),
            "{status:?}"
        );
        auth.shutdown().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_login_watcher_leaves_a_request_to_a_subscriber_that_answers_it() {
        // A turn's own subscriber (the adapter's, in 27c) answers the
        // app-server's requests; a refusal from the watcher on top of it
        // would fail a live tool call and hand the app-server two answers.
        let auth = auth_with_state(json!({"session": "none", "login": "silent"}));
        auth.login(LoginMethod::DeviceCode).await.unwrap();
        let server = auth.app_server().await.unwrap();
        let mut turn = server.subscribe();
        let asked = tokio::spawn({
            let server = server.clone();
            async move { server.request("fake/ask", json!({})).await }
        });
        let (id, method) = loop {
            match tokio::time::timeout(Duration::from_secs(10), turn.recv())
                .await
                .expect("the app-server's request arrives")
                .expect("the subscription is live")
            {
                Incoming::Request { id, method, .. } => break (id, method),
                _ => continue,
            }
        };
        assert_eq!(method, "item/tool/call");
        // Time for a watcher that refused wrongly to have done so first.
        tokio::time::sleep(Duration::from_millis(200)).await;
        server
            .respond(&id, Ok(json!({"success": true})))
            .await
            .unwrap();
        let answer = asked.await.unwrap().unwrap();
        assert_eq!(
            answer["answered"]["success"], true,
            "the subscriber's answer is the one that counts: {answer}"
        );
        auth.shutdown().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_new_login_replaces_a_pending_one() {
        let auth = auth_with_state(json!({"session": "none"}));
        let first = auth.login(LoginMethod::Browser).await.unwrap();
        let second = auth.login(LoginMethod::DeviceCode).await.unwrap();
        assert_ne!(first.id, second.id);
        let status = auth.status().await;
        assert_eq!(
            status.login.as_ref().map(|login| login.id.as_str()),
            Some(second.id.as_str()),
            "the poll follows the new login"
        );
        let after = status_when(&auth, "the second login to complete", completed).await;
        assert_eq!(after.login.as_ref().unwrap().id, second.id);
        assert_eq!(after.login.as_ref().unwrap().state, LoginState::Completed);
        auth.shutdown().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn logout_forgets_the_account_and_the_login() {
        let auth = auth_with_state(json!({"session": "none"}));
        auth.login(LoginMethod::DeviceCode).await.unwrap();
        let logged_in = status_when(&auth, "the login to complete", |s| s.logged_in).await;
        assert!(logged_in.login.is_some());
        let after = auth.logout().await.unwrap();
        assert!(!after.logged_in, "{after:?}");
        assert_eq!(after.account, None);
        assert_eq!(after.login, None, "no stale login after a logout");
        assert_eq!(after.error, None);
        assert!(after.compatible);
        let again = auth.status().await;
        assert!(!again.logged_in && again.login.is_none(), "{again:?}");
        auth.shutdown().await;
    }

    #[test]
    fn a_completion_for_another_login_is_not_this_ones() {
        let ours = |success: bool, error: Option<&str>| Incoming::Notification {
            method: ACCOUNT_LOGIN_COMPLETED.into(),
            params: json!({"loginId": "login_a", "success": success, "error": error}),
        };
        assert_eq!(login_outcome(&ours(true, None), "login_a"), Some(Ok(())));
        assert_eq!(
            login_outcome(&ours(false, Some("denied")), "login_a"),
            Some(Err("denied".into()))
        );
        assert!(
            matches!(login_outcome(&ours(false, None), "login_a"), Some(Err(why)) if !why.is_empty()),
            "a failure without a reason still has one"
        );
        assert_eq!(login_outcome(&ours(true, None), "login_b"), None);
        // A completion that names no login is about the only one there is.
        let unnamed = Incoming::Notification {
            method: ACCOUNT_LOGIN_COMPLETED.into(),
            params: json!({"loginId": null, "success": true, "error": null}),
        };
        assert_eq!(login_outcome(&unnamed, "login_a"), Some(Ok(())));
        let other = Incoming::Notification {
            method: "account/updated".into(),
            params: json!({"authMode": "chatgpt"}),
        };
        assert_eq!(login_outcome(&other, "login_a"), None);
        let started = Incoming::Started {
            generation: 2,
            pid: 1,
        };
        assert_eq!(login_outcome(&started, "login_a"), None);
        assert!(matches!(
            login_outcome(
                &Incoming::Exited {
                    generation: 1,
                    reason: "codex app-server (pid 1) exited".into()
                },
                "login_a"
            ),
            Some(Err(why)) if why.contains("exited")
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn debug_output_of_the_state_types_names_no_token_bearing_field() {
        let auth = auth_with_state(json!({"session": "none"}));
        let login = auth.login(LoginMethod::DeviceCode).await.unwrap();
        let status = status_when(&auth, "the login to complete", completed).await;
        for (label, text) in [
            ("login", format!("{login:?}")),
            ("status", format!("{status:?}")),
            ("binary", format!("{:?}", status.binary)),
            ("account", format!("{:?}", status.account)),
            ("method", format!("{}", login.method)),
        ] {
            let lowered = text.to_lowercase();
            for banned in ["token", "secret", "auth.json", "api_key:", "apikey"] {
                assert!(!lowered.contains(banned), "{label} says {banned:?}: {text}");
            }
        }
        auth.shutdown().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_login_flow_logs_neither_the_code_nor_the_urls_nor_the_address() {
        let logs = captured_logs();
        log::info!("[codex] capture probe");
        assert!(
            logs.0
                .lock()
                .unwrap()
                .iter()
                .any(|l| l.contains("capture probe")),
            "the capturing logger is installed"
        );
        let auth = auth_with_state(json!({"session": "none"}));
        auth.status().await;
        auth.login(LoginMethod::DeviceCode).await.unwrap();
        status_when(&auth, "the login to complete", completed).await;
        auth.login(LoginMethod::Browser).await.unwrap();
        status_when(&auth, "the login to complete", completed).await;
        auth.logout().await.unwrap();
        auth.shutdown().await;
        let lines = logs.0.lock().unwrap().clone();
        let ours: Vec<_> = lines.iter().filter(|l| l.contains("codex")).collect();
        assert!(!ours.is_empty(), "the flow logs what it did");
        for line in &lines {
            for secret in [
                FIXTURE_CODE,
                FIXTURE_VERIFICATION_URL,
                FIXTURE_AUTH_URL_MARK,
                FIXTURE_EMAIL,
            ] {
                assert!(
                    !line.contains(secret),
                    "a log line carries {secret:?}: {line}"
                );
            }
        }
    }
}
