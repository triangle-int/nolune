//! The channel to one Cua Driver process.
//!
//! [`DriverTransport`] is the only thing the driver client needs: send a tool
//! call, get the structured payload back. The production implementation
//! speaks MCP over stdio to a `cua-driver mcp` child; tests use an in-memory
//! fake so no driver binary is ever required.

use std::{future::Future, path::Path, pin::Pin, time::Duration};

use anyhow::Context as _;
use cua_protocol::driver_mcp::DriverCallFailure;
use rmcp::{
    model::{
        CallToolRequest, CallToolRequestParams, CallToolResult, ClientRequest, RawContent,
        ServerResult,
    },
    service::{PeerRequestOptions, ServerSink, ServiceError},
};
use serde_json::{Map, Value};

use crate::config::McpServerConfig;

/// The structured payload of one tool call, or why there is none.
pub type CallOutcome = Result<Value, DriverCallFailure>;

pub type TransportFuture<'a> = Pin<Box<dyn Future<Output = CallOutcome> + Send + 'a>>;

/// One driver process the client can call tools on.
pub trait DriverTransport: Send + Sync {
    /// Call one MCP tool by its driver name and return its structured payload.
    fn call_tool(&self, name: &str, arguments: Map<String, Value>) -> TransportFuture<'_>;

    /// Stop the driver for good: later calls fail and the child, if any, is
    /// killed. Idempotent, so a shutdown hook and a drop can both call it.
    fn close(&self);

    /// Resolve once the driver is gone for any reason: the child exited or
    /// crashed on its own, or `close` ran. Resolves at once for a driver that
    /// is already gone, so a watcher can never miss the exit.
    fn exited(&self) -> ExitFuture<'_>;
}

pub type ExitFuture<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

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

/// MCP over stdio to one persistent `cua-driver mcp` child process.
///
/// The child lives exactly as long as this transport: the keep-alive task
/// owns the rmcp service, and dropping the transport aborts that task, which
/// drops the service, closes the connection and kills the child. The task
/// also ends by itself when the child exits, which is how [`exited`]
/// (`DriverTransport::exited`) learns that the driver is gone.
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
    /// and the error names the driver.
    pub async fn spawn_with(driver: &Path, timeouts: DriverTimeouts) -> anyhow::Result<Self> {
        let config = McpServerConfig {
            name: "cua-driver".to_owned(),
            url: None,
            command: Some(driver.to_string_lossy().into_owned()),
            args: vec!["mcp".to_owned()],
            headers: Default::default(),
            trust: Default::default(),
            enabled_tools: Vec::new(),
        };
        // On expiry the connect future is dropped with the child process
        // still inside it, which kills the child.
        let connected = tokio::time::timeout(
            timeouts.handshake,
            crate::services::mcp::connect_stdio(&config),
        )
        .await;
        let (sink, keep_alive) = match connected {
            Ok(connection) => {
                connection.with_context(|| format!("could not start {} mcp", driver.display()))?
            }
            Err(_elapsed) => anyhow::bail!(
                "{} mcp did not complete the MCP handshake within {:?}",
                driver.display(),
                timeouts.handshake
            ),
        };
        log::info!("[cua] driver started: {} mcp", driver.display());
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

    /// The deadlines this transport holds the driver to.
    #[cfg(test)]
    pub fn timeouts(&self) -> DriverTimeouts {
        self.timeouts
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
            log::info!("[cua] driver stopped");
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

#[cfg(test)]
pub(crate) mod fake {
    //! An in-memory driver for tests: records every call and answers from a
    //! queue of canned outcomes.

    use super::*;
    use std::{collections::VecDeque, sync::Mutex};

    pub struct FakeTransport {
        calls: Mutex<Vec<(String, Map<String, Value>)>>,
        outcomes: Mutex<VecDeque<CallOutcome>>,
        closed: std::sync::atomic::AtomicBool,
        /// Flipped by `close` and by `crash`: the stand-in for a child that
        /// is no longer there.
        gone: tokio::sync::watch::Sender<bool>,
    }

    impl Default for FakeTransport {
        fn default() -> Self {
            Self::answering([])
        }
    }

    impl FakeTransport {
        pub fn answering(outcomes: impl IntoIterator<Item = CallOutcome>) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                outcomes: Mutex::new(outcomes.into_iter().collect()),
                closed: std::sync::atomic::AtomicBool::new(false),
                gone: tokio::sync::watch::Sender::new(false),
            }
        }

        /// The driver child died on its own: `exited` resolves and every
        /// later call fails, while `closed` stays false because nobody
        /// asked for the stop.
        pub fn crash(&self) {
            self.gone.send_replace(true);
        }

        /// Whether the driver is gone, by a crash or a close.
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
            .block_on(StdioDriverTransport::spawn_with(
                &missing,
                DriverTimeouts::default(),
            ))
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
        let warmed = std::process::Command::new(&path)
            .arg("warm-up")
            .status()
            .unwrap();
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
        transport.close();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn close_kills_the_driver_child_and_fails_later_calls() {
        use std::time::{Duration, Instant};

        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        let stub = script(
            dir.path(),
            "closing-driver",
            &format!(
                "echo $$ > '{}'\n{HANDSHAKE_ONLY_SERVER}",
                pid_file.display()
            ),
        );
        let transport = StdioDriverTransport::spawn_with(
            &stub,
            DriverTimeouts {
                handshake: Duration::from_secs(10),
                call: Duration::from_millis(500),
            },
        )
        .await
        .expect("the stub completes the handshake");
        let pid = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .to_owned();
        assert!(process_exists(&pid), "the child runs after the handshake");

        transport.close();
        transport.close(); // idempotent

        let gone_by = Instant::now() + Duration::from_secs(5);
        while process_exists(&pid) && Instant::now() < gone_by {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(!process_exists(&pid), "child {pid} outlived close()");
        let after = transport.call_tool("health_report", Map::new()).await;
        assert!(
            after.is_err(),
            "a closed transport answers nothing: {after:?}"
        );
    }

    /// A stdio MCP server that completes the handshake and exits as soon as
    /// the client's `initialized` notification arrives: a driver that dies
    /// right after it was described.
    #[cfg(unix)]
    const EXIT_AFTER_HANDSHAKE_SERVER: &str = r#"
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"stub","version":"0"}}}\n' "$id"
      ;;
    *'"method":"notifications/initialized"'*)
      exit 3
      ;;
  esac
