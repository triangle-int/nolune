//! OpenRouter (#26): one key, models from many vendors, spoken as OpenAI
//! Chat Completions at openrouter.ai. The wire format lives here only.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use futures::StreamExt;
use futures::future::BoxFuture;

use crate::services::tool::ToolDefinition;

use super::contract::{
    Capabilities, EventSink, LlmError, LlmEvent, LlmRequest, ProviderAdapter, StopReason, Usage,
    retry_after,
};
use super::types::{
    ContentBlock, ImageSource, LlmBackend, LlmResponse, Message, OPENROUTER_BASE_URL, ToolCall,
    ToolOutputContent,
};

const CHAT_COMPLETIONS: &str = "/api/v1/chat/completions";
const MODELS: &str = "/api/v1/models";
const STREAM_TIMEOUT: Duration = Duration::from_secs(480);

/// What an OpenRouter model offers until the catalog says otherwise:
/// the request goes out, and the vendor answers what it cannot do.
const DEFAULT_CAPABILITIES: Capabilities = Capabilities {
    vision: true,
    documents: false,
    tools: true,
    streaming: true,
    reasoning_controls: true,
    model_discovery: true,
    token_counting: false,
};

// ═══════════════════════════════════════════════════════════════════════════
// Wire conversion
// ═══════════════════════════════════════════════════════════════════════════

/// A canonical image as a Chat Completions `image_url` part.
fn image_part(source: &ImageSource) -> serde_json::Value {
    let url = match source {
        ImageSource::Base64 { media_type, data } => format!("data:{media_type};base64,{data}"),
        ImageSource::Url { url } => url.clone(),
    };
    serde_json::json!({"type": "image_url", "image_url": {"url": url}})
}

fn tool_output_text(content: &ToolOutputContent) -> String {
    match content {
        ToolOutputContent::Text(text) => text.clone(),
        ToolOutputContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        ToolOutputContent::Legacy(value) => value.to_string(),
    }
}

/// Convert our internal Message format to Chat Completions messages: the
/// system prompt first, a user turn's tool results as `tool` messages, and
/// its text and images (with the images the tool results carried, since a
/// tool message holds text only) as one user message after them; an
/// assistant turn's text and `tool_calls` together.
///
/// The `tool` messages of a turn stay contiguous: Chat Completions rejects
/// a request where anything but a `tool` message follows the assistant's
/// `tool_calls` before every call is answered, and the agent loop answers
/// all of a turn's calls in one user message.
pub(crate) fn messages_to_openrouter(
    system: &[&str],
    messages: &[Message],
) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    let instructions = system
        .iter()
        .filter(|block| !block.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join("\n\n");
    if !instructions.is_empty() {
        out.push(serde_json::json!({"role": "system", "content": instructions}));
    }
    for message in messages {
        match message {
            Message::User { content } => {
                let mut parts: Vec<serde_json::Value> = Vec::new();
                for block in content {
                    match block {
                        ContentBlock::Text { text } => {
                            parts.push(serde_json::json!({"type": "text", "text": text}));
                        }
                        ContentBlock::Image { source, .. } => parts.push(image_part(source)),
                        ContentBlock::ToolOutput { call_id, content } => {
                            out.push(serde_json::json!({
                                "role": "tool",
                                "tool_call_id": call_id,
                                "content": tool_output_text(content),
                            }));
                            if let ToolOutputContent::Blocks(blocks) = content {
                                for block in blocks {
                                    if let ContentBlock::Image { source, .. } = block {
                                        parts.push(image_part(source));
                                    }
                                }
                            }
                        }
                        ContentBlock::ContextSummary { content }
                        | ContentBlock::LegacyContextSummary {
                            summary: content, ..
                        } => parts.push(serde_json::json!({
                            "type": "text",
                            "text": format!("Conversation summary:\n{content}"),
                        })),
                        _ => {}
                    }
                }
                if !parts.is_empty() {
                    out.push(serde_json::json!({"role": "user", "content": parts}));
                }
            }
            Message::Assistant { content } => {
                let mut text = String::new();
                let mut tool_calls = Vec::new();
                for block in content {
                    match block {
                        ContentBlock::Text { text: t } => text.push_str(t),
                        ContentBlock::ToolCall {
                            id,
                            name,
                            arguments,
                        } => tool_calls.push(serde_json::json!({
                            "id": id,
                            "type": "function",
                            "function": {
                                "name": name,
                                "arguments": serde_json::to_string(arguments).unwrap_or_default(),
                            },
                        })),
                        ContentBlock::ContextSummary { content }
                        | ContentBlock::LegacyContextSummary {
                            summary: content, ..
                        } => text.push_str(&format!("\nConversation summary:\n{content}")),
                        _ => {}
                    }
                }
                if text.is_empty() && tool_calls.is_empty() {
                    continue;
                }
                let mut message = serde_json::json!({
                    "role": "assistant",
                    "content": if text.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(text) },
                });
                if !tool_calls.is_empty() {
                    message["tool_calls"] = serde_json::Value::Array(tool_calls);
                }
                out.push(message);
            }
        }
    }
    out.into_iter()
        .map(crate::services::tools::redact_value)
        .collect()
}

