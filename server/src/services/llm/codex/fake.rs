//! A stand-in `codex app-server` for the tests, so CI never needs the real
//! binary: this very test binary, re-executed to run
//! [`fake_app_server_entry_point`] alone, plays the pinned fixture over
//! stdio. The crate has no library target and ships one binary, so the
//! test executable is the one program that is always built when these
//! tests run.
//!
//! The fixture (`fixtures/codex-<pin>.jsonl`) is one JSON object per line.
//! Lines with `on` script one method; several lines may script the same
//! method, and the first one whose conditions hold is the one played, so a
//! scenario is picked by what the client sent and by what happened before:
//! `when` (a subset of the request params: every key given must be present
//! and equal, arrays element by element at the same length), `env` (a
//! subset of the fake's environment) and `if` (a subset of the fake's
//! state, a flat object that starts as [`STATE_ENV`] says, empty by
//! default, a member never set reading as `null`, and that an entry's
//! `set` rewrites: before the answer, or, when `then_delay_ms` is set,
//! after that delay and before the `then` events; that is how a logout
//! changes what `account/read` says next, and a login only once its
//! completion fires, without teaching the fake the protocol's meaning).
//! Anywhere in an entry a string `$params.<path>` is replaced by that
//! part of the request params (`$params.threadId`), so an answer can echo
//! what it was asked. An entry then plays either `steps`, in order, each
//! one of `notify` (an event; `repeat` plays it that many times, for a
//! turn that streams more than a subscriber's buffer), `reply` (the
//! result), `error` (an error object), `ask` (a request the client must
//! answer before the next step), `delay_ms`, `exit` (die), or `raw` (a
//! verbatim line with `$ID` replaced by the request id), a step's `set`
//! rewriting the state before the rest of it plays; or, without
//! `steps`, the older shape: `reply` |
//! `echo` (the request params as the result) | `error`; `notify` events
//! before the answer and `then` events after it, after `then_delay_ms`
//! when that is set; `delay_ms` before answering; `exit`; `ask` (answer
//! only once the client answered that); `raw`. `stdin` on either shape
//! stops serving stdin after this request while stdout stays open, the
//! way a wedged app-server does: `ignore` leaves the pipe unread so the
//! client's writes block once it is full, `close` closes the read end so
//! they fail at once. Other lines are comments. Requests are served
//! concurrently, so answers come back out of order like the real
//! app-server's do. `initialize` must come first (`-32600 Not
//! initialized` otherwise) and an unscripted method is `-32600 Invalid
//! request: unknown variant`, the live error shapes.
//!
//! The environment steers the process: [`FIXTURE_ENV`] names the fixture
//! and turns the entry point into the server, [`MODE_ENV`] is `serve`
//! (default), `silent` (never answer) or `exit:<code>` (die at once),
//! [`PID_FILE_ENV`] gets the pid appended at every start, [`STDERR_ENV`] is
//! a line written to stderr at start, [`STATE_ENV`] is the initial state
//! as a JSON object, and [`LOG_ENV`] names a file every frame the client
//! wrote is appended to, one JSON object per line, so a test can read the
//! wire the way the HTTP mocks capture their requests.

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
pub const LOG_ENV: &str = "NOLUNE_FAKE_APP_SERVER_LOG";
/// Set to `none` to play the fixture's logged-out `account/read` answer.
pub const ACCOUNT_ENV: &str = "NOLUNE_FAKE_APP_SERVER_ACCOUNT";
/// Set to `hidden` to play a `config/read` that lists no MCP server while
/// the thread entries still start the configured ones: a server the
/// effective config did not show.
pub const MCP_ENV: &str = "NOLUNE_FAKE_APP_SERVER_MCP";

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

/// [`launch`] with every frame the client writes appended to `log`.
pub fn launch_logged(log: &Path) -> Launch {
    let mut launch = launch(None);
    launch
        .env
        .push((LOG_ENV.into(), log.as_os_str().to_owned()));
    launch
}

/// The frames a [`launch_logged`] fake received so far, in order. A line
/// still being written (no newline yet) is not there yet.
pub fn wire_log(log: &Path) -> Vec<Value> {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let complete = match text.rfind('\n') {
        Some(end) => &text[..end],
        None => "",
    };
    complete
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect(line))
        .collect()
}

