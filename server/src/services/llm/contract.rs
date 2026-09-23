//! Provider boundary. Conversation storage and the agent loop do not own API payloads.
use std::path::Path;
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

/// The conversation a request continues, for a provider that keeps a
/// thread of its own per chat (Codex, #27): where the chat's `meta.json`
/// lives, so the thread id can be read back after a restart. A request
/// without one is a one-shot run whose thread outlives nothing.
#[derive(Clone, Copy, Debug)]
pub struct ConversationRef<'a> {
    pub instance_slug: &'a str,
    pub chat_id: &'a str,
    pub workspace_dir: &'a Path,
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
    /// The conversation this request continues, when it is one; the HTTP
    /// adapters send the whole history each turn and never read it.
    pub conversation: Option<ConversationRef<'a>>,
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
            conversation: None,
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
        PROBE_TIMEOUT,
        anthropic::messages_to_anthropic,
        conformance::{
            Case, MustNotExecute, backend, mock_server, mock_server_with, run_case,
            wire::{END_TURN, anthropic_error_body},
        },
        openai::messages_to_openai,
        types::{DocumentSource, HistoryEntry, ImageSource},
    };
    use super::*;
    use crate::config::LlmProvider;
    use serde_json::{Value, json};

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

    #[test]
    fn documents_convert_to_openai_input_files() {
        let inline = ContentBlock::Document {
            source: DocumentSource::Base64 {
                media_type: "application/pdf".into(),
                data: "JVBERi0=".into(),
            },
            resource_provenance: None,
        };
        let linked = ContentBlock::Document {
            source: DocumentSource::Url {
                url: "https://example.test/doc.pdf".into(),
            },
            resource_provenance: None,
        };
        let messages = [Message::User {
            content: vec![
                inline.clone(),
                linked,
                ContentBlock::ToolOutput {
                    call_id: "call1".into(),
                    content: super::super::types::ToolOutputContent::Blocks(vec![
                        ContentBlock::text("read report.pdf"),
                        inline,
                    ]),
                },
            ],
        }];
        let (_, input) = messages_to_openai(&[], &messages);
        assert_eq!(
            input[0]["content"][0],
            json!({
                "type": "input_file",
                "filename": "document.pdf",
                "file_data": "data:application/pdf;base64,JVBERi0=",
            })
        );
        assert_eq!(
            input[1]["content"][0],
            json!({"type": "input_file", "file_url": "https://example.test/doc.pdf"})
        );
        assert_eq!(input[2]["output"], "read report.pdf");
        assert_eq!(input[3]["content"][0]["type"], "input_file");
        assert_eq!(input.len(), 4);
    }

    // ── The conformance matrix (#29) ─────────────────────────────────────
    //
    // Each test below is one case of `conformance::Case`, run for every
    // provider the case applies to; the bodies live in `conformance.rs`
    // beside the wire shapes they need.

    #[tokio::test]
    async fn capabilities_and_cancellation_reject_before_network() {
        let adapter = backend(LlmProvider::Openrouter, "http://127.0.0.1:1")
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
        let plain = plain.adapter().unwrap();
        assert!(matches!(
            plain.complete(request).await,
            Err(LlmError::UnsupportedCapability("reasoning controls"))
        ));
        assert!(matches!(
            plain.discover_models().await,
            Err(LlmError::UnsupportedCapability("model discovery"))
        ));
        run_case(Case::CancellationBeforeNetwork).await;
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

    /// The same completion through every adapter: the HTTP ones from a
    /// mocked answer, Codex from the fake app-server's "hello" turn.
    #[tokio::test]
    async fn both_adapters_complete_with_tools_and_usage_through_same_contract() {
        run_case(Case::CompletesWithToolsAndUsage).await;
    }

    #[tokio::test]
    async fn structured_output_stays_in_adapters() {
        run_case(Case::StructuredOutput).await;
    }

    #[tokio::test]
    async fn both_adapters_stream_canonical_events_and_tool_calls() {
        run_case(Case::StreamsCanonicalEvents).await;
    }

    /// A truncated stream, a broken event, a body that is not JSON, a
    /// turn that ends out of protocol; chatter on the Codex wire is
    /// ignored.
    #[tokio::test]
    async fn adapters_return_typed_http_and_truncated_stream_errors() {
        run_case(Case::MalformedEvents).await;
    }

    #[tokio::test]
    async fn cancellation_interrupts_in_flight_http_request() {
        run_case(Case::CancellationInterruptsInFlight).await;
    }

    #[tokio::test]
    async fn tool_call_round_trip_keeps_every_result_next_to_its_call() {
        run_case(Case::ToolCallRoundTrip).await;
    }

    #[tokio::test]
    async fn invalid_tool_calls_are_rejected_in_both_modes() {
        run_case(Case::InvalidToolCallsRejected).await;
    }

    // ── Typed provider errors (#24, #25) ─────────────────────────────────

    #[tokio::test]
    async fn authentication_errors_are_typed_for_every_provider() {
        run_case(Case::AuthenticationErrors).await;
    }

    #[tokio::test]
    async fn rate_limits_carry_the_providers_retry_after_for_every_provider() {
        run_case(Case::RateLimits).await;
    }

    #[tokio::test]
    async fn context_overflow_is_typed_for_every_provider() {
        run_case(Case::ContextOverflow).await;
    }

    /// Every adapter answers the same failures with the same variants, in
    /// both modes; `Http` is only the remainder.
    #[tokio::test]
    async fn adapters_map_provider_errors_to_typed_variants() {
        run_case(Case::RemainingErrorsStayTyped).await;
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
            ("gpt-6-sol", true),
            ("gpt-6-luna", true),
            ("gpt-5.6-sol", true),
            ("gpt-5.6-luna", true),
            ("gpt-5", true),
            ("gpt-10", true),
            ("o3", true),
            ("o4-mini", true),
            ("gpt-4.1", false),
            ("gpt-4o", false),
            ("gpt-", false),
            ("chatgpt-4o-latest", false),
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
        run_case(Case::KeyProbe).await;
    }

    /// The connection test (#28) sends the same one-token request as the key
    /// probe, but only a real answer counts: a rate limit or a missing model
    /// comes back as its variant instead of passing as "past authentication".
    #[tokio::test]
    async fn connection_tests_only_accept_a_real_answer() {
        run_case(Case::ConnectionTest).await;
    }

    /// A provider that accepts the connection and never answers ends both
    /// probes at the deadline as `Timeout` (#28).
    #[tokio::test]
    async fn probes_give_up_on_a_provider_that_never_answers() {
        run_case(Case::ProbeDeadline).await;
        assert!(
            PROBE_TIMEOUT >= Duration::from_secs(10),
            "a live probe must outwait a slow first token"
        );
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