/// Convert tool definitions to Chat Completions `tools`.
pub(crate) fn tools_to_openrouter(tool_defs: &[ToolDefinition]) -> Vec<serde_json::Value> {
    tool_defs
        .iter()
        .map(|tool| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.parameters,
                },
            })
        })
        .collect()
}

/// The request body for one turn. Usage accounting is always asked for so
/// the answer carries the tokens and the cost; structured output asks for
/// a JSON object and puts the schema in the system prompt, like the OpenAI
/// adapter; routing preferences ride along as the `provider` object.
fn request_body(backend: &LlmBackend, request: &LlmRequest<'_>, stream: bool) -> serde_json::Value {
    let mut messages = messages_to_openrouter(request.system, request.messages);
    if let Some(schema) = request.json_schema {
        let instructions = crate::services::tools::redact_secrets(&format!(
            "{}\n\nRespond with ONLY valid JSON matching this schema:\n{}",
            request.system.join("\n\n"),
            schema
        ));
        let system = serde_json::json!({"role": "system", "content": instructions});
        if messages
            .first()
            .is_some_and(|first| first["role"] == "system")
        {
            messages[0] = system;
        } else {
            messages.insert(0, system);
        }
    }
    let mut body = serde_json::json!({
        "model": backend.model,
        "messages": messages,
        "max_tokens": request.max_tokens,
        "stream": stream,
        "usage": {"include": true},
    });
    let tools = tools_to_openrouter(request.tools);
    if !tools.is_empty() {
        body["tools"] = serde_json::Value::Array(tools);
    }
    if let Some(effort) = request.reasoning {
        body["reasoning"] = serde_json::json!({"effort": effort});
    }
    if let Some(routing) = backend.openrouter.routing_json() {
        body["provider"] = routing;
    }
    if request.json_schema.is_some() {
        body["response_format"] = serde_json::json!({"type": "json_object"});
    }
    body
}

// ═══════════════════════════════════════════════════════════════════════════
// Transport
// ═══════════════════════════════════════════════════════════════════════════

/// The key and, only when `[llm.openrouter]` names them, the attribution
/// headers openrouter.ai lists apps by. Nothing else about this server
/// goes out.
fn with_headers(backend: &LlmBackend, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    let mut request = request
        .header("Authorization", format!("Bearer {}", backend.api_key))
        .header("Content-Type", "application/json");
    if let Some(site) = backend.openrouter.site_url() {
        request = request.header("HTTP-Referer", site);
    }
    if let Some(name) = backend.openrouter.app_name() {
        request = request.header("X-Title", name);
    }
    request
}

fn transport(error: reqwest::Error) -> LlmError {
    LlmError::Transport(crate::services::tools::redact_secrets(&error.to_string()))
}

