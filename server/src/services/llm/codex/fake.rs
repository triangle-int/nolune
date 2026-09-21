//! A stand-in `codex app-server` for the tests, so CI never needs the real
//! binary: this very test binary, re-executed to run
//! [`fake_app_server_entry_point`] alone, plays the pinned fixture over
//! stdio. The crate has no library target and ships one binary, so the
//! test executable is the one program that is always built when these
//! tests run.
//!
//! The fixture (`fixtures/codex-<pin>.jsonl`) is one JSON object per line.
//! Lines with `on` script one method: `reply` (the result), `echo` (the
//! request params as the result) or `error` (an error object); `notify`
//! events emitted before the answer and `then` events after it, after
//! `then_delay_ms` when that is set; `delay_ms` before answering; `exit` to
//! die without answering; `ask` to send the client a request and answer
//! only once the client answered that; `raw` to write a verbatim line with
//! `$ID` replaced by the request id; `stdin` to stop serving stdin after
//! this request while stdout stays open, the way a wedged app-server does:
//! `ignore` leaves the pipe unread so the client's writes block once it is
//! full, `close` closes the read end so they fail at once. Other lines are
//! comments. Requests are served concurrently, so answers come back out of
//! order like the real app-server's do. `initialize` must come first
//! (`-32600 Not initialized` otherwise) and an unscripted method is
//! `-32600 Invalid request: unknown variant`, the live error shapes.
//!
//! Several lines may script one method; the first whose conditions hold
//! serves the request. `when` names members the request params must carry
//! with these values (`account/login/start` answers by its `type`), and
//! `if` names members of the fake's state, a flat object that starts as
//! [`STATE_ENV`] says (empty by default; a member never set reads as
//! `null`) and that `set` rewrites: before the answer, or, when
//! `then_delay_ms` is set, after that delay and before the `then` events.
//! That is how a logout changes what `account/read` says next, and a
//! login only once its completion fires, without teaching the fake the
//! protocol's meaning.
//!
//! The environment steers the process: [`FIXTURE_ENV`] names the fixture
//! and turns the entry point into the server, [`MODE_ENV`] is `serve`
//! (default), `silent` (never answer) or `exit:<code>` (die at once),
//! [`PID_FILE_ENV`] gets the pid appended at every start, [`STDERR_ENV`]
//! is a line written to stderr at start, and [`STATE_ENV`] is the initial
//! state as a JSON object.

use std::{
    collections::HashMap,
    ffi::OsString,
    io::{BufRead as _, Write as _},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{CODEX_VERSION, process::Launch};

pub const FIXTURE_ENV: &str = "NOLUNE_FAKE_APP_SERVER_FIXTURE";
pub const MODE_ENV: &str = "NOLUNE_FAKE_APP_SERVER_MODE";
pub const PID_FILE_ENV: &str = "NOLUNE_FAKE_APP_SERVER_PID_FILE";
pub const STDERR_ENV: &str = "NOLUNE_FAKE_APP_SERVER_STDERR";
pub const STATE_ENV: &str = "NOLUNE_FAKE_APP_SERVER_STATE";

/// How long an `ask` waits for the client's answer.
const ASK_TIMEOUT: Duration = Duration::from_secs(10);

/// The fixture recorded against the pinned release.
pub fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/services/llm/fixtures")
        .join(format!("codex-{CODEX_VERSION}.jsonl"))
}

/// The libtest name of the entry point: the module path without the crate.
fn entry_point() -> String {
    let module = module_path!()
        .split_once("::")
        .map_or(module_path!(), |(_, rest)| rest);
    format!("{module}::fake_app_server_entry_point")
}

/// A [`Launch`] that starts this fake instead of codex, with the default
/// deadlines and restart policy.
pub fn launch(pid_file: Option<&Path>) -> Launch {
    launch_with_state(pid_file, None)
}

/// [`launch`] with the fake's initial state, for the entries that answer
/// by `if`.
pub fn launch_with_state(pid_file: Option<&Path>, state: Option<Value>) -> Launch {
    let mut launch = Launch::new(std::env::current_exe().expect("the test binary has a path"));
    launch.args = vec![
        OsString::from(entry_point()),
        OsString::from("--exact"),
        OsString::from("--nocapture"),
        OsString::from("--quiet"),
    ];
    launch
        .env
        .push((FIXTURE_ENV.into(), fixture_path().into_os_string()));
    if let Some(pid_file) = pid_file {
        launch
            .env
            .push((PID_FILE_ENV.into(), pid_file.as_os_str().to_owned()));
    }
    if let Some(state) = state {
        launch
            .env
            .push((STATE_ENV.into(), state.to_string().into()));
    }
    launch
}

