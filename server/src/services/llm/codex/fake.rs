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
//! events emitted before the answer and `then` events after it; `delay_ms`
//! before answering; `exit` to die without answering; `ask` to send the
//! client a request and answer only once the client answered that; `raw`
//! to write a verbatim line with `$ID` replaced by the request id; `stdin`
//! to stop serving stdin after this request while stdout stays open, the
//! way a wedged app-server does: `ignore` leaves the pipe unread so the
//! client's writes block once it is full, `close` closes the read end so
//! they fail at once. Other lines are comments. Requests are served
//! concurrently, so answers come
//! back out of order like the real app-server's do. `initialize` must come
//! first (`-32600 Not initialized` otherwise) and an unscripted method is
//! `-32600 Invalid request: unknown variant`, the live error shapes.
//!
//! The environment steers the process: [`FIXTURE_ENV`] names the fixture
//! and turns the entry point into the server, [`MODE_ENV`] is `serve`
//! (default), `silent` (never answer) or `exit:<code>` (die at once),
//! [`PID_FILE_ENV`] gets the pid appended at every start, and
//! [`STDERR_ENV`] is a line written to stderr at start.

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
    exit: Option<i32>,
    #[serde(default)]
    ask: Option<Ask>,
    #[serde(default)]
    raw: Option<String>,
    #[serde(default)]
    stdin: Option<String>,
}

fn load(fixture: &Path) -> HashMap<String, Entry> {
    let text = std::fs::read_to_string(fixture)
        .unwrap_or_else(|error| panic!("{}: {error}", fixture.display()));
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let value: Value = serde_json::from_str(line).expect(line);
            value.get("on")?;
            let entry: Entry = serde_json::from_value(value).expect(line);
            Some((entry.on.clone(), entry))
        })
        .collect()
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
    script: Arc<HashMap<String, Entry>>,
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
    let Some(entry) = script.get(&method) else {
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
    if entry.echo {
        emit(json!({"id": id, "result": params}));
    } else if let Some(error) = &entry.error {
        emit(json!({"id": id, "error": error}));
    } else if let Some(reply) = &entry.reply {
        emit(json!({"id": id, "result": reply}));
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

    let script = Arc::new(load(fixture));
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
                let stdin_after = script.get(&method).and_then(|entry| entry.stdin.clone());
                let (script, initialized, asks) =
                    (script.clone(), initialized.clone(), asks.clone());
                thread::spawn(move || handle(script, initialized, asks, id, method, params));
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
