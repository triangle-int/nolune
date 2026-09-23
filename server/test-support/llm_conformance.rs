//! The provider conformance harness (#29). Every adapter is driven through
//! one matrix of cases: the HTTP adapters against in-process mock servers
//! that answer in their own wire shapes, Codex against the fake app-server
//! playing the fixture recorded from the pinned release. A case is written
//! once and runs for every provider it applies to; the tests in
//! `services/llm/contract.rs` name the cases they run, and
//! `every_case_is_run_by_a_contract_test` keeps a new case from being left
//! out.
//!
//! The file lives under `server/test-support/` and is mounted as
//! `services::llm::conformance` under `#[cfg(test)]`: it is test source
//! that names every provider's wire format, which the guards that scan
//! `server/src` for production leaks must not read.
//!
//! Adding a provider: a variant in `PROVIDERS`, its answers in `wire`, and
//! a branch wherever a case matches on the provider; the compiler names
//! the rest.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::future::BoxFuture;
use serde_json::{Value, json};

use super::PROBE_TIMEOUT;
use super::codex::{self, Runtime, fake};
use super::contract::{Capabilities, ExecutionScope, LlmError, LlmEvent, LlmRequest, StopReason};
use super::types::{ContentBlock, LlmBackend, Message, ToolOutputContent};
use crate::config::{Config, LlmProvider};
use crate::services::tool::{ToolDefinition, ToolDyn, ToolError};

/// Every provider the contract covers; each new adapter joins here.
pub(super) const PROVIDERS: [LlmProvider; 4] = [
    LlmProvider::Anthropic,
    LlmProvider::Openai,
    LlmProvider::Openrouter,
    LlmProvider::Codex,
];

/// The providers spoken over HTTP, for the cases that mock a server and
/// answer with status codes or probe a key; Codex (#27) runs a local
/// process and joins the other cases through the fake app-server instead.
pub(super) const HTTP_PROVIDERS: [LlmProvider; 3] = [
    LlmProvider::Anthropic,
    LlmProvider::Openai,
    LlmProvider::Openrouter,
];

macro_rules! cases {
    ($($(#[$doc:meta])* $name:ident => $providers:expr),* $(,)?) => {
        /// One row of the matrix. `ALL` and `providers` are generated from
        /// the same list, so a case cannot be declared without saying whom
        /// it applies to.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(super) enum Case {
            $($(#[$doc])* $name,)*
        }

        impl Case {
            pub(super) const ALL: &'static [Case] = &[$(Case::$name),*];

            /// The providers the case runs for.
            pub(super) fn providers(self) -> &'static [LlmProvider] {
                match self {
                    $(Case::$name => $providers,)*
                }
            }
        }
    };
}

cases! {
    /// A cancelled token is answered before anything is started.
    CancellationBeforeNetwork => &PROVIDERS,
    /// One completion with text, a tool call and usage.
    CompletesWithToolsAndUsage => &PROVIDERS,
    /// `chat_json` sends the schema the provider's way and returns the text.
    StructuredOutput => &PROVIDERS,
    /// The canonical stream events: text deltas, tool call start and
    /// argument deltas, usage; never local tool activity.
    StreamsCanonicalEvents => &PROVIDERS,
    /// A stream that ends early, a body or an event that is not the
    /// protocol, a turn that ends out of protocol: `InvalidResponse`, and
    /// what is merely chatter on the wire is ignored.
    MalformedEvents => &PROVIDERS,
    /// Cancelling mid-request drops the request (HTTP) or interrupts the
    /// turn (Codex) and answers `Cancelled`.
    CancellationInterruptsInFlight => &PROVIDERS,
    /// Two tool calls through the agent loop, every result next to its
    /// call in the provider's own shape.
    ToolCallRoundTrip => &PROVIDERS,
    /// Malformed tool calls in both modes are `InvalidResponse` and no
    /// tool runs.
    InvalidToolCallsRejected => &PROVIDERS,
    /// 401 and 403 (Codex: `unauthorized`, an upstream 403) are
    /// `Authentication` in both modes.
    AuthenticationErrors => &PROVIDERS,
    /// 429, with the provider's `Retry-After` when it sends one, is
    /// `RateLimited { retry_after }` in both modes.
    RateLimits => &PROVIDERS,
    /// A request the model's context cannot hold is `ContextLength`.
    ContextOverflow => &PROVIDERS,
    /// Every other failure keeps its status as `Http`, a dead child is
    /// `Transport`, a missing login is `SetupRequired` before any turn.
    RemainingErrorsStayTyped => &PROVIDERS,
    /// The key probe: any answer past authentication proves the key.
    KeyProbe => &HTTP_PROVIDERS,
    /// The connection test: only a real answer counts.
    ConnectionTest => &PROVIDERS,
    /// A provider that never answers ends both probes at the deadline.
    ProbeDeadline => &HTTP_PROVIDERS,
}

impl Case {
    /// Run the case for one provider; every assertion names the provider.
    pub(super) async fn run(self, provider: LlmProvider) {
        match self {
            Case::CancellationBeforeNetwork => cancellation_before_network(provider).await,
            Case::CompletesWithToolsAndUsage => completes_with_tools_and_usage(provider).await,
            Case::StructuredOutput => structured_output(provider).await,
            Case::StreamsCanonicalEvents => streams_canonical_events(provider).await,
            Case::MalformedEvents => malformed_events(provider).await,
            Case::CancellationInterruptsInFlight => {
                cancellation_interrupts_in_flight(provider).await
            }
            Case::ToolCallRoundTrip => tool_call_round_trip(provider).await,
            Case::InvalidToolCallsRejected => invalid_tool_calls_rejected(provider).await,
            Case::AuthenticationErrors => authentication_errors(provider).await,
            Case::RateLimits => rate_limits(provider).await,
            Case::ContextOverflow => context_overflow(provider).await,
            Case::RemainingErrorsStayTyped => remaining_errors_stay_typed(provider).await,
            Case::KeyProbe => key_probe(provider).await,
            Case::ConnectionTest => connection_test(provider).await,
            Case::ProbeDeadline => probe_deadline(provider).await,
        }
    }
}

/// One case for every provider it applies to.
pub(super) async fn run_case(case: Case) {
    for provider in case.providers() {
        case.run(*provider).await;
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Backends and servers
// ═══════════════════════════════════════════════════════════════════════════

/// A backend for `provider` at `url`; a Codex backend runs on a fake
/// app-server of its own instead (nothing is started until a turn).
pub(super) fn backend(provider: LlmProvider, url: &str) -> LlmBackend {
    let mut config = Config::default();
    config.llm.add_test_presets(provider);
    config.llm.tokens.anthropic = "test".into();
    config.llm.tokens.open_ai = "test".into();
    config.llm.tokens.open_router = "test".into();
    let preset = match provider {
        LlmProvider::Anthropic => "sonnet".to_owned(),
        LlmProvider::Openai => "gpt-sol".to_owned(),
        LlmProvider::Openrouter | LlmProvider::Codex => {
            crate::config::test_presets(provider)[0].id.clone()
        }
    };
    let mut backend = LlmBackend::for_preset(&config, reqwest::Client::new(), &preset).unwrap();
    backend.base_url = url.into();
    if provider == LlmProvider::Codex {
        backend.codex = Runtime::for_launch(fake::launch(None));
    }
    backend
}

/// A Codex backend whose fake app-server holds no login.
pub(super) fn codex_backend_logged_out() -> LlmBackend {
    let mut launch = fake::launch(None);
    launch.env.push((fake::ACCOUNT_ENV.into(), "none".into()));
    let mut backend = backend(LlmProvider::Codex, "");
    backend.codex = Runtime::for_launch(launch);
    backend
}

/// A Codex backend on a fake app-server whose wire is logged: the
/// counterpart of a mock server's captured requests.
pub(super) struct CodexHarness {
    pub(super) backend: LlmBackend,
    log: std::path::PathBuf,
    _dir: tempfile::TempDir,
}

pub(super) fn codex_harness() -> CodexHarness {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("wire.jsonl");
    let mut backend = backend(LlmProvider::Codex, "");
    backend.codex = Runtime::for_launch(fake::launch_logged(&log));
    CodexHarness {
        backend,
        log,
        _dir: dir,
    }
}

impl CodexHarness {
    pub(super) fn sent(&self, method: &str) -> Vec<Value> {
        fake::sent(&self.log, method)
    }
    pub(super) fn answers(&self) -> Vec<(Value, Result<Value, Value>)> {
        fake::answers(&self.log)
    }
    pub(super) fn close(&self) {
        self.backend.codex.close();
    }
}

pub(super) async fn mock_server(
    status: u16,
    body: String,
) -> (String, Arc<Mutex<Vec<Value>>>, tokio::task::JoinHandle<()>) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    let app = axum::Router::new().fallback(axum::routing::post(
        move |axum::Json(request): axum::Json<Value>| {
            let captured = captured.clone();
            let body = body.clone();
            async move {
                captured.lock().unwrap().push(request);
                (axum::http::StatusCode::from_u16(status).unwrap(), body)
            }
        },
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (url, requests, task)
}

/// Request path and body, per request the mock received.
pub(super) type CapturedRequests = Arc<Mutex<Vec<(String, Value)>>>;

/// Like `mock_server`, but records each request's path and answers with
/// `headers` as well.
pub(super) async fn mock_server_with(
    status: u16,
    headers: Vec<(&'static str, String)>,
    body: String,
) -> (String, CapturedRequests, tokio::task::JoinHandle<()>) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    let app = axum::Router::new().fallback(axum::routing::post(
        move |uri: axum::http::Uri, axum::Json(request): axum::Json<Value>| {
            let captured = captured.clone();
            let body = body.clone();
            let headers = headers.clone();
            async move {
                captured
                    .lock()
                    .unwrap()
                    .push((uri.path().to_owned(), request));
                let mut map = axum::http::HeaderMap::new();
                for (name, value) in headers {
                    map.insert(name, value.parse().unwrap());
                }
                (axum::http::StatusCode::from_u16(status).unwrap(), map, body)
            }
        },
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (url, requests, task)
}

/// Like `mock_server_with`, but the n-th POST is answered with the n-th
/// body (the last one repeats), so a multi-turn loop can be driven.
pub(super) async fn mock_server_sequence(
    bodies: Vec<String>,
) -> (String, CapturedRequests, tokio::task::JoinHandle<()>) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    let bodies = Arc::new(bodies);
    let app = axum::Router::new().fallback(axum::routing::post(
        move |uri: axum::http::Uri, axum::Json(request): axum::Json<Value>| {
            let captured = captured.clone();
            let bodies = bodies.clone();
            async move {
                let mut captured = captured.lock().unwrap();
                let body = bodies[captured.len().min(bodies.len() - 1)].clone();
                captured.push((uri.path().to_owned(), request));
                (axum::http::StatusCode::OK, body)
            }
        },
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (url, requests, task)
}

/// A server that accepts every POST, tells `entered` and never answers.
async fn hanging_server() -> (
    String,
    Arc<tokio::sync::Notify>,
    tokio::task::JoinHandle<()>,
) {
    let entered = Arc::new(tokio::sync::Notify::new());
    let notify = entered.clone();
    let app = axum::Router::new().fallback(axum::routing::post(move || {
        let notify = notify.clone();
        async move {
            notify.notify_one();
            std::future::pending::<String>().await
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (url, entered, task)
}

/// A listener that accepts connections and never reads them.
async fn stalled_server() -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let mut held = Vec::new();
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            held.push(socket);
        }
    });
    (url, task)
}

// ═══════════════════════════════════════════════════════════════════════════
// Tools
// ═══════════════════════════════════════════════════════════════════════════

/// A tool the agent loop must never reach.
pub(super) struct MustNotExecute;
impl ToolDyn for MustNotExecute {
    fn name(&self) -> String {
        "search".into()
    }
    fn definition<'a>(&'a self, _: String) -> BoxFuture<'a, ToolDefinition> {
        Box::pin(async {
            ToolDefinition {
                name: "search".into(),
                description: "test".into(),
                parameters: json!({"type":"object"}),
            }
        })
    }
    fn call<'a>(&'a self, _: String) -> BoxFuture<'a, Result<String, ToolError>> {
        Box::pin(async { panic!("invalid provider response reached tool execution") })
    }
}

/// A tool that answers every call with the same text.
pub(super) struct Answers {
    pub(super) name: &'static str,
    pub(super) output: &'static str,
}
impl ToolDyn for Answers {
    fn name(&self) -> String {
        self.name.into()
    }
    fn definition<'a>(&'a self, _: String) -> BoxFuture<'a, ToolDefinition> {
        Box::pin(async {
            ToolDefinition {
                name: self.name.into(),
                description: "test".into(),
                parameters: json!({"type":"object"}),
            }
        })
    }
    fn call<'a>(&'a self, _: String) -> BoxFuture<'a, Result<String, ToolError>> {
        Box::pin(async { Ok(self.output.into()) })
    }
}

