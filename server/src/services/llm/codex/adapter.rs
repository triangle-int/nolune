//! The Codex provider (#27): Nolune conversations as codex app-server
//! threads, turns streamed into the provider-neutral events, and Nolune's
//! tools bridged through `dynamicTools`.
//!
//! One thread per conversation. The first turn of a chat starts a thread
//! (`thread/start`) and the id is kept in the chat's `meta.json`; after a
//! restart the next turn resumes it (`thread/resume`). The thread is
//! started read-only (`sandbox: read-only`), with no approvals
//! (`approvalPolicy: never`), with codex's own shell, file, browser, MCP
//! and plugin surfaces switched off, and with Nolune's tool definitions as
//! `dynamicTools`: the only tools the model can call. A one-shot run (a
//! title, a memory extraction, a connection test) gets an ephemeral thread.
//!
//! A turn sends the trailing user content as `turn/start` input (codex
//! keeps the earlier turns itself) and reads the stream: agent message
//! deltas become `TextDelta`, `thread/tokenUsage/updated` becomes `Usage`,
//! `turn/completed` ends it. When codex asks `item/tool/call`, the adapter
//! answers nothing itself: it hands the call back to the agent loop as a
//! `ToolCall` and returns with `StopReason::ToolCalls`, the turn left open.
//! The loop runs the tool through Nolune's capability and approval layer
//! and calls again with the result; the adapter finds the open turn by the
//! call id, answers codex, and reads on. Any approval codex asks for is
//! declined, and a command or file change codex runs on its own fails the
//! turn: nothing executes but Nolune's tools.
//!
//! Cancellation sends `turn/interrupt`; a child that dies mid-turn fails
//! the turn with a transport error and the next turn resumes the thread in
//! the replaced child; a failed turn maps its `codexErrorInfo` to the typed
//! variants the callers act on.

use std::collections::HashMap;
use std::hash::{Hash as _, Hasher as _};
use std::path::Path;
use std::time::Duration;

use futures::future::BoxFuture;
use serde_json::{Value, json};
use tokio::sync::broadcast;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::services::tool::ToolDefinition;

use super::super::contract::{
    Capabilities, ConversationRef, EventSink, LlmError, LlmEvent, LlmRequest, ProviderAdapter,
    StopReason, Usage,
};
use super::super::types::{
    ContentBlock, ImageSource, LlmBackend, LlmResponse, Message, ToolCall, ToolOutputContent,
};
use super::AppServerError;
use super::process::{AppServer, Incoming};
use super::protocol::RpcError;

pub const CAPABILITIES: Capabilities = Capabilities {
    vision: false,
    documents: false,
    tools: true,
    streaming: true,
    reasoning_controls: false,
    model_discovery: false,
    token_counting: false,
};

/// How long a turn may go without a single event before it is interrupted
/// and reported as timed out; a model that thinks for minutes still emits
/// reasoning and message items along the way.
const IDLE_TIMEOUT: Duration = Duration::from_secs(600);

/// How long an interrupt waits for the turn to end before the adapter
/// gives up on hearing it.
const INTERRUPT_GRACE: Duration = Duration::from_secs(5);

/// What a turn asks the user to do when the app-server holds no login.
const LOGIN_REQUIRED: &str = "Codex login required: sign in with ChatGPT from Settings › Connections, or run `codex login` on this machine.";

/// The bookkeeping the runtime keeps for the adapter: which thread each
/// conversation continues in, and the turns left open on a tool call.
#[derive(Default)]
pub(super) struct Threads {
    /// Conversation key → thread.
    by_conversation: HashMap<String, ThreadState>,
    /// Thread id → the turn waiting on Nolune for a tool result.
    open: HashMap<String, OpenTurn>,
    /// Call id → thread id, for the request that carries the result.
    pending: HashMap<String, String>,
}

impl Threads {
    /// Forget everything: the child is gone.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn clear(&mut self) {
        self.by_conversation.clear();
        self.open.clear();
        self.pending.clear();
    }

    /// The open turn that `call_ids` answer, taken out of the books.
    fn take_open_answering(&mut self, call_ids: &[&str]) -> Option<OpenTurn> {
        let thread_id = call_ids
            .iter()
            .find_map(|call_id| self.pending.get(*call_id).cloned())?;
        self.take_open(&thread_id)
    }

    /// The open turn of `thread_id`, taken out of the books.
    fn take_open(&mut self, thread_id: &str) -> Option<OpenTurn> {
        let open = self.open.remove(thread_id)?;
        self.pending.retain(|_, thread| thread != thread_id);
        Some(open)
    }

    /// Leave a turn open on its tool call(s).
    fn store_open(&mut self, open: OpenTurn) {
        for call_id in open.calls.keys() {
            self.pending.insert(call_id.clone(), open.thread_id.clone());
        }
        self.open.insert(open.thread_id.clone(), open);
    }
}

/// A thread attached in this process.
#[derive(Clone)]
struct ThreadState {
    thread_id: String,
    /// The names of the tools the thread was started with, when this
    /// process started it; unknown for a resumed thread.
    tools: Option<Vec<String>>,
    /// A digest of the instructions and model the thread was last
    /// configured with.
    configured: u64,
    /// The app-server child the thread is loaded in; another child has to
    /// resume it first.
    generation: u64,
}

/// A turn that asked for a tool and waits for the answer.
struct OpenTurn {
    thread_id: String,
    turn_id: String,
    events: broadcast::Receiver<Incoming>,
    /// The calls handed to the agent loop, by call id: the app-server's
    /// request id to answer with.
    calls: HashMap<String, Value>,
    /// The child the turn runs in; a replacement child never heard of it.
    generation: u64,
}

pub struct CodexAdapter(pub LlmBackend);

// ═══════════════════════════════════════════════════════════════════════════
// Wire shapes
// ═══════════════════════════════════════════════════════════════════════════

/// Nolune's tools as codex `dynamicTools`: name, description and schema,
/// nothing that lets codex run them itself.
pub(super) fn dynamic_tools(tools: &[ToolDefinition]) -> Vec<Value> {
    tools
        .iter()
        .map(|tool| {
            json!({
                "type": "function",
                "name": tool.name,
                "description": tool.description,
                "inputSchema": tool.parameters,
            })
        })
        .collect()
}

/// The config overrides every thread is started with: no project docs, no
/// MCP servers, and codex's own tool surface switched off, so the model
/// has Nolune's tools and nothing else.
pub(super) fn thread_config() -> Value {
    json!({
        "project_doc_max_bytes": 0,
        "mcp_servers": {},
        "features": {
            "shell_tool": false,
            "unified_exec": false,
            "unified_exec_tty": false,
            "view_image": false,
            "image_generation": false,
            "browser_use": false,
            "browser_use_external": false,
            "browser_use_full_cdp_access": false,
            "computer_use": false,
            "multi_agent": false,
            "multi_agent_v2": false,
            "apps": false,
            "plugins": false,
            "hooks": false,
            "sleep_tool": false,
            "tool_suggest": false,
        },
        "tools": {
            "web_search": false,
            "view_image": false,
        },
    })
}

/// The system blocks as the thread's developer instructions, with the
/// output schema appended for a structured request; secrets redacted like
/// every other outgoing payload.
pub(crate) fn developer_instructions(system: &[&str], json_schema: Option<&Value>) -> String {
    let mut blocks: Vec<String> = system
        .iter()
        .filter(|block| !block.is_empty())
        .map(|block| (*block).to_owned())
        .collect();
    if let Some(schema) = json_schema {
        blocks.push(format!(
            "Respond with ONLY valid JSON matching this schema:\n{schema}"
        ));
    }
    crate::services::tools::redact_secrets(&blocks.join("\n\n"))
}

/// One `text` input item.
fn text_item(text: String) -> Value {
    json!({"type": "text", "text": text})
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

/// The conversation before `messages`' last turn, told as text for a
/// thread that was not there for it.
fn recap(earlier: &[Message]) -> String {
    let mut lines = vec!["Earlier in this conversation, before this thread:".to_owned()];
    for message in earlier {
        match message {
            Message::User { content } => {
                for block in content {
                    match block {
                        ContentBlock::Text { text } => lines.push(format!("user: {text}")),
                        ContentBlock::ToolOutput { call_id, content } => {
                            lines.push(format!(
                                "tool result {call_id}: {}",
                                tool_output_text(content)
                            ));
                        }
                        ContentBlock::ContextSummary { content }
                        | ContentBlock::LegacyContextSummary {
                            summary: content, ..
                        } => lines.push(format!("summary: {content}")),
                        ContentBlock::Image { .. } => lines.push("user: [image]".into()),
                        ContentBlock::Document { .. } => lines.push("user: [document]".into()),
                        _ => {}
                    }
                }
            }
            Message::Assistant { content } => {
                for block in content {
                    match block {
                        ContentBlock::Text { text } => lines.push(format!("assistant: {text}")),
                        ContentBlock::ToolCall {
                            name, arguments, ..
                        } => lines.push(format!("assistant called {name}({arguments})")),
                        _ => {}
                    }
                }
            }
        }
    }
    lines.join("\n")
}

/// The `turn/start` input for `messages`: the user content after the last
/// assistant message as text items, with a recap of everything before it
/// when the thread is fresh and the conversation is not (a chat switched
/// to Codex mid-way, or a thread codex no longer has). Tool results that
/// answer no open call are text too, so a turn interrupted between a call
/// and its result still tells the model what happened.
pub(crate) fn turn_input(messages: &[Message], fresh_thread: bool) -> Vec<Value> {
    let split = messages
        .iter()
        .rposition(|message| matches!(message, Message::Assistant { .. }))
        .map_or(0, |last| last + 1);
    let (earlier, trailing) = messages.split_at(split);
    let mut input = Vec::new();
    if fresh_thread && !earlier.is_empty() {
        input.push(text_item(recap(earlier)));
    }
    for message in trailing {
        let Message::User { content } = message else {
            continue;
        };
        for block in content {
            match block {
                ContentBlock::Text { text } => input.push(text_item(text.clone())),
                ContentBlock::ToolOutput { call_id, content } => input.push(text_item(format!(
                    "Result of the earlier tool call {call_id}:\n{}",
                    tool_output_text(content)
                ))),
                ContentBlock::ContextSummary { content }
                | ContentBlock::LegacyContextSummary {
                    summary: content, ..
                } => input.push(text_item(format!("Conversation summary:\n{content}"))),
                _ => {}
            }
        }
    }
    input
        .into_iter()
        .map(crate::services::tools::redact_value)
        .collect()
}

/// A tool result as the `contentItems` of a `item/tool/call` answer.
pub(super) fn tool_output_items(content: &ToolOutputContent) -> Vec<Value> {
    let mut items = match content {
        ToolOutputContent::Text(text) => vec![json!({"type": "inputText", "text": text})],
        ToolOutputContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(json!({"type": "inputText", "text": text})),
                ContentBlock::Image { source, .. } => {
                    let url = match source {
                        ImageSource::Base64 { media_type, data } => {
                            format!("data:{media_type};base64,{data}")
                        }
                        ImageSource::Url { url } => url.clone(),
                    };
                    Some(json!({"type": "inputImage", "imageUrl": url}))
                }
                _ => None,
            })
            .collect(),
        ToolOutputContent::Legacy(value) => {
            vec![json!({"type": "inputText", "text": value.to_string()})]
        }
    };
    if items.is_empty() {
        items.push(json!({"type": "inputText", "text": ""}));
    }
    items
        .into_iter()
        .map(crate::services::tools::redact_value)
        .collect()
}