/// The test that is the fake: a no-op in a normal run, the server when
/// [`FIXTURE_ENV`] is set. It never returns in that case.
#[test]
fn fake_app_server_entry_point() {
    let Some(fixture) = std::env::var_os(FIXTURE_ENV) else {
        return;
    };
    serve(Path::new(&fixture));
}

#[derive(Clone, Deserialize)]
struct Event {
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Clone, Deserialize)]
struct Ask {
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Clone, Deserialize)]
struct Entry {
    on: String,
    /// Members the request params must carry, with these values.
    #[serde(default)]
    when: Option<Map<String, Value>>,
    /// Members the fake's state must carry, with these values.
    #[serde(default, rename = "if")]
    only_if: Option<Map<String, Value>>,
    /// State members rewritten before the answer, or after `then_delay_ms`
    /// and before the `then` events when that is set.
    #[serde(default)]
    set: Option<Map<String, Value>>,
    #[serde(default)]
    reply: Option<Value>,
    #[serde(default)]
    echo: bool,
    #[serde(default)]
    error: Option<Value>,
    #[serde(default)]
    notify: Vec<Event>,
    #[serde(default)]
    then: Vec<Event>,
    #[serde(default)]
    delay_ms: Option<u64>,
    #[serde(default)]
    then_delay_ms: Option<u64>,
    #[serde(default)]
    exit: Option<i32>,
    #[serde(default)]
    ask: Option<Ask>,
    #[serde(default)]
    raw: Option<String>,
    #[serde(default)]
    stdin: Option<String>,
}

impl Entry {
    /// Whether every member of `expected` is in `actual` with that value; a
    /// member `actual` lacks reads as `null`.
    fn subset(expected: Option<&Map<String, Value>>, actual: &Value) -> bool {
        expected.is_none_or(|expected| {
            expected
                .iter()
                .all(|(key, value)| actual.get(key).unwrap_or(&Value::Null) == value)
        })
    }

    fn serves(&self, method: &str, params: &Value, state: &Value) -> bool {
        self.on == method
            && Self::subset(self.when.as_ref(), params)
            && Self::subset(self.only_if.as_ref(), state)
    }
}

/// The fake's state: what `if` reads and `set` writes.
type FakeState = Arc<Mutex<Value>>;

/// The script in file order: the first entry that serves a request wins.
fn load(fixture: &Path) -> Vec<Entry> {
    let text = std::fs::read_to_string(fixture)
        .unwrap_or_else(|error| panic!("{}: {error}", fixture.display()));
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let value: Value = serde_json::from_str(line).expect(line);
            value.get("on")?;
            Some(serde_json::from_value(value).expect(line))
        })
        .collect()
}

/// The entry that serves this request right now, if any is scripted.
fn select(script: &[Entry], method: &str, params: &Value, state: &FakeState) -> Option<Entry> {
    let state = state.lock().unwrap();
    script
        .iter()
        .find(|entry| entry.serves(method, params, &state))
        .cloned()
}

/// The initial state from [`STATE_ENV`]: a JSON object, or empty.
fn initial_state() -> Value {
    std::env::var(STATE_ENV)
        .ok()
        .map(|text| serde_json::from_str(&text).expect("the initial state is a JSON object"))
        .unwrap_or_else(|| Value::Object(Map::new()))
}

