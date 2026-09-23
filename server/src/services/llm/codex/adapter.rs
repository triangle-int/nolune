//! The Codex provider (#27): Nolune conversations as codex app-server
//! threads, turns streamed into the provider-neutral events, and Nolune's
//! tools bridged through `dynamicTools`.
//!
//! One thread per conversation. The first turn of a chat starts a thread
//! (`thread/start`) and the id is kept in the chat's `meta.json`; after a
//! restart the next turn resumes it (`thread/resume`). The thread is
//! started read-only (`sandbox: read-only`), with no approvals
//! (`approvalPolicy: never`), with codex's own shell, file, browser and
//! plugin surfaces switched off, with every MCP server codex's effective
//! config lists disabled by name (the app-server merges the thread's
//! config overrides per key, so an empty `mcp_servers` table disables
//! nothing), in an empty directory of Nolune's own, and with Nolune's tool
//! definitions as `dynamicTools`: the only tools the model can call. Before
//! every turn the thread's MCP servers are listed, and a thread any server
//! stands for is refused before its turn begins. A one-shot run (a title,
//! a memory extraction, a connection test) gets an ephemeral thread,
//! unsubscribed once its turn is over so the app-server unloads it.
//!
//! A turn sends the trailing user content as `turn/start` input (codex
//! keeps the earlier turns itself) and reads the stream: agent message
//! deltas become `TextDelta`, `thread/tokenUsage/updated` becomes `Usage`,
//! `turn/completed` ends it. When codex asks `item/tool/call`, the adapter
//! answers nothing itself: it hands the call back to the agent loop as a
//! `ToolCall` and returns with `StopReason::ToolCalls`, the turn left open.
//! The loop runs the tool through Nolune's capability and approval layer
//! and calls again with the result; the adapter finds the open turn by the
//! call id, answers codex, and reads on. While the turn waits, a task of
//! its own keeps draining the supervisor's event stream and queues only
//! this thread's events, so the other threads' streams never overrun it.
//! Any approval codex asks for is declined, and a command, file change,
//! web search, image read or MCP server codex runs on its own fails the
//! turn: nothing executes but Nolune's tools.
//!
//! Cancellation sends `turn/interrupt`; a child that dies mid-turn fails
//! the turn with a transport error and the next turn resumes the thread in
//! the replaced child; a failed turn maps its `codexErrorInfo` to the typed
//! variants the callers act on.

