//! OpenRouter (#26): one key, models from many vendors, spoken as OpenAI
//! Chat Completions at openrouter.ai. The wire format lives here only.

use futures::future::BoxFuture;

use crate::services::tool::ToolDefinition;

use super::contract::{Capabilities, EventSink, LlmError, LlmRequest, ProviderAdapter};
use super::types::{LlmBackend, LlmResponse, Message};

/// What an OpenRouter model offers until the catalog says otherwise.
const DEFAULT_CAPABILITIES: Capabilities = Capabilities {
    vision: true,
    documents: false,
    tools: true,
    streaming: true,
    reasoning_controls: true,
    model_discovery: true,
    token_counting: false,
};

/// Convert our internal Message format to Chat Completions messages.
pub(crate) fn messages_to_openrouter(
    system: &[&str],
    messages: &[Message],
) -> Vec<serde_json::Value> {
    let _ = (system, messages);
    Vec::new()
}

/// Convert tool definitions to Chat Completions `tools`.
pub(crate) fn tools_to_openrouter(tool_defs: &[ToolDefinition]) -> Vec<serde_json::Value> {
    let _ = tool_defs;
    Vec::new()
}

/// What openrouter.ai offers for one model id, from the cached catalog.
pub(super) fn capabilities_for(model: &str) -> Capabilities {
    let _ = model;
    DEFAULT_CAPABILITIES
}

