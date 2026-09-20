use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub role: ChatRole,
    pub content: String,
    pub created_at: String,
    /// Distinguishes regular messages from tool activity.
    /// Defaults to "message" for backward compat with existing messages.json files.
    #[serde(default)]
    pub kind: MessageKind,
    /// Tool name, present only when kind is tool_call or tool_output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// HTML content for MCP App rendering (kind = mcp_app only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_app_html: Option<String>,
    /// Tool input JSON for MCP App rendering (kind = mcp_app only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_app_input: Option<String>,
    /// Model name used to generate this message (assistant only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    #[default]
    Message,
    ToolCall,
    ToolOutput,
    McpApp,
    Compaction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatRole {
    User,
    Assistant,
}

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub instance_slug: String,
    pub content: String,
    #[serde(default = "default_chat_id")]
    pub chat_id: String,
    #[serde(default)]
    pub voice_mode: bool,
    /// The computer the user chose for this conversation (#80): a known
    /// machine's stable id, or `server-home`. Absent means nothing chosen.
    #[serde(default)]
    pub machine_id: Option<String>,
}

fn default_chat_id() -> String {
    "default".into()
}

#[derive(Debug, Serialize)]
pub struct ChatResponse {
    pub instance_slug: String,
    pub chat_id: String,
    pub messages: Vec<ChatMessage>,
    pub agent_running: bool,
}

/// Summary of a chat session for listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSummary {
    pub id: String,
    pub title: String,
    /// Preset this chat pins (#156); None follows the Chat slot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    pub message_count: usize,
    pub last_message_at: Option<String>,
    pub created_at: String,
}

/// Metadata stored alongside each chat's messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMeta {
    pub id: String,
    pub title: String,
    pub created_at: String,
    /// Preset this chat pins (#156); None follows the Chat slot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
}

/// Why a conversation's agent loop stopped, as the loop itself reports it.
/// A caller that waits for a conversation to stop reads this instead of
/// guessing the reason from chat text, which other writers append to as
/// well (mood lines, rhythm updates, a desktop connecting).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentLoopExit {
    /// The turn (or turns) finished and the conversation is idle.
    Finished,
    /// A turn errored; `error` is the label the loop wrote to the chat.
    Failed { error: String },
    /// Stopped by the user, or by whoever holds the conversation's token.
    Cancelled,
    /// No chat model is configured, so no turn ran.
    NoModel,
}