/// The typed reading of a failed turn's `error` (`TurnError`): the
/// `codexErrorInfo` names the class, as a string for the simple variants
/// and as `{variant: {httpStatusCode}}` for the transport ones.
pub(super) fn map_turn_error(error: &Value) -> LlmError {
    let message = crate::services::tools::redact_secrets(
        error["message"].as_str().unwrap_or("the turn failed"),
    );
    let info = &error["codexErrorInfo"];
    if let Some(kind) = info.as_str() {
        return match kind {
            "usageLimitExceeded" | "rateLimitExceeded" | "serverOverloaded" => {
                LlmError::RateLimited {
                    retry_after: None,
                    message,
                }
            }
            "contextWindowExceeded" | "sessionBudgetExceeded" => LlmError::ContextLength(message),
            "unauthorized" => LlmError::Authentication(message),
            "internalServerError" => LlmError::Http {
                status: 500,
                message,
            },
            "badRequest" => LlmError::Http {
                status: 400,
                message,
            },
            "interrupted" => LlmError::Cancelled,
            _ => LlmError::Transport(message),
        };
    }
    let upstream = [
        "httpConnectionFailed",
        "responseStreamConnectionFailed",
        "responseStreamDisconnected",
        "responseTooManyFailedAttempts",
    ]
    .into_iter()
    .find_map(|variant| info.get(variant));
    match upstream.and_then(|detail| detail["httpStatusCode"].as_u64()) {
        Some(401 | 403) => LlmError::Authentication(message),
        Some(429) => LlmError::RateLimited {
            retry_after: None,
            message,
        },
        Some(status) => LlmError::Http {
            status: status as u16,
            message,
        },
        None => LlmError::Transport(message),
    }
}

/// Output-equivalent tokens by the ratios the OpenAI adapter uses: the
/// same models sit behind codex.
fn normalized_tokens(usage: &Usage, structured: bool) -> u64 {
    if structured {
        return usage.input_tokens + usage.output_tokens;
    }
    let uncached = usage.input_tokens.saturating_sub(usage.cache_read_tokens);
    (usage.output_tokens as f64 + uncached as f64 * 0.2 + usage.cache_read_tokens as f64 * 0.1)
        as u64
}

/// `thread/tokenUsage/updated`'s `last` breakdown as usage.
fn parse_usage(last: &Value) -> Usage {
    Usage {
        input_tokens: last["inputTokens"].as_u64().unwrap_or(0),
        output_tokens: last["outputTokens"].as_u64().unwrap_or(0),
        cache_read_tokens: last["cachedInputTokens"].as_u64().unwrap_or(0),
        cache_write_tokens: last["cacheWriteInputTokens"].as_u64().unwrap_or(0),
        cost: None,
    }
}

/// The `thread/start` params: read-only, no approvals, codex's surfaces
/// off, Nolune's tools, on the preset's model.
fn thread_start_params(
    backend: &LlmBackend,
    request: &LlmRequest<'_>,
    cwd: &Path,
    ephemeral: bool,
) -> Value {
    json!({
        "approvalPolicy": "never",
        "config": thread_config(),
        "cwd": cwd.to_string_lossy(),
        "developerInstructions": developer_instructions(request.system, request.json_schema),
        "dynamicTools": dynamic_tools(request.tools),
        "ephemeral": ephemeral,
        "model": backend.model,
        "sandbox": "read-only",
    })
}

/// The `thread/resume` params: the same settings, on the thread codex
/// already has; its turns stay where they are.
fn thread_resume_params(
    backend: &LlmBackend,
    request: &LlmRequest<'_>,
    thread_id: &str,
    cwd: &Path,
) -> Value {
    json!({
        "threadId": thread_id,
        "approvalPolicy": "never",
        "config": thread_config(),
        "cwd": cwd.to_string_lossy(),
        "developerInstructions": developer_instructions(request.system, request.json_schema),
        "excludeTurns": true,
        "model": backend.model,
        "sandbox": "read-only",
    })
}

/// A digest of what a thread is configured with, to know when to
/// reconfigure it.
fn configuration_digest(backend: &LlmBackend, request: &LlmRequest<'_>) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    backend.model.hash(&mut hasher);
    developer_instructions(request.system, request.json_schema).hash(&mut hasher);
    hasher.finish()
}

/// The tool names a request carries, sorted: a thread's tools are fixed
/// when it starts.
fn tool_names(tools: &[ToolDefinition]) -> Vec<String> {
    let mut names: Vec<String> = tools.iter().map(|tool| tool.name.clone()).collect();
    names.sort();
    names.dedup();
    names
}

/// The tool results in the user content after the last assistant message:
/// what the agent loop brings back for the calls of an open turn.
fn trailing_tool_outputs(messages: &[Message]) -> Vec<(&str, &ToolOutputContent)> {
    let split = messages
        .iter()
        .rposition(|message| matches!(message, Message::Assistant { .. }))
        .map_or(0, |last| last + 1);
    messages[split..]
        .iter()
        .flat_map(|message| match message {
            Message::User { content } => content.as_slice(),
            Message::Assistant { .. } => &[],
        })
        .filter_map(|block| match block {
            ContentBlock::ToolOutput { call_id, content } => Some((call_id.as_str(), content)),
            _ => None,
        })
        .collect()
}

/// The id a `thread/start` or `thread/resume` answer names.
fn thread_id_of(reply: &Value) -> Result<String, LlmError> {
    reply["thread"]["id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| LlmError::InvalidResponse("the thread answer names no thread id".into()))
}

/// Items codex must never produce on its own: anything that executes,
/// edits or delegates outside Nolune's tools.
fn is_forbidden_item(kind: &str) -> bool {
    matches!(
        kind,
        "commandExecution"
            | "fileChange"
            | "mcpToolCall"
            | "collabAgentToolCall"
            | "subAgentActivity"
    )
}

// ═══════════════════════════════════════════════════════════════════════════
// The turn
// ═══════════════════════════════════════════════════════════════════════════

/// The thread a request runs in, attached to the live child, and whether
/// it is new to the conversation (a recap of the earlier messages goes in
/// its first turn).
struct Attached {
    thread_id: String,
    fresh: bool,
}

/// How one turn ended.
enum Outcome {
    Completed,
    /// Codex asked for a tool; the turn stays open on this call.
    ToolCall {
        call: ToolCall,
        request_id: Value,
    },
}

/// One turn being read: what streamed so far and where it came from.
struct Turn<'a> {
    server: AppServer,
    thread_id: String,
    turn_id: String,
    events: broadcast::Receiver<Incoming>,
    sink: &'a EventSink<'a>,
    cancel: &'a CancellationToken,
    text: String,
    usage: Usage,
    /// Agent message items that streamed deltas, by item id, so a completed
    /// item is not appended twice.
    streamed: std::collections::HashSet<String>,
}