fn must_not_execute() -> Vec<Box<dyn ToolDyn>> {
    vec![Box::new(MustNotExecute)]
}

/// Run `tools` through the agent loop in either mode, for the cases that
/// prove no tool runs on a bad answer. The streaming loop gets a workspace
/// of its own, empty and gone afterwards: the Codex adapter keeps its
/// thread cwd and the chat's `meta.json` under the workspace, and a
/// shared one would make later rows, and later runs, resume a remembered
/// thread instead of starting their own.
async fn agent_boundary(
    backend: &LlmBackend,
    prompt: &str,
    streaming: bool,
    tools: Vec<Box<dyn ToolDyn>>,
) -> anyhow::Result<()> {
    if streaming {
        let workspace = tempfile::tempdir().unwrap();
        backend
            .chat_with_tools_streaming(
                &[],
                Message::user(prompt),
                vec![],
                tools,
                tokio::sync::broadcast::channel(32).0,
                "test",
                "test",
                workspace.path(),
                None,
                Default::default(),
            )
            .await
            .map(|_| ())
    } else {
        backend
            .chat_with_tools_traced("", prompt, vec![], tools)
            .await
            .map(|_| ())
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Wire shapes per provider
// ═══════════════════════════════════════════════════════════════════════════

/// What each HTTP provider answers on the wire. Codex answers from the
/// fixture, picked by the prompt text, so its rows are prompts.
pub(super) mod wire {
    use super::*;

    /// A finished Anthropic answer: "done", 10 in, 4 out.
    pub(crate) const END_TURN: &str = r#"{"content":[{"type":"text","text":"done"}],"stop_reason":"end_turn","usage":{"input_tokens":10,"output_tokens":4}}"#;

    /// A screenshot-like tool result: text plus an image, as the computer,
    /// files, image and memory tools return them.
    pub(crate) const SHOT_OUTPUT: &str = r#"[{"type":"text","text":"captured"},{"type":"image","source":{"type":"base64","media_type":"image/png","data":"abc"}}]"#;
    pub(crate) const SHOT_DATA_URL: &str = "data:image/png;base64,abc";

    /// A finished Chat Completions answer, as openrouter.ai sends it.
    pub(crate) fn openrouter_completion(
        text: &str,
        tool_calls: Value,
        finish: &str,
        usage: Value,
    ) -> Value {
        json!({"id":"gen-1","choices":[{"index":0,"message":{"role":"assistant","content":text,"tool_calls":tool_calls},"finish_reason":finish}],"usage":usage})
    }

    /// A plain text answer with the given usage, in the provider's shape.
    pub(crate) fn completion(provider: LlmProvider, text: &str, input: u64, output: u64) -> String {
        match provider {
            LlmProvider::Anthropic => {
                json!({"content":[{"type":"text","text":text}],"stop_reason":"end_turn","usage":{"input_tokens":input,"output_tokens":output}}).to_string()
            }
            LlmProvider::Openai => {
                json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":text}]}],"usage":{"input_tokens":input,"output_tokens":output}}).to_string()
            }
            LlmProvider::Openrouter => openrouter_completion(
                text,
                Value::Null,
                "stop",
                json!({"prompt_tokens":input,"completion_tokens":output}),
            )
            .to_string(),
            LlmProvider::Codex => unreachable!("the fake app-server answers from its fixture"),
        }
    }

    /// "hello" plus `search({"q":"rust"})` as `call1`, with cache usage.
    pub(crate) fn completion_with_tool(provider: LlmProvider) -> String {
        match provider {
            LlmProvider::Anthropic => {
                json!({"content":[{"type":"text","text":"hello"},{"type":"tool_use","id":"call1","name":"search","input":{"q":"rust"}}],"stop_reason":"tool_use","usage":{"input_tokens":10,"output_tokens":4,"cache_read_input_tokens":2,"cache_creation_input_tokens":3}}).to_string()
            }
            LlmProvider::Openai => {
                json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"hello"}]},{"type":"function_call","call_id":"call1","name":"search","arguments":"{\"q\":\"rust\"}"}],"usage":{"input_tokens":15,"output_tokens":4,"input_tokens_details":{"cached_tokens":2}}}).to_string()
            }
            LlmProvider::Openrouter => openrouter_completion(
                "hello",
                json!([{"id":"call1","type":"function","function":{"name":"search","arguments":"{\"q\":\"rust\"}"}}]),
                "tool_calls",
                json!({"prompt_tokens":15,"completion_tokens":4,"prompt_tokens_details":{"cached_tokens":2}}),
            )
            .to_string(),
            LlmProvider::Codex => unreachable!("the fake app-server answers from its fixture"),
        }
    }

    /// A stream of "hello", then `search` as `call1` with `{}` arguments,
    /// 5 tokens in and 3 out.
    pub(crate) fn stream_with_tool(provider: LlmProvider) -> String {
        match provider {
            LlmProvider::Anthropic => concat!(
                "event: message_start\ndata: {\"message\":{\"usage\":{\"input_tokens\":5}}}\n\n",
                "event: content_block_start\ndata: {\"content_block\":{\"type\":\"text\"}}\n\n",
                "event: content_block_delta\ndata: {\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n",
                "event: content_block_start\ndata: {\"content_block\":{\"type\":\"tool_use\",\"id\":\"call1\",\"name\":\"search\"}}\n\n",
                "event: content_block_delta\ndata: {\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{}\"}}\n\n",
                "event: content_block_stop\ndata: {}\n\n",
                "event: message_delta\ndata: {\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":3}}\n\n",
                "event: message_stop\ndata: {}\n\n"
            )
            .into(),
            LlmProvider::Openai => concat!(
                "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n",
                "data: {\"type\":\"response.output_item.added\",\"output_index\":1,\"item\":{\"type\":\"function_call\",\"call_id\":\"call1\",\"name\":\"search\"}}\n\n",
                "data: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":1,\"delta\":\"{}\"}\n\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[{\"type\":\"function_call\",\"call_id\":\"call1\",\"name\":\"search\",\"arguments\":\"{}\"}],\"usage\":{\"input_tokens\":5,\"output_tokens\":3}}}\n\n"
            )
            .into(),
            LlmProvider::Openrouter => concat!(
                "data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"hello\"},\"finish_reason\":null}]}\n\n",
                "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call1\",\"type\":\"function\",\"function\":{\"name\":\"search\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
                "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
                "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":3}}\n\n",
                "data: [DONE]\n\n"
            )
            .into(),
            LlmProvider::Codex => unreachable!("the fake app-server answers from its fixture"),
        }
    }

    /// A stream that starts well and then carries a `data:` line that is
    /// not JSON.
    pub(crate) fn stream_with_broken_event(provider: LlmProvider) -> String {
        match provider {
            LlmProvider::Anthropic => concat!(
                "event: message_start\ndata: {\"message\":{\"usage\":{\"input_tokens\":5}}}\n\n",
                "event: content_block_start\ndata: {\"content_block\":{\"type\":\"text\"}}\n\n",
                "event: content_block_delta\ndata: {\"delta\":{\"type\":\"text_delta\",\"text\":\"hel\"}}\n\n",
                "event: content_block_delta\ndata: {\"delta\":{\"type\":\"text_delta\",\"text\":\"lo\"\n\n",
                "event: message_stop\ndata: {}\n\n"
            )
            .into(),
            LlmProvider::Openai => concat!(
                "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hel\"}\n\n",
                "data: {\"type\":\"response.output_text.delta\",\"delta\":\"lo\"\n\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[],\"usage\":{\"input_tokens\":5,\"output_tokens\":3}}}\n\n"
            )
            .into(),
            LlmProvider::Openrouter => concat!(
                "data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"hel\"},\"finish_reason\":null}]}\n\n",
                "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"lo\"},\"finish_reason\":null}\n\n",
                "data: [DONE]\n\n"
            )
            .into(),
            LlmProvider::Codex => unreachable!("the fake app-server answers from its fixture"),
        }
    }

    /// Turn 1: "Looking." plus two calls, `shot` (call1) and `search`
    /// (call2); turn 2: "done". Non-streaming and streaming bodies per
    /// provider.
    pub(crate) fn round_trip_bodies(provider: LlmProvider, streaming: bool) -> Vec<String> {
        match (provider, streaming) {
            (LlmProvider::Anthropic, false) => vec![
                json!({"content":[{"type":"text","text":"Looking."},{"type":"tool_use","id":"call1","name":"shot","input":{}},{"type":"tool_use","id":"call2","name":"search","input":{"q":"rust"}}],"stop_reason":"tool_use","usage":{"input_tokens":10,"output_tokens":4}}).to_string(),
                json!({"content":[{"type":"text","text":"done"}],"stop_reason":"end_turn","usage":{"input_tokens":20,"output_tokens":1}}).to_string(),
            ],
            (LlmProvider::Anthropic, true) => vec![
                concat!(
                    "event: message_start\ndata: {\"message\":{\"usage\":{\"input_tokens\":10}}}\n\n",
                    "event: content_block_start\ndata: {\"content_block\":{\"type\":\"text\"}}\n\n",
                    "event: content_block_delta\ndata: {\"delta\":{\"type\":\"text_delta\",\"text\":\"Looking.\"}}\n\n",
                    "event: content_block_stop\ndata: {}\n\n",
                    "event: content_block_start\ndata: {\"content_block\":{\"type\":\"tool_use\",\"id\":\"call1\",\"name\":\"shot\"}}\n\n",
                    "event: content_block_delta\ndata: {\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{}\"}}\n\n",
                    "event: content_block_stop\ndata: {}\n\n",
                    "event: content_block_start\ndata: {\"content_block\":{\"type\":\"tool_use\",\"id\":\"call2\",\"name\":\"search\"}}\n\n",
                    "event: content_block_delta\ndata: {\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"q\\\":\\\"rust\\\"}\"}}\n\n",
                    "event: content_block_stop\ndata: {}\n\n",
                    "event: message_delta\ndata: {\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":4}}\n\n",
                    "event: message_stop\ndata: {}\n\n"
                )
                .into(),
                concat!(
                    "event: message_start\ndata: {\"message\":{\"usage\":{\"input_tokens\":20}}}\n\n",
                    "event: content_block_start\ndata: {\"content_block\":{\"type\":\"text\"}}\n\n",
                    "event: content_block_delta\ndata: {\"delta\":{\"type\":\"text_delta\",\"text\":\"done\"}}\n\n",
                    "event: content_block_stop\ndata: {}\n\n",
                    "event: message_delta\ndata: {\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n\n",
                    "event: message_stop\ndata: {}\n\n"
                )
                .into(),
            ],
            (LlmProvider::Openai, false) => vec![
                json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"Looking."}]},{"type":"function_call","call_id":"call1","name":"shot","arguments":"{}"},{"type":"function_call","call_id":"call2","name":"search","arguments":"{\"q\":\"rust\"}"}],"usage":{"input_tokens":10,"output_tokens":4}}).to_string(),
                json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"done"}]}],"usage":{"input_tokens":20,"output_tokens":1}}).to_string(),
            ],
            (LlmProvider::Openai, true) => vec![
                concat!(
                    "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Looking.\"}\n\n",
                    "data: {\"type\":\"response.output_item.added\",\"output_index\":1,\"item\":{\"type\":\"function_call\",\"call_id\":\"call1\",\"name\":\"shot\"}}\n\n",
                    "data: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":1,\"delta\":\"{}\"}\n\n",
                    "data: {\"type\":\"response.output_item.added\",\"output_index\":2,\"item\":{\"type\":\"function_call\",\"call_id\":\"call2\",\"name\":\"search\"}}\n\n",
                    "data: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":2,\"delta\":\"{\\\"q\\\":\\\"rust\\\"}\"}\n\n",
                    "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[{\"type\":\"function_call\",\"call_id\":\"call1\",\"name\":\"shot\",\"arguments\":\"{}\"},{\"type\":\"function_call\",\"call_id\":\"call2\",\"name\":\"search\",\"arguments\":\"{\\\"q\\\":\\\"rust\\\"}\"}],\"usage\":{\"input_tokens\":10,\"output_tokens\":4}}}\n\n"
                )
                .into(),
                concat!(
                    "data: {\"type\":\"response.output_text.delta\",\"delta\":\"done\"}\n\n",
                    "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"done\"}]}],\"usage\":{\"input_tokens\":20,\"output_tokens\":1}}}\n\n"
                )
                .into(),
            ],
            (LlmProvider::Openrouter, false) => vec![
                openrouter_completion(
                    "Looking.",
                    json!([
                        {"id":"call1","type":"function","function":{"name":"shot","arguments":"{}"}},
                        {"id":"call2","type":"function","function":{"name":"search","arguments":"{\"q\":\"rust\"}"}}
                    ]),
                    "tool_calls",
                    json!({"prompt_tokens":10,"completion_tokens":4}),
                )
                .to_string(),
                openrouter_completion(
                    "done",
                    Value::Null,
                    "stop",
                    json!({"prompt_tokens":20,"completion_tokens":1}),
                )
                .to_string(),
            ],
            (LlmProvider::Openrouter, true) => vec![
                concat!(
                    "data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Looking.\"},\"finish_reason\":null}]}\n\n",
                    "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call1\",\"type\":\"function\",\"function\":{\"name\":\"shot\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
                    "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{}\"}}]},\"finish_reason\":null}]}\n\n",
                    "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"call2\",\"type\":\"function\",\"function\":{\"name\":\"search\",\"arguments\":\"{\\\"q\\\":\"}}]},\"finish_reason\":null}]}\n\n",
                    "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":1,\"function\":{\"arguments\":\"\\\"rust\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
                    "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":4}}\n\n",
                    "data: [DONE]\n\n"
                )
                .into(),
                concat!(
                    "data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"done\"},\"finish_reason\":\"stop\"}]}\n\n",
                    "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":20,\"completion_tokens\":1}}\n\n",
                    "data: [DONE]\n\n"
                )
                .into(),
            ],
            (LlmProvider::Codex, _) => unreachable!("the fake app-server plays its own turn"),
        }
    }

    /// Where a provider keeps a tool call's id, name and arguments.
    pub(crate) fn tool_call_keys(provider: LlmProvider) -> (&'static str, &'static str) {
        match provider {
            LlmProvider::Anthropic => ("id", "input"),
            LlmProvider::Openai => ("call_id", "arguments"),
            LlmProvider::Openrouter => ("id", "arguments"),
            LlmProvider::Codex => unreachable!("codex has no HTTP tool call shape"),
        }
    }

    /// A well-formed tool call in the provider's own shape.
    pub(crate) fn valid_tool_call(provider: LlmProvider) -> Value {
        match provider {
            LlmProvider::Anthropic => {
                json!({"type":"tool_use","id":"id","name":"search","input":{}})
            }
            LlmProvider::Openai => {
                json!({"type":"function_call","call_id":"id","name":"search","arguments":"{}"})
            }
            LlmProvider::Openrouter => {
                json!({"id":"id","type":"function","function":{"name":"search","arguments":"{}"}})
            }
            LlmProvider::Codex => unreachable!("codex has no HTTP tool call shape"),
        }
    }

    /// Sets or removes one field of a tool call where that provider keeps it.
    pub(crate) fn corrupt_tool_call(
        provider: LlmProvider,
        call: &mut Value,
        field: &str,
        bad: &Option<Value>,
    ) {
        let target = match (provider, field) {
            (LlmProvider::Openrouter, "name" | "arguments") => &mut call["function"],
            _ => call,
        };
        match bad {
            Some(value) => target[field] = value.clone(),
            None => {
                target.as_object_mut().unwrap().remove(field);
            }
        }
    }

    /// A completion carrying `call` as its one tool call.
    pub(crate) fn completion_with_call(provider: LlmProvider, call: &Value) -> Value {
        match provider {
            LlmProvider::Anthropic => json!({"content":[call.clone()],"stop_reason":"tool_use"}),
            LlmProvider::Openai => json!({"status":"completed","output":[call.clone()]}),
            LlmProvider::Openrouter => openrouter_completion(
                "",
                json!([call.clone()]),
                "tool_calls",
                json!({"prompt_tokens":1,"completion_tokens":1}),
            ),
            LlmProvider::Codex => unreachable!("codex has no HTTP tool call shape"),
        }
    }

    /// A stream carrying `call` as its one tool call; `arguments` is what
    /// Anthropic streams as the call's argument delta (the Responses API
    /// streams the call's own `arguments`, Chat Completions the call).
    pub(crate) fn stream_with_call(provider: LlmProvider, call: &Value, arguments: &str) -> String {
        match provider {
            LlmProvider::Anthropic => {
                format!(
                    "event: content_block_start\ndata: {}\n\nevent: content_block_delta\ndata: {}\n\nevent: content_block_stop\ndata: {{}}\n\nevent: message_delta\ndata: {{\"delta\":{{\"stop_reason\":\"tool_use\"}}}}\n\nevent: message_stop\ndata: {{}}\n\n",
                    json!({"content_block":call}),
                    json!({"delta":{"type":"input_json_delta","partial_json":arguments}})
                )
            }
            LlmProvider::Openai => {
                let args = call.get("arguments").and_then(Value::as_str).unwrap_or("");
                format!(
                    "data: {}\n\ndata: {}\n\ndata: {}\n\n",
                    json!({"type":"response.output_item.added","output_index":0,"item":call}),
                    json!({"type":"response.function_call_arguments.delta","output_index":0,"delta":args}),
                    json!({"type":"response.completed","response":completion_with_call(provider, call)})
                )
            }
            LlmProvider::Openrouter => {
                let mut delta = call.clone();
                delta["index"] = json!(0);
                format!(
                    "data: {}\n\ndata: [DONE]\n\n",
                    json!({"choices":[{"index":0,"delta":{"tool_calls":[delta]},"finish_reason":"tool_calls"}]})
                )
            }
            LlmProvider::Codex => unreachable!("codex has no HTTP tool call shape"),
        }
    }

    pub(crate) fn anthropic_error_body(kind: &str, message: &str) -> String {
        json!({"type": "error", "error": {"type": kind, "message": message}}).to_string()
    }

    pub(crate) fn openai_error_body(code: &str, message: &str) -> String {
        json!({"error": {"type": "invalid_request_error", "code": code, "message": message}})
            .to_string()
    }

    pub(crate) fn openrouter_error_body(code: u16, message: &str) -> String {
        json!({"error": {"code": code, "message": message}}).to_string()
    }

    /// The request field that bounds the output, and the least the API
    /// accepts, which a probe asks for.
    pub(crate) fn smallest_completion(provider: LlmProvider) -> (&'static str, u64) {
        match provider {
            LlmProvider::Anthropic => ("max_tokens", 1),
            LlmProvider::Openai => ("max_output_tokens", 16),
            LlmProvider::Openrouter => ("max_tokens", 16),
            LlmProvider::Codex => unreachable!("no HTTP request to bound"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Typed error expectations
// ═══════════════════════════════════════════════════════════════════════════

/// What a failure must map to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Expect {
    Authentication,
    RateLimited(Option<Duration>),
    ContextLength,
    Http(u16),
    Transport,
    Invalid,
    SetupRequired,
}

pub(super) fn assert_error(label: &str, error: &LlmError, expect: Expect) {
    let label = format!("{label}: {error:?}");
    match expect {
        Expect::Authentication => {
            assert!(matches!(error, LlmError::Authentication(_)), "{label}")
        }
        Expect::RateLimited(wait) => {
            let LlmError::RateLimited { retry_after, .. } = error else {
                panic!("{label}");
            };
            assert_eq!(*retry_after, wait, "{label}");
        }
        Expect::ContextLength => assert!(matches!(error, LlmError::ContextLength(_)), "{label}"),
        Expect::Http(status) => assert!(
            matches!(error, LlmError::Http { status: got, .. } if *got == status),
            "{label}"
        ),
        Expect::Transport => assert!(matches!(error, LlmError::Transport(_)), "{label}"),
        Expect::Invalid => assert!(matches!(error, LlmError::InvalidResponse(_)), "{label}"),
        Expect::SetupRequired => assert!(matches!(error, LlmError::SetupRequired(_)), "{label}"),
    }
}

/// An HTTP answer and the variant it must become, in both modes.
struct HttpFailure {
    status: u16,
    headers: Vec<(&'static str, String)>,
    body: String,
    expect: Expect,
}

fn failure(status: u16, body: String, expect: Expect) -> HttpFailure {
    HttpFailure {
        status,
        headers: vec![],
        body,
        expect,
    }
}

fn retry_after(seconds: u64) -> Vec<(&'static str, String)> {
    vec![("retry-after", seconds.to_string())]
}

/// Ask the adapter in both modes and check the variant of each answer.
async fn expect_failure(provider: LlmProvider, backend: &LlmBackend, prompt: &str, expect: Expect) {
    let adapter = backend.adapter().unwrap();
    let messages = [Message::user(prompt)];
    let complete = adapter
        .complete(LlmRequest::new(
            ExecutionScope::Subagent,
            &[],
            &messages,
            &[],
        ))
        .await
        .unwrap_err();
    let stream = adapter
        .stream(
            LlmRequest::new(ExecutionScope::Subagent, &[], &messages, &[]),
            &|_| {},
        )
        .await
        .unwrap_err();
    for (mode, error) in [("complete", complete), ("stream", stream)] {
        assert_error(&format!("{provider:?} {mode} {prompt:?}"), &error, expect);
    }
}

/// Serve each failure from a mock and check both modes.
async fn expect_http_failures(provider: LlmProvider, failures: Vec<HttpFailure>) {
    for HttpFailure {
        status,
        headers,
        body,
        expect,
    } in failures
    {
        let (url, _, task) = mock_server_with(status, headers, body.clone()).await;
        let backend = backend(provider, &url);
        expect_failure(provider, &backend, &format!("{status} {body}"), expect).await;
        task.abort();
    }
}

/// Play each fixture prompt on a fake of its own and check both modes.
async fn expect_codex_failures(prompts: Vec<(&str, Expect)>) {
    for (prompt, expect) in prompts {
        let backend = backend(LlmProvider::Codex, "");
        expect_failure(LlmProvider::Codex, &backend, prompt, expect).await;
        backend.codex.close();
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// The cases
// ═══════════════════════════════════════════════════════════════════════════

async fn cancellation_before_network(provider: LlmProvider) {
    let adapter = backend(provider, "http://127.0.0.1:1").adapter().unwrap();
    let request = LlmRequest::new(ExecutionScope::Subagent, &[], &[], &[]);
    request.cancellation.cancel();
    let result = adapter.stream(request, &|_| {}).await;
    assert!(
        matches!(result, Err(LlmError::Cancelled)),
        "{provider:?}: {result:?}"
    );
}

/// The same completion through every adapter: the HTTP ones from a
/// mocked answer, Codex from the fake app-server's "hello" turn.
async fn completes_with_tools_and_usage(provider: LlmProvider) {
    let (backend, requests, task, harness) = if provider == LlmProvider::Codex {
        let harness = codex_harness();
        (harness.backend.clone(), None, None, Some(harness))
    } else {
        let (url, requests, task) = mock_server(200, wire::completion_with_tool(provider)).await;
        (backend(provider, &url), Some(requests), Some(task), None)
    };
    let adapter = backend.adapter().unwrap();
    let messages = [Message::user("hello")];
    let response = adapter
        .complete(LlmRequest::new(
            ExecutionScope::Subagent,
            &["system"],
            &messages,
            &[],
        ))
        .await
        .unwrap_or_else(|error| panic!("{provider:?}: {error:?}"));
    assert_eq!(response.text, "hello", "{provider:?}");
    assert_eq!(response.stop_reason, StopReason::ToolCalls, "{provider:?}");
    assert_eq!(response.tool_calls[0].id, "call1", "{provider:?}");
    assert_eq!(
        response.tool_calls[0].arguments["q"], "rust",
        "{provider:?}"
    );
    assert_eq!(response.usage.input_tokens, 15, "{provider:?}");
    assert_eq!(response.usage.output_tokens, 4, "{provider:?}");
    assert_eq!(response.usage.cache_read_tokens, 2, "{provider:?}");
    assert_eq!(
        response.usage.cache_write_tokens,
        if provider == LlmProvider::Anthropic {
            3
        } else {
            0
        }
    );
    if let Some(requests) = requests {
        assert_eq!(requests.lock().unwrap().len(), 1);
    }
    if let Some(harness) = harness {
        assert_eq!(harness.sent("turn/start").len(), 1);
        assert_eq!(
            harness.sent("thread/start")[0]["developerInstructions"],
            "system"
        );
        harness.close();
    }
    if let Some(task) = task {
        task.abort();
    }
}

async fn structured_output(provider: LlmProvider) {
    let schema = json!({"type":"object","properties":{},"additionalProperties":false});
    if provider == LlmProvider::Codex {
        let harness = codex_harness();
        let (text, tokens) = harness
            .backend
            .chat_json("system", "json please", schema.clone())
            .await
            .unwrap();
        assert_eq!(text, "{}");
        assert_eq!(tokens, 14);
        let turn = &harness.sent("turn/start")[0];
        assert_eq!(turn["outputSchema"], schema);
        harness.close();
        return;
    }
    let (url, requests, task) = mock_server(200, wire::completion(provider, "{}", 10, 4)).await;
    let backend = backend(provider, &url);
    let (text, tokens) = backend
        .chat_json("system", "prompt", schema.clone())
        .await
        .unwrap();
    assert_eq!(text, "{}");
    assert_eq!(tokens, 14);
    let requests = requests.lock().unwrap();
    match provider {
        LlmProvider::Anthropic => {
            assert_eq!(requests[0]["output_config"]["format"]["schema"], schema);
        }
        LlmProvider::Openai => {
            // Responses looks for the word "json" only in `input`, so the
            // schema travels as the format itself, enforced.
            let format = &requests[0]["text"]["format"];
            assert_eq!(format["type"], "json_schema");
            assert_eq!(format["schema"], schema);
            assert_eq!(format["strict"], true);
            assert!(format["name"].is_string());
            assert_eq!(requests[0]["store"], false);
        }
        LlmProvider::Openrouter => {
            assert_eq!(requests[0]["response_format"]["type"], "json_object");
        }
        LlmProvider::Codex => unreachable!("handled above"),
    }
    task.abort();
}

async fn streams_canonical_events(provider: LlmProvider) {
    let (backend, task) = if provider == LlmProvider::Codex {
        (backend(provider, ""), None)
    } else {
        let (url, _, task) = mock_server(200, wire::stream_with_tool(provider)).await;
        (backend(provider, &url), Some(task))
    };
    let events = Mutex::new(Vec::new());
    let sink = |event| events.lock().unwrap().push(event);
    let adapter = backend.adapter().unwrap();
    // The fake app-server picks its "stream" turn by this text; the
    // mocks answer whatever is asked.
    let messages = [Message::user("stream")];
    let response = adapter
        .stream(
            LlmRequest::new(ExecutionScope::Subagent, &[], &messages, &[]),
            &sink,
        )
        .await
        .unwrap_or_else(|error| panic!("{provider:?}: {error:?}"));
    assert_eq!(response.text, "hello", "{provider:?}");
    assert_eq!(response.stop_reason, StopReason::ToolCalls, "{provider:?}");
    assert_eq!(response.tool_calls[0].id, "call1", "{provider:?}");
    assert_eq!(response.usage.output_tokens, 3, "{provider:?}");
    let events = events.lock().unwrap();
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, LlmEvent::Activity { name, .. } if name == "search")),
        "local tool activity is emitted by the agent loop only"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, LlmEvent::TextDelta(s) if s == "hello"))
    );
    assert!(events.iter().any(
        |e| matches!(e, LlmEvent::ToolCallStarted { id, name } if id == "call1" && name == "search")
    ));
    assert!(events.iter().any(|e| matches!(e, LlmEvent::ToolArgumentsDelta { id, delta, .. } if id == "call1" && delta == "{}")));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, LlmEvent::Usage(u) if u.input_tokens == 5)),
        "{provider:?}"
    );
    drop(events);
    if let Some(task) = task {
        task.abort();
    }
    if provider == LlmProvider::Codex {
        backend.codex.close();
    }
}

