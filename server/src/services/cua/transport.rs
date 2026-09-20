//! The channel to one Cua Driver process.
//!
//! [`DriverTransport`] is the only thing the driver client needs: send a tool
//! call, get the structured payload back. The production implementation
//! speaks MCP over stdio to a `cua-driver mcp` child; tests use an in-memory
//! fake so no driver binary is ever required.

use std::{
    collections::VecDeque,
    future::Future,
    path::Path,
    pin::Pin,
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::Context as _;
use cua_protocol::driver_mcp::DriverCallFailure;
use rmcp::{
    ServiceExt as _,
    model::{
        CallToolRequest, CallToolRequestParams, CallToolResult, ClientRequest, RawContent,
        ServerResult,
    },
    service::{PeerRequestOptions, ServerSink, ServiceError},
    transport::TokioChildProcess,
};
use serde_json::{Map, Value};
use tokio::io::AsyncBufReadExt as _;

/// The structured payload of one tool call, or why there is none.
pub type CallOutcome = Result<Value, DriverCallFailure>;

pub type TransportFuture<'a> = Pin<Box<dyn Future<Output = CallOutcome> + Send + 'a>>;

/// One driver process the client can call tools on.
pub trait DriverTransport: Send + Sync {
    /// Call one MCP tool by its driver name and return its structured payload.
    fn call_tool(&self, name: &str, arguments: Map<String, Value>) -> TransportFuture<'_>;
}

/// The structured payload of an MCP `tools/call` result.
///
/// The driver puts the typed result in `structuredContent` and a human
/// rendering in the text block, so the text is only consulted when it is
/// itself JSON. An `isError` result carries the driver's structured `code`
/// beside its text.
pub(crate) fn payload_from_call_result(result: CallToolResult) -> CallOutcome {
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
    if text.trim_start().starts_with(['{', '['])
        && let Ok(parsed) = serde_json::from_str::<Value>(&text)
    {
        return Ok(parsed);
    }
    Err(DriverCallFailure::Malformed(
        "driver returned no structured payload".to_owned(),
    ))
}

/// The deadlines one driver process is held to.
///
/// Without them an executable that never speaks MCP (a wrapper script, the
/// wrong binary) wedges the `initialize` handshake, and a driver blocked on
/// a TCC prompt wedges every later tool call, silently and forever.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverTimeouts {
    /// How long `<driver> mcp` may take to answer the MCP `initialize`
    /// handshake before `spawn` gives up and kills the child.
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

/// How many of the driver's stderr lines are kept for an error message.
const STDERR_LINES_KEPT: usize = 16;
/// How long a stderr line may be in an error message.
const STDERR_LINE_LIMIT: usize = 400;
/// How long the stderr reader gets to drain after the child failed.
const STDERR_DRAIN: Duration = Duration::from_millis(300);

/// The last lines a driver child wrote to stderr, read as they arrive.
///
/// The driver explains itself there (`mcp launched without CuaDriver.app's
/// TCC grants; auto-launching the daemon`, `grant Accessibility + Screen
/// Recording to CuaDriver.app in System Settings and retry`), and only a
/// message that repeats it is actionable; inherited stderr reaches the
/// terminal after the parent has already printed its own verdict.
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
                    log::info!("[cua] driver: {line}");
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
            // reach end of file. Past that the pipe is held open by a
            // grandchild: report what arrived so far and stop reading.
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

/// MCP over stdio to one persistent `cua-driver mcp` child process.
///
/// The child lives exactly as long as this transport: the keep-alive task
/// owns the rmcp service, and dropping the transport aborts that task, which
/// drops the service, closes the connection and kills the child.
pub struct StdioDriverTransport {
    sink: ServerSink,
    keep_alive: tokio::task::JoinHandle<()>,
    timeouts: DriverTimeouts,
}

impl Drop for StdioDriverTransport {
    fn drop(&mut self) {
        self.keep_alive.abort();
    }
}

impl StdioDriverTransport {
    /// Spawn `<driver> mcp` and complete the MCP handshake with the default
    /// [`DriverTimeouts`].
    pub async fn spawn(driver: &Path) -> anyhow::Result<Self> {
        Self::spawn_with(driver, DriverTimeouts::default()).await
    }

    /// Spawn `<driver> mcp` and complete the MCP handshake within
    /// `timeouts.handshake`; a child that has not answered by then is killed
    /// and the error names the driver and repeats what it said on stderr,
    /// which is where the driver explains a missing daemon or permission.
    pub async fn spawn_with(driver: &Path, timeouts: DriverTimeouts) -> anyhow::Result<Self> {
        let mut command = tokio::process::Command::new(driver);
        command.arg("mcp");
        let (process, stderr) = TokioChildProcess::builder(command)
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("could not start {} mcp", driver.display()))?;
        let said = DriverStderr::capture(stderr);
        // On expiry the connect future is dropped with the child process
        // still inside it, which kills the child.
        let connected = tokio::time::timeout(
            timeouts.handshake,
            crate::services::mcp::client_info().serve(process),
        )
        .await;
        let running = match connected {
            Ok(Ok(running)) => running,
            Ok(Err(error)) => {
                return Err(anyhow::Error::new(error).context(format!(
                    "could not start {} mcp{}",
                    driver.display(),
                    said.suffix().await
                )));
            }
            Err(_elapsed) => anyhow::bail!(
                "{} mcp did not complete the MCP handshake within {:?}{}",
                driver.display(),
                timeouts.handshake,
                said.suffix().await
            ),
        };
        let sink = running.peer().clone();
        let keep_alive = tokio::spawn(async move {
            let _ = running.waiting().await;
        });
        log::info!("[cua] driver started: {} mcp", driver.display());
        Ok(Self {
            sink,
            keep_alive,
            timeouts,
        })
    }

    /// The deadlines this transport holds the driver to.
    pub fn timeouts(&self) -> DriverTimeouts {
        self.timeouts
    }

    /// Stop the driver child; the process is killed when the connection drops.
    pub fn shutdown(self) {
        log::info!("[cua] driver stopped");
        drop(self);
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
}

#[cfg(test)]
pub(crate) mod fake {
    //! An in-memory driver for tests: records every call and answers from a
    //! queue of canned outcomes.

    use super::*;
    use std::{collections::VecDeque, sync::Mutex};

    #[derive(Default)]
    pub struct FakeTransport {
        calls: Mutex<Vec<(String, Map<String, Value>)>>,
        outcomes: Mutex<VecDeque<CallOutcome>>,
    }

    impl FakeTransport {
        pub fn answering(outcomes: impl IntoIterator<Item = CallOutcome>) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                outcomes: Mutex::new(outcomes.into_iter().collect()),
            }
        }

        /// Every `(tool, arguments)` pair in the order it was called.
        pub fn calls(&self) -> Vec<(String, Map<String, Value>)> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl DriverTransport for FakeTransport {
        fn call_tool(&self, name: &str, arguments: Map<String, Value>) -> TransportFuture<'_> {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::Content;
    use serde_json::json;

    #[test]
    fn structured_content_is_the_payload() {
        let payload = json!({"apps": []});
        assert_eq!(
            payload_from_call_result(CallToolResult::structured(payload.clone())),
            Ok(payload)
        );
    }

    #[test]
    fn text_only_json_results_are_parsed() {
        let result = CallToolResult::success(vec![Content::text(r#"{"apps": []}"#)]);
        assert_eq!(payload_from_call_result(result), Ok(json!({"apps": []})));
    }

    #[test]
    fn human_text_without_structured_content_is_malformed() {
        let result = CallToolResult::success(vec![Content::text("✅ cua-driver 0.28.2 — ok")]);
        assert!(matches!(
            payload_from_call_result(result),
            Err(DriverCallFailure::Malformed(_))
        ));
        let empty = CallToolResult::success(vec![]);
        assert!(matches!(
            payload_from_call_result(empty),
            Err(DriverCallFailure::Malformed(_))
        ));
    }

    #[test]
    fn error_results_carry_the_structured_code_and_the_text() {
        // The shape the live 0.28.2 driver returns for a closed window.
        let mut result = CallToolResult::error(vec![Content::text(
            "window_id 1 is not a live window (closed, or the id is stale).",
        )]);
        result.structured_content = Some(json!({
            "code": "window_id_not_found", "pid": 1, "window_id": 1,
            "suggestion": "call list_windows for current window_ids"
        }));
        assert_eq!(
            payload_from_call_result(result),
            Err(DriverCallFailure::Tool {
                code: Some("window_id_not_found".into()),
                message: "window_id 1 is not a live window (closed, or the id is stale).".into(),
            })
        );

        let bare = CallToolResult::error(vec![Content::text("boom")]);
        assert_eq!(
            payload_from_call_result(bare),
            Err(DriverCallFailure::Tool {
                code: None,
                message: "boom".into(),
            })
        );
    }

    #[test]
    fn spawning_a_missing_driver_fails_instead_of_hanging() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let missing = std::env::temp_dir().join("nolune-no-such-cua-driver");
        let error = runtime
            .block_on(StdioDriverTransport::spawn(&missing))
            .err()
            .expect("a missing binary cannot be spawned");
        assert!(
            error.to_string().contains("nolune-no-such-cua-driver"),
            "names the driver: {error}"
        );
    }

    /// An executable shell script standing in for a driver binary.
    ///
    /// It is run once before it is returned: macOS scans a freshly written
    /// executable on its first run, which takes one to two seconds on this
    /// host and must not count against the deadlines under test.
    #[cfg(unix)]
    fn script(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(
            &path,
            format!("#!/bin/sh\n[ \"$1\" = warm-up ] && exit 0\n{body}"),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        // Other tests in this binary fork concurrently; on Linux a child forked
        // while the script was still open for writing holds that descriptor
        // until it execs, and running the script meanwhile fails with
        // ETXTBSY. Retry briefly instead of failing on that window.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let warmed = loop {
            match std::process::Command::new(&path).arg("warm-up").status() {
                Ok(status) => break status,
                Err(error)
                    if error.kind() == std::io::ErrorKind::ExecutableFileBusy
                        && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(error) => panic!("{}: {error}", path.display()),
            }
        };
        assert!(warmed.success(), "{}", path.display());
        path
    }

    /// A stdio MCP server that completes the `initialize` handshake and then
    /// never answers another request: the shape of a driver stuck behind a
    /// TCC prompt.
    #[cfg(unix)]
    const HANDSHAKE_ONLY_SERVER: &str = r#"
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"stub","version":"0"}}}\n' "$id"
      ;;
  esac
done
"#;

    /// Whether a process with this id still exists (a reaped one does not).
    #[cfg(unix)]
    fn process_exists(pid: &str) -> bool {
        std::process::Command::new("kill")
            .args(["-0", pid])
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_driver_that_never_speaks_mcp_fails_the_handshake_within_the_deadline() {
        use std::time::{Duration, Instant};

        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        let silent = script(
            dir.path(),
            "silent-driver",
            &format!("echo $$ > '{}'\nexec sleep 60\n", pid_file.display()),
        );
        let timeouts = DriverTimeouts {
            handshake: Duration::from_millis(500),
            call: Duration::from_secs(1),
        };

        let started = Instant::now();
        let error = StdioDriverTransport::spawn_with(&silent, timeouts)
            .await
            .err()
            .expect("a child that never answers initialize is not a driver");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the handshake deadline bounds spawn, took {:?}",
            started.elapsed()
        );
        let message = error.to_string();
        assert!(message.contains("handshake"), "{message}");
        assert!(
            message.contains("silent-driver"),
            "names the driver: {message}"
        );

        // The child does not outlive the failed handshake.
        let pid = loop {
            if let Ok(pid) = std::fs::read_to_string(&pid_file) {
                break pid.trim().to_owned();
            }
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "the child never reached its first line"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        assert!(!pid.is_empty());
        let gone_by = Instant::now() + Duration::from_secs(5);
        while process_exists(&pid) && Instant::now() < gone_by {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(!process_exists(&pid), "child {pid} was left running");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_tool_call_the_driver_never_answers_fails_as_a_timeout() {
        use std::time::{Duration, Instant};

        let dir = tempfile::tempdir().unwrap();
        let stub = script(dir.path(), "stub-driver", HANDSHAKE_ONLY_SERVER);
        let timeouts = DriverTimeouts {
            handshake: Duration::from_secs(10),
            call: Duration::from_millis(500),
        };
        let transport = StdioDriverTransport::spawn_with(&stub, timeouts)
            .await
            .expect("the stub completes the handshake");
        assert_eq!(transport.timeouts(), timeouts);

        let started = Instant::now();
        let outcome = transport.call_tool("health_report", Map::new()).await;
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the call deadline bounds call_tool, took {:?}",
            started.elapsed()
        );
        match outcome {
            Err(DriverCallFailure::Timeout(message)) => {
                assert!(message.contains("health_report"), "{message}");
                assert!(message.contains("500ms"), "{message}");
            }
            other => panic!("expected a timeout, got {other:?}"),
        }

        // A cancelled call does not wedge the connection: the next call gets
        // its own deadline instead of waiting behind the first.
        let again = transport.call_tool("list_apps", Map::new()).await;
        assert!(
            matches!(again, Err(DriverCallFailure::Timeout(_))),
            "{again:?}"
        );
        transport.shutdown();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_failed_handshake_reports_what_the_driver_said_on_stderr() {
        use std::time::Duration;

        let dir = tempfile::tempdir().unwrap();
        // The shape of the real driver on a fresh Mac: it explains itself on
        // stderr, then never answers `initialize`.
        let hinting = script(
            dir.path(),
            "hinting-driver",
            "echo 'mcp launched without CuaDriver.app grants' >&2\n\
             echo 'grant Accessibility + Screen Recording to CuaDriver.app in System Settings and retry' >&2\n\
             exec sleep 60\n",
        );
        let timeouts = DriverTimeouts {
            handshake: Duration::from_millis(500),
            call: Duration::from_secs(1),
        };
        let error = StdioDriverTransport::spawn_with(&hinting, timeouts)
            .await
            .err()
            .expect("the handshake never completes");
        let message = format!("{error:#}");
        assert!(message.contains("handshake"), "{message}");
        assert!(
            message.contains("grant Accessibility + Screen Recording to CuaDriver.app"),
            "the driver's own hint is part of the error: {message}"
        );
        assert!(
            message.contains("mcp launched without"),
            "every stderr line is kept, in order: {message}"
        );

        // A driver that exits at once with a reason: the same hint, no wait.
        let exiting = script(
            dir.path(),
            "exiting-driver",
            "echo 'this host has no CuaDriver.app' >&2\nexit 3\n",
        );
        let error = StdioDriverTransport::spawn_with(&exiting, timeouts)
            .await
            .err()
            .expect("a child that exits is not a driver");
        let message = format!("{error:#}");
        assert!(
            message.contains("this host has no CuaDriver.app"),
            "{message}"
        );

        // Silence stays silent: no empty "the driver said" suffix.
        let mute = script(dir.path(), "mute-driver", "exit 3\n");
        let error = StdioDriverTransport::spawn_with(&mute, timeouts)
            .await
            .err()
            .unwrap();
        let message = format!("{error:#}");
        assert!(!message.contains("said"), "{message}");
    }

    #[test]
    fn the_default_deadlines_are_generous_but_finite() {
        let defaults = DriverTimeouts::default();
        assert!(defaults.handshake >= std::time::Duration::from_secs(5));
        assert!(defaults.call >= std::time::Duration::from_secs(15));
        assert!(defaults.call <= std::time::Duration::from_secs(120));
    }
}