/// The typed error for a non-2xx answer, or for the same error object sent
/// mid-stream. Callers act on the variant; the redacted body rides along
/// for logs. An unknown model id stays `Http`: it is a preset problem the
/// status carries, not a key or a quota one.
fn openrouter_error(status: u16, retry_after: Option<Duration>, body: &str) -> LlmError {
    let message = crate::services::tools::redact_secrets(body);
    let parsed: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    let detail = parsed["error"]["message"]
        .as_str()
        .unwrap_or("")
        .to_ascii_lowercase();
    match status {
        401 | 403 => LlmError::Authentication(message),
        429 => LlmError::RateLimited {
            retry_after,
            message,
        },
        400 if detail.contains("context length")
            || detail.contains("context window")
            || detail.contains("maximum context")
            || detail.contains("too many tokens") =>
        {
            LlmError::ContextLength(message)
        }
        _ => LlmError::Http { status, message },
    }
}

/// An `error` object inside a 200 answer or an SSE event maps like the
/// status it names.
fn embedded_error(event: &serde_json::Value) -> LlmError {
    let code = event["error"]["code"]
        .as_u64()
        .or_else(|| event["error"]["code"].as_str()?.parse().ok())
        .and_then(|code| u16::try_from(code).ok());
    match code {
        Some(status) => openrouter_error(status, None, &event.to_string()),
        None => {
            LlmError::InvalidResponse(crate::services::tools::redact_secrets(&event.to_string()))
        }
    }
}

/// Posts one chat request; a non-2xx answer comes back typed.
async fn post_chat(
    backend: &LlmBackend,
    body: &serde_json::Value,
) -> Result<reqwest::Response, LlmError> {
    let url = format!("{}{CHAT_COMPLETIONS}", backend.base_url);
    let response = with_headers(backend, backend.http.post(&url))
        .json(&crate::services::tools::redact_value(body.clone()))
        .send()
        .await
        .map_err(transport)?;
    let status = response.status();
    if !status.is_success() {
        let retry_after = retry_after(response.headers());
        let text = response.text().await.unwrap_or_default();
        return Err(openrouter_error(status.as_u16(), retry_after, &text));
    }
    Ok(response)
}

fn stop_reason(finish_reason: &str) -> Result<StopReason, LlmError> {
    Ok(match finish_reason {
        "stop" => StopReason::Complete,
        "tool_calls" | "function_call" => StopReason::ToolCalls,
        "length" => StopReason::OutputLimit,
        "error" => {
            return Err(LlmError::InvalidResponse(
                "provider finished the answer with an error".into(),
            ));
        }
        _ => StopReason::Incomplete,
    })
}

/// Chat Completions usage, with the cost openrouter.ai adds when asked.
fn parse_usage(raw: &serde_json::Value) -> Usage {
    let input_tokens = raw["prompt_tokens"].as_u64().unwrap_or(0);
    let details = &raw["prompt_tokens_details"];
    Usage {
        input_tokens,
        output_tokens: raw["completion_tokens"].as_u64().unwrap_or(0),
        cache_read_tokens: details["cached_tokens"].as_u64().unwrap_or(0),
        cache_write_tokens: details["cache_write_tokens"].as_u64().unwrap_or(0),
        cost: raw["cost"].as_f64(),
    }
}

/// Output-equivalent tokens by the same ratios the OpenAI adapter uses;
/// vendors behind OpenRouter price differently, and the cost field is the
/// exact figure.
fn normalized_tokens(usage: &Usage, structured: bool) -> u64 {
    if structured {
        return usage.input_tokens + usage.output_tokens;
    }
    let uncached = usage.input_tokens.saturating_sub(usage.cache_read_tokens);
    (usage.output_tokens as f64 + uncached as f64 * 0.2 + usage.cache_read_tokens as f64 * 0.1)
        as u64
}