/// Codex has no bodies: a turn that ends out of protocol is invalid in
/// both modes, and lines on the wire that are not frames are chatter the
/// transport ignores, so the turn around them completes intact. The HTTP
/// adapters answer a stream that ends early, a `data:` line that is not
/// JSON after a good delta, and a completion body that is not JSON with
/// `InvalidResponse`, never with the text so far.
async fn malformed_events(provider: LlmProvider) {
    if provider == LlmProvider::Codex {
        expect_codex_failures(vec![("weird", Expect::Invalid)]).await;
        let harness = codex_harness();
        let adapter = harness.backend.adapter().unwrap();
        let messages = [Message::user("garbled")];
        let deltas = Mutex::new(Vec::new());
        let sink = |event: LlmEvent| {
            if let LlmEvent::TextDelta(delta) = event {
                deltas.lock().unwrap().push(delta);
            }
        };
        let response = adapter
            .stream(
                LlmRequest::new(ExecutionScope::Subagent, &[], &messages, &[]),
                &sink,
            )
            .await
            .unwrap_or_else(|error| panic!("Codex garbled: {error:?}"));
        assert_eq!(response.text, "hello");
        assert_eq!(*deltas.lock().unwrap(), ["hel", "lo"]);
        assert_eq!(response.usage.input_tokens, 15);
        assert_eq!(response.stop_reason, StopReason::Complete);
        let response = adapter
            .complete(LlmRequest::new(
                ExecutionScope::Subagent,
                &[],
                &messages,
                &[],
            ))
            .await
            .unwrap_or_else(|error| panic!("Codex garbled: {error:?}"));
        assert_eq!(response.text, "hello");
        assert_eq!(harness.sent("turn/start").len(), 2);
        harness.close();
        return;
    }
    let (url, _, task) = mock_server(200, "data: {}\n\n".into()).await;
    let adapter = backend(provider, &url).adapter().unwrap();
    let result = adapter
        .stream(
            LlmRequest::new(ExecutionScope::Subagent, &[], &[], &[]),
            &|_| {},
        )
        .await;
    assert!(
        matches!(result, Err(LlmError::InvalidResponse(_))),
        "{provider:?} truncated stream: {result:?}"
    );
    task.abort();

    let (url, _, task) = mock_server(200, wire::stream_with_broken_event(provider)).await;
    let adapter = backend(provider, &url).adapter().unwrap();
    let result = adapter
        .stream(
            LlmRequest::new(ExecutionScope::Subagent, &[], &[], &[]),
            &|_| {},
        )
        .await;
    assert!(
        matches!(result, Err(LlmError::InvalidResponse(_))),
        "{provider:?} broken event: {result:?}"
    );
    task.abort();

    let (url, _, task) = mock_server(200, "<html>not json</html>".into()).await;
    let adapter = backend(provider, &url).adapter().unwrap();
    let result = adapter
        .complete(LlmRequest::new(ExecutionScope::Subagent, &[], &[], &[]))
        .await;
    assert!(
        matches!(result, Err(LlmError::InvalidResponse(_))),
        "{provider:?} non-JSON body: {result:?}"
    );
    task.abort();
}