/// The params of every request for `method` in a wire log.
pub fn sent(log: &Path, method: &str) -> Vec<Value> {
    wire_log(log)
        .into_iter()
        .filter(|frame| frame["method"] == method && frame.get("id").is_some())
        .map(|frame| frame["params"].clone())
        .collect()
}

/// The client's answers to the fake's own requests, `(id, result | error)`
/// in the order they were written.
pub fn answers(log: &Path) -> Vec<(Value, Result<Value, Value>)> {
    wire_log(log)
        .into_iter()
        .filter(|frame| frame.get("method").is_none() && frame.get("id").is_some())
        .map(|frame| {
            let outcome = match (frame.get("result"), frame.get("error")) {
                (Some(result), _) => Ok(result.clone()),
                (None, Some(error)) => Err(error.clone()),
                (None, None) => Err(Value::Null),
            };
            (frame["id"].clone(), outcome)
        })
        .collect()
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

/// One step of a scripted answer, played in order.
#[derive(Clone, Deserialize)]
struct Step {
    /// State members rewritten when the step plays, before anything else
    /// it does.
    #[serde(default)]
    set: Option<Map<String, Value>>,
    #[serde(default)]
    notify: Option<Event>,
    /// How many times `notify` is emitted; once when absent.
    #[serde(default)]
    repeat: Option<u32>,
    #[serde(default)]
    reply: Option<Value>,
    #[serde(default)]
    error: Option<Value>,
    #[serde(default)]
    ask: Option<Ask>,
    #[serde(default)]
    delay_ms: Option<u64>,
    #[serde(default)]
    exit: Option<i32>,
    #[serde(default)]
    raw: Option<String>,
}

/// One scripted line. `on`, `when`, `env` and `if` are read from the raw
/// JSON when an entry is selected; they are here so a fixture typo fails
/// at load.
#[derive(Clone, Deserialize)]
struct Entry {
    #[serde(rename = "on")]
    _on: String,
    #[serde(default, rename = "when")]
    _when: Option<Value>,
    #[serde(default, rename = "env")]
    _env: Option<HashMap<String, String>>,
    #[serde(default, rename = "if")]
    _only_if: Option<Map<String, Value>>,
    /// State members rewritten before the answer, or after `then_delay_ms`
    /// and before the `then` events when that is set.
    #[serde(default)]
    set: Option<Map<String, Value>>,
    #[serde(default)]
    steps: Option<Vec<Step>>,
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

/// The fake's state: what `if` reads and `set` writes.
type FakeState = Arc<Mutex<Value>>;

/// The initial state from [`STATE_ENV`]: a JSON object, or empty.
fn initial_state() -> Value {
    std::env::var(STATE_ENV)
        .ok()
        .map(|text| serde_json::from_str(&text).expect("the initial state is a JSON object"))
        .unwrap_or_else(|| Value::Object(Map::new()))
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

/// Whether the state holds every member `only_if` names with that value;
/// a member never set reads as `null`.
fn state_matches(only_if: &Value, state: &Value) -> bool {
    only_if.as_object().is_some_and(|only_if| {
        only_if
            .iter()
            .all(|(key, value)| state.get(key).unwrap_or(&Value::Null) == value)
    })
}

/// Every scripted line, in file order; the raw JSON is kept so the
/// `$params.` references can be filled in per request.
type Script = Vec<(String, Value)>;

fn load(fixture: &Path) -> Script {
    let text = std::fs::read_to_string(fixture)
        .unwrap_or_else(|error| panic!("{}: {error}", fixture.display()));
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let value: Value = serde_json::from_str(line).expect(line);
            let on = value.get("on")?.as_str()?.to_owned();
            // Validate the shape now, so a typo fails loudly at start.
            let _: Entry = serde_json::from_value(value.clone()).expect(line);
            Some((on, value))
        })
        .collect()
}

/// Whether `params` contains `when`: every object key given must be
/// present and match, arrays match element by element at the same length,
/// everything else must be equal.
fn matches(when: &Value, params: &Value) -> bool {
    match (when, params) {
        (Value::Object(want), Value::Object(have)) => want
            .iter()
            .all(|(key, value)| have.get(key).is_some_and(|got| matches(value, got))),
        (Value::Array(want), Value::Array(have)) => {
            want.len() == have.len() && want.iter().zip(have).all(|(w, h)| matches(w, h))
        }
        _ => when == params,
    }
}

/// Whether the fake's environment holds every variable in `env` with that value.
fn env_matches(env: &Value) -> bool {
    env.as_object().is_some_and(|env| {
        env.iter().all(|(name, value)| {
            value
                .as_str()
                .is_some_and(|value| std::env::var(name).is_ok_and(|got| got == value))
        })
    })
}

/// `value` with every `$params.<path>` string replaced by that part of `params`.
fn substitute(value: Value, params: &Value) -> Value {
    match value {
        Value::String(text) => match text.strip_prefix("$params.") {
            Some(path) => path
                .split('.')
                .try_fold(params, |current, segment| match current {
                    Value::Array(items) => segment.parse::<usize>().ok().and_then(|i| items.get(i)),
                    other => other.get(segment),
                })
                .cloned()
                .unwrap_or(Value::Null),
            None => Value::String(text),
        },
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| substitute(item, params))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, item)| (key, substitute(item, params)))
                .collect(),
        ),
        other => other,
    }
}