/// The transport implementation is private to this adapter.
pub(super) struct OpenrouterAdapter(pub LlmBackend);
impl ProviderAdapter for OpenrouterAdapter {
    fn capabilities(&self) -> Capabilities {
        capabilities_for(&self.0.model)
    }
    fn complete<'a>(
        &'a self,
        request: LlmRequest<'a>,
    ) -> BoxFuture<'a, Result<LlmResponse, LlmError>> {
        Box::pin(async move {
            request.validate(self.capabilities(), false)?;
            Err(LlmError::InvalidResponse("not implemented".into()))
        })
    }
    fn stream<'a>(
        &'a self,
        request: LlmRequest<'a>,
        events: &'a EventSink<'a>,
    ) -> BoxFuture<'a, Result<LlmResponse, LlmError>> {
        let _ = events;
        Box::pin(async move {
            request.validate(self.capabilities(), true)?;
            Err(LlmError::InvalidResponse("not implemented".into()))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::contract::{ExecutionScope, LlmEvent, StopReason};
    use super::super::types::{ContentBlock, ImageSource, ToolOutputContent};
    use super::*;
    use crate::config::{Config, LlmProvider, OpenrouterConfig};
    use serde_json::{Value, json};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    /// One request the mock saw: method, path, lower-cased headers, JSON body.
    struct Captured {
        method: String,
        path: String,
        headers: HashMap<String, String>,
        body: Value,
    }

    type Requests = Arc<Mutex<Vec<Captured>>>;

    /// An openrouter.ai stand-in: `GET /api/v1/models` answers with
    /// `catalog` (404 without one); everything else answers with `status`,
    /// `headers` and `body`.
    async fn mock(
        catalog: Option<Value>,
        status: u16,
        headers: Vec<(&'static str, String)>,
        body: String,
    ) -> (String, Requests, tokio::task::JoinHandle<()>) {
        let requests: Requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let app = axum::Router::new().fallback(move |request: axum::extract::Request| {
            let captured = captured.clone();
            let catalog = catalog.clone();
            let headers = headers.clone();
            let body = body.clone();
            async move {
                let (parts, raw) = request.into_parts();
                let bytes = axum::body::to_bytes(raw, usize::MAX).await.unwrap();
                captured.lock().unwrap().push(Captured {
                    method: parts.method.to_string(),
                    path: parts.uri.path().to_owned(),
                    headers: parts
                        .headers
                        .iter()
                        .map(|(name, value)| {
                            (
                                name.as_str().to_ascii_lowercase(),
                                value.to_str().unwrap_or("").to_owned(),
                            )
                        })
                        .collect(),
                    body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
                });
                let mut map = axum::http::HeaderMap::new();
                if parts.method == axum::http::Method::GET && parts.uri.path() == "/api/v1/models" {
                    return match catalog {
                        Some(catalog) => (axum::http::StatusCode::OK, map, catalog.to_string()),
                        None => (
                            axum::http::StatusCode::NOT_FOUND,
                            map,
                            "no catalog here".to_owned(),
                        ),
                    };
                }
                for (name, value) in headers {
                    map.insert(name, value.parse().unwrap());
                }
                (axum::http::StatusCode::from_u16(status).unwrap(), map, body)
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (url, requests, task)
    }

    fn backend(url: &str, model: &str) -> LlmBackend {
        let mut backend = LlmBackend::probe(
            reqwest::Client::new(),
            LlmProvider::Openrouter,
            model,
            "test-key",
        );
        backend.base_url = url.to_owned();
        backend
    }

    fn posts(requests: &Requests) -> Vec<Value> {
        requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| request.method == "POST")
            .map(|request| request.body.clone())
            .collect()
    }

    fn paths(requests: &Requests) -> Vec<String> {
        requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| format!("{} {}", request.method, request.path))
            .collect()
    }

    fn tool() -> ToolDefinition {
        ToolDefinition {
            name: "search".into(),
            description: "Search the web".into(),
            parameters: json!({"type":"object","properties":{"q":{"type":"string"}}}),
        }
    }

    fn completion(content: Value, tool_calls: Value, finish: &str, usage: Value) -> Value {
        json!({
            "id": "gen-1",
            "model": "anthropic/claude-sonnet-4.6",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": content, "tool_calls": tool_calls},
                "finish_reason": finish
            }],
            "usage": usage
        })
    }

    const USAGE_WITH_COST: &str = r#"{"prompt_tokens":12,"completion_tokens":5,"total_tokens":17,"cost":0.00042,"prompt_tokens_details":{"cached_tokens":4}}"#;

    /// Text and a tool call arrive as Chat Completions deltas: the call's
    /// id and name first, its arguments in pieces, `finish_reason` on the
    /// last content chunk, then a usage-only chunk and `[DONE]`.
    const STREAM_WITH_TOOL: &str = concat!(
        ": OPENROUTER PROCESSING\n\n",
        r#"data: {"id":"gen-1","choices":[{"index":0,"delta":{"role":"assistant","content":"hel"},"finish_reason":null}]}"#,
        "\n\n",
        r#"data: {"id":"gen-1","choices":[{"index":0,"delta":{"content":"lo"},"finish_reason":null}]}"#,
        "\n\n",
        r#"data: {"id":"gen-1","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call1","type":"function","function":{"name":"search","arguments":""}}]},"finish_reason":null}]}"#,
        "\n\n",
        r#"data: {"id":"gen-1","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"q\":"}}]},"finish_reason":null}]}"#,
        "\n\n",
        r#"data: {"id":"gen-1","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"rust\"}"}}]},"finish_reason":"tool_calls"}]}"#,
        "\n\n",
        r#"data: {"id":"gen-1","choices":[],"usage":{"prompt_tokens":12,"completion_tokens":5,"total_tokens":17,"cost":0.00042,"prompt_tokens_details":{"cached_tokens":4}}}"#,
        "\n\n",
        "data: [DONE]\n\n"
    );

    fn catalog() -> Value {
        json!({"data": [
            {
                "id": "test/full",
                "name": "Full",
                "architecture": {"input_modalities": ["text", "image"], "output_modalities": ["text"]},
                "supported_parameters": ["tools", "tool_choice", "reasoning", "response_format", "max_tokens"]
            },
            {
                "id": "test/text-only",
                "name": "Text only",
                "architecture": {"input_modalities": ["text"], "output_modalities": ["text"]},
                "supported_parameters": ["max_tokens", "temperature"]
            }
        ]})
    }

    #[tokio::test]
    async fn streams_chat_completion_deltas_into_canonical_events() {
        let (url, requests, task) = mock(None, 200, vec![], STREAM_WITH_TOOL.into()).await;
        let adapter = backend(&url, "anthropic/claude-sonnet-4.6")
            .adapter()
            .unwrap();
        let events = Mutex::new(Vec::new());
        let sink = |event| events.lock().unwrap().push(event);
        let messages = [Message::user("hello")];
        let tools = [tool()];
        let response = adapter
            .stream(
                LlmRequest::new(ExecutionScope::Conversation, &["system"], &messages, &tools),
                &sink,
            )
            .await
            .unwrap();
        task.abort();

        assert_eq!(response.text, "hello");
        assert_eq!(response.stop_reason, StopReason::ToolCalls);
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].id, "call1");
        assert_eq!(response.tool_calls[0].name, "search");
        assert_eq!(response.tool_calls[0].arguments["q"], "rust");
        assert_eq!(response.usage.input_tokens, 12);
        assert_eq!(response.usage.output_tokens, 5);
        assert_eq!(response.usage.cache_read_tokens, 4);
        assert_eq!(response.usage.cost, Some(0.00042));
        assert!(response.tokens_used > 0);

        let events = events.lock().unwrap();
        let text: String = events
            .iter()
            .filter_map(|event| match event {
                LlmEvent::TextDelta(delta) => Some(delta.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "hello");
        assert!(events.iter().any(|event| matches!(
            event,
            LlmEvent::ToolCallStarted { id, name } if id == "call1" && name == "search"
        )));
        let arguments: String = events
            .iter()
            .filter_map(|event| match event {
                LlmEvent::ToolArgumentsDelta { id, name, delta }
                    if id == "call1" && name == "search" =>
                {
                    Some(delta.as_str())
                }
                _ => None,
            })
            .collect();
        assert_eq!(arguments, r#"{"q":"rust"}"#);
        assert!(events.iter().any(|event| matches!(
            event,
            LlmEvent::Usage(usage) if usage.input_tokens == 12 && usage.cost == Some(0.00042)
        )));

        let posts = posts(&requests);
        assert_eq!(posts.len(), 1);
        let body = &posts[0];
        assert_eq!(body["model"], "anthropic/claude-sonnet-4.6");
        assert_eq!(body["stream"], true);
        assert_eq!(body["usage"]["include"], true);
        assert_eq!(body["max_tokens"], 16384);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], "system");
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["function"]["name"], "search");
        assert_eq!(
            body["tools"][0]["function"]["parameters"]["properties"]["q"]["type"],
            "string"
        );
        assert!(
            body.get("provider").is_none(),
            "no routing unless configured"
        );
        let request = &requests.lock().unwrap();
        let post = request.iter().find(|r| r.method == "POST").unwrap();
        assert_eq!(post.path, "/api/v1/chat/completions");
        assert_eq!(post.headers["authorization"], "Bearer test-key");
    }

    #[tokio::test]
    async fn completes_and_parses_usage_including_cost() {
        let usage: Value = serde_json::from_str(USAGE_WITH_COST).unwrap();
        let call = json!({"id":"call1","type":"function","function":{"name":"search","arguments":"{\"q\":\"rust\"}"}});
        let body = completion(json!("hello"), json!([call]), "tool_calls", usage.clone());
        let (url, requests, task) = mock(None, 200, vec![], body.to_string()).await;
        let adapter = backend(&url, "openai/gpt-5.4-mini").adapter().unwrap();
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
        assert_eq!(response.usage.input_tokens, 12);
        assert_eq!(response.usage.output_tokens, 5);
        assert_eq!(response.usage.cache_read_tokens, 4);
        assert_eq!(response.usage.cost, Some(0.00042));
        let body = &posts(&requests)[0];
        assert_eq!(body["stream"], false);
        assert_eq!(body["usage"]["include"], true);
        assert_eq!(body["model"], "openai/gpt-5.4-mini");
        task.abort();

        // Without a cost the field is absent; `length` is the output limit;
        // a null content is an empty answer, not an invalid one.
        let body = completion(
            Value::Null,
            Value::Null,
            "length",
            json!({"prompt_tokens": 3, "completion_tokens": 1}),
        );
        let (url, _, task) = mock(None, 200, vec![], body.to_string()).await;
        let adapter = backend(&url, "openai/gpt-5.4-mini").adapter().unwrap();
        let response = adapter
            .complete(LlmRequest::new(ExecutionScope::Subagent, &[], &[], &[]))
            .await
            .unwrap();
        assert_eq!(response.text, "");
        assert_eq!(response.stop_reason, StopReason::OutputLimit);
        assert_eq!(response.usage.cost, None);
        assert!(response.tool_calls.is_empty());
        task.abort();

        // Structured output asks for a JSON object and puts the schema in
        // the system prompt, like the OpenAI adapter.
        let body = completion(
            json!("{}"),
            Value::Null,
            "stop",
            json!({"prompt_tokens": 3, "completion_tokens": 1}),
        );
        let (url, requests, task) = mock(None, 200, vec![], body.to_string()).await;
        let schema = json!({"type":"object","properties":{"color":{"type":"string"}}});
        let (text, tokens) = backend(&url, "openai/gpt-5.4-mini")
            .chat_json("system", "prompt", schema.clone())
            .await
            .unwrap();
        assert_eq!(text, "{}");
        assert_eq!(tokens, 4);
        let body = &posts(&requests)[0];
        assert_eq!(body["response_format"]["type"], "json_object");
        let system = body["messages"][0]["content"].as_str().unwrap();
        assert!(system.contains("color"), "{system}");
        task.abort();
    }

    /// Provider, status, headers, body, expected variant: what OpenRouter
    /// answers with in practice, in both modes.
    #[tokio::test]
    async fn maps_openrouter_errors_to_typed_variants() {
        let error = |code: u16, message: &str| {
            json!({"error": {"code": code, "message": message}}).to_string()
        };
        let cases: Vec<(u16, Vec<(&'static str, String)>, String, &'static str)> = vec![
            (
                401,
                vec![],
                error(401, "No auth credentials found"),
                "authentication",
            ),
            (
                403,
                vec![],
                error(403, "Key limit exceeded"),
                "authentication",
            ),
            (
                429,
                vec![("retry-after", "7".into())],
                error(429, "Rate limit exceeded"),
                "rate_limited",
            ),
            (429, vec![], "slow down".into(), "rate_limited"),
            (
                400,
                vec![],
                error(
                    400,
                    "This endpoint's maximum context length is 8192 tokens. However, you requested about 9000 tokens",
                ),
                "context_length",
            ),
            (
                400,
                vec![],
                error(400, "anthropic/nope is not a valid model ID"),
                "http",
            ),
            (
                404,
                vec![],
                error(404, "No endpoints found for anthropic/nope"),
                "http",
            ),
            (402, vec![], error(402, "Insufficient credits"), "http"),
            (502, vec![], error(502, "Provider returned error"), "http"),
        ];
        for (status, headers, body, expected) in cases {
            let (url, _, task) = mock(None, status, headers.clone(), body.clone()).await;
            let adapter = backend(&url, "anthropic/claude-sonnet-4.6")
                .adapter()
                .unwrap();
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
                let label = format!("{status} {body}: {error:?}");
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
                    _ => {
                        let LlmError::Http {
                            status: got,
                            message,
                        } = error
                        else {
                            panic!("{label}");
                        };
                        assert_eq!(got, status, "{label}");
                        assert!(
                            message.contains("model")
                                || message.contains("credits")
                                || message.contains("error"),
                            "{label}"
                        );
                    }
                }
            }
        }

        // Mid-stream, OpenRouter sends the same error object as an SSE
        // data event; it maps like the status it stands for.
        for (code, expected) in [
            (429, "rate_limited"),
            (401, "authentication"),
            (502, "http"),
        ] {
            let sse = format!(
                "data: {}\n\ndata: {}\n\n",
                json!({"id":"gen-1","choices":[{"index":0,"delta":{"content":"par"},"finish_reason":null}]}),
                json!({"id":"gen-1","error":{"code":code,"message":"upstream"},"choices":[{"index":0,"delta":{},"finish_reason":"error"}]})
            );
            let (url, _, task) = mock(None, 200, vec![], sse).await;
            let adapter = backend(&url, "anthropic/claude-sonnet-4.6")
                .adapter()
                .unwrap();
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
                _ => matches!(error, LlmError::Http { status: 502, .. }),
            };
            assert!(ok, "{code}: {error:?}");
        }
    }

    /// Attribution is opt-in: a default backend sends neither header, and
    /// the instance's `public_url` never stands in for the site URL.
    #[tokio::test]
    async fn attribution_headers_are_sent_only_when_configured() {
        let mut config = Config::default();
        config.public_url = "https://private.example".into();
        config.llm.tokens.open_router = "test-key".into();
        config.llm.seed_presets(LlmProvider::Openrouter);
        let preset = crate::config::default_presets(LlmProvider::Openrouter)[0]
            .id
            .clone();
        let ok = completion(
            json!("ok"),
            Value::Null,
            "stop",
            json!({"prompt_tokens": 1, "completion_tokens": 1}),
        )
        .to_string();

        let (url, requests, task) = mock(None, 200, vec![], ok.clone()).await;
        let mut backend = LlmBackend::for_preset(&config, reqwest::Client::new(), &preset).unwrap();
        backend.base_url = url;
        backend
            .adapter()
            .unwrap()
            .complete(LlmRequest::new(ExecutionScope::Subagent, &[], &[], &[]))
            .await
            .unwrap();
        task.abort();
        for request in requests.lock().unwrap().iter() {
            assert!(
                !request.headers.contains_key("http-referer"),
                "{} {}: HTTP-Referer sent by default",
                request.method,
                request.path
            );
            assert!(
                !request.headers.contains_key("x-title"),
                "{} {}: X-Title sent by default",
                request.method,
                request.path
            );
            for (name, value) in &request.headers {
                assert!(
                    !value.contains("private.example"),
                    "{name} leaks the instance URL: {value}"
                );
            }
        }

        config.llm.openrouter = OpenrouterConfig {
            site_url: "https://nolune.example".into(),
            app_name: "Nolune".into(),
            routing: None,
        };
        let (url, requests, task) = mock(None, 200, vec![], ok).await;
        let mut backend = LlmBackend::for_preset(&config, reqwest::Client::new(), &preset).unwrap();
        backend.base_url = url;
        backend
            .adapter()
            .unwrap()
            .stream(
                LlmRequest::new(ExecutionScope::Subagent, &[], &[], &[]),
                &|_| {},
            )
            .await
            .unwrap_err();
        task.abort();
        let requests = requests.lock().unwrap();
        let post = requests.iter().find(|r| r.method == "POST").unwrap();
        assert_eq!(post.headers["http-referer"], "https://nolune.example");
        assert_eq!(post.headers["x-title"], "Nolune");
    }

    #[tokio::test]
    async fn routing_preferences_ride_along_as_the_provider_object() {
        let ok = completion(
            json!("ok"),
            Value::Null,
            "stop",
            json!({"prompt_tokens": 1, "completion_tokens": 1}),
        )
        .to_string();
        let (url, requests, task) = mock(None, 200, vec![], ok).await;
        let mut backend = backend(&url, "anthropic/claude-sonnet-4.6");
        backend.openrouter.routing = Some(
            toml::from_str("order = ['anthropic', 'google']\nallow_fallbacks = false").unwrap(),
        );
        backend
            .adapter()
            .unwrap()
            .complete(LlmRequest::new(ExecutionScope::Subagent, &[], &[], &[]))
            .await
            .unwrap();
        task.abort();
        let body = &posts(&requests)[0];
        assert_eq!(body["provider"]["order"], json!(["anthropic", "google"]));
        assert_eq!(body["provider"]["allow_fallbacks"], false);
    }

    /// `GET /api/v1/models` says what each model takes; a preset whose
    /// model lacks tools or image input is refused before the network,
    /// and unknown models keep the defaults.
    #[tokio::test]
    async fn discovered_model_capabilities_gate_tools_and_vision() {
        let (url, requests, task) = mock(Some(catalog()), 200, vec![], String::new()).await;
        let adapter = backend(&url, "test/text-only").adapter().unwrap();
        assert!(adapter.capabilities().model_discovery);
        let ids = adapter.discover_models().await.unwrap();
        assert_eq!(ids, ["test/full", "test/text-only"]);
        assert_eq!(paths(&requests), ["GET /api/v1/models"]);

        let text_only = adapter.capabilities();
        assert!(!text_only.tools, "{text_only:?}");
        assert!(!text_only.vision, "{text_only:?}");
        assert!(!text_only.reasoning_controls, "{text_only:?}");
        assert!(text_only.streaming);
        let full = backend(&url, "test/full").adapter().unwrap().capabilities();
        assert!(
            full.tools && full.vision && full.reasoning_controls,
            "{full:?}"
        );
        let unknown = backend(&url, "test/unknown")
            .adapter()
            .unwrap()
            .capabilities();
        assert!(unknown.tools && unknown.vision, "{unknown:?}");

        let tools = [tool()];
        assert!(matches!(
            adapter
                .complete(LlmRequest::new(ExecutionScope::Subagent, &[], &[], &tools))
                .await,
            Err(LlmError::UnsupportedCapability("tools"))
        ));
        let image = [Message::User {
            content: vec![ContentBlock::Image {
                source: ImageSource::Url {
                    url: "https://example.test/a.png".into(),
                },
                resource_provenance: None,
            }],
        }];
        assert!(matches!(
            adapter
                .stream(
                    LlmRequest::new(ExecutionScope::Subagent, &[], &image, &[]),
                    &|_| {}
                )
                .await,
            Err(LlmError::UnsupportedCapability("vision"))
        ));
        let mut reasoning = LlmRequest::new(ExecutionScope::Subagent, &[], &[], &[]);
        reasoning.reasoning = Some("low");
        assert!(matches!(
            adapter.complete(reasoning).await,
            Err(LlmError::UnsupportedCapability("reasoning controls"))
        ));
        assert!(posts(&requests).is_empty(), "rejected before the network");
        task.abort();

        // Without an explicit discovery the first request loads the
        // catalog and is judged by it.
        let (url, requests, task) = mock(Some(catalog()), 200, vec![], String::new()).await;
        let adapter = backend(&url, "test/text-only").adapter().unwrap();
        assert!(matches!(
            adapter
                .complete(LlmRequest::new(ExecutionScope::Subagent, &[], &[], &tools))
                .await,
            Err(LlmError::UnsupportedCapability("tools"))
        ));
        assert_eq!(paths(&requests), ["GET /api/v1/models"]);
        task.abort();

        // An unavailable catalog is not fatal: the request goes out with the
        // defaults, and the failed lookup is not repeated on every turn.
        let ok = completion(
            json!("ok"),
            Value::Null,
            "stop",
            json!({"prompt_tokens": 1, "completion_tokens": 1}),
        )
        .to_string();
        let (url, requests, task) = mock(None, 200, vec![], ok).await;
        let adapter = backend(&url, "test/text-only").adapter().unwrap();
        for _ in 0..2 {
            adapter
                .complete(LlmRequest::new(ExecutionScope::Subagent, &[], &[], &tools))
                .await
                .unwrap();
        }
        assert_eq!(
            paths(&requests),
            [
                "GET /api/v1/models",
                "POST /api/v1/chat/completions",
                "POST /api/v1/chat/completions"
            ]
        );
        assert!(matches!(
            adapter.discover_models().await,
            Err(LlmError::Http { status: 404, .. })
        ));
        task.abort();
    }

    #[test]
    fn messages_convert_tool_calls_results_images_and_summaries() {
        let secret = "issue116-openrouter-secret";
        crate::services::tools::register_control_secret(secret);
        let messages = [
            Message::User {
                content: vec![
                    ContentBlock::text("hi"),
                    ContentBlock::Image {
                        source: ImageSource::Base64 {
                            media_type: "image/png".into(),
                            data: "abc".into(),
                        },
                        resource_provenance: None,
                    },
                ],
            },
            Message::Assistant {
                content: vec![
                    ContentBlock::text("Let me search."),
                    ContentBlock::ToolCall {
                        id: "call1".into(),
                        name: "search".into(),
                        arguments: json!({"q": "rust"}),
                    },
                ],
            },
            Message::User {
                content: vec![ContentBlock::ToolOutput {
                    call_id: "call1".into(),
                    content: ToolOutputContent::Blocks(vec![
                        ContentBlock::text("found"),
                        ContentBlock::Image {
                            source: ImageSource::Url {
                                url: "https://example.test/shot.png".into(),
                            },
                            resource_provenance: None,
                        },
                    ]),
                }],
            },
            Message::Assistant {
                content: vec![ContentBlock::ContextSummary {
                    content: "summary".into(),
                }],
            },
            Message::user(format!("token={secret}")),
        ];
        let wire = messages_to_openrouter(&["system a", "", "system b"], &messages);
        assert_eq!(
            wire[0],
            json!({"role": "system", "content": "system a\n\nsystem b"})
        );
        assert_eq!(wire[1]["role"], "user");
        assert_eq!(wire[1]["content"][0], json!({"type": "text", "text": "hi"}));
        assert_eq!(wire[1]["content"][1]["type"], "image_url");
        assert_eq!(
            wire[1]["content"][1]["image_url"]["url"],
            "data:image/png;base64,abc"
        );
        assert_eq!(wire[2]["role"], "assistant");
        assert_eq!(wire[2]["content"], "Let me search.");
        assert_eq!(
            wire[2]["tool_calls"][0],
            json!({"id": "call1", "type": "function", "function": {"name": "search", "arguments": "{\"q\":\"rust\"}"}})
        );
        assert_eq!(
            wire[3],
            json!({"role": "tool", "tool_call_id": "call1", "content": "found"})
        );
        assert_eq!(wire[4]["role"], "user");
        assert_eq!(
            wire[4]["content"][0]["image_url"]["url"],
            "https://example.test/shot.png"
        );
        assert_eq!(wire[5]["role"], "assistant");
        assert!(
            wire[5]["content"].as_str().unwrap().contains("summary"),
            "{}",
            wire[5]
        );
        assert!(!serde_json::to_string(&wire).unwrap().contains(secret));

        let tools = tools_to_openrouter(&[tool()]);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["function"]["name"], "search");
        assert_eq!(tools[0]["function"]["description"], "Search the web");
        assert_eq!(
            tools[0]["function"]["parameters"]["properties"]["q"]["type"],
            "string"
        );
        assert!(messages_to_openrouter(&[], &[]).is_empty());
    }
}