/// Message content as text: a string, or the text of an array of parts.
fn content_text(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

fn tool_call_from_message(call: &serde_json::Value) -> Result<ToolCall, LlmError> {
    let function = &call["function"];
    Ok(ToolCall {
        id: ToolCall::required_string(call, "id")?,
        name: ToolCall::required_string(function, "name")?,
        arguments: ToolCall::parse_arguments(&ToolCall::required_string(function, "arguments")?)?,
    })
}

/// Non-streaming Chat Completions call.
pub(crate) async fn openrouter_complete(
    backend: &LlmBackend,
    request: &LlmRequest<'_>,
) -> Result<LlmResponse, LlmError> {
    let body = request_body(backend, request, false);
    let response = post_chat(backend, &body).await?;
    let text = response.text().await.map_err(transport)?;
    let answer: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| LlmError::InvalidResponse(format!("malformed completion: {error}")))?;
    if answer.get("error").is_some_and(|error| !error.is_null()) {
        return Err(embedded_error(&answer));
    }
    let choice = answer["choices"]
        .get(0)
        .ok_or_else(|| LlmError::InvalidResponse("completion without choices".into()))?;
    let message = &choice["message"];
    let text = content_text(&message["content"]);
    let tool_calls: Vec<ToolCall> = message["tool_calls"]
        .as_array()
        .map(|calls| calls.iter().map(tool_call_from_message).collect())
        .transpose()?
        .unwrap_or_default();
    let mut stop_reason = choice["finish_reason"]
        .as_str()
        .map(stop_reason)
        .transpose()?
        .unwrap_or(StopReason::Complete);
    if !tool_calls.is_empty() && stop_reason == StopReason::Complete {
        stop_reason = StopReason::ToolCalls;
    }
    let usage = parse_usage(&answer["usage"]);
    log::debug!(
        "openrouter usage: input={} (cached={}) output={} cost={:?}",
        usage.input_tokens,
        usage.cache_read_tokens,
        usage.output_tokens,
        usage.cost
    );
    Ok(LlmResponse {
        ordered_content: vec![ContentBlock::text(&text)],
        text,
        tool_calls,
        stop_reason,
        tokens_used: normalized_tokens(&usage, request.json_schema.is_some()),
        usage,
    })
}

/// A tool call assembled from stream deltas: the first delta at an index
/// carries the id and name, later ones append arguments.
struct StreamedCall {
    id: String,
    name: String,
    arguments: String,
}

/// The calls of one streamed answer, in the order they started. A piece
/// names its call by `index`; a piece at a known index whose id differs
/// from the call there starts a new call (some vendors number every call
/// 0), and the pieces that follow at that index continue the newest one.
/// A piece without an index starts a new call when it carries an id and
/// continues the last call otherwise.
#[derive(Default)]
struct StreamedCalls {
    calls: Vec<StreamedCall>,
    /// Wire index → position in `calls` of the call it currently names.
    slots: HashMap<usize, usize>,
}

impl StreamedCalls {
    /// The position of the call `piece` belongs to, and whether it starts one.
    fn place(&mut self, piece: &serde_json::Value) -> (usize, bool) {
        let has_id = piece.get("id").is_some_and(|id| !id.is_null());
        let Some(index) = piece["index"].as_u64().map(|index| index as usize) else {
            return if has_id {
                (self.calls.len(), true)
            } else {
                (self.calls.len().saturating_sub(1), false)
            };
        };
        match self.slots.get(&index) {
            Some(&position) if !has_id || piece["id"] == self.calls[position].id.as_str() => {
                (position, false)
            }
            _ => {
                self.slots.insert(index, self.calls.len());
                (self.calls.len(), true)
            }
        }
    }
}