/// The first entry for `method` whose selectors accept `params`, the
/// environment and the state right now, with its `$params.` references
/// filled in.
fn select(script: &Script, method: &str, params: &Value, state: &Value) -> Option<Entry> {
    script
        .iter()
        .filter(|(on, _)| on == method)
        .find(|(_, raw)| {
            raw.get("when").is_none_or(|when| matches(when, params))
                && raw.get("env").is_none_or(env_matches)
                && raw
                    .get("if")
                    .is_none_or(|only_if| state_matches(only_if, state))
        })
        .map(|(_, raw)| {
            serde_json::from_value(substitute(raw.clone(), params)).expect("validated at load")
        })
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

/// Send the client a request and wait for its answer.
fn ask_client(asks: &Asks, ask: &Ask) -> Result<Result<Value, Value>, mpsc::RecvTimeoutError> {
    let (tx, rx) = mpsc::channel();
    asks.lock().unwrap().insert(ask.id.to_string(), tx);
    emit(json!({"id": ask.id, "method": ask.method, "params": ask.params}));
    rx.recv_timeout(ASK_TIMEOUT)
}

fn write_raw(raw: &str, id: &Value) {
    let mut out = std::io::stdout().lock();
    writeln!(out, "{}", raw.replace("$ID", &id.to_string())).expect("stdout");
    out.flush().expect("stdout");
}

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
    if let Some(steps) = &entry.steps {
        for step in steps {
            rewrite(&state, step.set.as_ref());
            if let Some(event) = &step.notify {
                for _ in 0..step.repeat.unwrap_or(1) {
                    emit(notification(event));
                }
            }
            if let Some(ms) = step.delay_ms {
                thread::sleep(Duration::from_millis(ms));
            }
            if let Some(reply) = &step.reply {
                emit(json!({"id": id, "result": reply}));
            }
            if let Some(error) = &step.error {
                emit(json!({"id": id, "error": error}));
            }
            if let Some(raw) = &step.raw {
                write_raw(raw, &id);
            }
            if let Some(ask) = &step.ask {
                // Whatever the client answered, or did not, the script goes on.
                let _ = ask_client(&asks, ask);
            }
            if let Some(code) = step.exit {
                std::process::exit(code);
            }
        }
        return;
    }
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
        write_raw(raw, &id);
        return;
    }
    if let Some(ask) = &entry.ask {
        match ask_client(&asks, ask) {
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
    let mut log = std::env::var_os(LOG_ENV).map(|path| {
        std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
            .expect("wire log")
    });

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
        if let Some(log) = log.as_mut() {
            // One write per line, so a reader never sees half a frame.
            let mut line = Value::Object(frame.clone()).to_string();
            line.push('\n');
            log.write_all(line.as_bytes()).expect("wire log");
        }
        let method = frame
            .get("method")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let id = frame.remove("id");
        match (method, id) {
            (Some(method), Some(id)) => {
                let params = frame.remove("params").unwrap_or(Value::Object(Map::new()));
                let entry = {
                    let state = state.lock().unwrap();
                    select(&script, &method, &params, &state)
                };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn when_is_a_subset_match_with_arrays_compared_at_the_same_length() {
        let params =
            json!({"threadId": "t", "input": [{"type": "text", "text": "hi"}], "extra": 1});
        assert!(matches(&json!({}), &params));
        assert!(matches(&json!({"threadId": "t"}), &params));
        assert!(matches(&json!({"input": [{"text": "hi"}]}), &params));
        assert!(!matches(&json!({"input": []}), &params));
        assert!(!matches(&json!({"input": [{"text": "bye"}]}), &params));
        assert!(!matches(&json!({"missing": true}), &params));
        assert!(matches(&json!({"input": []}), &json!({"input": []})));
    }

    #[test]
    fn params_references_are_filled_in_anywhere_in_an_entry() {
        let params = json!({"threadId": "thr_9", "turnId": "turn_2", "input": [{"text": "x"}]});
        let filled = substitute(
            json!({"reply": {"thread": {"id": "$params.threadId"}}, "then": [{"params": {"turn": {"id": "$params.turnId"}, "first": "$params.input.0.text", "gone": "$params.nope"}}], "plain": "$other"}),
            &params,
        );
        assert_eq!(filled["reply"]["thread"]["id"], "thr_9");
        assert_eq!(filled["then"][0]["params"]["turn"]["id"], "turn_2");
        assert_eq!(filled["then"][0]["params"]["first"], "x");
        assert_eq!(filled["then"][0]["params"]["gone"], Value::Null);
        assert_eq!(filled["plain"], "$other");
    }

    #[test]
    fn the_first_matching_entry_wins_and_its_answer_echoes_the_request() {
        let script = load(&fixture_path());
        let none = Value::Object(Map::new());
        let select = |method: &str, params: &Value| select(&script, method, params, &none);
        // The thread entries model the live merge of the config overrides:
        // only a start that disables every server `config/read` lists by
        // name gets a thread without MCP servers; anything else (an empty
        // table included) gets the thread the servers start for.
        let disabled = json!({"mcp_servers": {"filesystem": {"enabled": false}, "github": {"enabled": false}}});
        let ephemeral = select(
            "thread/start",
            &json!({"ephemeral": true, "model": "m", "config": disabled}),
        )
        .unwrap();
        assert_eq!(
            ephemeral.reply.unwrap()["thread"]["id"],
            "thr_fixture_ephemeral"
        );
        let durable = select("thread/start", &json!({"model": "m", "config": disabled})).unwrap();
        assert_eq!(durable.reply.unwrap()["thread"]["id"], "thr_fixture_1");
        for config in [
            json!({"mcp_servers": {}}),
            json!({"mcp_servers": {"filesystem": {"enabled": false}}}),
            json!({}),
        ] {
            let leaky = select(
                "thread/start",
                &json!({"ephemeral": true, "model": "m", "config": config}),
            )
            .unwrap();
            assert_eq!(
                leaky.reply.unwrap()["thread"]["id"],
                "thr_fixture_leaky",
                "{config}"
            );
            assert!(
                leaky
                    .then
                    .iter()
                    .any(|event| event.method == "mcpServer/startupStatus/updated"),
                "{config}: the servers start for the thread"
            );
        }
        let resumed = select(
            "thread/resume",
            &json!({"threadId": "thr_saved_7", "config": disabled}),
        )
        .unwrap();
        assert_eq!(resumed.reply.unwrap()["thread"]["id"], "thr_saved_7");
        let leaky = select(
            "thread/resume",
            &json!({"threadId": "thr_saved_7", "config": {"mcp_servers": {}}}),
        )
        .unwrap();
        assert!(
            leaky
                .notify
                .iter()
                .any(|event| event.method == "mcpServer/startupStatus/updated"),
            "a resume starts them before it answers"
        );
        assert!(
            select("thread/resume", &json!({"threadId": "thr_missing"}))
                .unwrap()
                .error
                .is_some()
        );
        let hidden = select("config/read", &json!({})).unwrap();
        assert_eq!(
            hidden.reply.unwrap()["config"]["mcp_servers"]
                .as_object()
                .unwrap()
                .len(),
            2,
            "the config lists two servers unless the environment hides them"
        );
        assert!(select("no/such", &json!({})).is_none());
    }
}
