//! Provider boundary. Conversation storage and the agent loop do not own API payloads.
use std::time::Duration;

use futures::future::BoxFuture;
use tokio_util::sync::CancellationToken;

use super::types::{ContentBlock, LlmResponse, Message};
use crate::services::tool::ToolDefinition;

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Capabilities {
    pub vision: bool,
    pub documents: bool,
    pub tools: bool,
    pub streaming: bool,
    pub reasoning_controls: bool,
    pub model_discovery: bool,
    /// Whether `ProviderAdapter::count_tokens` returns the provider's own count.
    pub token_counting: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Usage {
    /// Total input, including cache reads and writes.
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    /// What the provider charged for the turn, in USD, when it says (OpenRouter).
    pub cost: Option<f64>,
}

/// Every failure a caller can act on has its own variant; `Http` is only the
/// remainder. Callers match on the variant, never on status strings (#24, #25).
#[derive(Debug)]
pub enum LlmError {
    UnsupportedCapability(&'static str),
    SetupRequired(String),
    /// The provider rejected the API key or its permissions (401, 403).
    Authentication(String),
    /// The provider asked for a pause: 429, Anthropic's 529, or an overloaded
    /// error object. `retry_after` is the `Retry-After` header when one was sent.
    RateLimited {
        retry_after: Option<Duration>,
        message: String,
    },
    /// The request no longer fits the model's context window.
    ContextLength(String),
    Cancelled,
    Timeout,
    Http {
        status: u16,
        message: String,
    },
    Transport(String),
    InvalidResponse(String),
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedCapability(c) => write!(f, "selected provider does not support {c}"),
            Self::SetupRequired(s) => write!(f, "provider setup required: {s}"),
            Self::Authentication(s) => write!(f, "LLM authentication failed: {s}"),
            Self::RateLimited {
                retry_after: Some(wait),
                message,
            } => write!(
                f,
                "LLM rate limited, retry after {}s: {message}",
                wait.as_secs()
            ),
            Self::RateLimited {
                retry_after: None,
                message,
            } => write!(f, "LLM rate limited: {message}"),
            Self::ContextLength(s) => write!(f, "LLM context length exceeded: {s}"),
            Self::Cancelled => write!(f, "LLM request cancelled"),
            Self::Timeout => write!(f, "LLM stream timed out"),
            Self::Http { status, message } => write!(f, "LLM API error {status}: {message}"),
            Self::Transport(s) => write!(f, "LLM transport error: {s}"),
            Self::InvalidResponse(s) => write!(f, "invalid LLM response: {s}"),
        }
    }
}
impl std::error::Error for LlmError {}

/// The `Retry-After` header as a delay, when the provider sent one in seconds.
pub(super) fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
        .map(Duration::from_secs_f64)
}

impl From<anyhow::Error> for LlmError {
    fn from(error: anyhow::Error) -> Self {
        match error.downcast::<Self>() {
            Ok(error) => error,
            Err(error) => {
                if error.is::<reqwest::Error>() {
                    Self::Transport(error.to_string())
                } else {
                    Self::InvalidResponse(error.to_string())
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    Complete,
    ToolCalls,
    OutputLimit,
    Incomplete,
    Continue,
    ContextUpdated,
}
/// Provider-independent stream notifications. UI/MCP routing stays in the caller.
#[derive(Debug)]
pub enum LlmEvent {
    TextDelta(String),
    ToolCallStarted {
        id: String,
        name: String,
    },
    ToolArgumentsDelta {
        id: String,
        name: String,
        delta: String,
    },
    Activity {
        name: String,
        description: String,
    },
    Usage(Usage),
}
pub type EventSink<'a> = dyn Fn(LlmEvent) + Send + Sync + 'a;

/// The loop a request belongs to. A conversation waits on a person between
/// turns; a subagent run (companion routine, background one-shot) chains its
/// requests within minutes and then stops.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionScope {
    Conversation,
    Subagent,
}

impl ExecutionScope {
    /// Anthropic `cache_control.ttl` for prompt-cache entries written in this
    /// scope. The only place these literals live (#137).
    pub fn cache_ttl(self) -> &'static str {
        match self {
            Self::Conversation => "1h",
            Self::Subagent => "5m",
        }
    }
}

pub struct LlmRequest<'a> {
    /// Chooses the prompt-cache lifetime; see `ExecutionScope::cache_ttl`.
    pub scope: ExecutionScope,
    pub system: &'a [&'a str],
    pub messages: &'a [Message],
    pub tools: &'a [ToolDefinition],
    pub max_tokens: u64,
    pub json_schema: Option<&'a serde_json::Value>,
    /// Reserved explicitly, so unsupported controls cannot be silently ignored.
    pub reasoning: Option<&'a str>,
    pub cancellation: CancellationToken,
}
impl<'a> LlmRequest<'a> {
    pub fn new(
        scope: ExecutionScope,
        system: &'a [&'a str],
        messages: &'a [Message],
        tools: &'a [ToolDefinition],
    ) -> Self {
        Self {
            scope,
            system,
            messages,
            tools,
            max_tokens: 16384,
            json_schema: None,
            reasoning: None,
            cancellation: CancellationToken::new(),
        }
    }

    pub fn validate(&self, capabilities: Capabilities, streaming: bool) -> Result<(), LlmError> {
        if self.cancellation.is_cancelled() {
            return Err(LlmError::Cancelled);
        }
        if streaming && !capabilities.streaming {
            return Err(LlmError::UnsupportedCapability("streaming"));
        }
        if streaming && self.json_schema.is_some() {
            return Err(LlmError::UnsupportedCapability(
                "streaming structured output",
            ));
        }
        if self.reasoning.is_some() && !capabilities.reasoning_controls {
            return Err(LlmError::UnsupportedCapability("reasoning controls"));
        }
        if !self.tools.is_empty() && !capabilities.tools {
            return Err(LlmError::UnsupportedCapability("tools"));
        }
        fn validate_blocks(blocks: &[ContentBlock], c: Capabilities) -> Result<(), LlmError> {
            for block in blocks {
                match block {
                    ContentBlock::Image { .. } if !c.vision => {
                        return Err(LlmError::UnsupportedCapability("vision"));
                    }
                    ContentBlock::Document { .. } if !c.documents => {
                        return Err(LlmError::UnsupportedCapability("documents"));
                    }
                    ContentBlock::ToolCall { .. } | ContentBlock::ToolOutput { .. } if !c.tools => {
                        return Err(LlmError::UnsupportedCapability("tools"));
                    }
                    ContentBlock::ToolOutput {
                        content: super::types::ToolOutputContent::Blocks(blocks),
                        ..
                    } => {
                        validate_blocks(blocks, c)?;
                    }
                    _ => {}
                }
            }
            Ok(())
        }
        for message in self.messages {
            let (Message::User { content } | Message::Assistant { content }) = message;
            validate_blocks(content, capabilities)?;
        }
        Ok(())
    }
}