/// Streaming Chat Completions call: `data:` events with `choices[0].delta`
/// text and `tool_calls` pieces, `finish_reason` on the last content event,
/// a usage-only event when asked, `[DONE]` at the end. `: OPENROUTER
/// PROCESSING` comments keep the connection warm and carry nothing.
pub(crate) async fn openrouter_stream(
    backend: &LlmBackend,
    request: &LlmRequest<'_>,
    events: &EventSink<'_>,
) -> Result<LlmResponse, LlmError> {
    let body = request_body(backend, request, true);
    let response = post_chat(backend, &body).await?;

    let mut text = String::new();
    let mut calls = StreamedCalls::default();
    let mut finish: Option<StopReason> = None;
    let mut usage = Usage::default();
    let mut done = false;

    let mut stream = response.bytes_stream();
    let mut buf = Vec::new();
    'events: loop {
        let chunk = match tokio::time::timeout(STREAM_TIMEOUT, stream.next()).await {
            Ok(Some(Ok(chunk))) => chunk,
            Ok(Some(Err(error))) => return Err(transport(error)),
            Ok(None) => break,
            Err(_) => return Err(LlmError::Timeout),
        };
        buf.extend_from_slice(&chunk);

        while let Some(newline) = buf.iter().position(|&byte| byte == b'\n') {
            let line = String::from_utf8_lossy(&buf[..newline]).trim().to_owned();
            buf.drain(..=newline);
            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data == "[DONE]" {
                done = true;
                break 'events;
            }
            let event: serde_json::Value = serde_json::from_str(data).map_err(|error| {
                LlmError::InvalidResponse(format!("malformed stream event: {error}"))
            })?;
            if event.get("error").is_some_and(|error| !error.is_null()) {
                return Err(embedded_error(&event));
            }
            if let Some(choice) = event["choices"].get(0) {
                if choice.get("error").is_some_and(|error| !error.is_null()) {
                    return Err(embedded_error(
                        &serde_json::json!({"error": choice["error"]}),
                    ));
                }
                let delta = &choice["delta"];
                if let Some(content) = delta["content"].as_str().filter(|c| !c.is_empty()) {
                    text.push_str(content);
                    events(LlmEvent::TextDelta(content.to_owned()));
                }
                if let Some(pieces) = delta["tool_calls"].as_array() {
                    for piece in pieces {
                        let function = &piece["function"];
                        let (position, starts_call) = calls.place(piece);
                        if starts_call {
                            let id = ToolCall::required_string(piece, "id")?;
                            let name = ToolCall::required_string(function, "name")?;
                            events(LlmEvent::ToolCallStarted {
                                id: id.clone(),
                                name: name.clone(),
                            });
                            calls.calls.push(StreamedCall {
                                id,
                                name,
                                arguments: String::new(),
                            });
                        }
                        if let Some(arguments) =
                            function.get("arguments").filter(|value| !value.is_null())
                        {
                            let arguments = arguments.as_str().ok_or_else(|| {
                                LlmError::InvalidResponse(
                                    "tool arguments delta must be a string".into(),
                                )
                            })?;
                            if !arguments.is_empty() {
                                let call = calls.calls.get_mut(position).ok_or_else(|| {
                                    LlmError::InvalidResponse(
                                        "arguments without tool metadata".into(),
                                    )
                                })?;
                                call.arguments.push_str(arguments);
                                events(LlmEvent::ToolArgumentsDelta {
                                    id: call.id.clone(),
                                    name: call.name.clone(),
                                    delta: arguments.to_owned(),
                                });
                            }
                        }
                    }
                }
                if let Some(reason) = choice["finish_reason"].as_str() {
                    finish = Some(stop_reason(reason)?);
                }
            }
            if let Some(raw) = event.get("usage").filter(|raw| raw.is_object()) {
                usage = parse_usage(raw);
            }
        }
    }

    if !done && finish.is_none() {
        return Err(LlmError::InvalidResponse(
            "stream ended before completion".into(),
        ));
    }

    let mut tool_calls = Vec::with_capacity(calls.calls.len());
    for call in calls.calls {
        tool_calls.push(ToolCall {
            arguments: ToolCall::parse_arguments(&call.arguments)?,
            id: call.id,
            name: call.name,
        });
    }
    let mut stop_reason = finish.unwrap_or(StopReason::Complete);
    if !tool_calls.is_empty() && stop_reason == StopReason::Complete {
        stop_reason = StopReason::ToolCalls;
    }
    if tool_calls.is_empty() && stop_reason == StopReason::ToolCalls {
        return Err(LlmError::InvalidResponse(
            "tool_calls finish without a tool call".into(),
        ));
    }
    log::debug!(
        "openrouter usage: input={} (cached={}) output={} cost={:?}",
        usage.input_tokens,
        usage.cache_read_tokens,
        usage.output_tokens,
        usage.cost
    );
    events(LlmEvent::Usage(usage));
    let ordered_content = if text.is_empty() {
        Vec::new()
    } else {
        vec![ContentBlock::text(&text)]
    };
    Ok(LlmResponse {
        text,
        tool_calls,
        stop_reason,
        tokens_used: normalized_tokens(&usage, false),
        ordered_content,
        usage,
    })
}