use std::collections::HashMap;
use std::hash::{Hash as _, Hasher as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::future::BoxFuture;
use serde_json::{Value, json};
use tokio::sync::{broadcast, mpsc};
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
use super::runtime::Runtime;

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

    /// Let go of a thread in this process: the next turn of its
    /// conversation attaches it again, with the overrides applied anew and
    /// its MCP servers checked, instead of carrying on in a thread that
    /// ran something of its own.
    fn forget_thread(&mut self, thread_id: &str) {
        self.by_conversation
            .retain(|_, state| state.thread_id != thread_id);
        self.take_open(thread_id);
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
    events: ThreadEvents,
    /// The calls handed to the agent loop, by call id: the app-server's
    /// request id to answer with.
    calls: HashMap<String, Value>,
    /// The child the turn runs in; a replacement child never heard of it.
    generation: u64,
    /// A one-shot's thread, to release once the turn ends.
    ephemeral: bool,
}

/// What a turn hears: the child's events that concern its thread, and
/// whether the receiver that carried them fell behind.
enum TurnEvent {
    Incoming(Incoming),
    /// The forwarder missed this many events of the supervisor's stream;
    /// a request the app-server waits on may be among them.
    Lagged(u64),
}

/// A turn's ear on the child. A task of its own drains the
/// supervisor's broadcast and queues only this thread's events (and the
/// child's starts and exits), so a turn parked on a tool call while the
/// agent loop runs it is never overrun by what the other threads stream
/// meanwhile: the broadcast keeps a fixed number of events for every
/// subscriber, and a parked receiver would be told it lagged once another
/// turn streamed that many. The queue is unbounded: codex sends a thread
/// nothing while it waits on a tool answer, so what queues up is what its
/// own turn streams faster than it is read, which the reader drains, and
/// never a budget a slow moment could exhaust.
struct ThreadEvents {
    queue: mpsc::UnboundedReceiver<TurnEvent>,
}

impl ThreadEvents {
    /// Start forwarding `thread_id`'s events from `events`, which was
    /// subscribed before the thread was attached, so nothing the attach
    /// itself provoked is missed.
    fn start(events: broadcast::Receiver<Incoming>, thread_id: String) -> Self {
        let (tx, queue) = mpsc::unbounded_channel();
        tokio::spawn(forward(events, thread_id, tx));
        Self { queue }
    }

    /// The next event, or `None` once the child's stream is closed.
    async fn recv(&mut self) -> Option<TurnEvent> {
        self.queue.recv().await
    }
}

/// Whether an event is about `thread_id`; a start or an exit of the
/// child concerns every thread.
fn concerns(incoming: &Incoming, thread_id: &str) -> bool {
    match incoming {
        Incoming::Notification { params, .. } | Incoming::Request { params, .. } => {
            params["threadId"] == thread_id
        }
        Incoming::Started { .. } | Incoming::Exited { .. } => true,
    }
}

/// Drain `events` into `queue` for as long as the turn holds the other
/// end. This task does nothing but receive and forward, so it falls behind
/// the broadcast only when it is not scheduled for a whole buffer's worth
/// of events, and reports that as [`TurnEvent::Lagged`] when it reads on.
async fn forward(
    mut events: broadcast::Receiver<Incoming>,
    thread_id: String,
    queue: mpsc::UnboundedSender<TurnEvent>,
) {
    loop {
        let next = tokio::select! {
            biased;
            _ = queue.closed() => return,
            next = events.recv() => next,
        };
        let event = match next {
            Ok(incoming) if concerns(&incoming, &thread_id) => TurnEvent::Incoming(incoming),
            Ok(_other_thread) => continue,
            Err(broadcast::error::RecvError::Lagged(missed)) => TurnEvent::Lagged(missed),
            Err(broadcast::error::RecvError::Closed) => return,
        };
        if queue.send(event).is_err() {
            return;
        }
    }
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

/// The config overrides every thread is started or resumed with: no
/// project docs, every MCP server the effective config lists disabled by
/// name, and codex's own tool surface switched off, so the model has
/// Nolune's tools and nothing else. The app-server deep-merges the
/// overrides into the user's `config.toml`, per key: `mcp_servers = {}`
/// removes nothing (verified against the pinned release: every configured
/// server still started for the thread), `mcp_servers.<name>.enabled =
/// false` does.
///
/// Each switch was checked against the pinned release by the tool list it
/// sends the model. Web search is the top-level `web_search` mode:
/// `tools.web_search = false` parses and is dropped, which leaves the
/// cached search on. `goals` and `tools.experimental_request_user_input`
/// are on by default and each adds tools. The skills catalog is a block
/// of instructions (`skills.include_instructions`) and, on a thread without
/// an environment, a `skills` tool namespace (`orchestrator.skills`).
/// `apply_patch` follows the model, not a flag: [`NO_ENVIRONMENTS`] is what
/// takes it away.
pub(super) fn thread_config(mcp_servers: &[String]) -> Value {
    let disabled: serde_json::Map<String, Value> = mcp_servers
        .iter()
        .map(|name| (name.clone(), json!({"enabled": false})))
        .collect();
    json!({
        "project_doc_max_bytes": 0,
        "mcp_servers": disabled,
        "web_search": "disabled",
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
            "goals": false,
        },
        "tools": {
            "experimental_request_user_input": {"enabled": false},
        },
        "skills": {
            "include_instructions": false,
        },
        "orchestrator": {
            "skills": {"enabled": false},
        },
    })
}

/// The `environments` every thread starts with and every turn carries: none,
/// so codex has no workspace to act in and offers no tool that needs one
/// (`apply_patch` above all, which every catalog model gets otherwise). A
/// thread's own list does not survive `thread/resume`, which takes none, so
/// each turn repeats it.
const NO_ENVIRONMENTS: [Value; 0] = [];

/// The MCP servers a `config/read` answer lists, sorted: the names the
/// thread config has to disable. A server that is not in the table cannot
/// be disabled by name, which is what the status check after the start is
/// for.
pub(super) fn mcp_server_names(reply: &Value) -> Vec<String> {
    let mut names: Vec<String> = reply["config"]["mcp_servers"]
        .as_object()
        .map(|servers| servers.keys().cloned().collect())
        .unwrap_or_default();
    names.sort();
    names
}

/// The MCP servers that stand for a thread according to a
/// `mcpServerStatus/list` page: every server whose runtime status is
/// anything but `disabled`, a status that is missing included (the
/// app-server reports `null` when the configuration changed under it).
fn standing_mcp_servers(page: &Value) -> Vec<String> {
    page["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|server| server["runtimeStatus"] != "disabled")
        .filter_map(|server| server["name"].as_str())
        .map(str::to_owned)
        .collect()
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

/// Where a thread runs and which MCP servers its config has to disable:
/// read from the app-server right before a start or a resume, so a server
/// added to codex's config since is disabled too.
struct Placement {
    cwd: PathBuf,
    mcp_servers: Vec<String>,
}

/// The `thread/start` params: read-only, no approvals, no environment,
/// codex's surfaces off, Nolune's tools, on the preset's model.
fn thread_start_params(
    backend: &LlmBackend,
    request: &LlmRequest<'_>,
    placement: &Placement,
    ephemeral: bool,
) -> Value {
    json!({
        "approvalPolicy": "never",
        "config": thread_config(&placement.mcp_servers),
        "cwd": placement.cwd.to_string_lossy(),
        "developerInstructions": developer_instructions(request.system, request.json_schema),
        "dynamicTools": dynamic_tools(request.tools),
        "environments": NO_ENVIRONMENTS,
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
    placement: &Placement,
) -> Value {
    json!({
        "threadId": thread_id,
        "approvalPolicy": "never",
        "config": thread_config(&placement.mcp_servers),
        "cwd": placement.cwd.to_string_lossy(),
        "developerInstructions": developer_instructions(request.system, request.json_schema),
        "excludeTurns": true,
        "model": backend.model,
        "sandbox": "read-only",
    })
}

/// The empty directory a conversation's thread runs in: Nolune's own,
/// under the workspace but holding nothing, so no thread-rooted surface
/// of codex (project docs, skills, `@file` mentions, a file search) is
/// rooted at Nolune's config, chats and memory. A one-shot's thread runs
/// in the runtime's scratch directory instead.
fn durable_cwd(workspace_dir: &Path) -> Result<PathBuf, LlmError> {
    let cwd = workspace_dir.join("codex").join("cwd");
    std::fs::create_dir_all(&cwd)
        .map_err(|error| LlmError::Transport(format!("creating {}: {error}", cwd.display())))?;
    Ok(cwd)
}

/// Where a thread runs and what its config must disable: the effective
/// config as seen from `cwd`, which is how the thread will see it.
async fn placement(server: &AppServer, cwd: PathBuf) -> Result<Placement, LlmError> {
    let reply = server
        .request("config/read", json!({"cwd": cwd.to_string_lossy()}))
        .await?;
    Ok(Placement {
        cwd,
        mcp_servers: mcp_server_names(&reply),
    })
}

/// Refuse a thread any MCP server stands for. `mcpServerStatus/list` names
/// every server the thread's runtime knows and how each stands, without
/// starting one that is disabled; a server that is not disabled is a tool
/// surface the model could call outside Nolune's capability layer.
async fn refuse_mcp_servers(server: &AppServer, thread_id: &str) -> Result<(), LlmError> {
    let mut standing = Vec::new();
    let mut cursor = Value::Null;
    loop {
        let mut params = json!({"threadId": thread_id, "detail": "toolsAndAuthOnly"});
        if !cursor.is_null() {
            params["cursor"] = cursor.clone();
        }
        let page = server.request("mcpServerStatus/list", params).await?;
        standing.extend(standing_mcp_servers(&page));
        cursor = page["nextCursor"].clone();
        if cursor.is_null() {
            break;
        }
    }
    if standing.is_empty() {
        return Ok(());
    }
    log::error!(
        "[codex] the app-server started MCP server(s) {standing:?} for thread {thread_id}; refusing the thread"
    );
    Err(LlmError::InvalidResponse(format!(
        "codex started MCP server(s) {standing:?} for the thread; Nolune allows none"
    )))
}

/// Let the app-server drop an ephemeral thread: `thread/unsubscribe` ends
/// this client's interest and the app-server unloads the thread after its
/// own delay (`thread/closed`). A child that is gone took the thread with
/// it, and is not started again for this. Errors are logged: the thread
/// may be gone already, and either way nothing more is sent to it.
async fn release_ephemeral(server: &AppServer, thread_id: &str) {
    if server.pid().is_none() {
        return;
    }
    if let Err(error) = server
        .request("thread/unsubscribe", json!({"threadId": thread_id}))
        .await
    {
        log::debug!("[codex] thread/unsubscribe for {thread_id}: {error}");
    }
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
/// edits, reads a file, reaches the web, makes an image or delegates
/// outside Nolune's tools.
fn is_forbidden_item(kind: &str) -> bool {
    matches!(
        kind,
        "commandExecution"
            | "fileChange"
            | "mcpToolCall"
            | "collabAgentToolCall"
            | "subAgentActivity"
            | "webSearch"
            | "imageView"
            | "imageGeneration"
    )
}

// ═══════════════════════════════════════════════════════════════════════════
// The turn
// ═══════════════════════════════════════════════════════════════════════════

/// The thread a request runs in, attached to the live child, and whether
/// it is new to the conversation (a recap of the earlier messages goes in
/// its first turn) or a one-shot's (released once the turn is over).
struct Attached {
    thread_id: String,
    fresh: bool,
    ephemeral: bool,
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
    runtime: Runtime,
    server: AppServer,
    thread_id: String,
    turn_id: String,
    events: ThreadEvents,
    /// A one-shot's thread, released once the turn ends.
    ephemeral: bool,
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
                    Ok(None) => {
                        return Err(LlmError::Transport(
                            "codex app-server event stream closed".into(),
                        ));
                    }
                    Ok(Some(TurnEvent::Lagged(missed))) => {
                        // A request the app-server waits on may be among
                        // the missed events: the turn cannot go on.
                        self.interrupt().await;
                        return Err(LlmError::Transport(format!(
                            "fell {missed} events behind the codex app-server"
                        )));
                    }
                    Ok(Some(TurnEvent::Incoming(incoming))) => incoming,
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
            // An MCP server the app-server starts for this thread, at any
            // point: a tool surface outside Nolune's, however it got there.
            "mcpServer/startupStatus/updated" => {
                let name = params["name"].as_str().unwrap_or("");
                let status = params["status"].as_str().unwrap_or("");
                log::error!(
                    "[codex] the app-server started MCP server {name:?} for thread {} ({status}); interrupting the turn",
                    self.thread_id
                );
                self.interrupt().await;
                self.forget();
                return Err(LlmError::InvalidResponse(format!(
                    "codex started MCP server {name:?} for the thread ({status}); Nolune allows none"
                )));
            }
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
                    self.forget();
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

    /// Let go of the thread in this process after it ran something of its
    /// own: the next turn attaches it anew, overrides re-applied and MCP
    /// servers checked, rather than carrying on in it.
    fn forget(&self) {
        self.runtime
            .threads()
            .lock()
            .unwrap()
            .forget_thread(&self.thread_id);
    }
}

/// Send `turn/interrupt` and drain `events` until that turn completes, or
/// the grace period ends. `turn/start` answers before the turn runs, and an
/// interrupt in between is refused (`no active turn to interrupt`), so a
/// refusal waits for the turn's `turn/started` and asks once more. Other
/// errors are logged: the turn may be over already, or the child gone, and
/// either way there is nothing left to stop.
async fn interrupt_turn(
    server: &AppServer,
    thread_id: &str,
    turn_id: &str,
    events: &mut ThreadEvents,
) {
    let deadline = Instant::now() + INTERRUPT_GRACE;
    let params = json!({"threadId": thread_id, "turnId": turn_id});
    match server
        .request(super::protocol::TURN_INTERRUPT, params.clone())
        .await
    {
        Ok(_) => {}
        Err(AppServerError::Rpc(refusal)) => match next_turn_event(events, turn_id, deadline).await
        {
            Some("turn/started") => {
                if let Err(error) = server
                    .request(super::protocol::TURN_INTERRUPT, params)
                    .await
                {
                    log::warn!("[codex] turn/interrupt for {turn_id}: {error}");
                    return;
                }
            }
            // It ended on its own meanwhile.
            Some(_) => return,
            None => {
                log::warn!(
                    "[codex] turn/interrupt for {turn_id}: {}",
                    AppServerError::Rpc(refusal)
                );
                return;
            }
        },
        Err(error) => {
            log::warn!("[codex] turn/interrupt for {turn_id}: {error}");
            return;
        }
    }
    while let Some(method) = next_turn_event(events, turn_id, deadline).await {
        if method == "turn/completed" {
            return;
        }
    }
}

/// Drain `events` up to the next `turn/started` or `turn/completed` of
/// `turn_id` and say which it was; `None` when the child is gone or
/// `deadline` passed first.
async fn next_turn_event(
    events: &mut ThreadEvents,
    turn_id: &str,
    deadline: Instant,
) -> Option<&'static str> {
    loop {
        match tokio::time::timeout_at(deadline, events.recv()).await {
            Ok(Some(TurnEvent::Incoming(Incoming::Notification { method, params })))
                if params["turn"]["id"] == turn_id =>
            {
                match method.as_str() {
                    "turn/started" => return Some("turn/started"),
                    "turn/completed" => return Some("turn/completed"),
                    _ => {}
                }
            }
            Ok(Some(TurnEvent::Incoming(Incoming::Exited { .. }))) | Ok(None) | Err(_) => {
                return None;
            }
            Ok(Some(_)) => {}
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
                    runtime: runtime.clone(),
                    server: server.clone(),
                    thread_id: open.thread_id.clone(),
                    turn_id: open.turn_id,
                    events: open.events,
                    ephemeral: open.ephemeral,
                    sink: events,
                    cancel: &request.cancellation,
                    text: String::new(),
                    usage: Usage::default(),
                    streamed: Default::default(),
                };
                let outcome = self.finish(request, turn).await;
                if open.ephemeral && !leaves_thread_open(&outcome) {
                    release_ephemeral(&server, &open.thread_id).await;
                }
                return outcome;
            }
        }

        let server = runtime.app_server().await?;
        // The app-server accepts a turn without a login and retries the
        // model for a minute before it fails: ask first.
        if !runtime.account().await?.is_logged_in() {
            return Err(LlmError::SetupRequired(LOGIN_REQUIRED.into()));
        }
        // Subscribe before the thread is attached: an MCP server the
        // app-server starts for the thread announces itself right after
        // `thread/start`, or before the `thread/resume` answer.
        let listener = server.subscribe();
        let attached = self.attach(request, &server).await?;
        let turn_events = ThreadEvents::start(listener, attached.thread_id.clone());
        let thread_id = attached.thread_id.clone();
        let ephemeral = attached.ephemeral;
        let outcome = self
            .start(request, events, &server, attached, turn_events)
            .await;
        if ephemeral && !leaves_thread_open(&outcome) {
            release_ephemeral(&server, &thread_id).await;
        }
        outcome
    }

    /// Start the turn on an attached thread and read it.
    async fn start(
        &self,
        request: &LlmRequest<'_>,
        events: &EventSink<'_>,
        server: &AppServer,
        attached: Attached,
        turn_events: ThreadEvents,
    ) -> Result<LlmResponse, LlmError> {
        let Attached {
            thread_id,
            fresh,
            ephemeral,
        } = attached;
        if request.cancellation.is_cancelled() {
            return Err(LlmError::Cancelled);
        }
        let mut params = json!({
            "threadId": thread_id,
            "input": turn_input(request.messages, fresh),
            "environments": NO_ENVIRONMENTS,
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
            runtime: self.0.codex.clone(),
            server: server.clone(),
            thread_id,
            turn_id,
            events: turn_events,
            ephemeral,
            sink: events,
            cancel: &request.cancellation,
            text: String::new(),
            usage: Usage::default(),
            streamed: Default::default(),
        };
        self.finish(request, turn).await
    }

    /// Read the turn to its end, or to a tool call left open; a turn that
    /// asked for a tool is left open in the books, its receiver kept
    /// for the answer.
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
                    ephemeral: turn.ephemeral,
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
    /// an ephemeral thread for a one-shot run. Every start or resume reads
    /// codex's effective config first, to disable its MCP servers by name,
    /// and every attach checks the thread's MCP servers after: a thread
    /// any server stands for is refused before a turn is sent, released
    /// when it is a one-shot's, and let go of in this process otherwise,
    /// so the next turn attaches it anew.
    async fn attach(
        &self,
        request: &LlmRequest<'_>,
        server: &AppServer,
    ) -> Result<Attached, LlmError> {
        let attached = self.attach_unchecked(request, server).await?;
        if let Err(refused) = refuse_mcp_servers(server, &attached.thread_id).await {
            if attached.ephemeral {
                release_ephemeral(server, &attached.thread_id).await;
            } else {
                self.0
                    .codex
                    .threads()
                    .lock()
                    .unwrap()
                    .forget_thread(&attached.thread_id);
            }
            return Err(refused);
        }
        Ok(attached)
    }

    async fn attach_unchecked(
        &self,
        request: &LlmRequest<'_>,
        server: &AppServer,
    ) -> Result<Attached, LlmError> {
        let backend = &self.0;
        let Some(conversation) = request.conversation else {
            let cwd = backend.codex.scratch_dir().map_err(|error| {
                LlmError::Transport(format!("creating a scratch directory: {error}"))
            })?;
            let placement = placement(server, cwd).await?;
            let reply = server
                .request(
                    "thread/start",
                    thread_start_params(backend, request, &placement, true),
                )
                .await?;
            return Ok(Attached {
                thread_id: thread_id_of(&reply)?,
                fresh: true,
                ephemeral: true,
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
                let placement = placement(server, durable_cwd(workspace_dir)?).await?;
                match server
                    .request(
                        "thread/resume",
                        thread_resume_params(backend, request, &state.thread_id, &placement),
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
                return Ok(Attached {
                    thread_id: state.thread_id,
                    fresh: false,
                    ephemeral: false,
                });
            }
            return Ok(Attached {
                thread_id: state.thread_id,
                fresh: false,
                ephemeral: false,
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
            let placement = placement(server, durable_cwd(workspace_dir)?).await?;
            match server
                .request(
                    "thread/resume",
                    thread_resume_params(backend, request, &thread_id, &placement),
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
                        ephemeral: false,
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
        let placement = placement(server, durable_cwd(workspace_dir)?).await?;
        let reply = server
            .request(
                "thread/start",
                thread_start_params(backend, request, &placement, false),
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
            ephemeral: false,
        })
    }
}

/// Whether the turn stays open on a tool call, its thread needed for the
/// answer: the one outcome that does not release a one-shot's thread.
fn leaves_thread_open(outcome: &Result<LlmResponse, LlmError>) -> bool {
    matches!(outcome, Ok(response) if response.stop_reason == StopReason::ToolCalls)
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
                LlmBackend::for_preset(&config, reqwest::Client::new(), "codex-sol").unwrap();
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

        /// Where a durable thread of this workspace runs.
        fn durable_cwd(&self) -> PathBuf {
            self.dir.path().join("codex").join("cwd")
        }
    }

    /// The servers the fixture's `config/read` lists, disabled the way the
    /// thread config must carry them.
    fn fixture_mcp_servers() -> Vec<String> {
        vec!["filesystem".into(), "github".into()]
    }

    /// Whether `path` is an existing directory with nothing in it.
    fn is_empty_dir(path: &Path) -> bool {
        std::fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_none())
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

    /// The exact wire params: a fresh conversation reads codex's effective
    /// config, starts a durable thread on the preset's model, read-only,
    /// never asking for approvals, with codex's own surfaces off, every
    /// MCP server the config lists disabled by name (the app-server merges
    /// the overrides per key, so an empty table disables nothing), in an
    /// empty directory of Nolune's own, with Nolune's tools as the dynamic
    /// tools; checks that no MCP server stands for the thread; then sends
    /// a turn with the user's text. The thread id is remembered in the
    /// chat's meta.json and the thread is kept, never unsubscribed.
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
                "config/read",
                "thread/start",
                "mcpServerStatus/list",
                "turn/start"
            ],
            "the login is checked, the config read, the thread started and checked, the turn sent; nothing else"
        );
        let cwd = harness.durable_cwd();
        assert!(
            is_empty_dir(&cwd),
            "the thread runs in an empty directory of Nolune's own: {}",
            cwd.display()
        );
        assert_eq!(
            harness.sent("config/read"),
            [json!({"cwd": cwd.to_string_lossy()})],
            "the effective config as the thread will see it"
        );
        let started = harness.sent("thread/start");
        assert_eq!(
            started[0],
            json!({
                "approvalPolicy": "never",
                "config": thread_config(&fixture_mcp_servers()),
                "cwd": cwd.to_string_lossy(),
                "developerInstructions": "system one\n\nsystem two",
                "dynamicTools": [{
                    "type": "function",
                    "name": "search",
                    "description": "test",
                    "inputSchema": {"type": "object", "properties": {"q": {"type": "string"}}},
                }],
                "environments": [],
                "ephemeral": false,
                "model": "gpt-6-sol",
                "sandbox": "read-only",
            })
        );
        let config = &started[0]["config"];
        assert_eq!(config["project_doc_max_bytes"], 0);
        assert_eq!(
            config["mcp_servers"],
            json!({"filesystem": {"enabled": false}, "github": {"enabled": false}}),
            "every server the config lists, disabled by name"
        );
        assert_eq!(
            harness.sent("mcpServerStatus/list"),
            [json!({"threadId": "thr_fixture_1", "detail": "toolsAndAuthOnly"})],
            "the thread's MCP servers are checked before the turn"
        );
        assert!(
            harness.sent("thread/unsubscribe").is_empty(),
            "a conversation's thread is kept"
        );
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
        assert_eq!(config["web_search"], "disabled");
        assert_eq!(
            harness.sent("turn/start")[0],
            json!({
                "threadId": "thr_fixture_1",
                "input": [{"type": "text", "text": "hello"}],
                "environments": [],
            }),
            "every turn runs without an environment, so codex has nowhere to apply a patch"
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
                "config": thread_config(&fixture_mcp_servers()),
                "cwd": harness.durable_cwd().to_string_lossy(),
                "developerInstructions": "soul",
                "excludeTurns": true,
                "model": "gpt-6-sol",
                "sandbox": "read-only",
            })
        );
        // A resume reads the config and checks the thread like a start.
        assert_eq!(harness.sent("config/read").len(), 1);
        assert_eq!(
            harness.sent("mcpServerStatus/list"),
            [json!({"threadId": "thr_saved_7", "detail": "toolsAndAuthOnly"})]
        );
        // Codex holds the earlier turns: only the new message is sent,
        // no recap.
        assert_eq!(
            harness.sent("turn/start")[0],
            json!({
                "threadId": "thr_saved_7",
                "input": [{"type": "text", "text": "hi"}],
                "environments": [],
            }),
            "a resumed thread keeps no environment of its own: the turn says none again"
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
        assert_eq!(
            harness.sent("config/read").len(),
            2,
            "the config is read for a start or a resume, not for every turn"
        );
        assert_eq!(
            harness.sent("mcpServerStatus/list").len(),
            3,
            "the thread's MCP servers are checked before every turn"
        );
        assert!(harness.sent("thread/unsubscribe").is_empty());
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

    /// A one-shot (a title, a memory extraction, the connection test) runs
    /// in an ephemeral thread, in an empty scratch directory of the
    /// runtime's own, and the thread is unsubscribed once the turn is
    /// over, so the app-server can unload it instead of keeping every
    /// one-shot loaded until it exits.
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
            started["config"]["mcp_servers"],
            json!({"filesystem": {"enabled": false}, "github": {"enabled": false}})
        );
        let cwd = PathBuf::from(started["cwd"].as_str().unwrap());
        assert!(
            is_empty_dir(&cwd),
            "an empty directory of the runtime's own: {}",
            cwd.display()
        );
        assert!(
            !cwd.starts_with(harness.workspace()),
            "never Nolune's workspace: {}",
            cwd.display()
        );
        assert!(
            cwd.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("nolune-codex-")),
            "{}",
            cwd.display()
        );
        assert_eq!(
            harness.sent("turn/start")[0]["threadId"],
            "thr_fixture_ephemeral"
        );
        assert!(
            !harness.workspace().join("instances").exists(),
            "no chat directory appears for a one-shot"
        );
        assert!(
            !harness.workspace().join("codex").exists(),
            "no thread directory appears under the workspace for a one-shot"
        );
        assert_eq!(
            harness.sent("thread/unsubscribe"),
            [json!({"threadId": "thr_fixture_ephemeral"})],
            "the thread is released once the turn is over"
        );
        let methods = harness.methods();
        let unsubscribe = methods
            .iter()
            .position(|method| method == "thread/unsubscribe")
            .unwrap();
        let turn = methods
            .iter()
            .position(|method| method == "turn/start")
            .unwrap();
        assert!(unsubscribe > turn, "{methods:?}");

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
        assert_eq!(
            harness.sent("thread/unsubscribe").len(),
            2,
            "every one-shot releases its thread"
        );
        assert_eq!(
            PathBuf::from(harness.sent("thread/start")[1]["cwd"].as_str().unwrap()),
            cwd,
            "one scratch directory per runtime"
        );
        harness.runtime.close();
    }

    /// A one-shot that fails (here: the turn is interrupted by the
    /// cancellation) still releases its thread; one that is left open on a
    /// tool call keeps it until the loop comes back and it ends.
    #[tokio::test]
    async fn an_ephemeral_thread_is_released_when_its_turn_ends_however_it_ends() {
        let harness = Harness::new();
        let backend = harness.backend();
        let messages = [Message::user("rate me")];
        let request = LlmRequest::new(ExecutionScope::Subagent, &[], &messages, &[]);
        let result = backend.adapter().unwrap().complete(request).await;
        assert!(
            matches!(result, Err(LlmError::RateLimited { .. })),
            "{result:?}"
        );
        assert_eq!(
            harness.sent("thread/unsubscribe"),
            [json!({"threadId": "thr_fixture_ephemeral"})],
            "a failed turn releases the thread too"
        );

        // A tool call leaves the turn, and the thread, open.
        let tools = [tool("shot"), tool("search")];
        let mut messages = vec![Message::user("look it up")];
        let request = LlmRequest::new(ExecutionScope::Subagent, &[], &messages, &tools);
        let first = backend.adapter().unwrap().complete(request).await.unwrap();
        assert_eq!(first.stop_reason, StopReason::ToolCalls);
        assert_eq!(
            harness.sent("thread/unsubscribe").len(),
            1,
            "a turn waiting on a tool keeps its thread"
        );
        messages.push(Message::Assistant {
            content: vec![ContentBlock::ToolCall {
                id: "call1".into(),
                name: "shot".into(),
                arguments: json!({}),
            }],
        });
        messages.push(Message::User {
            content: vec![ContentBlock::tool_output(
                "call1".into(),
                "captured".into(),
                false,
            )],
        });
        let request = LlmRequest::new(ExecutionScope::Subagent, &[], &messages, &tools);
        let second = backend.adapter().unwrap().complete(request).await.unwrap();
        assert_eq!(second.tool_calls[0].id, "call2");
        assert_eq!(harness.sent("thread/unsubscribe").len(), 1);
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
        let request = LlmRequest::new(ExecutionScope::Subagent, &[], &messages, &tools);
        let third = backend.adapter().unwrap().complete(request).await.unwrap();
        assert_eq!(third.text, "done");
        assert_eq!(
            harness.sent("thread/unsubscribe").len(),
            2,
            "released once the turn ends"
        );
        assert_eq!(harness.sent("thread/start").len(), 2);
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

    /// `turn/start` answers before its turn runs, and an interrupt that lands
    /// in between is refused (`no active turn to interrupt`, the live shape):
    /// the cancel waits for `turn/started` and interrupts the turn then,
    /// rather than leaving it running in codex with nobody reading it.
    #[tokio::test]
    async fn a_cancel_before_the_turn_started_interrupts_it_once_it_has() {
        let harness = Harness::new();
        let backend = harness.backend();
        let messages = [Message::user("cancel early")];
        let mut request = LlmRequest::new(ExecutionScope::Conversation, &[], &messages, &[]);
        request.conversation = Some(harness.conversation());
        let token = request.cancellation.clone();
        let cancel_on_send = async {
            while harness.sent("turn/start").is_empty() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            token.cancel();
        };
        let ((result, _), ()) = tokio::join!(stream(&backend, request), cancel_on_send);
        assert!(matches!(result, Err(LlmError::Cancelled)), "{result:?}");
        let interrupt = json!({"threadId": "thr_fixture_1", "turnId": "turn_fixture_early"});
        assert_eq!(
            harness.sent("turn/interrupt"),
            [interrupt.clone(), interrupt],
            "refused before the turn started, sent again once it had"
        );
        // The turn is over in codex: the thread takes the next one.
        let messages = [Message::user("cancel early"), Message::user("hi")];
        let mut request = LlmRequest::new(ExecutionScope::Conversation, &[], &messages, &[]);
        request.conversation = Some(harness.conversation());
        let response = stream(&backend, request).await.0.unwrap();
        assert_eq!(response.text, "Hello from the fixture");
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
            json!(harness.durable_cwd().to_string_lossy()),
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

    /// An MCP server codex starts for a thread is a tool surface outside
    /// Nolune's capability layer. The app-server merges the thread config
    /// per key, so the adapter disables every server the effective config
    /// lists by name; a server that starts anyway (one the config did not
    /// show: `NOLUNE_FAKE_APP_SERVER_MCP=hidden` lists none, and the fake's
    /// thread entries start the configured ones for any other override,
    /// the live behaviour) is caught by the status check after the start
    /// or resume: the turn is refused before it begins, nothing is sent to
    /// the model, and the thread is not kept in this process, so the next
    /// turn checks again.
    #[tokio::test]
    async fn a_thread_codex_starts_an_mcp_server_for_is_refused_before_the_turn() {
        let harness = Harness::with_env(&[(fake::MCP_ENV, "hidden")]);
        let backend = harness.backend();
        let messages = [Message::user("hi")];
        let mut request = LlmRequest::new(ExecutionScope::Conversation, &[], &messages, &[]);
        request.conversation = Some(harness.conversation());
        let (result, events) = stream(&backend, request).await;
        assert!(
            matches!(&result, Err(LlmError::InvalidResponse(reason)) if reason.contains("MCP") && reason.contains("filesystem")),
            "{result:?}"
        );
        assert!(events.is_empty());
        assert_eq!(
            harness.sent("thread/start")[0]["config"]["mcp_servers"],
            json!({}),
            "nothing to disable as far as the config said"
        );
        assert_eq!(
            harness.sent("mcpServerStatus/list"),
            [json!({"threadId": "thr_fixture_leaky", "detail": "toolsAndAuthOnly"})]
        );
        assert!(
            harness.sent("turn/start").is_empty(),
            "the model never hears from a thread with an MCP server"
        );

        // The next message resumes the remembered thread and checks again:
        // still refused, still no turn.
        let mut request = LlmRequest::new(ExecutionScope::Conversation, &[], &messages, &[]);
        request.conversation = Some(harness.conversation());
        let result = stream(&backend, request).await.0;
        assert!(
            matches!(&result, Err(LlmError::InvalidResponse(reason)) if reason.contains("MCP")),
            "{result:?}"
        );
        assert_eq!(harness.sent("thread/resume").len(), 1, "checked again");
        assert_eq!(harness.sent("mcpServerStatus/list").len(), 2);
        assert!(harness.sent("turn/start").is_empty());
        assert!(
            harness.sent("thread/unsubscribe").is_empty(),
            "a conversation's thread is kept for the next check"
        );

        // A one-shot is refused the same way and its thread released.
        let request = LlmRequest::new(ExecutionScope::Subagent, &[], &messages, &[]);
        let result = backend.adapter().unwrap().complete(request).await;
        assert!(
            matches!(&result, Err(LlmError::InvalidResponse(reason)) if reason.contains("MCP")),
            "{result:?}"
        );
        assert!(harness.sent("turn/start").is_empty());
        assert_eq!(
            harness.sent("thread/unsubscribe"),
            [json!({"threadId": "thr_fixture_leaky"})]
        );
        harness.runtime.close();
    }

    /// An MCP server that starts for the thread while a turn runs (a
    /// config reload from another client, a server that came up late) is
    /// announced by the app-server; the turn is interrupted and fails
    /// before the model can call the server's tools, and the thread is
    /// checked again on the next turn.
    #[tokio::test]
    async fn an_mcp_server_that_starts_mid_turn_fails_the_turn() {
        let harness = Harness::new();
        let backend = harness.backend();
        let messages = [Message::user("mcp sneaks in")];
        let mut request = LlmRequest::new(ExecutionScope::Conversation, &[], &messages, &[]);
        request.conversation = Some(harness.conversation());
        let (result, events) = stream(&backend, request).await;
        assert!(
            matches!(&result, Err(LlmError::InvalidResponse(reason)) if reason.contains("MCP") && reason.contains("github")),
            "{result:?}"
        );
        assert!(!text_of(&events).contains("I have tools now"), "{events:?}");
        assert_eq!(
            harness.sent("turn/interrupt"),
            [json!({"threadId": "thr_fixture_1", "turnId": "turn_fixture_31"})]
        );

        // The thread was let go of in this process: the next turn resumes
        // it with the overrides applied again and checks it before going on.
        let messages = [Message::user("mcp sneaks in"), Message::user("hi")];
        let mut request = LlmRequest::new(ExecutionScope::Conversation, &[], &messages, &[]);
        request.conversation = Some(harness.conversation());
        let response = stream(&backend, request).await.0.unwrap();
        assert_eq!(response.text, "Hello from the fixture");
        assert_eq!(harness.sent("thread/start").len(), 1);
        assert_eq!(harness.sent("thread/resume").len(), 1);
        assert_eq!(harness.sent("mcpServerStatus/list").len(), 2);
        harness.runtime.close();
    }

    /// A turn parked on a tool call keeps only its own thread's events
    /// while the agent loop runs the tool: another thread (a one-shot
    /// routine, a second chat) may stream far more events than the
    /// supervisor's broadcast buffer holds meanwhile, and the parked turn
    /// still goes on when its result comes back.
    #[tokio::test]
    async fn a_turn_left_open_on_a_tool_survives_another_thread_streaming_past_the_buffer() {
        let harness = Harness::new();
        let backend = harness.backend();
        let tools = [tool("shot"), tool("search")];
        let mut messages = vec![Message::user("look it up")];
        let mut request = LlmRequest::new(ExecutionScope::Conversation, &[], &messages, &tools);
        request.conversation = Some(harness.conversation());
        let first = stream(&backend, request).await.0.unwrap();
        assert_eq!(first.stop_reason, StopReason::ToolCalls);
        assert_eq!(first.tool_calls[0].id, "call1");

        // Meanwhile a one-shot streams a long reasoning trace on a thread
        // of its own: more events than the broadcast buffer keeps.
        let flood = std::fs::read_to_string(fake::fixture_path()).unwrap();
        let repeat = flood
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find(|entry| entry["when"]["input"][0]["text"] == "flood")
            .and_then(|entry| {
                entry["steps"]
                    .as_array()?
                    .iter()
                    .find_map(|step| step["repeat"].as_u64())
            })
            .expect("the flood scenario repeats a notification");
        assert!(
            repeat as usize > super::super::process::EVENT_BUFFER,
            "the flood ({repeat}) must exceed the buffer ({})",
            super::super::process::EVENT_BUFFER
        );
        let flooding = [Message::user("flood")];
        let request = LlmRequest::new(ExecutionScope::Subagent, &[], &flooding, &[]);
        let flooded = backend.adapter().unwrap().complete(request).await.unwrap();
        assert_eq!(flooded.text, "flooded");
        assert_eq!(flooded.usage.output_tokens, 1500);

        // The loop is back with the result: the parked turn goes on.
        messages.push(Message::Assistant {
            content: vec![ContentBlock::ToolCall {
                id: "call1".into(),
                name: "shot".into(),
                arguments: json!({}),
            }],
        });
        messages.push(Message::User {
            content: vec![ContentBlock::tool_output(
                "call1".into(),
                "captured".into(),
                false,
            )],
        });
        let mut request = LlmRequest::new(ExecutionScope::Conversation, &[], &messages, &tools);
        request.conversation = Some(harness.conversation());
        let second = stream(&backend, request).await.0.unwrap();
        assert_eq!(second.stop_reason, StopReason::ToolCalls, "{second:?}");
        assert_eq!(second.tool_calls[0].id, "call2");
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
        let third = stream(&backend, request).await.0.unwrap();
        assert_eq!(third.text, "done");
        assert_eq!(third.stop_reason, StopReason::Complete);
        assert_eq!(
            harness.sent("turn/start").len(),
            2,
            "the conversation's one turn and the one-shot's"
        );
        assert!(harness.sent("turn/interrupt").is_empty());
        let answers = harness.answers();
        assert_eq!(answers.len(), 2, "{answers:?}");
        assert!(answers.iter().all(|(_, outcome)| outcome.is_ok()));
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
        let config = thread_config(&["github".into(), "filesystem".into()]);
        assert_eq!(config["project_doc_max_bytes"], 0);
        assert_eq!(
            config["mcp_servers"],
            json!({"filesystem": {"enabled": false}, "github": {"enabled": false}}),
            "each server by name: the app-server merges the table per key"
        );
        assert_eq!(thread_config(&[])["mcp_servers"], json!({}));
        assert!(
            config["features"]
                .as_object()
                .unwrap()
                .values()
                .all(|flag| *flag == false),
            "{config}"
        );
        // The names come from the effective config's table, whatever else
        // it says about each server.
        assert_eq!(
            mcp_server_names(&json!({"config": {"mcp_servers": {
                "github": {"command": "gh-mcp", "enabled": true},
                "filesystem": {"url": "http://localhost:1", "enabled": false},
            }}})),
            ["filesystem", "github"]
        );
        assert!(mcp_server_names(&json!({"config": {}})).is_empty());
        assert!(mcp_server_names(&json!({"config": {"mcp_servers": null}})).is_empty());
        // What codex must never run on its own: anything that executes,
        // edits, reads a file, searches the web or delegates.
        for kind in [
            "commandExecution",
            "fileChange",
            "mcpToolCall",
            "collabAgentToolCall",
            "subAgentActivity",
            "webSearch",
            "imageView",
            "imageGeneration",
        ] {
            assert!(is_forbidden_item(kind), "{kind}");
        }
        for kind in [
            "userMessage",
            "agentMessage",
            "reasoning",
            "plan",
            "dynamicToolCall",
            "contextCompaction",
        ] {
            assert!(!is_forbidden_item(kind), "{kind}");
        }
        // The switches that take the rest of codex's tools away, each one
        // checked against the pinned release by the tool list the model got.
        assert_eq!(config["web_search"], "disabled", "{config}");
        assert_eq!(
            config["tools"],
            json!({"experimental_request_user_input": {"enabled": false}}),
            "no tools.* key the release ignores: tools.web_search = false parses and is dropped"
        );
        assert_eq!(config["features"]["goals"], false);
        assert_eq!(config["skills"]["include_instructions"], false);
        assert_eq!(config["orchestrator"]["skills"]["enabled"], false);
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