done
"#;

    #[cfg(unix)]
    #[tokio::test]
    async fn a_driver_that_exits_on_its_own_is_reported_gone() {
        use std::time::{Duration, Instant};

        let dir = tempfile::tempdir().unwrap();
        let dying = script(dir.path(), "dying-driver", EXIT_AFTER_HANDSHAKE_SERVER);
        let transport = StdioDriverTransport::spawn_with(
            &dying,
            DriverTimeouts {
                handshake: Duration::from_secs(10),
                call: Duration::from_millis(500),
            },
        )
        .await
        .expect("the stub completes the handshake before it exits");

        let started = Instant::now();
        tokio::time::timeout(Duration::from_secs(5), transport.exited())
            .await
            .expect("exited resolves once the child is gone");
        assert!(started.elapsed() < Duration::from_secs(5));
        // Resolves again at once for a driver that is already gone.
        tokio::time::timeout(Duration::from_millis(100), transport.exited())
            .await
            .expect("an exited driver stays exited");
        let after = transport.call_tool("health_report", Map::new()).await;
        assert!(after.is_err(), "a dead driver answers nothing: {after:?}");
        // Closing what already exited is harmless.
        transport.close();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn close_resolves_exited_too() {
        use std::time::Duration;

        let dir = tempfile::tempdir().unwrap();
        let stub = script(dir.path(), "closing-driver", HANDSHAKE_ONLY_SERVER);
        let transport = StdioDriverTransport::spawn_with(&stub, DriverTimeouts::default())
            .await
            .expect("the stub completes the handshake");
        let still_running =
            tokio::time::timeout(Duration::from_millis(200), transport.exited()).await;
        assert!(still_running.is_err(), "a live driver has not exited");

        transport.close();
        tokio::time::timeout(Duration::from_secs(5), transport.exited())
            .await
            .expect("close is an exit as well");
    }

    #[tokio::test]
    async fn the_fake_reports_a_crash_and_a_close_alike_but_remembers_which() {
        use fake::FakeTransport;
        use std::time::Duration;

        let crashed = FakeTransport::answering([Ok(json!({"apps": []}))]);
        let alive = tokio::time::timeout(Duration::from_millis(50), crashed.exited()).await;
        assert!(alive.is_err(), "a fresh fake is alive");
        crashed.crash();
        tokio::time::timeout(Duration::from_millis(50), crashed.exited())
            .await
            .expect("a crashed fake has exited");
        assert!(!crashed.closed(), "nobody closed it");
        assert!(crashed.gone());
        let after = crashed.call_tool("list_apps", Map::new()).await;
        assert!(
            matches!(&after, Err(DriverCallFailure::Transport(message)) if message.contains("exited")),
            "{after:?}"
        );
        assert!(
            crashed.calls().is_empty(),
            "a call to a dead driver is not recorded as delivered"
        );

        let closed = FakeTransport::default();
        closed.close();
        tokio::time::timeout(Duration::from_millis(50), closed.exited())
            .await
            .expect("a closed fake has exited");
        assert!(closed.closed());
        assert!(closed.gone());
    }

    #[test]
    fn the_default_deadlines_are_generous_but_finite() {
        let defaults = DriverTimeouts::default();
        assert!(defaults.handshake >= std::time::Duration::from_secs(5));
        assert!(defaults.call >= std::time::Duration::from_secs(15));
        assert!(defaults.call <= std::time::Duration::from_secs(120));
    }
}