async fn cancellation_interrupts_in_flight(provider: LlmProvider) {
    if provider == LlmProvider::Codex {
        // The turn is in flight once its first delta arrives; cancelling
        // then sends `turn/interrupt` and answers `Cancelled`.
        let harness = codex_harness();
        let adapter = harness.backend.adapter().unwrap();
        let messages = [Message::user("hang")];
        let request = LlmRequest::new(ExecutionScope::Subagent, &[], &messages, &[]);
        let token = request.cancellation.clone();
        let sink = |event: LlmEvent| {
            if matches!(event, LlmEvent::TextDelta(_)) {
                token.cancel();
            }
        };
        let result = tokio::time::timeout(Duration::from_secs(10), adapter.stream(request, &sink))
            .await
            .unwrap();
        assert!(matches!(result, Err(LlmError::Cancelled)), "{result:?}");
        let interrupts = harness.sent("turn/interrupt");
        assert_eq!(interrupts.len(), 1, "{interrupts:?}");
        assert_eq!(interrupts[0]["turnId"], "turn_fixture_13");
        harness.close();
        return;
    }
    let (url, entered, server) = hanging_server().await;
    let adapter = backend(provider, &url).adapter().unwrap();
    let request = LlmRequest::new(ExecutionScope::Subagent, &[], &[], &[]);
    let token = request.cancellation.clone();
    let cancel = async {
        entered.notified().await;
        token.cancel();
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(adapter.stream(request, &|_| {}), cancel)
    })
    .await
    .unwrap();
    assert!(
        matches!(result, Err(LlmError::Cancelled)),
        "{provider:?}: {result:?}"
    );
    server.abort();
}