impl Turn<'_> {
    /// Read events until the turn completes, asks for a tool, fails, is
    /// cancelled, or goes quiet for too long.
    async fn read(&mut self) -> Result<Outcome, LlmError> {
        loop {
            let incoming = tokio::select! {
                biased;
                _ = self.cancel.cancelled() => {
                    self.interrupt().await;
                    return Err(LlmError::Cancelled);
                }
                next = tokio::time::timeout(IDLE_TIMEOUT, self.events.recv()) => match next {
                    Err(_elapsed) => {
                        self.interrupt().await;
                        return Err(LlmError::Timeout);
                    }
                    Ok(Err(broadcast::error::RecvError::Closed)) => {
                        return Err(LlmError::Transport(
                            "codex app-server event stream closed".into(),
                        ));
                    }
                    Ok(Err(broadcast::error::RecvError::Lagged(missed))) => {
                        // A request the app-server waits on may be among
                        // the missed events: the turn cannot go on.
                        self.interrupt().await;
                        return Err(LlmError::Transport(format!(
                            "fell {missed} events behind the codex app-server"
                        )));
                    }
                    Ok(Ok(incoming)) => incoming,
                }
            };
            match incoming {
                Incoming::Started { .. } => {}
                Incoming::Exited { reason, .. } => {
                    return Err(LlmError::Transport(reason));
                }
                Incoming::Notification { method, params } => {
                    if let Some(outcome) = self.notification(&method, &params).await? {
                        return Ok(outcome);
                    }
                }
                Incoming::Request { id, method, params } => {
                    if let Some(outcome) = self.request(id, &method, params).await? {
                        return Ok(outcome);
                    }
                }
            }
        }
    }

    fn is_ours(&self, params: &Value) -> bool {
        params["threadId"] == self.thread_id.as_str() && params["turnId"] == self.turn_id.as_str()
    }

    async fn notification(
        &mut self,
        method: &str,
        params: &Value,
    ) -> Result<Option<Outcome>, LlmError> {
        if params["threadId"] != self.thread_id.as_str() {
            return Ok(None);
        }
        match method {
            "item/agentMessage/delta" if self.is_ours(params) => {
                if let Some(delta) = params["delta"].as_str() {
                    if let Some(item) = params["itemId"].as_str() {
                        self.streamed.insert(item.to_owned());
                    }
                    self.text.push_str(delta);
                    (self.sink)(LlmEvent::TextDelta(delta.to_owned()));
                }
            }
            "item/started" if self.is_ours(params) => {
                let item = &params["item"];
                let kind = item["type"].as_str().unwrap_or("");
                if is_forbidden_item(kind) {
                    let detail = item["command"]
                        .as_str()
                        .or_else(|| item["tool"].as_str())
                        .or_else(|| item["id"].as_str())
                        .unwrap_or("")
                        .to_owned();
                    log::error!(
                        "[codex] the app-server started a {kind} item on its own ({detail}); interrupting the turn"
                    );
                    self.interrupt().await;
                    return Err(LlmError::InvalidResponse(format!(
                        "codex ran a {kind} item outside Nolune's tools: {detail}"
                    )));
                }
            }
            "item/completed" if self.is_ours(params) => {
                let item = &params["item"];
                if item["type"] == "agentMessage"
                    && let Some(text) = item["text"].as_str()
                    && !item["id"]
                        .as_str()
                        .is_some_and(|id| self.streamed.contains(id))
                    && !text.is_empty()
                {
                    // No deltas came for this item: the whole text at once.
                    self.text.push_str(text);
                    (self.sink)(LlmEvent::TextDelta(text.to_owned()));
                }
            }
            "thread/tokenUsage/updated" if self.is_ours(params) => {
                self.usage = parse_usage(&params["tokenUsage"]["last"]);
                (self.sink)(LlmEvent::Usage(self.usage));
            }
            "turn/completed" if params["turn"]["id"] == self.turn_id.as_str() => {
                let turn = &params["turn"];
                return match turn["status"].as_str().unwrap_or("") {
                    "completed" => Ok(Some(Outcome::Completed)),
                    "interrupted" => Err(LlmError::Cancelled),
                    "failed" => Err(map_turn_error(&turn["error"])),
                    other => Err(LlmError::InvalidResponse(format!(
                        "the turn ended with status {other:?}"
                    ))),
                };
            }
            _ => {}
        }
        Ok(None)
    }

    /// A request from the app-server: a tool call for this turn is handed
    /// to the agent loop; everything that asks to run, change or grant
    /// something is declined; the rest is refused so nothing waits.
    async fn request(
        &mut self,
        id: Value,
        method: &str,
        params: Value,
    ) -> Result<Option<Outcome>, LlmError> {
        if params["threadId"] != self.thread_id.as_str() {
            return Ok(None);
        }
        if params["turnId"] != self.turn_id.as_str() {
            self.refuse(&id, method, "this turn is over").await;
            return Ok(None);
        }
        match method {
            "item/tool/call" => {
                let call = ToolCall::required_string(&params, "callId").and_then(|call_id| {
                    let name = ToolCall::required_string(&params, "tool")?;
                    ToolCall::validate_arguments(&params["arguments"])?;
                    Ok(ToolCall {
                        id: call_id,
                        name,
                        arguments: params["arguments"].clone(),
                    })
                });
                match call {
                    Ok(call) => {
                        (self.sink)(LlmEvent::ToolCallStarted {
                            id: call.id.clone(),
                            name: call.name.clone(),
                        });
                        (self.sink)(LlmEvent::ToolArgumentsDelta {
                            id: call.id.clone(),
                            name: call.name.clone(),
                            delta: call.arguments.to_string(),
                        });
                        Ok(Some(Outcome::ToolCall {
                            call,
                            request_id: id,
                        }))
                    }
                    Err(error) => {
                        self.refuse(&id, method, &error.to_string()).await;
                        self.interrupt().await;
                        Err(error)
                    }
                }
            }
            "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
                log::warn!(
                    "[codex] the app-server asked for approval ({method}); declined, nothing runs but Nolune's tools"
                );
                self.answer(&id, json!({"decision": "decline"})).await;
                Ok(None)
            }
            "item/permissions/requestApproval" => {
                log::warn!("[codex] the app-server asked for permissions; none granted");
                self.answer(&id, json!({"permissions": {}})).await;
                Ok(None)
            }
            "item/tool/requestUserInput" => {
                self.answer(&id, json!({"answers": {}})).await;
                Ok(None)
            }
            "mcpServer/elicitation/request" => {
                self.answer(&id, json!({"action": "decline"})).await;
                Ok(None)
            }
            _ => {
                self.refuse(&id, method, "nolune does not handle this request")
                    .await;
                Ok(None)
            }
        }
    }

    async fn answer(&self, id: &Value, result: Value) {
        if let Err(error) = self.server.respond(id, Ok(result)).await {
            log::warn!("[codex] could not answer the app-server's request {id}: {error}");
        }
    }

    async fn refuse(&self, id: &Value, method: &str, why: &str) {
        let refusal = RpcError {
            code: -32601,
            message: format!("nolune refused {method}: {why}"),
            data: None,
        };
        if let Err(error) = self.server.respond(id, Err(refusal)).await {
            log::warn!("[codex] could not refuse the app-server's {method} request {id}: {error}");
        }
    }

    /// Stop the turn and wait, briefly, for the app-server to say it did.
    async fn interrupt(&mut self) {
        interrupt_turn(
            &self.server,
            &self.thread_id,
            &self.turn_id,
            &mut self.events,
        )
        .await;
    }
}

/// Send `turn/interrupt` and drain `events` until that turn completes, or
/// the grace period ends. Errors are logged: the turn may be over already,
/// or the child gone, and either way there is nothing left to stop.
async fn interrupt_turn(
    server: &AppServer,
    thread_id: &str,
    turn_id: &str,
    events: &mut broadcast::Receiver<Incoming>,
) {
    if let Err(error) = server
        .request(
            super::protocol::TURN_INTERRUPT,
            json!({"threadId": thread_id, "turnId": turn_id}),
        )
        .await
    {
        log::warn!("[codex] turn/interrupt for {turn_id}: {error}");
        return;
    }
    let deadline = Instant::now() + INTERRUPT_GRACE;
    loop {
        match tokio::time::timeout_at(deadline, events.recv()).await {
            Ok(Ok(Incoming::Notification { method, params }))
                if method == "turn/completed" && params["turn"]["id"] == turn_id =>
            {
                return;
            }
            Ok(Ok(Incoming::Exited { .. })) | Ok(Err(_)) | Err(_) => return,
            Ok(Ok(_)) => {}
        }
    }
}