// ═══════════════════════════════════════════════════════════════════════════
// Model catalog: what each model takes, from `GET /api/v1/models`
// ═══════════════════════════════════════════════════════════════════════════

/// What the catalog says one model accepts. A model the catalog does not
/// describe keeps the defaults.
#[derive(Clone, Copy, Debug)]
struct ModelInfo {
    tools: bool,
    vision: bool,
    reasoning: bool,
}

type Catalog = Arc<HashMap<String, ModelInfo>>;

/// One catalog per base URL, or the moment a fetch failed so the next turn
/// is not delayed by the same failure.
struct CatalogEntry {
    fetched: Instant,
    models: Option<Catalog>,
}

const CATALOG_TTL: Duration = Duration::from_secs(60 * 60);
const CATALOG_RETRY: Duration = Duration::from_secs(60);

fn catalogs() -> &'static Mutex<HashMap<String, CatalogEntry>> {
    static CATALOGS: OnceLock<Mutex<HashMap<String, CatalogEntry>>> = OnceLock::new();
    CATALOGS.get_or_init(Default::default)
}

fn cached_catalog(base_url: &str) -> Option<Catalog> {
    catalogs()
        .lock()
        .unwrap()
        .get(base_url)
        .and_then(|entry| entry.models.clone())
}

fn catalog_is_fresh(base_url: &str) -> bool {
    catalogs()
        .lock()
        .unwrap()
        .get(base_url)
        .is_some_and(|entry| {
            let ttl = if entry.models.is_some() {
                CATALOG_TTL
            } else {
                CATALOG_RETRY
            };
            entry.fetched.elapsed() < ttl
        })
}

#[cfg(test)]
fn forget_catalog(base_url: &str) {
    catalogs().lock().unwrap().remove(base_url);
}

fn parse_catalog(json: &serde_json::Value) -> Result<HashMap<String, ModelInfo>, LlmError> {
    let data = json["data"]
        .as_array()
        .ok_or_else(|| LlmError::InvalidResponse("model catalog without data".into()))?;
    Ok(data
        .iter()
        .filter_map(|model| {
            let id = model["id"].as_str()?;
            let parameters = model["supported_parameters"].as_array();
            let supports =
                |name: &str| parameters.is_none_or(|list| list.iter().any(|p| p == name));
            let modalities = model["architecture"]["input_modalities"].as_array();
            Some((
                id.to_owned(),
                ModelInfo {
                    tools: supports("tools"),
                    vision: modalities.is_none_or(|list| list.iter().any(|m| m == "image")),
                    reasoning: supports("reasoning"),
                },
            ))
        })
        .collect())
}

/// `GET /api/v1/models`, cached per base URL.
async fn fetch_catalog(backend: &LlmBackend) -> Result<Catalog, LlmError> {
    let url = format!("{}{MODELS}", backend.base_url);
    let response = with_headers(backend, backend.http.get(&url))
        .send()
        .await
        .map_err(transport)?;
    let status = response.status();
    let retry_after = retry_after(response.headers());
    let text = response.text().await.map_err(transport)?;
    if !status.is_success() {
        return Err(openrouter_error(status.as_u16(), retry_after, &text));
    }
    let json: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| LlmError::InvalidResponse(format!("malformed model catalog: {error}")))?;
    let models: Catalog = Arc::new(parse_catalog(&json)?);
    catalogs().lock().unwrap().insert(
        backend.base_url.clone(),
        CatalogEntry {
            fetched: Instant::now(),
            models: Some(models.clone()),
        },
    );
    Ok(models)
}