/// Both adapters implement this same object-safe contract. Dropping a future or
/// cancelling its token drops the HTTP request/stream; no background task survives.
pub trait ProviderAdapter: Send + Sync {
    fn capabilities(&self) -> Capabilities;
    fn complete<'a>(
        &'a self,
        request: LlmRequest<'a>,
    ) -> BoxFuture<'a, Result<LlmResponse, LlmError>>;
    fn stream<'a>(
        &'a self,
        request: LlmRequest<'a>,
        events: &'a EventSink<'a>,
    ) -> BoxFuture<'a, Result<LlmResponse, LlmError>>;
    /// The provider's own input-token count for `request`, when
    /// `Capabilities::token_counting` says it has one.
    fn count_tokens<'a>(&'a self, request: LlmRequest<'a>) -> BoxFuture<'a, Result<u64, LlmError>> {
        let _ = request;
        Box::pin(async { Err(LlmError::UnsupportedCapability("token counting")) })
    }
    // Reserved extension point; current adapters advertise discovery as unsupported.
    #[allow(dead_code)]
    fn discover_models(&self) -> BoxFuture<'_, Result<Vec<String>, LlmError>> {
        Box::pin(async { Err(LlmError::UnsupportedCapability("model discovery")) })
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        anthropic::messages_to_anthropic,
        openai::messages_to_openai,
        types::{DocumentSource, HistoryEntry, ImageSource, LlmBackend, ToolOutputContent},
    };
    use super::*;
    use crate::config::{Config, LlmProvider};
    use serde_json::{Value, json};
    use std::sync::{Arc, Mutex};

    /// Every provider the contract covers; each new adapter joins here.
    const PROVIDERS: [LlmProvider; 3] = [
        LlmProvider::Anthropic,
        LlmProvider::Openai,
        LlmProvider::Openrouter,
    ];

    fn backend(provider: LlmProvider, url: &str) -> LlmBackend {
        let mut config = Config::default();
        config.llm.seed_presets(provider);
        config.llm.tokens.anthropic = "test".into();
        config.llm.tokens.open_ai = "test".into();
        config.llm.tokens.open_router = "test".into();
        let preset = match provider {
            LlmProvider::Anthropic => "sonnet".to_owned(),
            LlmProvider::Openai => "gpt".to_owned(),
            LlmProvider::Openrouter => crate::config::default_presets(provider)[0].id.clone(),
        };
        let mut backend = LlmBackend::for_preset(&config, reqwest::Client::new(), &preset).unwrap();
        backend.base_url = url.into();
        backend
    }

    /// A finished Chat Completions answer, as openrouter.ai sends it.
    fn openrouter_completion(text: &str, tool_calls: Value, finish: &str, usage: Value) -> Value {
        json!({"id":"gen-1","choices":[{"index":0,"message":{"role":"assistant","content":text,"tool_calls":tool_calls},"finish_reason":finish}],"usage":usage})
    }

    #[test]
    fn stored_history_converts_to_both_providers_without_mutation() {
        // Existing flattened history with tool calls, results, summary and metadata.
        let raw = json!([
            {"role":"user", "content":[{"type":"text","text":"hello"}],"ts":"1","id":"u"},
            {"role":"assistant", "content":[{"type":"tool_use","id":"call1","name":"search","input":{"q":"rust"}}],"model":"old-model"},
            {"role":"user", "content":[{"type":"tool_result","tool_use_id":"call1","content":"found"}]},
            {"role":"assistant", "content":[{"type":"compaction","content":"summary"}]}
        ]);
        let entries: Vec<HistoryEntry> = serde_json::from_value(raw.clone()).unwrap();
        let messages = HistoryEntry::to_messages(&entries);
        let anthropic = messages_to_anthropic(&messages);
        let (_, openai) = messages_to_openai(&[], &messages);
        assert_eq!(anthropic[1]["content"][0]["id"], "call1");
        assert_eq!(anthropic[2]["content"][0]["tool_use_id"], "call1");
        assert_eq!(openai[1]["call_id"], "call1");
        assert_eq!(openai[2]["call_id"], "call1");
        assert_eq!(openai[2]["output"], "found");
        assert!(openai[3]["content"].as_str().unwrap().contains("summary"));
        assert_eq!(serde_json::to_value(entries).unwrap(), raw);
    }

    #[test]
    fn opaque_metadata_only_returns_to_originating_provider() {
        let messages = [Message::Assistant {
            content: vec![
                ContentBlock::text("answer"),
                ContentBlock::ProviderData {
                    provider: LlmProvider::Anthropic,
                    data: json!({"type":"server_tool_use","id":"srv","name":"web_search"}),
                },
            ],
        }];
        let saved = serde_json::to_value(&messages).unwrap();
        let restored: Vec<Message> = serde_json::from_value(saved).unwrap();
        assert_eq!(
            messages_to_anthropic(&restored)[0]["content"][1]["id"],
            "srv"
        );
        let (_, openai) = messages_to_openai(&[], &restored);
        assert_eq!(openai.len(), 1);
        assert_eq!(openai[0]["content"], "answer");
    }

    #[test]
    fn images_and_multimodal_tool_results_convert_without_losing_images() {
        let image = ContentBlock::Image {
            source: ImageSource::Base64 {
                media_type: "image/png".into(),
                data: "abc".into(),
            },
            resource_provenance: None,
        };
        let messages = [Message::User {
            content: vec![
                image.clone(),
                ContentBlock::ToolOutput {
                    call_id: "call1".into(),
                    content: super::super::types::ToolOutputContent::Blocks(vec![
                        ContentBlock::text("screenshot"),
                        image,
                    ]),
                },
            ],
        }];
        let (_, input) = messages_to_openai(&[], &messages);
        assert_eq!(input[0]["content"][0]["type"], "input_image");
        assert_eq!(
            input[0]["content"][0]["image_url"],
            "data:image/png;base64,abc"
        );
        assert_eq!(input[1]["output"], "screenshot");
        assert_eq!(input[2]["content"][0]["type"], "input_image");
        assert_eq!(
            messages_to_anthropic(&messages)[0]["content"][1]["content"][1]["type"],
            "image"
        );
    }

    #[tokio::test]
    async fn capabilities_and_cancellation_reject_before_network() {
        let adapter = backend(LlmProvider::Openai, "http://127.0.0.1:1")
            .adapter()
            .unwrap();
        let messages = [Message::User {
            content: vec![ContentBlock::Document {
                source: DocumentSource::Url {
                    url: "https://example.test/doc.pdf".into(),
                },
                resource_provenance: None,
            }],
        }];
        assert!(matches!(
            adapter
                .complete(LlmRequest::new(
                    ExecutionScope::Subagent,
                    &[],
                    &messages,
                    &[]
                ))
                .await,
            Err(LlmError::UnsupportedCapability("documents"))
        ));
        let mut plain = backend(LlmProvider::Openai, "http://127.0.0.1:1");
        plain.model = "gpt-4.1".into();
        let mut request = LlmRequest::new(ExecutionScope::Subagent, &[], &[], &[]);
        request.reasoning = Some("high");
        assert!(matches!(
            plain.adapter().unwrap().complete(request).await,
            Err(LlmError::UnsupportedCapability("reasoning controls"))
        ));
        assert!(matches!(
            adapter.discover_models().await,
            Err(LlmError::UnsupportedCapability("model discovery"))
        ));
        for provider in PROVIDERS {
            let adapter = backend(provider, "http://127.0.0.1:1").adapter().unwrap();
            let request = LlmRequest::new(ExecutionScope::Subagent, &[], &[], &[]);
            request.cancellation.cancel();
            assert!(matches!(
                adapter.stream(request, &|_| {}).await,
                Err(LlmError::Cancelled)
            ));
        }
    }

    #[test]
    fn capability_validation_includes_nested_tool_results() {
        let capabilities = Capabilities {
            vision: false,
            documents: false,
            tools: true,
            streaming: false,
            reasoning_controls: false,
            model_discovery: false,
            token_counting: false,
        };
        let messages = [Message::User {
            content: vec![ContentBlock::ToolOutput {
                call_id: "id".into(),
                content: super::super::types::ToolOutputContent::Blocks(vec![
                    ContentBlock::Image {
                        source: ImageSource::Url {
                            url: "https://example.test/image".into(),
                        },
                        resource_provenance: None,
                    },
                ]),
            }],
        }];
        assert!(matches!(
            LlmRequest::new(ExecutionScope::Subagent, &[], &messages, &[])
                .validate(capabilities, false),
            Err(LlmError::UnsupportedCapability("vision"))
        ));
        assert!(matches!(
            LlmRequest::new(ExecutionScope::Subagent, &[], &[], &[]).validate(capabilities, true),
            Err(LlmError::UnsupportedCapability("streaming"))
        ));
        let capabilities = Capabilities {
            tools: false,
            ..capabilities
        };
        assert!(matches!(
            LlmRequest::new(ExecutionScope::Subagent, &[], &messages, &[])
                .validate(capabilities, false),
            Err(LlmError::UnsupportedCapability("tools"))
        ));
    }

    async fn mock_server(
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
    type CapturedRequests = Arc<Mutex<Vec<(String, Value)>>>;

    /// Like `mock_server`, but records each request's path and answers with
    /// `headers` as well.
    async fn mock_server_with(
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

    #[tokio::test]
    async fn both_adapters_complete_with_tools_and_usage_through_same_contract() {
        for (provider, body) in [
            (
                LlmProvider::Anthropic,
                json!({"content":[{"type":"text","text":"hello"},{"type":"tool_use","id":"call1","name":"search","input":{"q":"rust"}}],"stop_reason":"tool_use","usage":{"input_tokens":10,"output_tokens":4,"cache_read_input_tokens":2,"cache_creation_input_tokens":3}}),
            ),
            (
                LlmProvider::Openai,
                json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"hello"}]},{"type":"function_call","call_id":"call1","name":"search","arguments":"{\"q\":\"rust\"}"}],"usage":{"input_tokens":15,"output_tokens":4,"input_tokens_details":{"cached_tokens":2}}}),
            ),
            (
                LlmProvider::Openrouter,
                openrouter_completion(
                    "hello",
                    json!([{"id":"call1","type":"function","function":{"name":"search","arguments":"{\"q\":\"rust\"}"}}]),
                    "tool_calls",
                    json!({"prompt_tokens":15,"completion_tokens":4,"prompt_tokens_details":{"cached_tokens":2}}),
                ),
            ),
        ] {
            let (url, requests, task) = mock_server(200, body.to_string()).await;
            let adapter = backend(provider, &url).adapter().unwrap();
            let messages = [Message::user("hello")];
            let response = adapter
                .complete(LlmRequest::new(
                    ExecutionScope::Subagent,
                    &["system"],
                    &messages,
                    &[],
                ))
                .await
                .unwrap();
            assert_eq!(response.text, "hello");
            assert_eq!(response.stop_reason, StopReason::ToolCalls);
            assert_eq!(response.tool_calls[0].id, "call1");
            assert_eq!(response.tool_calls[0].arguments["q"], "rust");
            assert_eq!(response.usage.input_tokens, 15);
            assert_eq!(response.usage.output_tokens, 4);
            assert_eq!(response.usage.cache_read_tokens, 2);
            assert_eq!(
                response.usage.cache_write_tokens,
                if provider == LlmProvider::Anthropic {
                    3
                } else {
                    0
                }
            );
            assert_eq!(requests.lock().unwrap().len(), 1);
            task.abort();
        }
    }

    #[tokio::test]
    async fn structured_output_stays_in_adapters() {
        for (provider, body) in [
            (
                LlmProvider::Anthropic,
                json!({"content":[{"type":"text","text":"{}"}],"stop_reason":"end_turn","usage":{"input_tokens":10,"output_tokens":4}}),
            ),
            (
                LlmProvider::Openai,
                json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"{}"}]}],"usage":{"input_tokens":10,"output_tokens":4}}),
            ),
            (
                LlmProvider::Openrouter,
                openrouter_completion(
                    "{}",
                    Value::Null,
                    "stop",
                    json!({"prompt_tokens":10,"completion_tokens":4}),
                ),
            ),
        ] {
            let (url, requests, task) = mock_server(200, body.to_string()).await;
            let backend = backend(provider, &url);
            let schema = json!({"type":"object","properties":{}});
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
                    assert_eq!(requests[0]["text"]["format"]["type"], "json_object");
                    assert_eq!(requests[0]["store"], false);
                }
                LlmProvider::Openrouter => {
                    assert_eq!(requests[0]["response_format"]["type"], "json_object");
                }
            }
            task.abort();
        }
    }

    #[tokio::test]
    async fn both_adapters_stream_canonical_events_and_tool_calls() {
        let anthropic = concat!(
            "event: message_start\ndata: {\"message\":{\"usage\":{\"input_tokens\":5}}}\n\n",
            "event: content_block_start\ndata: {\"content_block\":{\"type\":\"text\"}}\n\n",
            "event: content_block_delta\ndata: {\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n",
            "event: content_block_start\ndata: {\"content_block\":{\"type\":\"tool_use\",\"id\":\"call1\",\"name\":\"search\"}}\n\n",
            "event: content_block_delta\ndata: {\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{}\"}}\n\n",
            "event: content_block_stop\ndata: {}\n\n",
            "event: message_delta\ndata: {\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":3}}\n\n",
            "event: message_stop\ndata: {}\n\n"
        );
        let openai = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":1,\"item\":{\"type\":\"function_call\",\"call_id\":\"call1\",\"name\":\"search\"}}\n\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":1,\"delta\":\"{}\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[{\"type\":\"function_call\",\"call_id\":\"call1\",\"name\":\"search\",\"arguments\":\"{}\"}],\"usage\":{\"input_tokens\":5,\"output_tokens\":3}}}\n\n"
        );
        let openrouter = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"hello\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call1\",\"type\":\"function\",\"function\":{\"name\":\"search\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":3}}\n\n",
            "data: [DONE]\n\n"
        );
        for (provider, body) in [
            (LlmProvider::Anthropic, anthropic),
            (LlmProvider::Openai, openai),
            (LlmProvider::Openrouter, openrouter),
        ] {
            let (url, _, task) = mock_server(200, body.into()).await;
            let events = Mutex::new(Vec::new());
            let sink = |event| events.lock().unwrap().push(event);
            let adapter = backend(provider, &url).adapter().unwrap();
            let response = adapter
                .stream(
                    LlmRequest::new(ExecutionScope::Subagent, &[], &[], &[]),
                    &sink,
                )
                .await
                .unwrap();
            assert_eq!(response.text, "hello");
            assert_eq!(response.stop_reason, StopReason::ToolCalls);
            assert_eq!(response.tool_calls[0].id, "call1");
            assert_eq!(response.usage.output_tokens, 3);
            let events = events.lock().unwrap();
            assert!(
                !events.iter().any(
                    |event| matches!(event, LlmEvent::Activity { name, .. } if name == "search")
                ),
                "local tool activity is emitted by the agent loop only"
            );
            assert!(
                events
                    .iter()
                    .any(|e| matches!(e, LlmEvent::TextDelta(s) if s == "hello"))
            );
            assert!(events.iter().any(|e| matches!(e, LlmEvent::ToolCallStarted { id, name } if id == "call1" && name == "search")));
            assert!(events.iter().any(|e| matches!(e, LlmEvent::ToolArgumentsDelta { id, delta, .. } if id == "call1" && delta == "{}")));
            assert!(
                events
                    .iter()
                    .any(|e| matches!(e, LlmEvent::Usage(u) if u.input_tokens == 5))
            );
            task.abort();
        }
    }

    #[tokio::test]
    async fn adapters_return_typed_http_and_truncated_stream_errors() {
        for provider in PROVIDERS {
            let (url, _, task) = mock_server(429, "rate limited".into()).await;
            let adapter = backend(provider, &url).adapter().unwrap();
            assert!(matches!(
                adapter
                    .complete(LlmRequest::new(ExecutionScope::Subagent, &[], &[], &[]))
                    .await,
                Err(LlmError::RateLimited { .. })
            ));
            assert!(matches!(
                adapter
                    .stream(
                        LlmRequest::new(ExecutionScope::Subagent, &[], &[], &[]),
                        &|_| {}
                    )
                    .await,
                Err(LlmError::RateLimited { .. })
            ));
            task.abort();
            let (url, _, task) = mock_server(418, "teapot".into()).await;
            let adapter = backend(provider, &url).adapter().unwrap();
            assert!(matches!(
                adapter
                    .complete(LlmRequest::new(ExecutionScope::Subagent, &[], &[], &[]))
                    .await,
                Err(LlmError::Http { status: 418, .. })
            ));
            task.abort();
            let (url, _, task) = mock_server(200, "data: {}\n\n".into()).await;
            let adapter = backend(provider, &url).adapter().unwrap();
            assert!(matches!(
                adapter
                    .stream(
                        LlmRequest::new(ExecutionScope::Subagent, &[], &[], &[]),
                        &|_| {}
                    )
                    .await,
                Err(LlmError::InvalidResponse(_))
            ));
            task.abort();
        }
    }

    #[tokio::test]
    async fn cancellation_interrupts_in_flight_http_request() {
        for provider in PROVIDERS {
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
            let adapter = backend(
                provider,
                &format!("http://{}", listener.local_addr().unwrap()),
            )
            .adapter()
            .unwrap();
            let server = tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            let request = LlmRequest::new(ExecutionScope::Subagent, &[], &[], &[]);
            let token = request.cancellation.clone();
            let cancel = async {
                entered.notified().await;
                token.cancel();
            };
            let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                tokio::join!(adapter.stream(request, &|_| {}), cancel)
            })
            .await
            .unwrap();
            assert!(matches!(result, Err(LlmError::Cancelled)));
            server.abort();
        }
    }

    struct MustNotExecute;
    impl crate::services::tool::ToolDyn for MustNotExecute {
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
        fn call<'a>(
            &'a self,
            _: String,
        ) -> BoxFuture<'a, Result<String, crate::services::tool::ToolError>> {
            Box::pin(async { panic!("invalid provider response reached tool execution") })
        }
    }

    /// A tool that answers every call with the same text.
    struct Answers {
        name: &'static str,
        output: &'static str,
    }
    impl crate::services::tool::ToolDyn for Answers {
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
        fn call<'a>(
            &'a self,
            _: String,
        ) -> BoxFuture<'a, Result<String, crate::services::tool::ToolError>> {
            Box::pin(async { Ok(self.output.into()) })
        }
    }

    /// Like `mock_server_with`, but the n-th POST is answered with the n-th
    /// body (the last one repeats), so a multi-turn loop can be driven.
    async fn mock_server_sequence(
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

    /// A screenshot-like tool result: text plus an image, as the computer,
    /// files, image and memory tools return them.
    const SHOT_OUTPUT: &str = r#"[{"type":"text","text":"captured"},{"type":"image","source":{"type":"base64","media_type":"image/png","data":"abc"}}]"#;
    const SHOT_DATA_URL: &str = "data:image/png;base64,abc";

    /// Turn 1: "Looking." plus two calls, `shot` (call1) and `search`
    /// (call2); turn 2: "done". Non-streaming and streaming bodies per
    /// provider.
    fn round_trip_bodies(provider: LlmProvider, streaming: bool) -> Vec<String> {
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
        }
    }

    /// Turn 1 answers with two tool calls, both tools run (the first one
    /// returns an image), and turn 2's request carries the assistant's
    /// calls and every result in the shape the provider requires: Chat
    /// Completions needs the `tool` messages contiguous right after the
    /// assistant's `tool_calls`, Anthropic every `tool_result` in the one
    /// user message that follows, the Responses API a `function_call_output`
    /// per call.
    #[tokio::test]
    async fn tool_call_round_trip_keeps_every_result_next_to_its_call() {
        for provider in PROVIDERS {
            for streaming in [false, true] {
                let label = format!("{provider:?} streaming={streaming}");
                let (url, requests, task) =
                    mock_server_sequence(round_trip_bodies(provider, streaming)).await;
                let tools: Vec<Box<dyn crate::services::tool::ToolDyn>> = vec![
                    Box::new(Answers {
                        name: "shot",
                        output: SHOT_OUTPUT,
                    }),
                    Box::new(Answers {
                        name: "search",
                        output: "found",
                    }),
                ];
                let backend = backend(provider, &url);
                let (text, trace) = if streaming {
                    let workspace = tempfile::tempdir().unwrap();
                    let result = backend
                        .chat_with_tools_streaming(
                            &["system"],
                            Message::user("hi"),
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
                        .chat_with_tools_traced("system", "hi", vec![], tools)
                        .await
                        .unwrap_or_else(|error| panic!("{label}: {error:?}"));
                    (text, trace)
                };
                task.abort();
                assert_eq!(text, "done", "{label}");

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
                            messages[2]["tool_calls"][1]["function"]["arguments"],
                            "{\"q\":\"rust\"}",
                            "{label}"
                        );
                        assert_eq!(messages[3]["tool_call_id"], "call1", "{label}");
                        assert_eq!(messages[3]["content"], "captured", "{label}");
                        assert_eq!(messages[4]["tool_call_id"], "call2", "{label}");
                        assert_eq!(messages[4]["content"], "found", "{label}");
                        assert_eq!(
                            messages[5]["content"][0]["image_url"]["url"], SHOT_DATA_URL,
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
                                && item["content"][0]["image_url"] == SHOT_DATA_URL),
                            "{label}: {second}"
                        );
                    }
                }
            }
        }
    }

    /// Where a provider keeps a tool call's id, name and arguments.
    fn tool_call_keys(provider: LlmProvider) -> (&'static str, &'static str) {
        match provider {
            LlmProvider::Anthropic => ("id", "input"),
            LlmProvider::Openai => ("call_id", "arguments"),
            LlmProvider::Openrouter => ("id", "arguments"),
        }
    }

    /// A well-formed tool call in the provider's own shape.
    fn valid_tool_call(provider: LlmProvider) -> Value {
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
        }
    }

    /// Sets or removes one field of a tool call where that provider keeps it.
    fn corrupt_tool_call(
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

    #[tokio::test]
    async fn invalid_tool_calls_are_rejected_in_both_modes() {
        for provider in PROVIDERS {
            let (id_key, args_key) = tool_call_keys(provider);
            for field in [id_key, "name", args_key] {
                for bad in [
                    None,
                    Some(json!("")),
                    Some(json!(null)),
                    Some(json!(7)),
                    Some(json!("broken{")),
                    Some(json!([])),
                ] {
                    let mut call = valid_tool_call(provider);
                    // Nonempty strings are valid IDs/names.
                    if field != args_key && bad == Some(json!("broken{")) {
                        continue;
                    }
                    corrupt_tool_call(provider, &mut call, field, &bad);
                    let complete = match provider {
                        LlmProvider::Anthropic => {
                            json!({"content":[call.clone()],"stop_reason":"tool_use"})
                        }
                        LlmProvider::Openai => {
                            json!({"status":"completed","output":[call.clone()]})
                        }
                        LlmProvider::Openrouter => openrouter_completion(
                            "",
                            json!([call.clone()]),
                            "tool_calls",
                            json!({"prompt_tokens":1,"completion_tokens":1}),
                        ),
                    };
                    let stream = match provider {
                        LlmProvider::Anthropic => {
                            let arguments = if field == args_key {
                                bad.as_ref()
                                    .map(|v| {
                                        v.as_str()
                                            .map(str::to_owned)
                                            .unwrap_or_else(|| v.to_string())
                                    })
                                    .unwrap_or_default()
                            } else {
                                "{}".into()
                            };
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
                                json!({"type":"response.completed","response":complete})
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
                    };
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
                        let tools: Vec<Box<dyn crate::services::tool::ToolDyn>> =
                            vec![Box::new(MustNotExecute)];
                        let result = if streaming {
                            backend
                                .chat_with_tools_streaming(
                                    &[],
                                    Message::user("test"),
                                    vec![],
                                    tools,
                                    tokio::sync::broadcast::channel(32).0,
                                    "test",
                                    "test",
                                    &std::env::temp_dir(),
                                    None,
                                    Default::default(),
                                )
                                .await
                                .map(|_| ())
                        } else {
                            backend
                                .chat_with_tools_traced("", "test", vec![], tools)
                                .await
                                .map(|_| ())
                        };
                        let error = result.unwrap_err();
                        assert!(matches!(
                            error.downcast_ref::<LlmError>(),
                            Some(LlmError::InvalidResponse(_))
                        ));
                        task.abort();
                    }
                }
            }
        }
    }

    // ── Typed provider errors (#24, #25) ─────────────────────────────────

    fn anthropic_error_body(kind: &str, message: &str) -> String {
        json!({"type": "error", "error": {"type": kind, "message": message}}).to_string()
    }

    fn openai_error_body(code: &str, message: &str) -> String {
        json!({"error": {"type": "invalid_request_error", "code": code, "message": message}})
            .to_string()
    }

    fn openrouter_error_body(code: u16, message: &str) -> String {
        json!({"error": {"code": code, "message": message}}).to_string()
    }

    /// Provider, status, response headers, body, expected variant.
    type ErrorCase = (
        LlmProvider,
        u16,
        Vec<(&'static str, String)>,
        String,
        &'static str,
    );

    /// Every adapter answers the same failures with the same variants, in
    /// both modes; `Http` is only the remainder.
    #[tokio::test]
    async fn adapters_map_provider_errors_to_typed_variants() {
        use LlmProvider::{Anthropic, Openai, Openrouter};
        let retry: Vec<(&'static str, String)> = vec![("retry-after", "7".into())];
        let cases: Vec<ErrorCase> = vec![
            (
                Openrouter,
                401,
                vec![],
                openrouter_error_body(401, "No auth credentials found"),
                "authentication",
            ),
            (
                Openrouter,
                403,
                vec![],
                openrouter_error_body(403, "Key limit exceeded"),
                "authentication",
            ),
            (
                Openrouter,
                429,
                retry.clone(),
                openrouter_error_body(429, "Rate limit exceeded"),
                "rate_limited",
            ),
            (Openrouter, 429, vec![], "plain text".into(), "rate_limited"),
            (
                Openrouter,
                400,
                vec![],
                openrouter_error_body(
                    400,
                    "This endpoint's maximum context length is 8192 tokens. However, you requested about 9000 tokens",
                ),
                "context_length",
            ),
            (
                Openrouter,
                400,
                vec![],
                openrouter_error_body(400, "anthropic/nope is not a valid model ID"),
                "http",
            ),
            (
                Openrouter,
                404,
                vec![],
                openrouter_error_body(404, "No endpoints found for anthropic/nope"),
                "http",
            ),
            (Openrouter, 502, vec![], "bad gateway".into(), "http"),
            (
                Anthropic,
                401,
                vec![],
                anthropic_error_body("authentication_error", "invalid x-api-key"),
                "authentication",
            ),
            (
                Openai,
                401,
                vec![],
                openai_error_body("invalid_api_key", "Incorrect API key provided"),
                "authentication",
            ),
            (
                Anthropic,
                403,
                vec![],
                anthropic_error_body("permission_error", "not allowed"),
                "authentication",
            ),
            (
                Openai,
                403,
                vec![],
                openai_error_body("unsupported_country_region_territory", "no"),
                "authentication",
            ),
            (
                Anthropic,
                429,
                retry.clone(),
                anthropic_error_body("rate_limit_error", "slow down"),
                "rate_limited",
            ),
            (
                Openai,
                429,
                retry.clone(),
                openai_error_body("rate_limit_exceeded", "Rate limit reached"),
                "rate_limited",
            ),
            (
                Anthropic,
                529,
                vec![],
                anthropic_error_body("overloaded_error", "Overloaded"),
                "rate_limited",
            ),
            (Openai, 429, vec![], "plain text".into(), "rate_limited"),
            (
                Anthropic,
                400,
                vec![],
                anthropic_error_body(
                    "invalid_request_error",
                    "prompt is too long: 213462 tokens > 200000 maximum",
                ),
                "context_length",
            ),
            (
                Openai,
                400,
                vec![],
                openai_error_body(
                    "context_length_exceeded",
                    "Your input exceeds the context window of this model.",
                ),
                "context_length",
            ),
            (
                Anthropic,
                400,
                vec![],
                anthropic_error_body("invalid_request_error", "messages: roles must alternate"),
                "http",
            ),
            (
                Openai,
                400,
                vec![],
                openai_error_body("invalid_value", "Unsupported parameter"),
                "http",
            ),
            (Anthropic, 500, vec![], "boom".into(), "http"),
            (Openai, 503, vec![], "down".into(), "http"),
        ];
        for (provider, status, headers, body, expected) in cases {
            let (url, _, task) = mock_server_with(status, headers.clone(), body.clone()).await;
            let adapter = backend(provider, &url).adapter().unwrap();
            let complete = adapter
                .complete(LlmRequest::new(ExecutionScope::Subagent, &[], &[], &[]))
                .await
                .unwrap_err();
            let stream = adapter
                .stream(
                    LlmRequest::new(ExecutionScope::Subagent, &[], &[], &[]),
                    &|_| {},
                )
                .await
                .unwrap_err();
            task.abort();
            for error in [complete, stream] {
                let label = format!("{provider:?} {status} {body}: {error:?}");
                match expected {
                    "authentication" => {
                        assert!(matches!(error, LlmError::Authentication(_)), "{label}")
                    }
                    "rate_limited" => {
                        let LlmError::RateLimited { retry_after, .. } = error else {
                            panic!("{label}");
                        };
                        let expected_wait =
                            (!headers.is_empty()).then_some(std::time::Duration::from_secs(7));
                        assert_eq!(retry_after, expected_wait, "{label}");
                    }
                    "context_length" => {
                        assert!(matches!(error, LlmError::ContextLength(_)), "{label}")
                    }
                    _ => assert!(
                        matches!(error, LlmError::Http { status: got, .. } if got == status),
                        "{label}"
                    ),
                }
            }
        }
    }

    /// Anthropic can send the same error object mid-stream as an SSE
    /// `error` event; it maps like the status it would have been.
    #[tokio::test]
    async fn anthropic_stream_error_events_map_like_status_errors() {
        for (kind, expected) in [
            ("overloaded_error", "rate_limited"),
            ("rate_limit_error", "rate_limited"),
            ("authentication_error", "authentication"),
            ("api_error", "invalid"),
        ] {
            let sse = format!(
                "event: message_start\ndata: {{\"message\":{{\"usage\":{{\"input_tokens\":5}}}}}}\n\nevent: error\ndata: {}\n\n",
                anthropic_error_body(kind, "Overloaded")
            );
            let (url, _, task) = mock_server(200, sse).await;
            let adapter = backend(LlmProvider::Anthropic, &url).adapter().unwrap();
            let error = adapter
                .stream(
                    LlmRequest::new(ExecutionScope::Subagent, &[], &[], &[]),
                    &|_| {},
                )
                .await
                .unwrap_err();
            task.abort();
            let ok = match expected {
                "rate_limited" => matches!(error, LlmError::RateLimited { .. }),
                "authentication" => matches!(error, LlmError::Authentication(_)),
                _ => matches!(error, LlmError::InvalidResponse(_)),
            };
            assert!(ok, "{kind}: {error:?}");
        }
    }

    // ── Token counting behind the adapter (#24) ──────────────────────────

    #[tokio::test]
    async fn token_counting_goes_through_the_adapter_in_its_wire_format() {
        let count = json!({"input_tokens": 42}).to_string();
        let (url, requests, task) = mock_server_with(200, vec![], count.clone()).await;
        let adapter = backend(LlmProvider::Anthropic, &url).adapter().unwrap();
        assert!(adapter.capabilities().token_counting);
        let tools = [ToolDefinition {
            name: "search".into(),
            description: "test".into(),
            parameters: json!({"type":"object","properties":{"q":{"type":"string"}}}),
        }];
        let messages = [Message::user("hello")];
        let counted = adapter
            .count_tokens(LlmRequest::new(
                ExecutionScope::Conversation,
                &["system"],
                &messages,
                &tools,
            ))
            .await
            .unwrap();
        assert_eq!(counted, 42);
        {
            let requests = requests.lock().unwrap();
            let (path, body) = &requests[0];
            assert_eq!(path, "/v1/messages/count_tokens");
            assert_eq!(body["model"], "claude-sonnet-4-6");
            assert_eq!(body["system"][0]["text"], "system");
            assert_eq!(body["messages"][0]["content"][0]["text"], "hello");
            // Tools are written as the Messages API reads them, never with
            // the ToolDefinition field name the old hand-built request sent.
            assert_eq!(body["tools"][0]["name"], "search");
            assert_eq!(
                body["tools"][0]["input_schema"]["properties"]["q"]["type"],
                "string"
            );
            assert!(body["tools"][0].get("parameters").is_none());
            // Generation-only fields stay out of a count.
            for key in [
                "max_tokens",
                "stream",
                "cache_control",
                "context_management",
            ] {
                assert!(body.get(key).is_none(), "count request carries {key}");
            }
        }
        task.abort();

        let (url, requests, task) = mock_server_with(200, vec![], count).await;
        let adapter = backend(LlmProvider::Openai, &url).adapter().unwrap();
        assert!(!adapter.capabilities().token_counting);
        assert!(matches!(
            adapter
                .count_tokens(LlmRequest::new(ExecutionScope::Conversation, &[], &[], &[]))
                .await,
            Err(LlmError::UnsupportedCapability("token counting"))
        ));
        assert!(
            requests.lock().unwrap().is_empty(),
            "an unsupported count never reaches the network"
        );
        task.abort();

        // A failing count is a typed error like any other call.
        let (url, _, task) = mock_server_with(
            401,
            vec![],
            anthropic_error_body("authentication_error", "invalid x-api-key"),
        )
        .await;
        let adapter = backend(LlmProvider::Anthropic, &url).adapter().unwrap();
        assert!(matches!(
            adapter
                .count_tokens(LlmRequest::new(ExecutionScope::Conversation, &[], &[], &[]))
                .await,
            Err(LlmError::Authentication(_))
        ));
        task.abort();
    }

    // ── OpenAI reasoning controls per model (#25) ────────────────────────

    #[tokio::test]
    async fn openai_forwards_reasoning_effort_only_to_reasoning_models() {
        let completed = json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"ok"}]}],"usage":{"input_tokens":1,"output_tokens":1}});
        for (model, supported) in [
            ("gpt-5.4", true),
            ("gpt-5.4-mini", true),
            ("o3", true),
            ("o4-mini", true),
            ("gpt-4.1", false),
            ("gpt-4o", false),
        ] {
            let capabilities = super::super::provider_capabilities(LlmProvider::Openai, model);
            assert_eq!(capabilities.reasoning_controls, supported, "{model}");
            for streaming in [false, true] {
                let body = if streaming {
                    format!(
                        "data: {}\n\n",
                        json!({"type": "response.completed", "response": completed})
                    )
                } else {
                    completed.to_string()
                };
                let (url, requests, task) = mock_server_with(200, vec![], body).await;
                let mut backend = backend(LlmProvider::Openai, &url);
                backend.model = model.into();
                let adapter = backend.adapter().unwrap();
                let mut request = LlmRequest::new(ExecutionScope::Subagent, &[], &[], &[]);
                request.reasoning = Some("low");
                let result = if streaming {
                    adapter.stream(request, &|_| {}).await
                } else {
                    adapter.complete(request).await
                };
                {
                    let requests = requests.lock().unwrap();
                    if supported {
                        result.unwrap();
                        assert_eq!(
                            requests[0].1["reasoning"]["effort"], "low",
                            "{model} streaming={streaming}"
                        );
                    } else {
                        assert!(
                            matches!(
                                result,
                                Err(LlmError::UnsupportedCapability("reasoning controls"))
                            ),
                            "{model} streaming={streaming}: {result:?}"
                        );
                        assert!(requests.is_empty(), "{model}: rejected before the network");
                    }
                }
                // Without a reasoning request nothing is sent, whatever the model.
                let adapter = backend.adapter().unwrap();
                let request = LlmRequest::new(ExecutionScope::Subagent, &[], &[], &[]);
                if streaming {
                    adapter.stream(request, &|_| {}).await.unwrap();
                } else {
                    adapter.complete(request).await.unwrap();
                }
                let requests = requests.lock().unwrap();
                assert!(
                    requests.last().unwrap().1.get("reasoning").is_none(),
                    "{model} streaming={streaming}"
                );
                drop(requests);
                task.abort();
            }
        }
        assert!(
            !super::super::provider_capabilities(LlmProvider::Anthropic, "claude-sonnet-4-6")
                .reasoning_controls
        );
    }

    // ── Key probe shared by both providers (#24, #25; #28 builds on it) ──

    #[tokio::test]
    async fn probe_key_accepts_authenticated_answers_and_rejects_bad_keys() {
        for provider in PROVIDERS {
            let success = match provider {
                LlmProvider::Anthropic => END_TURN.to_string(),
                LlmProvider::Openai => {
                    json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"ok"}]}],"usage":{"input_tokens":1,"output_tokens":1}}).to_string()
                }
                LlmProvider::Openrouter => openrouter_completion(
                    "ok",
                    Value::Null,
                    "stop",
                    json!({"prompt_tokens":1,"completion_tokens":1}),
                )
                .to_string(),
            };
            let cases = [
                (200, success, "ok"),
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
                let result = backend.probe_key().await;
                task.abort();
                let requests = requests.lock().unwrap();
                let body = &requests[0].1;
                assert_eq!(body["model"], "model-x");
                // The smallest completion each API accepts.
                let (limit, smallest) = match provider {
                    LlmProvider::Anthropic => ("max_tokens", 1),
                    LlmProvider::Openai => ("max_output_tokens", 16),
                    LlmProvider::Openrouter => ("max_tokens", 16),
                };
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
                backend.probe_key().await,
                Err(LlmError::Transport(_))
            ));
        }
    }

    /// The connection test (#28) sends the same one-token request as the key
    /// probe, but only a real answer counts: a rate limit or a missing model
    /// comes back as its variant instead of passing as "past authentication".
    #[tokio::test]
    async fn connection_tests_only_accept_a_real_answer() {
        for provider in PROVIDERS {
            let success = match provider {
                LlmProvider::Anthropic => END_TURN.to_string(),
                LlmProvider::Openai => {
                    json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"ok"}]}],"usage":{"input_tokens":10,"output_tokens":4}}).to_string()
                }
                LlmProvider::Openrouter => openrouter_completion(
                    "ok",
                    Value::Null,
                    "stop",
                    json!({"prompt_tokens":10,"completion_tokens":4}),
                )
                .to_string(),
            };
            let (url, requests, task) = mock_server_with(200, vec![], success).await;
            let mut backend = LlmBackend::probe(reqwest::Client::new(), provider, "model-x", "k");
            backend.base_url = url;
            let usage = backend.test_connection().await.unwrap();
            task.abort();
            assert_eq!(
                (usage.input_tokens, usage.output_tokens),
                (10, 4),
                "{provider:?}"
            );
            let requests = requests.lock().unwrap();
            assert_eq!(
                requests.len(),
                1,
                "{provider:?}: one completion, nothing else"
            );
            let body = &requests[0].1;
            assert_eq!(body["model"], "model-x");
            let (limit, smallest) = match provider {
                LlmProvider::Anthropic => ("max_tokens", 1),
                LlmProvider::Openai => ("max_output_tokens", 16),
                LlmProvider::Openrouter => ("max_tokens", 16),
            };
            assert_eq!(
                body[limit], smallest,
                "{provider:?}: a test asks for the least"
            );
            assert!(
                body["tools"].is_null(),
                "{provider:?}: a test carries no tools"
            );
            drop(requests);

            for (status, body, check) in [
                (401, "nope", "authentication"),
                (429, "slow down", "rate_limited"),
                (404, "no such model", "http"),
            ] {
                let (url, _, task) = mock_server_with(status, vec![], body.to_string()).await;
                let mut backend =
                    LlmBackend::probe(reqwest::Client::new(), provider, "model-x", "k");
                backend.base_url = url;
                let result = backend.test_connection().await;
                task.abort();
                let label = format!("{provider:?} {status}: {result:?}");
                match check {
                    "authentication" => assert!(
                        matches!(result, Err(LlmError::Authentication(_))),
                        "{label}"
                    ),
                    "rate_limited" => assert!(
                        matches!(result, Err(LlmError::RateLimited { .. })),
                        "{label}"
                    ),
                    _ => assert!(
                        matches!(result, Err(LlmError::Http { status: 404, .. })),
                        "{label}"
                    ),
                }
            }
        }
    }

    /// The `ttl` of every `cache_control` anywhere in a captured request
    /// body; `None` where a breakpoint carries no ttl.
    fn cache_ttls(value: &Value) -> Vec<Option<String>> {
        let mut out = Vec::new();
        match value {
            Value::Object(map) => {
                for (key, child) in map {
                    if key == "cache_control" {
                        out.push(child["ttl"].as_str().map(str::to_owned));
                    } else {
                        out.extend(cache_ttls(child));
                    }
                }
            }
            Value::Array(items) => out.extend(items.iter().flat_map(cache_ttls)),
            _ => {}
        }
        out
    }

    fn assert_cached_for(requests: &[Value], ttl: &str, path: &str) {
        assert!(!requests.is_empty(), "{path}: no request captured");
        for request in requests {
            let ttls = cache_ttls(request);
            assert!(!ttls.is_empty(), "{path}: request has no cache breakpoints");
            assert!(
                ttls.iter().all(|t| t.as_deref() == Some(ttl)),
                "{path}: every breakpoint must use ttl {ttl:?}, got {ttls:?}"
            );
        }
    }

    const END_TURN: &str = r#"{"content":[{"type":"text","text":"done"}],"stop_reason":"end_turn","usage":{"input_tokens":10,"output_tokens":4}}"#;

    #[tokio::test]
    async fn subagent_paths_request_five_minute_cache_entries() {
        let (url, requests, task) = mock_server(200, END_TURN.into()).await;
        let backend = backend(LlmProvider::Anthropic, &url);
        // Companion routines: one tool so the traced path does not short-circuit to chat().
        let tools: Vec<Box<dyn crate::services::tool::ToolDyn>> = vec![Box::new(MustNotExecute)];
        backend
            .chat_with_tools_traced("system", "prompt", vec![], tools)
            .await
            .unwrap();
        assert_cached_for(&requests.lock().unwrap(), "5m", "chat_with_tools_traced");
        requests.lock().unwrap().clear();
        // Background one-shots.
        backend.chat("system", "prompt", vec![]).await.unwrap();
        assert_cached_for(&requests.lock().unwrap(), "5m", "chat");
        requests.lock().unwrap().clear();
        backend
            .chat_json("system", "prompt", json!({"type":"object"}))
            .await
            .unwrap();
        assert_cached_for(&requests.lock().unwrap(), "5m", "chat_json");
        task.abort();
    }

    #[tokio::test]
    async fn conversation_path_requests_one_hour_cache_entries() {
        let sse = concat!(
            "event: message_start\ndata: {\"message\":{\"usage\":{\"input_tokens\":5}}}\n\n",
            "event: content_block_start\ndata: {\"content_block\":{\"type\":\"text\"}}\n\n",
            "event: content_block_delta\ndata: {\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n",
            "event: content_block_stop\ndata: {}\n\n",
            "event: message_delta\ndata: {\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":3}}\n\n",
            "event: message_stop\ndata: {}\n\n"
        );
        let (url, requests, task) = mock_server(200, sse.into()).await;
        let workspace = tempfile::tempdir().unwrap();
        let tools: Vec<Box<dyn crate::services::tool::ToolDyn>> = vec![Box::new(MustNotExecute)];
        let result = backend(LlmProvider::Anthropic, &url)
            .chat_with_tools_streaming(
                &["system"],
                Message::user("hi"),
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
            .unwrap();
        assert_eq!(result.text, "hello");
        assert_cached_for(&requests.lock().unwrap(), "1h", "chat_with_tools_streaming");
        task.abort();
    }

    #[test]
    fn legacy_history_fixtures_roundtrip_exactly() {
        for fixture in [
            include_str!("fixtures/history-text.json"),
            include_str!("fixtures/history-multimodal.json"),
        ] {
            let original: Value = serde_json::from_str(fixture).unwrap();
            let entries: Vec<HistoryEntry> = serde_json::from_str(fixture).unwrap();
            assert_eq!(serde_json::to_value(&entries).unwrap(), original);
            let messages = HistoryEntry::to_messages(&entries);
            let _ = messages_to_anthropic(&messages);
            let _ = messages_to_openai(&[], &messages);
            assert_eq!(serde_json::to_value(&entries).unwrap(), original);
        }
    }
}