impl CodexAdapter {
    /// One request: attach the conversation's thread, start or continue a
    /// turn, and read it until it completes or asks for a tool.
    async fn turn(
        &self,
        request: &LlmRequest<'_>,
        events: &EventSink<'_>,
    ) -> Result<LlmResponse, LlmError> {
        let backend = &self.0;
        let runtime = &backend.codex;

        // The agent loop is back with the result of a call codex waits on:
        // answer it and read on in the same turn.
        let outputs = trailing_tool_outputs(request.messages);
        if !outputs.is_empty() {
            let call_ids: Vec<&str> = outputs.iter().map(|(call_id, _)| *call_id).collect();
            let open = runtime
                .threads()
                .lock()
                .unwrap()
                .take_open_answering(&call_ids);
            if let Some(mut open) = open {
                let server = runtime.app_server().await?;
                if server.generation() != open.generation {
                    // The child died while the loop ran the tool: the turn
                    // died with it. The thread is resumed on the next turn.
                    return Err(LlmError::Transport(
                        "codex app-server restarted while the turn waited on a tool result".into(),
                    ));
                }
                for (call_id, content) in &outputs {
                    let Some(request_id) = open.calls.remove(*call_id) else {
                        continue;
                    };
                    server
                        .respond(
                            &request_id,
                            Ok(json!({"contentItems": tool_output_items(content), "success": true})),
                        )
                        .await?;
                }
                for (call_id, request_id) in open.calls.drain() {
                    log::warn!(
                        "[codex] tool call {call_id} came back without a result; refusing it"
                    );
                    let refusal = RpcError {
                        code: -32601,
                        message: "nolune has no result for this call".into(),
                        data: None,
                    };
                    let _ = server.respond(&request_id, Err(refusal)).await;
                }
                let turn = Turn {
                    server,
                    thread_id: open.thread_id,
                    turn_id: open.turn_id,
                    events: open.events,
                    sink: events,
                    cancel: &request.cancellation,
                    text: String::new(),
                    usage: Usage::default(),
                    streamed: Default::default(),
                };
                return self.finish(request, turn).await;
            }
        }

        let server = runtime.app_server().await?;
        // The app-server accepts a turn without a login and retries the
        // model for a minute before it fails: ask first.
        if !runtime.account().await?.is_logged_in() {
            return Err(LlmError::SetupRequired(LOGIN_REQUIRED.into()));
        }
        let Attached { thread_id, fresh } = self.attach(request, &server).await?;
        if request.cancellation.is_cancelled() {
            return Err(LlmError::Cancelled);
        }
        // Subscribe before the turn starts, so its first events are heard.
        let events_rx = server.subscribe();
        let mut params = json!({
            "threadId": thread_id,
            "input": turn_input(request.messages, fresh),
        });
        if let Some(schema) = request.json_schema {
            params["outputSchema"] = schema.clone();
        }
        let reply = server.request("turn/start", params).await?;
        let turn_id = reply["turn"]["id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| LlmError::InvalidResponse("turn/start named no turn id".into()))?;
        let turn = Turn {
            server,
            thread_id,
            turn_id,
            events: events_rx,
            sink: events,
            cancel: &request.cancellation,
            text: String::new(),
            usage: Usage::default(),
            streamed: Default::default(),
        };
        self.finish(request, turn).await
    }

    /// Read the turn to its end, or to a tool call left open.
    async fn finish(
        &self,
        request: &LlmRequest<'_>,
        mut turn: Turn<'_>,
    ) -> Result<LlmResponse, LlmError> {
        let outcome = turn.read().await?;
        let structured = request.json_schema.is_some();
        let (tool_calls, stop_reason) = match outcome {
            Outcome::Completed => (Vec::new(), StopReason::Complete),
            Outcome::ToolCall { call, request_id } => {
                let mut calls = HashMap::new();
                calls.insert(call.id.clone(), request_id);
                self.0.codex.threads().lock().unwrap().store_open(OpenTurn {
                    thread_id: turn.thread_id.clone(),
                    turn_id: turn.turn_id.clone(),
                    events: turn.events,
                    calls,
                    generation: turn.server.generation(),
                });
                (vec![call], StopReason::ToolCalls)
            }
        };
        let ordered_content = if turn.text.is_empty() {
            Vec::new()
        } else {
            vec![ContentBlock::text(&turn.text)]
        };
        Ok(LlmResponse {
            ordered_content,
            text: turn.text,
            tool_calls,
            stop_reason,
            tokens_used: normalized_tokens(&turn.usage, structured),
            usage: turn.usage,
        })
    }

    /// The thread this request runs in, attached to the live child: a
    /// conversation's thread from the books or its `meta.json`, resumed
    /// after a restart or reconfigured when the instructions or model
    /// changed, started fresh when it is new, lost, or its tools changed;
    /// an ephemeral thread for a one-shot run.
    async fn attach(
        &self,
        request: &LlmRequest<'_>,
        server: &AppServer,
    ) -> Result<Attached, LlmError> {
        let backend = &self.0;
        let Some(conversation) = request.conversation else {
            let cwd = std::env::temp_dir();
            let reply = server
                .request(
                    "thread/start",
                    thread_start_params(backend, request, &cwd, true),
                )
                .await?;
            return Ok(Attached {
                thread_id: thread_id_of(&reply)?,
                fresh: true,
            });
        };
        let ConversationRef {
            instance_slug,
            chat_id,
            workspace_dir,
        } = conversation;
        let key = format!("{instance_slug}/{chat_id}");
        let configured = configuration_digest(backend, request);
        let tools = tool_names(request.tools);
        let generation = server.generation();
        let known = backend
            .codex
            .threads()
            .lock()
            .unwrap()
            .by_conversation
            .get(&key)
            .cloned();

        if let Some(state) = known {
            // A turn left waiting on a tool result that never came: the
            // conversation moved on without it.
            let stale = backend
                .codex
                .threads()
                .lock()
                .unwrap()
                .take_open(&state.thread_id);
            if let Some(mut stale) = stale {
                log::info!(
                    "[codex] interrupting turn {} of {key}, left waiting on a tool result",
                    stale.turn_id
                );
                interrupt_turn(server, &stale.thread_id, &stale.turn_id, &mut stale.events).await;
                for (call_id, request_id) in stale.calls.drain() {
                    let refusal = RpcError {
                        code: -32601,
                        message: format!("nolune abandoned tool call {call_id}"),
                        data: None,
                    };
                    let _ = server.respond(&request_id, Err(refusal)).await;
                }
            }
            if state.tools.as_ref().is_some_and(|known| *known != tools) {
                log::info!("[codex] the tools of {key} changed; starting a fresh thread");
                return self
                    .start_durable(request, server, &key, workspace_dir, tools, configured)
                    .await;
            }
            if state.generation != generation || state.configured != configured {
                match server
                    .request(
                        "thread/resume",
                        thread_resume_params(backend, request, &state.thread_id, workspace_dir),
                    )
                    .await
                {
                    Ok(_) => {
                        let mut threads = backend.codex.threads().lock().unwrap();
                        threads.by_conversation.insert(
                            key,
                            ThreadState {
                                configured,
                                generation,
                                ..state.clone()
                            },
                        );
                    }
                    Err(AppServerError::Rpc(error)) => {
                        log::warn!(
                            "[codex] thread {} of {key} could not be resumed ({}); starting a fresh one",
                            state.thread_id,
                            error.message
                        );
                        return self
                            .start_durable(request, server, &key, workspace_dir, tools, configured)
                            .await;
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            return Ok(Attached {
                thread_id: state.thread_id,
                fresh: false,
            });
        }

        // Not attached in this process: the chat may remember a thread from
        // an earlier run.
        let remembered =
            crate::services::chat::get_chat_codex_thread(workspace_dir, instance_slug, chat_id)
                .map_err(|error| {
                    LlmError::Transport(format!("reading the chat's thread: {error}"))
                })?;
        if let Some(thread_id) = remembered {
            match server
                .request(
                    "thread/resume",
                    thread_resume_params(backend, request, &thread_id, workspace_dir),
                )
                .await
            {
                Ok(_) => {
                    backend
                        .codex
                        .threads()
                        .lock()
                        .unwrap()
                        .by_conversation
                        .insert(
                            key,
                            ThreadState {
                                thread_id: thread_id.clone(),
                                tools: None,
                                configured,
                                generation,
                            },
                        );
                    return Ok(Attached {
                        thread_id,
                        fresh: false,
                    });
                }
                Err(AppServerError::Rpc(error)) => {
                    log::warn!(
                        "[codex] thread {thread_id} of {key} is gone ({}); starting a fresh one",
                        error.message
                    );
                }
                Err(error) => return Err(error.into()),
            }
        }
        self.start_durable(request, server, &key, workspace_dir, tools, configured)
            .await
    }

    /// Start a durable thread for a conversation and remember it, in the
    /// books and in the chat's `meta.json`.
    async fn start_durable(
        &self,
        request: &LlmRequest<'_>,
        server: &AppServer,
        key: &str,
        workspace_dir: &Path,
        tools: Vec<String>,
        configured: u64,
    ) -> Result<Attached, LlmError> {
        let backend = &self.0;
        let reply = server
            .request(
                "thread/start",
                thread_start_params(backend, request, workspace_dir, false),
            )
            .await?;
        let thread_id = thread_id_of(&reply)?;
        let (instance_slug, chat_id) = key.split_once('/').unwrap_or((key, ""));
        crate::services::chat::set_chat_codex_thread(
            workspace_dir,
            instance_slug,
            chat_id,
            Some(&thread_id),
        )
        .map_err(|error| LlmError::Transport(format!("remembering the chat's thread: {error}")))?;
        backend
            .codex
            .threads()
            .lock()
            .unwrap()
            .by_conversation
            .insert(
                key.to_owned(),
                ThreadState {
                    thread_id: thread_id.clone(),
                    tools: Some(tools),
                    configured,
                    generation: server.generation(),
                },
            );
        Ok(Attached {
            thread_id,
            fresh: true,
        })
    }
}

impl ProviderAdapter for CodexAdapter {
    fn capabilities(&self) -> Capabilities {
        CAPABILITIES
    }
    fn complete<'a>(
        &'a self,
        request: LlmRequest<'a>,
    ) -> BoxFuture<'a, Result<LlmResponse, LlmError>> {
        Box::pin(async move {
            request.validate(CAPABILITIES, false)?;
            self.turn(&request, &|_| {}).await
        })
    }
    fn stream<'a>(
        &'a self,
        request: LlmRequest<'a>,
        events: &'a EventSink<'a>,
    ) -> BoxFuture<'a, Result<LlmResponse, LlmError>> {
        Box::pin(async move {
            request.validate(CAPABILITIES, true)?;
            self.turn(&request, events).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::contract::ExecutionScope;
    use super::super::super::types::ImageSource;
    use super::super::{fake, runtime::Runtime};
    use super::*;
    use crate::config::{Config, LlmProvider};
    use crate::services::chat::{get_chat_codex_thread, set_chat_codex_thread};
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    /// A fake app-server with its wire log, and a workspace for the chat.
    struct Harness {
        runtime: Runtime,
        log: PathBuf,
        dir: tempfile::TempDir,
    }

    impl Harness {
        fn new() -> Self {
            Self::with_env(&[])
        }

        fn with_env(env: &[(&str, &str)]) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let log = dir.path().join("wire.jsonl");
            let mut launch = fake::launch_logged(&log);
            for (name, value) in env {
                launch.env.push(((*name).into(), (*value).into()));
            }
            Self {
                runtime: Runtime::for_launch(launch),
                log,
                dir,
            }
        }

        fn workspace(&self) -> &Path {
            self.dir.path()
        }

        fn backend(&self) -> LlmBackend {
            let mut config = Config::default();
            config.llm.seed_presets(LlmProvider::Codex);
            let mut backend =
                LlmBackend::for_preset(&config, reqwest::Client::new(), "codex-astra").unwrap();
            backend.codex = self.runtime.clone();
            backend
        }

        fn conversation(&self) -> ConversationRef<'_> {
            ConversationRef {
                instance_slug: "moon",
                chat_id: "chat-1",
                workspace_dir: self.dir.path(),
            }
        }

        fn sent(&self, method: &str) -> Vec<Value> {
            fake::sent(&self.log, method)
        }

        fn methods(&self) -> Vec<String> {
            fake::wire_log(&self.log)
                .into_iter()
                .filter_map(|frame| frame["method"].as_str().map(str::to_owned))
                .collect()
        }

        fn answers(&self) -> Vec<(Value, Result<Value, Value>)> {
            fake::answers(&self.log)
        }

        fn remembered_thread(&self) -> Option<String> {
            get_chat_codex_thread(self.dir.path(), "moon", "chat-1").unwrap()
        }
    }

    fn tool(name: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.into(),
            description: "test".into(),
            parameters: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
        }
    }