/// Loads the catalog when the cached one is stale. A catalog that cannot
/// be fetched is not fatal: the request goes out with the defaults, and
/// the failure is remembered for a minute. Returns whether a new catalog
/// arrived, so the caller can validate against it.
async fn refresh_catalog(backend: &LlmBackend) -> bool {
    if catalog_is_fresh(&backend.base_url) {
        return false;
    }
    match fetch_catalog(backend).await {
        Ok(_) => true,
        Err(error) => {
            log::debug!("[openrouter] model catalog unavailable: {error}");
            catalogs().lock().unwrap().insert(
                backend.base_url.clone(),
                CatalogEntry {
                    fetched: Instant::now(),
                    models: None,
                },
            );
            false
        }
    }
}

fn capabilities_at(base_url: &str, model: &str) -> Capabilities {
    match cached_catalog(base_url).and_then(|catalog| catalog.get(model.trim()).copied()) {
        Some(info) => Capabilities {
            tools: info.tools,
            vision: info.vision,
            reasoning_controls: info.reasoning,
            ..DEFAULT_CAPABILITIES
        },
        None => DEFAULT_CAPABILITIES,
    }
}

/// What openrouter.ai offers for one model id, from the cached catalog.
pub(super) fn capabilities_for(model: &str) -> Capabilities {
    capabilities_at(OPENROUTER_BASE_URL, model)
}

