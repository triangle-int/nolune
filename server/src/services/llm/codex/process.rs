//! One supervised `codex app-server` child.
//!
//! [`AppServer`] owns at most one child at a time. It completes the
//! `initialize` handshake within a deadline, correlates answers to requests
//! by id (the app-server answers out of order), hands notifications and the
//! app-server's own requests to every subscriber, and when the child dies
//! it fails what was pending with a typed error and starts a fresh child on
//! the next request, after a bounded exponential backoff. `close` kills the
//! child and refuses restarts; dropping the last handle kills it too, so no
//! child outlives the gateway.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    time::Duration,
};

use serde_json::Value;
use tokio::sync::broadcast;

use super::{APP_SERVER_ARGS, AppServerError, protocol::RpcError};

/// The deadlines one child is held to.
///
/// Without them a binary that never speaks the protocol (a wrapper script,
/// the wrong build) wedges the handshake, and a request the app-server
/// never answers wedges its caller, silently and forever.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Timeouts {
    /// How long a fresh child may take to answer `initialize`.
    pub handshake: Duration,
    /// How long one request may wait for its answer. Turn output streams
    /// as notifications, so a request is only ever a short exchange.
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
        let _ = failures;
        todo!("bounded exponential backoff")
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
}

/// What subscribers hear from the child, in the order it happened.
#[derive(Clone, Debug, PartialEq)]
pub enum Incoming {
    /// A child completed the handshake; `generation` counts children.
    Started { generation: u64, pid: u32 },
    /// A streamed event.
    Notification { method: String, params: Value },
    /// A request the app-server sent; answer it with
    /// [`AppServer::respond`] and the same `id`.
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

/// The supervised child. Cheap to clone; every clone is the same process.
#[derive(Clone)]
pub struct AppServer {}

impl AppServer {
    /// Start the child and complete the handshake now, so a binary that
    /// cannot serve is reported at start rather than at the first turn.
    pub async fn start(launch: Launch) -> Result<Self, AppServerError> {
        let _ = launch;
        todo!("spawn and handshake")
    }

    /// Send a request and wait for its answer, starting a child first when
    /// the previous one is gone.
    pub async fn request(&self, method: &str, params: Value) -> Result<Value, AppServerError> {
        let _ = (method, params);
        todo!("request")
    }

    /// Send a notification.
    pub async fn notify(&self, method: &str, params: Value) -> Result<(), AppServerError> {
        let _ = (method, params);
        todo!("notify")
    }

    /// Answer a request the app-server sent ([`Incoming::Request`]).
    pub async fn respond(
        &self,
        id: &Value,
        outcome: Result<Value, RpcError>,
    ) -> Result<(), AppServerError> {
        let _ = (id, outcome);
        todo!("respond")
    }

    /// Listen to what the child says from now on.
    pub fn subscribe(&self) -> broadcast::Receiver<Incoming> {
        todo!("subscribe")
    }

    /// The live child's pid, if there is one.
    pub fn pid(&self) -> Option<u32> {
        todo!("pid")
    }

    /// How many children completed the handshake so far.
    pub fn generation(&self) -> u64 {
        todo!("generation")
    }

    /// The binary this supervisor starts.
    pub fn binary(&self) -> &Path {
        todo!("binary")
    }

    /// Kill the child and refuse restarts. Idempotent.
    pub fn close(&self) {
        todo!("close")
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
        assert_eq!(models["data"][0]["id"], "gpt-5.5");
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
        assert!(message.contains("3"), "the exit status is named: {message}");
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
    async fn a_crash_fails_the_pending_request_and_the_next_request_restarts_one_child() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pids");
        let server = start(Some(&pid_file)).await;
        let first = server.pid().unwrap();
        let mut events = server.subscribe();

        let error = server.request("fake/crash", json!({})).await.unwrap_err();
        assert!(
            matches!(&error, AppServerError::Exited(reason) if reason.contains("3")),
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