/// Write one frame as one line; the lock keeps concurrent handlers from
/// interleaving.
fn emit(frame: Value) {
    let mut out = std::io::stdout().lock();
    writeln!(out, "{frame}").expect("stdout");
    out.flush().expect("stdout");
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn notification(event: &Event) -> Value {
    json!({"method": event.method, "params": event.params, "emittedAtMs": now_ms()})
}

fn error_frame(id: &Value, code: i64, message: String) -> Value {
    // The live shape puts `error` before `id`.
    let mut frame = Map::new();
    frame.insert("error".into(), json!({"code": code, "message": message}));
    frame.insert("id".into(), id.clone());
    Value::Object(frame)
}

type Asks = Arc<Mutex<HashMap<String, mpsc::Sender<Result<Value, Value>>>>>;

fn handle(
    entry: Option<Entry>,
    state: FakeState,
    initialized: Arc<AtomicBool>,
    asks: Asks,
    id: Value,
    method: String,
    params: Value,
) {
    if method == "initialize" {
        initialized.store(true, Ordering::SeqCst);
    } else if !initialized.load(Ordering::SeqCst) {
        emit(error_frame(&id, -32600, "Not initialized".into()));
        return;
    }
    let Some(entry) = entry else {
        emit(error_frame(
            &id,
            -32600,
            format!("Invalid request: unknown variant `{method}`"),
        ));
        return;
    };
    for event in &entry.notify {
        emit(notification(event));
    }
    if let Some(ms) = entry.delay_ms {
        thread::sleep(Duration::from_millis(ms));
    }
    if let Some(code) = entry.exit {
        std::process::exit(code);
    }
    if let Some(raw) = &entry.raw {
        let mut out = std::io::stdout().lock();
        writeln!(out, "{}", raw.replace("$ID", &id.to_string())).expect("stdout");
        out.flush().expect("stdout");
        return;
    }
    if let Some(ask) = &entry.ask {
        let (tx, rx) = mpsc::channel();
        asks.lock().unwrap().insert(ask.id.to_string(), tx);
        emit(json!({"id": ask.id, "method": ask.method, "params": ask.params}));
        match rx.recv_timeout(ASK_TIMEOUT) {
            Ok(Ok(result)) => emit(json!({"id": id, "result": {"answered": result}})),
            Ok(Err(error)) => emit(json!({"id": id, "result": {"refused": error}})),
            Err(_) => emit(error_frame(&id, -32000, "the client never answered".into())),
        }
        return;
    }
    // The state changes when the entry's outcome lands: with the delayed
    // events when there is a delay (a login is complete only once its
    // completion fires), else before the answer, so a request the client
    // sends on the answer reads the new state, the way codex has logged
    // out by the time it answers `account/logout`. The reader thread
    // selects the entry for the next request, so a state rewritten after
    // the answer would race it.
    if entry.then_delay_ms.is_none() {
        rewrite(&state, entry.set.as_ref());
    }
    if entry.echo {
        emit(json!({"id": id, "result": params}));
    } else if let Some(error) = &entry.error {
        emit(json!({"id": id, "error": error}));
    } else if let Some(reply) = &entry.reply {
        emit(json!({"id": id, "result": reply}));
    }
    if let Some(ms) = entry.then_delay_ms {
        thread::sleep(Duration::from_millis(ms));
        rewrite(&state, entry.set.as_ref());
    }
    for event in &entry.then {
        emit(notification(event));
    }
}

/// Rewrite the members `set` names in the fake's state.
fn rewrite(state: &FakeState, set: Option<&Map<String, Value>>) {
    let Some(set) = set else { return };
    let mut state = state.lock().unwrap();
    let members = state.as_object_mut().expect("the state is an object");
    for (key, value) in set {
        members.insert(key.clone(), value.clone());
    }
}

/// Stop serving stdin but stay alive with stdout open, like an app-server
/// that wedged: `ignore` leaves the pipe unread, `close` closes the read
/// end as well. Runs on the reader thread, so no read is in flight.
fn stop_reading(how: &str) -> ! {
    if how == "close" {
        #[cfg(unix)]
        {
            use std::os::fd::FromRawFd as _;
            // SAFETY: fd 0 is this process's stdin; the only reader is this
            // thread, which reads no more, so closing it here closes the
            // pipe's last read end.
            drop(unsafe { std::fs::File::from_raw_fd(0) });
        }
    }
    loop {
        thread::sleep(Duration::from_secs(60));
    }
}

fn serve(fixture: &Path) -> ! {
    if let Some(pid_file) = std::env::var_os(PID_FILE_ENV) {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(pid_file)
            .expect("pid file");
        writeln!(file, "{}", std::process::id()).expect("pid file");
    }
    if let Some(line) = std::env::var_os(STDERR_ENV) {
        eprintln!("{}", line.to_string_lossy());
    }
    match std::env::var(MODE_ENV).as_deref() {
        Ok("silent") => loop {
            thread::sleep(Duration::from_secs(60));
        },
        Ok(mode) if mode.starts_with("exit:") => {
            std::process::exit(mode["exit:".len()..].parse().expect("exit code"));
        }
        _ => {}
    }

    let script = load(fixture);
    let state: FakeState = Arc::new(Mutex::new(initial_state()));
    let initialized = Arc::new(AtomicBool::new(false));
    let asks: Asks = Arc::default();
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(Value::Object(mut frame)) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let method = frame
            .get("method")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let id = frame.remove("id");
        match (method, id) {
            (Some(method), Some(id)) => {
                let params = frame.remove("params").unwrap_or(Value::Object(Map::new()));
                let entry = select(&script, &method, &params, &state);
                let stdin_after = entry.as_ref().and_then(|entry| entry.stdin.clone());
                let (state, initialized, asks) = (state.clone(), initialized.clone(), asks.clone());
                thread::spawn(move || handle(entry, state, initialized, asks, id, method, params));
                if let Some(how) = stdin_after {
                    stop_reading(&how);
                }
            }
            (Some(_notification), None) => {}
            (None, Some(id)) => {
                let outcome = match (frame.remove("result"), frame.remove("error")) {
                    (Some(result), _) => Ok(result),
                    (None, Some(error)) => Err(error),
                    (None, None) => continue,
                };
                if let Some(waiting) = asks.lock().unwrap().remove(&id.to_string()) {
                    let _ = waiting.send(outcome);
                }
            }
            (None, None) => {}
        }
    }
    std::process::exit(0)
}