    /// Stream one request, collecting the events it emits.
    async fn stream<'a>(
        backend: &LlmBackend,
        request: LlmRequest<'a>,
    ) -> (Result<LlmResponse, LlmError>, Vec<LlmEvent>) {
        let events = Mutex::new(Vec::new());
        let sink = |event| events.lock().unwrap().push(event);
        let result = tokio::time::timeout(
            Duration::from_secs(20),
            backend.adapter().unwrap().stream(request, &sink),
        )
        .await
        .expect("a turn ends");
        (result, events.into_inner().unwrap())
    }

    fn text_of(events: &[LlmEvent]) -> String {
        events
            .iter()
            .filter_map(|event| match event {
                LlmEvent::TextDelta(delta) => Some(delta.as_str()),
                _ => None,
            })
            .collect()
    }

    /// The exact wire params: a fresh conversation starts a durable thread
    /// on the preset's model, read-only, never asking for approvals, with
    /// codex's own surfaces off and Nolune's tools as the dynamic tools,
    /// then a turn with the user's text; the thread id is remembered in
    /// the chat's meta.json.
    #[tokio::test]
    async fn a_thread_starts_read_only_with_no_approvals_and_only_nolunes_tools() {
        let harness = Harness::new();
        let backend = harness.backend();
        let system = ["system one", "system two"];
        let messages = [Message::user("hello")];
        let tools = [tool("search")];
        let mut request = LlmRequest::new(ExecutionScope::Conversation, &system, &messages, &tools);
        request.conversation = Some(harness.conversation());
        let (result, events) = stream(&backend, request).await;
        let response = result.unwrap();
        assert_eq!(response.text, "hello");
        assert_eq!(text_of(&events), "hello");
        assert_eq!(response.stop_reason, StopReason::ToolCalls);
        assert_eq!(response.tool_calls[0].id, "call1");
        assert_eq!(response.tool_calls[0].name, "search");
        assert_eq!(response.tool_calls[0].arguments["q"], "rust");
        assert_eq!(response.usage.input_tokens, 15);
        assert_eq!(response.usage.cache_read_tokens, 2);
        assert_eq!(response.usage.output_tokens, 4);

        assert_eq!(
            harness.methods(),
            [
                "initialize",
                "initialized",
                "account/read",
                "thread/start",
                "turn/start"
            ],
            "the login is checked, the thread started, the turn sent; nothing else"
        );
        let started = harness.sent("thread/start");
        assert_eq!(
            started[0],
            json!({
                "approvalPolicy": "never",
                "config": thread_config(),
                "cwd": harness.workspace().to_string_lossy(),
                "developerInstructions": "system one\n\nsystem two",
                "dynamicTools": [{
                    "type": "function",
                    "name": "search",
                    "description": "test",
                    "inputSchema": {"type": "object", "properties": {"q": {"type": "string"}}},
                }],
                "ephemeral": false,
                "model": "gpt-6-astra",
                "sandbox": "read-only",
            })
        );
        let config = &started[0]["config"];
        assert_eq!(config["project_doc_max_bytes"], 0);
        assert_eq!(config["mcp_servers"], json!({}));
        for feature in [
            "shell_tool",
            "unified_exec",
            "view_image",
            "image_generation",
            "browser_use",
            "computer_use",
            "multi_agent",
            "apps",
            "plugins",
            "hooks",
        ] {
            assert_eq!(config["features"][feature], false, "{feature}");
        }
        assert_eq!(config["tools"]["web_search"], false);
        assert_eq!(
            harness.sent("turn/start")[0],
            json!({"threadId": "thr_fixture_1", "input": [{"type": "text", "text": "hello"}]})
        );
        assert_eq!(
            harness.remembered_thread().as_deref(),
            Some("thr_fixture_1"),
            "the thread id is kept beside the chat"
        );
        harness.runtime.close();
    }

    #[tokio::test]
    async fn a_remembered_thread_is_resumed_with_the_current_instructions_and_model() {
        let harness = Harness::new();
        set_chat_codex_thread(harness.workspace(), "moon", "chat-1", Some("thr_saved_7")).unwrap();
        let backend = harness.backend();
        let system = ["soul"];
        let messages = [
            Message::user("earlier"),
            Message::assistant("yes"),
            Message::user("hi"),
        ];
        let mut request = LlmRequest::new(ExecutionScope::Conversation, &system, &messages, &[]);
        request.conversation = Some(harness.conversation());
        let (result, events) = stream(&backend, request).await;
        let response = result.unwrap();
        assert_eq!(response.text, "Hello from the fixture");
        assert_eq!(text_of(&events), "Hello from the fixture");
        assert_eq!(response.stop_reason, StopReason::Complete);
        assert_eq!(response.usage.input_tokens, 10);
        assert_eq!(response.usage.output_tokens, 4);

        assert!(
            harness.sent("thread/start").is_empty(),
            "nothing started anew"
        );
        assert_eq!(
            harness.sent("thread/resume")[0],
            json!({
                "threadId": "thr_saved_7",
                "approvalPolicy": "never",
                "config": thread_config(),
                "cwd": harness.workspace().to_string_lossy(),
                "developerInstructions": "soul",
                "excludeTurns": true,
                "model": "gpt-6-astra",
                "sandbox": "read-only",
            })
        );
        // Codex holds the earlier turns: only the new message is sent,
        // no recap.
        assert_eq!(
            harness.sent("turn/start")[0],
            json!({"threadId": "thr_saved_7", "input": [{"type": "text", "text": "hi"}]})
        );

        // A second turn in the same process needs no resume; changed
        // instructions reconfigure the thread before the turn.
        let messages = [Message::user("hi")];
        let system = ["soul", "memory catalog"];
        let mut request = LlmRequest::new(ExecutionScope::Conversation, &system, &messages, &[]);
        request.conversation = Some(harness.conversation());
        stream(&backend, request).await.0.unwrap();
        let resumes = harness.sent("thread/resume");
        assert_eq!(resumes.len(), 2, "{resumes:?}");
        assert_eq!(
            resumes[1]["developerInstructions"],
            "soul\n\nmemory catalog"
        );
        assert_eq!(harness.sent("turn/start").len(), 2);
        let mut request = LlmRequest::new(ExecutionScope::Conversation, &system, &messages, &[]);
        request.conversation = Some(harness.conversation());
        stream(&backend, request).await.0.unwrap();
        assert_eq!(
            harness.sent("thread/resume").len(),
            2,
            "unchanged: no resume"
        );
        assert_eq!(harness.sent("turn/start").len(), 3);
        harness.runtime.close();
    }

    /// A thread codex no longer has (its rollout gone) is started again,
    /// and the new thread hears the conversation so far.
    #[tokio::test]
    async fn a_thread_codex_no_longer_has_is_started_again_with_a_recap() {
        let harness = Harness::new();
        set_chat_codex_thread(harness.workspace(), "moon", "chat-1", Some("thr_missing")).unwrap();
        let backend = harness.backend();
        let messages = [
            Message::user("earlier"),
            Message::assistant("yes"),
            Message::user("hi"),
        ];
        let mut request = LlmRequest::new(ExecutionScope::Conversation, &[], &messages, &[]);
        request.conversation = Some(harness.conversation());
        let response = stream(&backend, request).await.0.unwrap();
        assert_eq!(response.text, "Hello from the fixture");
        assert_eq!(harness.sent("thread/resume")[0]["threadId"], "thr_missing");
        assert_eq!(harness.sent("thread/start").len(), 1);
        let input = &harness.sent("turn/start")[0]["input"];
        assert_eq!(input.as_array().unwrap().len(), 2, "{input}");
        let recap = input[0]["text"].as_str().unwrap();
        assert!(
            recap.contains("earlier") && recap.contains("yes"),
            "{recap}"
        );
        assert_eq!(input[1], json!({"type": "text", "text": "hi"}));
        assert_eq!(
            harness.remembered_thread().as_deref(),
            Some("thr_fixture_1"),
            "the new thread replaces the lost one"
        );
        harness.runtime.close();
    }

    #[tokio::test]
    async fn a_one_shot_gets_an_ephemeral_thread_and_writes_nothing() {
        let harness = Harness::new();
        let backend = harness.backend();
        let system = ["extract"];
        let messages = [Message::user("hi")];
        let request = LlmRequest::new(ExecutionScope::Subagent, &system, &messages, &[]);
        let response = backend.adapter().unwrap().complete(request).await.unwrap();
        assert_eq!(response.text, "Hello from the fixture");
        assert_eq!(
            response.tokens_used, 6,
            "output 4 + 10 uncached input at 0.2"
        );
        let started = &harness.sent("thread/start")[0];
        assert_eq!(started["ephemeral"], true);
        assert_eq!(started["dynamicTools"], json!([]));
        assert_eq!(started["developerInstructions"], "extract");
        assert_eq!(
            harness.sent("turn/start")[0]["threadId"],
            "thr_fixture_ephemeral"
        );
        assert!(
            !harness.workspace().join("instances").exists(),
            "no chat directory appears for a one-shot"
        );

        // Structured output rides on the turn as its schema.
        let schema = json!({"type": "object", "properties": {"a": {"type": "string"}}});
        let messages = [Message::user("json please")];
        let mut request = LlmRequest::new(ExecutionScope::Subagent, &system, &messages, &[]);
        request.json_schema = Some(&schema);
        let response = backend.adapter().unwrap().complete(request).await.unwrap();
        assert_eq!(response.text, "{}");
        assert_eq!(response.tokens_used, 14, "structured: input + output");
        let turns = harness.sent("turn/start");
        assert_eq!(turns[1]["outputSchema"], schema);
        assert_eq!(
            turns[1]["input"],
            json!([{"type": "text", "text": "json please"}])
        );
        harness.runtime.close();
    }

    /// The bridge: codex asks for a tool, the adapter returns the call to
    /// the loop with the turn open, the loop comes back with the result in
    /// the next request, and the adapter answers codex and reads on until
    /// the next call or the end. Every result reaches codex as content
    /// items on the request it belongs to.
    #[tokio::test]
    async fn tool_outputs_answer_the_pending_call_and_the_turn_goes_on() {
        let harness = Harness::new();
        let backend = harness.backend();
        let tools = [tool("shot"), tool("search")];
        let mut messages = vec![Message::user("look it up")];
        let mut request = LlmRequest::new(ExecutionScope::Conversation, &[], &messages, &tools);
        request.conversation = Some(harness.conversation());
        let (result, events) = stream(&backend, request).await;
        let first = result.unwrap();
        assert_eq!(first.text, "Looking.");
        assert_eq!(first.stop_reason, StopReason::ToolCalls);
        assert_eq!(first.tool_calls.len(), 1, "codex asks one call at a time");
        assert_eq!(first.tool_calls[0].id, "call1");
        assert_eq!(first.tool_calls[0].name, "shot");
        assert_eq!(first.usage.input_tokens, 10);
        assert!(events.iter().any(|event| matches!(event, LlmEvent::ToolCallStarted { id, name } if id == "call1" && name == "shot")));
        assert!(events.iter().any(|event| matches!(event, LlmEvent::ToolArgumentsDelta { id, delta, .. } if id == "call1" && delta == "{}")));
        assert!(harness.answers().is_empty(), "nothing answered yet");

        // The loop ran the tool: its result answers call1.
        messages.push(Message::Assistant {
            content: vec![
                ContentBlock::text("Looking."),
                ContentBlock::ToolCall {
                    id: "call1".into(),
                    name: "shot".into(),
                    arguments: json!({}),
                },
            ],
        });
        messages.push(Message::User {
            content: vec![ContentBlock::tool_output(
                "call1".into(),
                r#"[{"type":"text","text":"captured"}]"#.into(),
                false,
            )],
        });
        let mut request = LlmRequest::new(ExecutionScope::Conversation, &[], &messages, &tools);
        request.conversation = Some(harness.conversation());
        let (result, events) = stream(&backend, request).await;
        let second = result.unwrap();
        assert_eq!(second.text, "", "no text between the two calls");
        assert_eq!(second.stop_reason, StopReason::ToolCalls);
        assert_eq!(second.tool_calls[0].id, "call2");
        assert_eq!(second.tool_calls[0].name, "search");
        assert_eq!(second.tool_calls[0].arguments, json!({"q": "rust"}));
        assert!(
            events.iter().any(
                |event| matches!(event, LlmEvent::ToolCallStarted { id, .. } if id == "call2")
            )
        );
        let answers = harness.answers();
        assert_eq!(answers.len(), 1, "{answers:?}");
        assert_eq!(answers[0].0, json!(40));
        assert_eq!(
            answers[0].1,
            Ok(
                json!({"contentItems": [{"type": "inputText", "text": "captured"}], "success": true})
            )
        );
        assert_eq!(harness.sent("turn/start").len(), 1, "the same turn goes on");

        messages.push(Message::Assistant {
            content: vec![ContentBlock::ToolCall {
                id: "call2".into(),
                name: "search".into(),
                arguments: json!({"q": "rust"}),
            }],
        });
        messages.push(Message::User {
            content: vec![ContentBlock::tool_output(
                "call2".into(),
                "found".into(),
                false,
            )],
        });
        let mut request = LlmRequest::new(ExecutionScope::Conversation, &[], &messages, &tools);
        request.conversation = Some(harness.conversation());
        let (result, events) = stream(&backend, request).await;
        let third = result.unwrap();
        assert_eq!(third.text, "done");
        assert_eq!(text_of(&events), "done");
        assert_eq!(third.stop_reason, StopReason::Complete);
        assert!(third.tool_calls.is_empty());
        assert_eq!(third.usage.input_tokens, 20);
        assert_eq!(third.usage.output_tokens, 1);
        let answers = harness.answers();
        assert_eq!(answers.len(), 2, "{answers:?}");
        assert_eq!(answers[1].0, json!(41));
        assert_eq!(
            answers[1].1,
            Ok(json!({"contentItems": [{"type": "inputText", "text": "found"}], "success": true}))
        );
        assert_eq!(harness.sent("turn/start").len(), 1);
        assert!(harness.sent("turn/interrupt").is_empty());
        harness.runtime.close();
    }

    /// The loop never came back with the result (a cancelled loop, a
    /// crashed tool): the next message for the chat interrupts the turn
    /// left waiting, refuses the call codex still waits on, and starts a
    /// fresh turn.
    #[tokio::test]
    async fn a_new_message_interrupts_a_turn_left_waiting_on_a_tool() {
        let harness = Harness::new();
        let backend = harness.backend();
        let tools = [tool("shot"), tool("search")];
        let messages = [Message::user("look it up")];
        let mut request = LlmRequest::new(ExecutionScope::Conversation, &[], &messages, &tools);
        request.conversation = Some(harness.conversation());
        stream(&backend, request).await.0.unwrap();

        let messages = [
            Message::user("look it up"),
            Message::assistant("Looking."),
            Message::user("hi"),
        ];
        let mut request = LlmRequest::new(ExecutionScope::Conversation, &[], &messages, &tools);
        request.conversation = Some(harness.conversation());
        let response = stream(&backend, request).await.0.unwrap();
        assert_eq!(response.text, "Hello from the fixture");
        assert_eq!(
            harness.sent("turn/interrupt"),
            [json!({"threadId": "thr_fixture_1", "turnId": "turn_fixture_4"})]
        );
        // The abandoned call is refused, never answered; whatever the old
        // turn asks after that (the fake plays on) is refused the same way.
        let answers = harness.answers();
        assert!(!answers.is_empty(), "{answers:?}");
        assert_eq!(answers[0].0, json!(40));
        assert!(
            answers.iter().all(|(_, outcome)| outcome.is_err()),
            "{answers:?}"
        );
        let turns = harness.sent("turn/start");
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[1]["input"], json!([{"type": "text", "text": "hi"}]));
        harness.runtime.close();
    }

    #[tokio::test]
    async fn cancellation_interrupts_the_turn() {
        let harness = Harness::new();
        let backend = harness.backend();
        let messages = [Message::user("hang")];
        let mut request = LlmRequest::new(ExecutionScope::Conversation, &[], &messages, &[]);
        request.conversation = Some(harness.conversation());
        let token = request.cancellation.clone();
        let events = Mutex::new(Vec::new());
        let sink = |event: LlmEvent| {
            if matches!(&event, LlmEvent::TextDelta(_)) {
                token.cancel();
            }
            events.lock().unwrap().push(event);
        };
        let result = tokio::time::timeout(
            Duration::from_secs(20),
            backend.adapter().unwrap().stream(request, &sink),
        )
        .await
        .expect("cancellation ends the turn");
        assert!(matches!(result, Err(LlmError::Cancelled)), "{result:?}");
        assert_eq!(
            harness.sent("turn/interrupt"),
            [json!({"threadId": "thr_fixture_1", "turnId": "turn_fixture_13"})]
        );
        // The thread is free for the next turn.
        let messages = [Message::user("hang"), Message::user("hi")];
        let mut request = LlmRequest::new(ExecutionScope::Conversation, &[], &messages, &[]);
        request.conversation = Some(harness.conversation());
        let response = stream(&backend, request).await.0.unwrap();
        assert_eq!(response.text, "Hello from the fixture");
        assert_eq!(
            harness.sent("turn/interrupt").len(),
            1,
            "nothing left to interrupt"
        );
        harness.runtime.close();
    }

    /// A child that dies mid-turn: the turn fails with a transport error
    /// naming the exit, the conversation is untouched, and the next turn
    /// resumes the same thread in the replaced child.
    #[tokio::test]
    async fn a_crash_mid_turn_is_a_transport_error_and_the_next_turn_resumes_the_thread() {
        let harness = Harness::new();
        let backend = harness.backend();
        let messages = [Message::user("crash")];
        let mut request = LlmRequest::new(ExecutionScope::Conversation, &[], &messages, &[]);
        request.conversation = Some(harness.conversation());
        let (result, events) = stream(&backend, request).await;
        assert!(
            matches!(&result, Err(LlmError::Transport(reason)) if reason.contains("exit status: 3")),
            "{result:?}"
        );
        assert_eq!(
            text_of(&events),
            "Half",
            "what streamed before the crash reached the sink"
        );
        assert_eq!(
            harness.remembered_thread().as_deref(),
            Some("thr_fixture_1")
        );
        let first_generation = harness.runtime.app_server().await.unwrap().generation();

        let messages = [Message::user("crash"), Message::user("hi")];
        let mut request = LlmRequest::new(ExecutionScope::Conversation, &[], &messages, &[]);
        request.conversation = Some(harness.conversation());
        let response = stream(&backend, request).await.0.unwrap();
        assert_eq!(response.text, "Hello from the fixture");
        assert!(
            harness.runtime.app_server().await.unwrap().generation() > first_generation,
            "a fresh child served the second turn"
        );
        assert_eq!(
            harness.sent("thread/start").len(),
            1,
            "the thread is not started twice"
        );
        let resumes = harness.sent("thread/resume");
        assert_eq!(resumes.len(), 1, "{resumes:?}");
        assert_eq!(resumes[0]["threadId"], "thr_fixture_1");
        assert_eq!(
            resumes[0]["cwd"],
            json!(harness.workspace().to_string_lossy()),
            "resumed with the conversation's settings"
        );
        harness.runtime.close();
    }

    /// The child dies while the loop runs the tool: the result has nowhere
    /// to go, the turn is reported lost, and the next turn resumes the
    /// thread in the replaced child instead of waiting on the old one.
    #[tokio::test]
    async fn a_crash_while_a_turn_waits_on_a_tool_loses_that_turn_only() {
        let harness = Harness::new();
        let backend = harness.backend();
        let tools = [tool("shot"), tool("search")];
        let messages = [Message::user("look it up")];
        let mut request = LlmRequest::new(ExecutionScope::Conversation, &[], &messages, &tools);
        request.conversation = Some(harness.conversation());
        let first = stream(&backend, request).await.0.unwrap();
        assert_eq!(first.stop_reason, StopReason::ToolCalls);
        harness
            .runtime
            .app_server()
            .await
            .unwrap()
            .request("fake/crash", json!({}))
            .await
            .unwrap_err();

        let messages = [
            Message::user("look it up"),
            Message::Assistant {
                content: vec![ContentBlock::ToolCall {
                    id: "call1".into(),
                    name: "shot".into(),
                    arguments: json!({}),
                }],
            },
            Message::User {
                content: vec![ContentBlock::tool_output(
                    "call1".into(),
                    "captured".into(),
                    false,
                )],
            },
        ];
        let mut request = LlmRequest::new(ExecutionScope::Conversation, &[], &messages, &tools);
        request.conversation = Some(harness.conversation());
        let started = std::time::Instant::now();
        let result = stream(&backend, request).await.0;
        // The dead child refuses the answer, or a replacement never heard
        // of the turn: either way a transport error, at once.
        assert!(
            matches!(&result, Err(LlmError::Transport(reason)) if reason.contains("exit status: 3") || reason.contains("restarted")),
            "{result:?}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "not held for the idle timeout"
        );

        let messages = [Message::user("look it up"), Message::user("hi")];
        let mut request = LlmRequest::new(ExecutionScope::Conversation, &[], &messages, &tools);
        request.conversation = Some(harness.conversation());
        let response = stream(&backend, request).await.0.unwrap();
        assert_eq!(response.text, "Hello from the fixture");
        assert_eq!(
            harness.sent("thread/resume").len(),
            1,
            "resumed in the new child"
        );
        assert_eq!(harness.sent("thread/start").len(), 1);
        harness.runtime.close();
    }

    #[tokio::test]
    async fn without_a_login_nothing_is_started_and_the_error_says_to_log_in() {
        let harness = Harness::with_env(&[(fake::ACCOUNT_ENV, "none")]);
        let backend = harness.backend();
        let messages = [Message::user("hi")];
        let mut request = LlmRequest::new(ExecutionScope::Conversation, &[], &messages, &[]);
        request.conversation = Some(harness.conversation());
        let (result, events) = stream(&backend, request).await;
        assert!(
            matches!(&result, Err(LlmError::SetupRequired(message)) if message.contains("Codex login required")),
            "{result:?}"
        );
        assert!(events.is_empty());
        assert_eq!(
            harness.methods(),
            ["initialize", "initialized", "account/read"],
            "no thread and no turn without a login"
        );
        assert_eq!(harness.remembered_thread(), None);
        harness.runtime.close();
    }

    /// Codex may ask to run a command or change a file; the answer is
    /// always no, and the turn goes on with that answer.
    #[tokio::test]
    async fn an_approval_request_is_declined_never_granted() {
        let harness = Harness::new();
        let backend = harness.backend();
        let messages = [Message::user("ask approval")];
        let mut request = LlmRequest::new(ExecutionScope::Conversation, &[], &messages, &[]);
        request.conversation = Some(harness.conversation());
        let response = stream(&backend, request).await.0.unwrap();
        assert_eq!(response.text, "declined");
        assert_eq!(response.stop_reason, StopReason::Complete);
        let answers = harness.answers();
        assert_eq!(answers.len(), 1, "{answers:?}");
        assert_eq!(answers[0].0, json!(60));
        assert_eq!(answers[0].1, Ok(json!({"decision": "decline"})));
        let wire = std::fs::read_to_string(&harness.log).unwrap();
        assert!(!wire.contains("accept"), "{wire}");
        harness.runtime.close();
    }

    /// A command codex runs on its own, outside Nolune's tools, is the
    /// sandbox not holding: the turn is interrupted and fails loudly.
    #[tokio::test]
    async fn a_command_codex_runs_itself_fails_the_turn() {
        let harness = Harness::new();
        let backend = harness.backend();
        let messages = [Message::user("run a command")];
        let mut request = LlmRequest::new(ExecutionScope::Conversation, &[], &messages, &[]);
        request.conversation = Some(harness.conversation());
        let (result, events) = stream(&backend, request).await;
        assert!(
            matches!(&result, Err(LlmError::InvalidResponse(reason)) if reason.contains("ls -la") && reason.contains("commandExecution")),
            "{result:?}"
        );
        assert!(!text_of(&events).contains("I ran it"));
        assert_eq!(
            harness.sent("turn/interrupt"),
            [json!({"threadId": "thr_fixture_1", "turnId": "turn_fixture_17"})]
        );
        harness.runtime.close();
    }

    /// A malformed `item/tool/call` (no tool name, arguments that are not
    /// an object, no call id) is refused on the wire, the turn is
    /// interrupted, and the loop gets `InvalidResponse`: nothing runs.
    #[tokio::test]
    async fn a_malformed_tool_call_is_refused_and_ends_the_turn() {
        for (prompt, ask_id) in [
            ("bad tool name", 50),
            ("bad arguments", 51),
            ("bad call id", 52),
        ] {
            let harness = Harness::new();
            let backend = harness.backend();
            let tools = [tool("search")];
            let messages = [Message::user(prompt)];
            let mut request = LlmRequest::new(ExecutionScope::Conversation, &[], &messages, &tools);
            request.conversation = Some(harness.conversation());
            let (result, events) = stream(&backend, request).await;
            assert!(
                matches!(&result, Err(LlmError::InvalidResponse(_))),
                "{prompt}: {result:?}"
            );
            assert!(
                !events
                    .iter()
                    .any(|event| matches!(event, LlmEvent::ToolCallStarted { .. })),
                "{prompt}: a call that cannot run is not announced"
            );
            let answers = harness.answers();
            assert_eq!(answers.len(), 1, "{prompt}: {answers:?}");
            assert_eq!(answers[0].0, json!(ask_id), "{prompt}");
            assert!(answers[0].1.is_err(), "{prompt}: refused, not answered");
            assert_eq!(harness.sent("turn/interrupt").len(), 1, "{prompt}");
            harness.runtime.close();
        }
    }

    /// Dynamic tools are fixed when a thread starts; a conversation whose
    /// tool set changed continues in a fresh thread that hears the
    /// conversation so far.
    #[tokio::test]
    async fn a_changed_tool_set_starts_a_fresh_thread_with_the_conversation_so_far() {
        let harness = Harness::new();
        let backend = harness.backend();
        let tools = [tool("search")];
        let messages = [Message::user("hi")];
        let mut request = LlmRequest::new(ExecutionScope::Conversation, &[], &messages, &tools);
        request.conversation = Some(harness.conversation());
        stream(&backend, request).await.0.unwrap();

        let tools = [tool("search"), tool("shot")];
        let messages = [
            Message::user("hi"),
            Message::assistant("Hello from the fixture"),
            Message::user("hi"),
        ];
        let mut request = LlmRequest::new(ExecutionScope::Conversation, &[], &messages, &tools);
        request.conversation = Some(harness.conversation());
        stream(&backend, request).await.0.unwrap();
        let started = harness.sent("thread/start");
        assert_eq!(started.len(), 2, "{started:?}");
        assert_eq!(started[1]["dynamicTools"].as_array().unwrap().len(), 2);
        let turns = harness.sent("turn/start");
        assert_eq!(
            turns[1]["input"].as_array().unwrap().len(),
            2,
            "recap + message"
        );
        assert!(
            turns[1]["input"][0]["text"]
                .as_str()
                .unwrap()
                .contains("Hello from the fixture")
        );

        // The same set again, in any order, keeps the thread.
        let tools = [tool("shot"), tool("search")];
        let mut request = LlmRequest::new(ExecutionScope::Conversation, &[], &messages, &tools);
        request.conversation = Some(harness.conversation());
        stream(&backend, request).await.0.unwrap();
        assert_eq!(harness.sent("thread/start").len(), 2);
        assert_eq!(harness.sent("turn/start").len(), 3);
        harness.runtime.close();
    }

    #[tokio::test]
    async fn a_turn_that_ends_out_of_protocol_is_an_invalid_response() {
        let harness = Harness::new();
        let backend = harness.backend();
        let messages = [Message::user("weird")];
        let mut request = LlmRequest::new(ExecutionScope::Conversation, &[], &messages, &[]);
        request.conversation = Some(harness.conversation());
        let result = stream(&backend, request).await.0;
        assert!(
            matches!(&result, Err(LlmError::InvalidResponse(reason)) if reason.contains("weird")),
            "{result:?}"
        );
        harness.runtime.close();
    }

    /// Needs the real binary at the pin and a ChatGPT login; run with
    /// `NOLUNE_CODEX_LIVE=1 cargo test ... -- --ignored`. One ephemeral
    /// thread and one short turn with one dynamic tool: the model has to
    /// call it, Nolune answers, and the model repeats the answer. Nothing
    /// is written under `~/.codex`; the login is read by codex alone.
    #[cfg(unix)]
    #[tokio::test]
    #[ignore]
    async fn live_turn_round_trips_a_tool_call_through_the_real_app_server() {
        let found = super::super::discovery::discover()
            .await
            .expect("codex at the pin");
        let runtime = Runtime::for_launch(super::super::process::Launch::new(found.path));
        let mut config = Config::default();
        config.llm.seed_presets(LlmProvider::Codex);
        let mut backend =
            LlmBackend::for_preset(&config, reqwest::Client::new(), "codex-luna").unwrap();
        backend.codex = runtime.clone();
        let tools = [ToolDefinition {
            name: "lookup_note".into(),
            description: "Returns the note the user saved under a name.".into(),
            parameters: json!({
                "type": "object",
                "properties": {"name": {"type": "string"}},
                "required": ["name"],
            }),
        }];
        let system = [
            "You are a terse assistant in an integration test. When asked, call the lookup_note tool and then answer with exactly what it returned.",
        ];
        let mut messages = vec![Message::user(
            "Call lookup_note with name \"garden\", then reply with exactly the text it returned and nothing else.",
        )];
        let request = LlmRequest::new(ExecutionScope::Subagent, &system, &messages, &tools);
        let first = tokio::time::timeout(
            Duration::from_secs(120),
            backend.adapter().unwrap().complete(request),
        )
        .await
        .expect("a live turn ends")
        .expect("the turn completes");
        assert_eq!(first.stop_reason, StopReason::ToolCalls, "{first:?}");
        assert_eq!(first.tool_calls.len(), 1, "{first:?}");
        assert_eq!(first.tool_calls[0].name, "lookup_note");
        assert_eq!(first.tool_calls[0].arguments["name"], "garden");
        let call_id = first.tool_calls[0].id.clone();
        assert!(!call_id.is_empty());

        messages.push(Message::Assistant {
            content: vec![ContentBlock::ToolCall {
                id: call_id.clone(),
                name: "lookup_note".into(),
                arguments: first.tool_calls[0].arguments.clone(),
            }],
        });
        messages.push(Message::User {
            content: vec![ContentBlock::tool_output(
                call_id,
                "the roses are blue this year".into(),
                false,
            )],
        });
        let request = LlmRequest::new(ExecutionScope::Subagent, &system, &messages, &tools);
        let second = tokio::time::timeout(
            Duration::from_secs(120),
            backend.adapter().unwrap().complete(request),
        )
        .await
        .expect("a live turn ends")
        .expect("the turn completes");
        assert_eq!(second.stop_reason, StopReason::Complete, "{second:?}");
        assert!(
            second.text.to_lowercase().contains("roses"),
            "the model repeats the tool result: {second:?}"
        );
        assert!(
            second.usage.input_tokens > 0 && second.usage.output_tokens > 0,
            "{second:?}"
        );
        runtime.close();
    }

    // ── Pure wire shapes ─────────────────────────────────────────────────

    #[test]
    fn turn_errors_map_to_typed_variants() {
        let error = |info: Value| json!({"message": "what codex said", "codexErrorInfo": info, "additionalDetails": null});
        assert!(matches!(
            map_turn_error(&error(json!("usageLimitExceeded"))),
            LlmError::RateLimited {
                retry_after: None,
                ..
            }
        ));
        for info in ["rateLimitExceeded", "serverOverloaded"] {
            assert!(
                matches!(
                    map_turn_error(&error(json!(info))),
                    LlmError::RateLimited { .. }
                ),
                "{info}"
            );
        }
        for info in ["contextWindowExceeded", "sessionBudgetExceeded"] {
            assert!(
                matches!(
                    map_turn_error(&error(json!(info))),
                    LlmError::ContextLength(_)
                ),
                "{info}"
            );
        }
        assert!(matches!(
            map_turn_error(&error(json!("unauthorized"))),
            LlmError::Authentication(_)
        ));
        assert!(matches!(
            map_turn_error(&error(json!("internalServerError"))),
            LlmError::Http { status: 500, .. }
        ));
        assert!(matches!(
            map_turn_error(&error(json!("badRequest"))),
            LlmError::Http { status: 400, .. }
        ));
        assert!(matches!(
            map_turn_error(&error(json!("interrupted"))),
            LlmError::Cancelled
        ));
        // The transport variants carry the upstream status when there was one.
        for variant in [
            "httpConnectionFailed",
            "responseStreamConnectionFailed",
            "responseStreamDisconnected",
            "responseTooManyFailedAttempts",
        ] {
            assert!(
                matches!(
                    map_turn_error(&error(json!({variant: {"httpStatusCode": 429}}))),
                    LlmError::RateLimited { .. }
                ),
                "{variant} 429"
            );
            assert!(
                matches!(
                    map_turn_error(&error(json!({variant: {"httpStatusCode": 401}}))),
                    LlmError::Authentication(_)
                ),
                "{variant} 401"
            );
            assert!(
                matches!(
                    map_turn_error(&error(json!({variant: {"httpStatusCode": 418}}))),
                    LlmError::Http { status: 418, .. }
                ),
                "{variant} 418"
            );
            assert!(
                matches!(
                    map_turn_error(&error(json!({variant: {"httpStatusCode": null}}))),
                    LlmError::Transport(_)
                ),
                "{variant} without a status"
            );
        }
        // Everything else is the turn failing for a reason of its own.
        for info in [
            json!("other"),
            json!("sandboxError"),
            json!("neverHeardOf"),
            Value::Null,
        ] {
            let mapped = map_turn_error(&error(info.clone()));
            assert!(
                matches!(&mapped, LlmError::Transport(message) if message.contains("what codex said")),
                "{info}: {mapped:?}"
            );
        }
        // Keys in the text are redacted like any other provider error.
        let leaky = json!({"message": "rejected sk-ant-api03-ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ", "codexErrorInfo": "other"});
        assert!(
            !map_turn_error(&leaky)
                .to_string()
                .contains("sk-ant-api03-ABC")
        );
    }

    #[test]
    fn the_turn_input_is_the_trailing_user_content_or_a_recap_on_a_fresh_thread() {
        let messages = [
            Message::user("first"),
            Message::Assistant {
                content: vec![
                    ContentBlock::text("Looking."),
                    ContentBlock::ToolCall {
                        id: "call1".into(),
                        name: "search".into(),
                        arguments: json!({"q": "rust"}),
                    },
                ],
            },
            Message::User {
                content: vec![ContentBlock::tool_output(
                    "call1".into(),
                    "found".into(),
                    false,
                )],
            },
            Message::assistant("It is a language."),
            Message::User {
                content: vec![
                    ContentBlock::ContextSummary {
                        content: "we talked about rust".into(),
                    },
                    ContentBlock::text("second"),
                ],
            },
        ];
        // An attached thread hears only what codex has not seen.
        assert_eq!(
            turn_input(&messages, false),
            json!([
                {"type": "text", "text": "Conversation summary:\nwe talked about rust"},
                {"type": "text", "text": "second"}
            ])
            .as_array()
            .unwrap()
            .as_slice()
        );
        // A fresh thread hears the conversation so far first, calls and
        // results included.
        let fresh = turn_input(&messages, true);
        assert_eq!(fresh.len(), 3, "{fresh:?}");
        let recap = fresh[0]["text"].as_str().unwrap();
        assert!(recap.starts_with("Earlier in this conversation"), "{recap}");
        for expected in [
            "user: first",
            "assistant: Looking.",
            "search({\"q\":\"rust\"})",
            "call1: found",
            "assistant: It is a language.",
        ] {
            assert!(
                recap.contains(expected),
                "{expected:?} missing from {recap}"
            );
        }
        assert!(
            !recap.contains("second"),
            "the new message is not in the recap"
        );
        assert_eq!(
            fresh[1]["text"],
            "Conversation summary:\nwe talked about rust"
        );
        assert_eq!(fresh[2]["text"], "second");
        // A fresh thread with nothing before the message needs no recap.
        assert_eq!(turn_input(&messages[4..], true).len(), 2);
        // A tool result that answers no open call is told as text.
        let orphan = [
            Message::user("look"),
            Message::assistant("Looking."),
            Message::User {
                content: vec![ContentBlock::tool_output(
                    "call9".into(),
                    "late".into(),
                    false,
                )],
            },
        ];
        let input = turn_input(&orphan, false);
        assert_eq!(input.len(), 1, "{input:?}");
        let text = input[0]["text"].as_str().unwrap();
        assert!(text.contains("call9") && text.contains("late"), "{text}");
        // Empty stays empty.
        assert!(turn_input(&[], true).is_empty());
        assert!(turn_input(&[], false).is_empty());
    }

    #[test]
    fn tool_outputs_become_content_items() {
        assert_eq!(
            tool_output_items(&ToolOutputContent::Text("found".into())),
            [json!({"type": "inputText", "text": "found"})]
        );
        let blocks = ToolOutputContent::Blocks(vec![
            ContentBlock::text("captured"),
            ContentBlock::Image {
                source: ImageSource::Base64 {
                    media_type: "image/png".into(),
                    data: "abc".into(),
                },
                resource_provenance: None,
            },
            ContentBlock::Image {
                source: ImageSource::Url {
                    url: "https://example.test/a.png".into(),
                },
                resource_provenance: None,
            },
        ]);
        assert_eq!(
            tool_output_items(&blocks),
            [
                json!({"type": "inputText", "text": "captured"}),
                json!({"type": "inputImage", "imageUrl": "data:image/png;base64,abc"}),
                json!({"type": "inputImage", "imageUrl": "https://example.test/a.png"}),
            ]
        );
        let legacy = ToolOutputContent::Legacy(json!([{"type": "expense", "amount": 1}]));
        assert_eq!(
            tool_output_items(&legacy),
            [json!({"type": "inputText", "text": "[{\"amount\":1,\"type\":\"expense\"}]"})]
        );
        // Nothing at all is still one (empty) text item: codex wants a list.
        assert_eq!(
            tool_output_items(&ToolOutputContent::Blocks(vec![])),
            [json!({"type": "inputText", "text": ""})]
        );
    }

    #[test]
    fn the_thread_shape_carries_nolunes_tools_and_switches_codexs_own_off() {
        let tools = dynamic_tools(&[tool("search")]);
        assert_eq!(
            tools,
            [json!({
                "type": "function",
                "name": "search",
                "description": "test",
                "inputSchema": {"type": "object", "properties": {"q": {"type": "string"}}},
            })]
        );
        let config = thread_config();
        assert_eq!(config["project_doc_max_bytes"], 0);
        assert_eq!(config["mcp_servers"], json!({}));
        assert!(
            config["features"]
                .as_object()
                .unwrap()
                .values()
                .all(|flag| *flag == false),
            "{config}"
        );
        assert!(
            config["tools"]
                .as_object()
                .unwrap()
                .values()
                .all(|flag| *flag == false),
            "{config}"
        );
        assert_eq!(developer_instructions(&["a", "", "b"], None), "a\n\nb");
        let with_schema = developer_instructions(&["a"], Some(&json!({"type": "object"})));
        assert!(with_schema.starts_with("a\n\n"), "{with_schema}");
        assert!(
            with_schema.contains("{\"type\":\"object\"}"),
            "{with_schema}"
        );
        assert_eq!(developer_instructions(&[], None), "");
    }

    #[test]
    fn usage_is_read_from_the_last_breakdown_and_normalized_like_openais() {
        let usage = parse_usage(&json!({
            "inputTokens": 100, "cachedInputTokens": 40, "outputTokens": 10,
            "reasoningOutputTokens": 5, "totalTokens": 110, "cacheWriteInputTokens": 0
        }));
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.cache_read_tokens, 40);
        assert_eq!(usage.output_tokens, 10);
        assert_eq!(usage.cache_write_tokens, 0);
        assert_eq!(usage.cost, None);
        // 10 output + 60 uncached * 0.2 + 40 cached * 0.1 = 26
        assert_eq!(normalized_tokens(&usage, false), 26);
        assert_eq!(normalized_tokens(&usage, true), 110);
        assert_eq!(parse_usage(&json!({})).input_tokens, 0);
    }
}