/// The transport implementation is private to this adapter.
///
/// `LlmRequest::scope` is accepted and ignored: prompt caching is each
/// vendor's own affair behind OpenRouter, with no lifetime to choose (#137).
pub(super) struct OpenrouterAdapter(pub LlmBackend);
impl ProviderAdapter for OpenrouterAdapter {
    fn capabilities(&self) -> Capabilities {
        capabilities_at(&self.0.base_url, &self.0.model)
    }
    fn complete<'a>(
        &'a self,
        request: LlmRequest<'a>,
    ) -> BoxFuture<'a, Result<LlmResponse, LlmError>> {
        Box::pin(async move {
            request.validate(self.capabilities(), false)?;
            let b = &self.0;
            tokio::select! {
                biased;
                _ = request.cancellation.cancelled() => Err(LlmError::Cancelled),
                result = async {
                    if refresh_catalog(b).await {
                        request.validate(self.capabilities(), false)?;
                    }
                    openrouter_complete(b, &request).await
                } => result,
            }
        })
    }
    fn stream<'a>(
        &'a self,
        request: LlmRequest<'a>,
        events: &'a EventSink<'a>,
    ) -> BoxFuture<'a, Result<LlmResponse, LlmError>> {
        Box::pin(async move {
            request.validate(self.capabilities(), true)?;
            let b = &self.0;
            tokio::select! {
                biased;
                _ = request.cancellation.cancelled() => Err(LlmError::Cancelled),
                result = async {
                    if refresh_catalog(b).await {
                        request.validate(self.capabilities(), true)?;
                    }
                    openrouter_stream(b, &request, events).await
                } => result,
            }
        })
    }
    fn discover_models(&self) -> BoxFuture<'_, Result<Vec<String>, LlmError>> {
        Box::pin(async move {
            let catalog = fetch_catalog(&self.0).await?;
            let mut ids: Vec<String> = catalog.keys().cloned().collect();
            ids.sort();
            Ok(ids)
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
        // A port an earlier test used may still have its catalog cached.
        forget_catalog(&url);
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

    /// Some vendors behind OpenRouter number every call `index: 0`. A piece
    /// whose id differs from the call at its index starts a new call, the
    /// argument pieces that follow at that index continue the newest one,
    /// and the earlier call is kept.
    #[tokio::test]
    async fn streamed_calls_that_reuse_an_index_are_kept_apart() {
        let sse = concat!(
            r#"data: {"id":"gen-1","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"a","type":"function","function":{"name":"one","arguments":"{\"n\":1}"}}]},"finish_reason":null}]}"#,
            "\n\n",
            r#"data: {"id":"gen-1","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"b","type":"function","function":{"name":"two","arguments":"{\"n\":"}}]},"finish_reason":null}]}"#,
            "\n\n",
            r#"data: {"id":"gen-1","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"2}"}}]},"finish_reason":"tool_calls"}]}"#,
            "\n\n",
            "data: [DONE]\n\n"
        );
        let (url, _, task) = mock(None, 200, vec![], sse.into()).await;
        let adapter = backend(&url, "anthropic/claude-sonnet-4.6")
            .adapter()
            .unwrap();
        let events = Mutex::new(Vec::new());
        let sink = |event| events.lock().unwrap().push(event);
        let response = adapter
            .stream(
                LlmRequest::new(ExecutionScope::Subagent, &[], &[], &[]),
                &sink,
            )
            .await
            .unwrap();
        task.abort();

        assert_eq!(response.stop_reason, StopReason::ToolCalls);
        let calls: Vec<(&str, &str, Value)> = response
            .tool_calls
            .iter()
            .map(|call| (call.id.as_str(), call.name.as_str(), call.arguments.clone()))
            .collect();
        assert_eq!(
            calls,
            [("a", "one", json!({"n": 1})), ("b", "two", json!({"n": 2}))]
        );
        let events = events.lock().unwrap();
        let started: Vec<&str> = events
            .iter()
            .filter_map(|event| match event {
                LlmEvent::ToolCallStarted { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(started, ["a", "b"]);
        let deltas: Vec<(&str, &str)> = events
            .iter()
            .filter_map(|event| match event {
                LlmEvent::ToolArgumentsDelta { id, delta, .. } => {
                    Some((id.as_str(), delta.as_str()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            deltas,
            [("a", "{\"n\":1}"), ("b", "{\"n\":"), ("b", "2}")],
            "the continuation piece belongs to the newest call at its index"
        );
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

    /// Status, response headers, body, expected variant.
    type ErrorCase = (u16, Vec<(&'static str, String)>, String, &'static str);

    /// What OpenRouter answers with in practice, in both modes.
    #[tokio::test]
    async fn maps_openrouter_errors_to_typed_variants() {
        let error = |code: u16, message: &str| {
            json!({"error": {"code": code, "message": message}}).to_string()
        };
        let cases: Vec<ErrorCase> = vec![
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
        let mut config = Config {
            public_url: "https://private.example".into(),
            ..Config::default()
        };
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

        // The agent loop answers every call of a turn in one user message.
        // Chat Completions wants those `tool` messages right after the
        // assistant's `tool_calls`, so the images the results carry follow
        // the last of them rather than splitting them.
        let batch = [Message::User {
            content: vec![
                ContentBlock::ToolOutput {
                    call_id: "call1".into(),
                    content: ToolOutputContent::Blocks(vec![
                        ContentBlock::text("captured"),
                        ContentBlock::Image {
                            source: ImageSource::Url {
                                url: "https://example.test/shot.png".into(),
                            },
                            resource_provenance: None,
                        },
                    ]),
                },
                ContentBlock::ToolOutput {
                    call_id: "call2".into(),
                    content: ToolOutputContent::Text("found".into()),
                },
            ],
        }];
        let wire = messages_to_openrouter(&[], &batch);
        let roles: Vec<&str> = wire.iter().map(|m| m["role"].as_str().unwrap()).collect();
        assert_eq!(roles, ["tool", "tool", "user"], "{wire:?}");
        assert_eq!(
            wire[0],
            json!({"role": "tool", "tool_call_id": "call1", "content": "captured"})
        );
        assert_eq!(
            wire[1],
            json!({"role": "tool", "tool_call_id": "call2", "content": "found"})
        );
        assert_eq!(
            wire[2]["content"],
            json!([{"type": "image_url", "image_url": {"url": "https://example.test/shot.png"}}])
        );

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
