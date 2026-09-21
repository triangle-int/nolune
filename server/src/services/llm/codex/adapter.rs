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
use std::time::Duration;

use futures::future::BoxFuture;
use serde_json::{Value, json};
use tokio::sync::broadcast;

use crate::services::tool::ToolDefinition;

use super::super::contract::{
    Capabilities, ConversationRef, EventSink, LlmError, LlmEvent, LlmRequest, ProviderAdapter,
    StopReason, Usage,
};
use super::super::types::{
    ContentBlock, LlmBackend, LlmResponse, Message, ToolCall, ToolOutputContent,
};
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

/// A thread attached in this process.
struct ThreadState {
    thread_id: String,
    /// The names of the tools the thread was started with, when this
    /// process started it; unknown for a resumed thread.
    tools: Option<Vec<String>>,
    /// The instructions and model the thread was last configured with.
    configured: String,
}

/// A turn that asked for a tool and waits for the answer.
struct OpenTurn {
    thread_id: String,
    turn_id: String,
    events: broadcast::Receiver<Incoming>,
    /// The calls handed to the agent loop, by call id: the app-server's
    /// request id to answer with.
    calls: HashMap<String, Value>,
}

pub struct CodexAdapter(pub LlmBackend);

// ═══════════════════════════════════════════════════════════════════════════
// Wire shapes
// ═══════════════════════════════════════════════════════════════════════════

/// Nolune's tools as codex `dynamicTools`: name, description and schema,
/// nothing that lets codex run them itself.
pub(super) fn dynamic_tools(tools: &[ToolDefinition]) -> Vec<Value> {
    let _ = tools;
    todo!("27c")
}

/// The config overrides every thread is started with: no project docs, no
/// MCP servers, and codex's own tool surface switched off, so the model
/// has Nolune's tools and nothing else.
pub(super) fn thread_config() -> Value {
    todo!("27c")
}

/// The system blocks as the thread's developer instructions, with the
/// output schema appended for a structured request; secrets redacted like
/// every other outgoing payload.
pub(crate) fn developer_instructions(system: &[&str], json_schema: Option<&Value>) -> String {
    let _ = (system, json_schema);
    todo!("27c")
}

/// The `turn/start` input for `messages`: the user content after the last
/// assistant message as text items, with a recap of everything before it
/// when the thread is fresh and the conversation is not (a chat switched
/// to Codex mid-way, or a thread codex no longer has). Tool results that
/// answer no open call are text too, so a turn interrupted between a call
/// and its result still tells the model what happened.
pub(crate) fn turn_input(messages: &[Message], fresh_thread: bool) -> Vec<Value> {
    let _ = (messages, fresh_thread);
    todo!("27c")
}

/// A tool result as the `contentItems` of a `item/tool/call` answer.
pub(super) fn tool_output_items(content: &ToolOutputContent) -> Vec<Value> {
    let _ = content;
    todo!("27c")
}

/// The typed reading of a failed turn's `error` (`TurnError`): the
/// `codexErrorInfo` names the class, as a string for the simple variants
/// and as `{variant: {httpStatusCode}}` for the transport ones.
pub(super) fn map_turn_error(error: &Value) -> LlmError {
    let _ = error;
    todo!("27c")
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
    let _ = last;
    todo!("27c")
}

// ═══════════════════════════════════════════════════════════════════════════
// The turn
// ═══════════════════════════════════════════════════════════════════════════

impl CodexAdapter {
    /// One request: attach the conversation's thread, start or continue a
    /// turn, and read it until it completes or asks for a tool.
    async fn turn(
        &self,
        request: &LlmRequest<'_>,
        events: &EventSink<'_>,
    ) -> Result<LlmResponse, LlmError> {
        let _ = (
            request,
            events,
            IDLE_TIMEOUT,
            INTERRUPT_GRACE,
            AppServer::request,
            RpcError {
                code: 0,
                message: String::new(),
                data: None,
            },
            ConversationRef {
                instance_slug: "",
                chat_id: "",
                workspace_dir: std::path::Path::new(""),
            },
            LlmEvent::TextDelta(String::new()),
            StopReason::Complete,
            ToolCall {
                id: String::new(),
                name: String::new(),
                arguments: Value::Null,
            },
            ContentBlock::text(""),
            normalized_tokens,
            parse_usage,
        );
        let _: HashMap<String, ThreadState> = HashMap::new();
        let _: Option<OpenTurn> = None;
        todo!("27c")
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
        let answers = harness.answers();
        assert_eq!(answers.len(), 1, "{answers:?}");
        assert_eq!(answers[0].0, json!(40));
        assert!(
            answers[0].1.is_err(),
            "the stale call is refused, never answered"
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
            [json!({"type": "inputText", "text": "[{\"type\":\"expense\",\"amount\":1}]"})]
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