/// Turn 1 answers with two tool calls, both tools run (the first one
/// returns an image), and turn 2's request carries the assistant's
/// calls and every result in the shape the provider requires: Chat
/// Completions needs the `tool` messages contiguous right after the
/// assistant's `tool_calls`, Anthropic every `tool_result` in the one
/// user message that follows, the Responses API a `function_call_output`
/// per call. Codex asks for its calls one at a time inside one turn,
/// so its trace is longer and each result answers the `item/tool/call`
/// it belongs to; its model cannot see images, so its screenshot is
/// text.
async fn tool_call_round_trip(provider: LlmProvider) {
    for streaming in [false, true] {
        let label = format!("{provider:?} streaming={streaming}");
        let (backend, requests, task, harness) = if provider == LlmProvider::Codex {
            let harness = codex_harness();
            (harness.backend.clone(), None, None, Some(harness))
        } else {
            let (url, requests, task) =
                mock_server_sequence(wire::round_trip_bodies(provider, streaming)).await;
            (backend(provider, &url), Some(requests), Some(task), None)
        };
        let tools: Vec<Box<dyn ToolDyn>> = vec![
            Box::new(Answers {
                name: "shot",
                output: if backend.adapter().unwrap().capabilities().vision {
                    wire::SHOT_OUTPUT
                } else {
                    "captured"
                },
            }),
            Box::new(Answers {
                name: "search",
                output: "found",
            }),
        ];
        let (text, trace) = if streaming {
            let workspace = tempfile::tempdir().unwrap();
            let result = backend
                .chat_with_tools_streaming(
                    &["system"],
                    Message::user("look it up"),
                    vec![],
                    tools,
                    tokio::sync::broadcast::channel(32).0,
                    "companion",
                    "chat",
                    workspace.path(),
                    None,
                    Default::default(),
                )
                .await
                .unwrap_or_else(|error| panic!("{label}: {error:?}"));
            (result.text, result.rig_history.unwrap())
        } else {
            let (text, _, trace) = backend
                .chat_with_tools_traced("system", "look it up", vec![], tools)
                .await
                .unwrap_or_else(|error| panic!("{label}: {error:?}"));
            (text, trace)
        };
        if let Some(task) = task {
            task.abort();
        }
        assert_eq!(text, "done", "{label}");

        if let Some(harness) = harness {
            // One turn, two calls asked in sequence, each answered
            // with its own result before the next was asked.
            assert_eq!(trace.len(), 6, "{label}: {trace:?}");
            let calls: Vec<(usize, &str)> = trace
                .iter()
                .enumerate()
                .filter_map(|(i, message)| match message {
                    Message::Assistant { content } => {
                        content.iter().find_map(|block| match block {
                            ContentBlock::ToolCall { id, .. } => Some((i, id.as_str())),
                            _ => None,
                        })
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(calls, [(1, "call1"), (3, "call2")], "{label}");
            for (index, call_id, output) in [(2, "call1", "captured"), (4, "call2", "found")] {
                let Message::User { content } = &trace[index] else {
                    panic!("{label}: {:?}", trace[index]);
                };
                assert!(
                    matches!(
                        &content[0],
                        ContentBlock::ToolOutput { call_id: id, content: ToolOutputContent::Text(text) }
                            if id == call_id && text == output
                    ),
                    "{label}: {:?}",
                    content[0]
                );
            }
            assert_eq!(harness.sent("turn/start").len(), 1, "{label}: one turn");
            let answers = harness.answers();
            assert_eq!(answers.len(), 2, "{label}: {answers:?}");
            assert_eq!(
                answers[0].1,
                Ok(
                    json!({"contentItems": [{"type": "inputText", "text": "captured"}], "success": true})
                ),
                "{label}"
            );
            assert_eq!(
                answers[1].1,
                Ok(
                    json!({"contentItems": [{"type": "inputText", "text": "found"}], "success": true})
                ),
                "{label}"
            );
            harness.close();
            continue;
        }
        let requests = requests.expect("an HTTP provider captures its requests");

        // The canonical trace: user, assistant with both calls, user
        // with both results (the first carrying its image), assistant.
        assert_eq!(trace.len(), 4, "{label}: {trace:?}");
        let Message::Assistant { content } = &trace[1] else {
            panic!("{label}: {:?}", trace[1]);
        };
        let calls: Vec<&str> = content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::ToolCall { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(calls, ["call1", "call2"], "{label}");
        let Message::User { content } = &trace[2] else {
            panic!("{label}: {:?}", trace[2]);
        };
        assert_eq!(content.len(), 2, "{label}: {content:?}");
        assert!(
            matches!(
                &content[0],
                ContentBlock::ToolOutput { call_id, content: ToolOutputContent::Blocks(blocks) }
                    if call_id == "call1" && blocks.iter().any(|b| matches!(b, ContentBlock::Image { .. }))
            ),
            "{label}: {:?}",
            content[0]
        );
        assert!(
            matches!(
                &content[1],
                ContentBlock::ToolOutput { call_id, content: ToolOutputContent::Text(text) }
                    if call_id == "call2" && text == "found"
            ),
            "{label}: {:?}",
            content[1]
        );

        // The second request, on the wire.
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 2, "{label}: {requests:?}");
        let second = &requests[1].1;
        match provider {
            LlmProvider::Openrouter => {
                let messages = second["messages"].as_array().unwrap();
                let roles: Vec<&str> = messages
                    .iter()
                    .map(|m| m["role"].as_str().unwrap())
                    .collect();
                assert_eq!(
                    roles,
                    ["system", "user", "assistant", "tool", "tool", "user"],
                    "{label}: {second}"
                );
                assert_eq!(messages[2]["content"], "Looking.", "{label}");
                assert_eq!(messages[2]["tool_calls"][0]["id"], "call1", "{label}");
                assert_eq!(
                    messages[2]["tool_calls"][1]["function"]["arguments"], "{\"q\":\"rust\"}",
                    "{label}"
                );
                assert_eq!(messages[3]["tool_call_id"], "call1", "{label}");
                assert_eq!(messages[3]["content"], "captured", "{label}");
                assert_eq!(messages[4]["tool_call_id"], "call2", "{label}");
                assert_eq!(messages[4]["content"], "found", "{label}");
                assert_eq!(
                    messages[5]["content"][0]["image_url"]["url"],
                    wire::SHOT_DATA_URL,
                    "{label}"
                );
            }
            LlmProvider::Anthropic => {
                let messages = second["messages"].as_array().unwrap();
                assert_eq!(messages.len(), 3, "{label}: {second}");
                let calls: Vec<&str> = messages[1]["content"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|block| block["type"] == "tool_use")
                    .map(|block| block["id"].as_str().unwrap())
                    .collect();
                assert_eq!(calls, ["call1", "call2"], "{label}");
                assert_eq!(messages[2]["role"], "user", "{label}");
                let results = messages[2]["content"].as_array().unwrap();
                assert_eq!(results.len(), 2, "{label}: {second}");
                assert!(
                    results.iter().all(|block| block["type"] == "tool_result"),
                    "{label}: {second}"
                );
                assert_eq!(results[0]["tool_use_id"], "call1", "{label}");
                assert_eq!(results[0]["content"][1]["type"], "image", "{label}");
                assert_eq!(results[1]["tool_use_id"], "call2", "{label}");
                assert_eq!(results[1]["content"], "found", "{label}");
            }
            LlmProvider::Openai => {
                let input = second["input"].as_array().unwrap();
                let calls: Vec<&str> = input
                    .iter()
                    .filter(|item| item["type"] == "function_call")
                    .map(|item| item["call_id"].as_str().unwrap())
                    .collect();
                assert_eq!(calls, ["call1", "call2"], "{label}: {second}");
                let outputs: Vec<(&str, &str)> = input
                    .iter()
                    .filter(|item| item["type"] == "function_call_output")
                    .map(|item| {
                        (
                            item["call_id"].as_str().unwrap(),
                            item["output"].as_str().unwrap(),
                        )
                    })
                    .collect();
                assert_eq!(
                    outputs,
                    [("call1", "captured"), ("call2", "found")],
                    "{label}: {second}"
                );
                let last_call = input
                    .iter()
                    .rposition(|item| item["type"] == "function_call")
                    .unwrap();
                let first_output = input
                    .iter()
                    .position(|item| item["type"] == "function_call_output")
                    .unwrap();
                assert!(last_call < first_output, "{label}: {second}");
                assert!(
                    input.iter().any(|item| item["type"] == "message"
                        && item["role"] == "user"
                        && item["content"][0]["image_url"] == wire::SHOT_DATA_URL),
                    "{label}: {second}"
                );
            }
            LlmProvider::Codex => unreachable!("checked above"),
        }
    }
}

async fn invalid_tool_calls_rejected(provider: LlmProvider) {
    if provider == LlmProvider::Codex {
        // An `item/tool/call` without a tool name, with arguments that are
        // not an object, or without a call id is refused on the wire and
        // reported as `InvalidResponse`; the agent boundary lets no tool
        // run for it.
        for prompt in ["bad tool name", "bad arguments", "bad call id"] {
            for streaming in [false, true] {
                let harness = codex_harness();
                let adapter = harness.backend.adapter().unwrap();
                let messages = [Message::user(prompt)];
                let request = LlmRequest::new(ExecutionScope::Subagent, &[], &messages, &[]);
                let result = if streaming {
                    adapter.stream(request, &|_| {}).await
                } else {
                    adapter.complete(request).await
                };
                assert!(
                    matches!(result, Err(LlmError::InvalidResponse(_))),
                    "Codex streaming={streaming} {prompt}: {result:?}"
                );
                let answers = harness.answers();
                assert!(
                    answers.len() == 1 && answers[0].1.is_err(),
                    "Codex {prompt}: refused on the wire: {answers:?}"
                );
                harness.close();
                // A fake of its own for the agent boundary: the fake plays a
                // scenario with the same thread and turn ids every time,
                // and the refused play above still emits its remaining
                // events once its handler wakes, which a second play on
                // the same process could hear as its own (the real
                // app-server never reuses an id).
                let harness = codex_harness();
                let error = agent_boundary(&harness.backend, prompt, streaming, must_not_execute())
                    .await
                    .unwrap_err();
                assert!(
                    matches!(
                        error.downcast_ref::<LlmError>(),
                        Some(LlmError::InvalidResponse(_))
                    ),
                    "Codex streaming={streaming} {prompt}: {error:?}"
                );
                // Every row starts from an empty workspace: the boundary
                // opens a thread of its own and never resumes one that
                // another row, or an earlier run, remembered in a chat's
                // `meta.json`.
                assert!(
                    harness.sent("thread/resume").is_empty(),
                    "Codex streaming={streaming} {prompt}: resumed a remembered thread: {:?}",
                    harness.sent("thread/resume")
                );
                assert_eq!(
                    harness.sent("thread/start").len(),
                    1,
                    "Codex streaming={streaming} {prompt}: {:?}",
                    harness.sent("thread/start")
                );
                harness.close();
            }
        }
        return;
    }
    let (id_key, args_key) = wire::tool_call_keys(provider);
    for field in [id_key, "name", args_key] {
        for bad in [
            None,
            Some(json!("")),
            Some(json!(null)),
            Some(json!(7)),
            Some(json!("broken{")),
            Some(json!([])),
        ] {
            let mut call = wire::valid_tool_call(provider);
            // Nonempty strings are valid IDs/names.
            if field != args_key && bad == Some(json!("broken{")) {
                continue;
            }
            wire::corrupt_tool_call(provider, &mut call, field, &bad);
            let complete = wire::completion_with_call(provider, &call);
            let streamed_arguments = if field == args_key {
                bad.as_ref()
                    .map(|v| {
                        v.as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| v.to_string())
                    })
                    .unwrap_or_default()
            } else {
                "{}".to_owned()
            };
            let stream = wire::stream_with_call(provider, &call, &streamed_arguments);
            for (streaming, body) in [(false, complete.to_string()), (true, stream)] {
                let (url, _, task) = mock_server(200, body).await;
                let adapter = backend(provider, &url).adapter().unwrap();
                let request = LlmRequest::new(ExecutionScope::Subagent, &[], &[], &[]);
                let result = if streaming {
                    adapter.stream(request, &|_| {}).await
                } else {
                    adapter.complete(request).await
                };
                assert!(
                    matches!(result, Err(LlmError::InvalidResponse(_))),
                    "{provider:?} streaming={streaming} field={field} bad={bad:?}: {result:?}"
                );
                // Exercise the real agent boundary as well: no tool may run.
                let backend = backend(provider, &url);
                let error = agent_boundary(&backend, "test", streaming, must_not_execute())
                    .await
                    .unwrap_err();
                assert!(matches!(
                    error.downcast_ref::<LlmError>(),
                    Some(LlmError::InvalidResponse(_))
                ));
                task.abort();
            }
        }
    }
}

async fn authentication_errors(provider: LlmProvider) {
    use wire::{anthropic_error_body, openai_error_body, openrouter_error_body};
    let failures = match provider {
        LlmProvider::Anthropic => vec![
            failure(
                401,
                anthropic_error_body("authentication_error", "invalid x-api-key"),
                Expect::Authentication,
            ),
            failure(
                403,
                anthropic_error_body("permission_error", "not allowed"),
                Expect::Authentication,
            ),
        ],
        LlmProvider::Openai => vec![
            failure(
                401,
                openai_error_body("invalid_api_key", "Incorrect API key provided"),
                Expect::Authentication,
            ),
            failure(
                403,
                openai_error_body("unsupported_country_region_territory", "no"),
                Expect::Authentication,
            ),
        ],
        LlmProvider::Openrouter => vec![
            failure(
                401,
                openrouter_error_body(401, "No auth credentials found"),
                Expect::Authentication,
            ),
            failure(
                403,
                openrouter_error_body(403, "Key limit exceeded"),
                Expect::Authentication,
            ),
        ],
        LlmProvider::Codex => {
            expect_codex_failures(vec![
                ("auth me", Expect::Authentication),
                ("forbidden", Expect::Authentication),
            ])
            .await;
            return;
        }
    };
    expect_http_failures(provider, failures).await;
}

/// `Retry-After` is read in seconds; a 429 without one carries no wait.
/// Codex's turn errors carry no retry hint in the pinned protocol, so
/// its rate limits (a usage limit, an upstream 429) say `None`.
async fn rate_limits(provider: LlmProvider) {
    use wire::{anthropic_error_body, openai_error_body, openrouter_error_body};
    let wait = Expect::RateLimited(Some(Duration::from_secs(7)));
    let no_wait = Expect::RateLimited(None);
    let failures = match provider {
        LlmProvider::Anthropic => vec![
            HttpFailure {
                status: 429,
                headers: retry_after(7),
                body: anthropic_error_body("rate_limit_error", "slow down"),
                expect: wait,
            },
            failure(429, "plain text".into(), no_wait),
            failure(
                529,
                anthropic_error_body("overloaded_error", "Overloaded"),
                no_wait,
            ),
        ],
        LlmProvider::Openai => vec![
            HttpFailure {
                status: 429,
                headers: retry_after(7),
                body: openai_error_body("rate_limit_exceeded", "Rate limit reached"),
                expect: wait,
            },
            failure(429, "plain text".into(), no_wait),
        ],
        LlmProvider::Openrouter => vec![
            HttpFailure {
                status: 429,
                headers: retry_after(7),
                body: openrouter_error_body(429, "Rate limit exceeded"),
                expect: wait,
            },
            failure(429, "plain text".into(), no_wait),
        ],
        LlmProvider::Codex => {
            expect_codex_failures(vec![("rate me", no_wait), ("overloaded", no_wait)]).await;
            return;
        }
    };
    expect_http_failures(provider, failures).await;
}

async fn context_overflow(provider: LlmProvider) {
    use wire::{anthropic_error_body, openai_error_body, openrouter_error_body};
    let failures = match provider {
        LlmProvider::Anthropic => vec![failure(
            400,
            anthropic_error_body(
                "invalid_request_error",
                "prompt is too long: 213462 tokens > 200000 maximum",
            ),
            Expect::ContextLength,
        )],
        LlmProvider::Openai => vec![failure(
            400,
            openai_error_body(
                "context_length_exceeded",
                "Your input exceeds the context window of this model.",
            ),
            Expect::ContextLength,
        )],
        LlmProvider::Openrouter => vec![failure(
            400,
            openrouter_error_body(
                400,
                "This endpoint's maximum context length is 8192 tokens. However, you requested about 9000 tokens",
            ),
            Expect::ContextLength,
        )],
        LlmProvider::Codex => {
            expect_codex_failures(vec![("context me", Expect::ContextLength)]).await;
            return;
        }
    };
    expect_http_failures(provider, failures).await;
}

/// `Http` is only the remainder; a Codex login the app-server has not got
/// is setup, reported before any turn is started.
async fn remaining_errors_stay_typed(provider: LlmProvider) {
    use wire::{anthropic_error_body, openai_error_body, openrouter_error_body};
    let failures = match provider {
        LlmProvider::Anthropic => vec![
            failure(
                400,
                anthropic_error_body("invalid_request_error", "messages: roles must alternate"),
                Expect::Http(400),
            ),
            failure(418, "teapot".into(), Expect::Http(418)),
            failure(500, "boom".into(), Expect::Http(500)),
        ],
        LlmProvider::Openai => vec![
            failure(
                400,
                openai_error_body("invalid_value", "Unsupported parameter"),
                Expect::Http(400),
            ),
            failure(418, "teapot".into(), Expect::Http(418)),
            failure(503, "down".into(), Expect::Http(503)),
        ],
        LlmProvider::Openrouter => vec![
            failure(
                400,
                openrouter_error_body(400, "anthropic/nope is not a valid model ID"),
                Expect::Http(400),
            ),
            failure(
                404,
                openrouter_error_body(404, "No endpoints found for anthropic/nope"),
                Expect::Http(404),
            ),
            failure(418, "teapot".into(), Expect::Http(418)),
            failure(502, "bad gateway".into(), Expect::Http(502)),
        ],
        LlmProvider::Codex => {
            expect_codex_failures(vec![
                ("teapot", Expect::Http(418)),
                ("break", Expect::Transport),
            ])
            .await;
            let backend = codex_backend_logged_out();
            let messages = [Message::user("hi")];
            let error = backend
                .adapter()
                .unwrap()
                .complete(LlmRequest::new(
                    ExecutionScope::Subagent,
                    &[],
                    &messages,
                    &[],
                ))
                .await
                .unwrap_err();
            assert_error("Codex logged out", &error, Expect::SetupRequired);
            assert!(
                matches!(&error, LlmError::SetupRequired(message) if message.contains("Codex login required")),
                "{error:?}"
            );
            backend.codex.close();
            return;
        }
    };
    expect_http_failures(provider, failures).await;
}

async fn key_probe(provider: LlmProvider) {
    let cases = [
        (200, wire::completion(provider, "ok", 1, 1), "ok"),
        // A rate limit, or any other answer past authentication,
        // proves the key.
        (429, "slow down".to_string(), "ok"),
        (
            404,
            json!({"error": {"message": "model not found"}}).to_string(),
            "ok",
        ),
        (401, "nope".to_string(), "authentication"),
        (503, "down".to_string(), "unavailable"),
    ];
    for (status, body, expected) in cases {
        let (url, requests, task) = mock_server_with(status, vec![], body).await;
        let mut backend = LlmBackend::probe(
            reqwest::Client::new(),
            provider,
            "model-x",
            "key-under-test",
        );
        backend.base_url = url;
        let result = backend.probe_key(PROBE_TIMEOUT).await;
        task.abort();
        let requests = requests.lock().unwrap();
        let body = &requests[0].1;
        assert_eq!(body["model"], "model-x");
        // The smallest completion each API accepts.
        let (limit, smallest) = wire::smallest_completion(provider);
        assert_eq!(
            body[limit], smallest,
            "{provider:?}: a probe asks for the least"
        );
        let label = format!("{provider:?} {status}: {result:?}");
        match expected {
            "ok" => assert!(result.is_ok(), "{label}"),
            "authentication" => {
                assert!(
                    matches!(result, Err(LlmError::Authentication(_))),
                    "{label}"
                )
            }
            _ => assert!(
                matches!(result, Err(LlmError::Http { status: 503, .. })),
                "{label}"
            ),
        }
    }
    // An unreachable provider says nothing about the key.
    let mut backend = LlmBackend::probe(reqwest::Client::new(), provider, "model-x", "k");
    backend.base_url = "http://127.0.0.1:1".into();
    assert!(matches!(
        backend.probe_key(PROBE_TIMEOUT).await,
        Err(LlmError::Transport(_))
    ));
}

/// The connection test (#28) sends the same one-token request as the key
/// probe, but only a real answer counts: a rate limit or a missing model
/// comes back as its variant instead of passing as "past authentication".
async fn connection_test(provider: LlmProvider) {
    if provider == LlmProvider::Codex {
        // One ephemeral turn with no tools, its usage reported; a missing
        // login is setup, not a failed test.
        let harness = codex_harness();
        let usage = harness
            .backend
            .test_connection(PROBE_TIMEOUT)
            .await
            .unwrap();
        assert_eq!((usage.input_tokens, usage.output_tokens), (10, 4));
        let started = harness.sent("thread/start");
        assert_eq!(started.len(), 1, "{started:?}");
        assert_eq!(started[0]["ephemeral"], true);
        assert_eq!(
            started[0]["dynamicTools"],
            json!([]),
            "a test carries no tools"
        );
        assert_eq!(
            harness.sent("turn/start").len(),
            1,
            "one turn, nothing else"
        );
        harness.close();
        let backend = codex_backend_logged_out();
        let result = backend.test_connection(PROBE_TIMEOUT).await;
        assert!(
            matches!(result, Err(LlmError::SetupRequired(_))),
            "{result:?}"
        );
        backend.codex.close();
        return;
    }
    let (url, requests, task) =
        mock_server_with(200, vec![], wire::completion(provider, "ok", 10, 4)).await;
    let mut backend = LlmBackend::probe(reqwest::Client::new(), provider, "model-x", "k");
    backend.base_url = url;
    let usage = backend.test_connection(PROBE_TIMEOUT).await.unwrap();
    task.abort();
    assert_eq!(
        (usage.input_tokens, usage.output_tokens),
        (10, 4),
        "{provider:?}"
    );
    {
        let requests = requests.lock().unwrap();
        assert_eq!(
            requests.len(),
            1,
            "{provider:?}: one completion, nothing else"
        );
        let body = &requests[0].1;
        assert_eq!(body["model"], "model-x");
        let (limit, smallest) = wire::smallest_completion(provider);
        assert_eq!(
            body[limit], smallest,
            "{provider:?}: a test asks for the least"
        );
        assert!(
            body["tools"].is_null(),
            "{provider:?}: a test carries no tools"
        );
    }

    for (status, body, expect) in [
        (401, "nope", Expect::Authentication),
        (429, "slow down", Expect::RateLimited(None)),
        (404, "no such model", Expect::Http(404)),
    ] {
        let (url, _, task) = mock_server_with(status, vec![], body.to_string()).await;
        let mut backend = LlmBackend::probe(reqwest::Client::new(), provider, "model-x", "k");
        backend.base_url = url;
        let error = backend.test_connection(PROBE_TIMEOUT).await.unwrap_err();
        task.abort();
        assert_error(&format!("{provider:?} test {status}"), &error, expect);
    }
}

/// A provider that accepts the connection and never answers ends both
/// probes at the deadline as `Timeout` (#28): the shared client has no
/// timeout of its own, so without one a key save or a connection test
/// would wait forever.
async fn probe_deadline(provider: LlmProvider) {
    let (url, task) = stalled_server().await;
    let mut backend = LlmBackend::probe(reqwest::Client::new(), provider, "model-x", "k");
    backend.base_url = url;
    let deadline = Duration::from_millis(200);
    let started = std::time::Instant::now();
    let test = backend.test_connection(deadline).await;
    let probe = backend.probe_key(deadline).await;
    task.abort();
    assert!(
        matches!(test, Err(LlmError::Timeout)),
        "{provider:?}: connection test answered {test:?}"
    );
    assert!(
        matches!(probe, Err(LlmError::Timeout)),
        "{provider:?}: key probe answered {probe:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{provider:?}: the probes waited {:?}",
        started.elapsed()
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// The harness's own tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// A case that no contract test names never runs; the matrix is only
    /// as complete as its wiring.
    #[test]
    fn every_case_is_run_by_a_contract_test() {
        let contract = include_str!("../src/services/llm/contract.rs");
        let missing: Vec<String> = Case::ALL
            .iter()
            .map(|case| format!("Case::{case:?}"))
            .filter(|name| !contract.contains(name.as_str()))
            .collect();
        assert!(
            missing.is_empty(),
            "contract.rs runs no test for {}",
            missing.join(", ")
        );
        for case in Case::ALL {
            assert!(!case.providers().is_empty(), "{case:?} applies to nobody");
            assert!(
                case.providers()
                    .iter()
                    .all(|provider| PROVIDERS.contains(provider)),
                "{case:?} names a provider outside PROVIDERS"
            );
        }
        assert!(
            HTTP_PROVIDERS
                .iter()
                .all(|provider| provider.auth() == crate::config::ProviderAuth::ApiKey),
            "HTTP_PROVIDERS are the key providers"
        );
        assert_eq!(
            PROVIDERS.len(),
            HTTP_PROVIDERS.len() + 1,
            "Codex is the one non-HTTP provider"
        );
    }

    /// The capability constants the docs table is checked against are the
    /// ones the adapters report through the contract.
    #[test]
    fn adapters_report_their_documented_capabilities() {
        for provider in PROVIDERS {
            let reported = backend(provider, "").adapter().unwrap().capabilities();
            let documented = super::super::provider_capabilities(
                provider,
                &crate::config::test_presets(provider)[0].model,
            );
            let same = |a: Capabilities, b: Capabilities| {
                (
                    a.vision,
                    a.documents,
                    a.tools,
                    a.streaming,
                    a.reasoning_controls,
                    a.model_discovery,
                    a.token_counting,
                ) == (
                    b.vision,
                    b.documents,
                    b.tools,
                    b.streaming,
                    b.reasoning_controls,
                    b.model_discovery,
                    b.token_counting,
                )
            };
            assert!(
                same(reported, documented),
                "{provider:?}: adapter {reported:?} vs provider_capabilities {documented:?}"
            );
            assert!(reported.streaming && reported.tools, "{provider:?}");
        }
    }

    /// Smoke test against the installed `codex` binary, skipped unless
    /// `which codex` finds one (it is `#[ignore]`d, so it runs only when
    /// asked). It uses an empty scratch `CODEX_HOME`, so it reads and
    /// writes nothing under the person's own codex home and needs no
    /// login: what it proves is that the binary on this machine is the
    /// pin, that the handshake, `model/list` (naming the seeded models)
    /// and `account/read` answer in the shapes the fixture records, that
    /// a turn without a login is refused as setup before any thread is
    /// started, and that closing the runtime leaves no `codex app-server`
    /// child behind. A logged-in turn, with `thread/start` and the tool
    /// bridge, is the live test in `codex/adapter.rs`.
    #[cfg(unix)]
    #[tokio::test]
    #[ignore]
    async fn codex_smoke_speaks_the_pinned_protocol_with_the_installed_binary() {
        let path = std::env::var_os("PATH");
        let located = codex::discovery::locate(codex::discovery::BinaryLookup {
            env_override: None,
            path: path.as_deref(),
        });
        let Ok((binary, _)) = located else {
            eprintln!("skipped: no codex on PATH ({located:?})");
            return;
        };
        let found = codex::discovery::discover()
            .await
            .unwrap_or_else(|error| panic!("{} is not the pin: {error}", binary.display()));
        assert_eq!(found.version, codex::CODEX_VERSION);

        let home = tempfile::tempdir().unwrap();
        let mut launch = codex::process::Launch::new(found.path);
        launch
            .env
            .push(("CODEX_HOME".into(), home.path().as_os_str().to_owned()));
        let runtime = Runtime::for_launch(launch);
        let server = runtime.app_server().await.expect("the handshake completes");
        let pid = server.pid().expect("a live child");

        let models = server
            .request("model/list", json!({}))
            .await
            .expect("model/list answers");
        let ids: Vec<&str> = models["data"]
            .as_array()
            .expect("a model list")
            .iter()
            .filter_map(|model| model["id"].as_str())
            .collect();
        for preset in crate::config::test_presets(LlmProvider::Codex) {
            assert!(
                ids.contains(&preset.model.as_str()),
                "the pinned release does not list the seeded model {}: {ids:?}",
                preset.model
            );
        }
        let account = server
            .request("account/read", json!({"refreshToken": false}))
            .await
            .expect("account/read answers");
        assert!(
            account["account"].is_null(),
            "a scratch home holds no login: {account}"
        );

        let mut backend = backend(LlmProvider::Codex, "");
        backend.codex = runtime.clone();
        let messages = [Message::user("hello")];
        let error = backend
            .adapter()
            .unwrap()
            .complete(LlmRequest::new(
                ExecutionScope::Subagent,
                &[],
                &messages,
                &[],
            ))
            .await
            .unwrap_err();
        assert!(
            matches!(&error, LlmError::SetupRequired(message) if message.contains("Codex login required")),
            "{error:?}"
        );
        assert!(
            home.path()
                .join("sessions")
                .read_dir()
                .map_or(true, |mut d| d.next().is_none()),
            "no thread was started without a login"
        );

        runtime.close();
        let gone = tokio::time::timeout(Duration::from_secs(5), async {
            while process_alive(pid) {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await;
        assert!(gone.is_ok(), "codex app-server {pid} outlived the runtime");
    }

    #[cfg(unix)]
    fn process_alive(pid: u32) -> bool {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
}
