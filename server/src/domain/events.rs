use serde::Serialize;

use crate::domain::{
    chat::ChatMessage, commitment::Commitment, drop::Drop, machine::KnownMachine,
    proactive::ProactiveRun, receipt::RecalledMemory,
};

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEvent {
    ChatMessageCreated {
        instance_slug: String,
        chat_id: String,
        message: ChatMessage,
    },
    MoodUpdated {
        instance_slug: String,
        mood: String,
    },
    AgentRunning {
        instance_slug: String,
        chat_id: String,
    },
    AgentStopped {
        instance_slug: String,
        chat_id: String,
    },
    DropCreated {
        instance_slug: String,
        drop: Drop,
    },
    /// A proactive run was created or changed (#94). Receipts only, no model text.
    ActivityUpdated {
        instance_slug: String,
        run: ProactiveRun,
    },
    /// A commitment was created or changed (#85). Bounded record, no model text.
    CommitmentUpdated {
        instance_slug: String,
        commitment: Commitment,
    },
    /// A request this companion sends a paired peer was queued or changed
    /// (#110): the outbox entry with its attempts and the peer's typed
    /// response, never anything else the peer said.
    OutboxUpdated {
        instance_slug: String,
        entry: crate::services::federation::outbox::OutboxEntry,
    },
    /// A proposal from a paired companion (#111) arrived, was accepted or
    /// dismissed, or lapsed: the proposal record, whose details are the
    /// peer's words for the owner's review and never model text.
    PeerProposalUpdated {
        instance_slug: String,
        proposal: crate::domain::federation_proposal::PeerProposal,
    },
    /// A known machine registered, disconnected, went stale, or was renamed (#80).
    MachineUpdated {
        instance_slug: String,
        machine: KnownMachine,
    },
    /// An offline known machine was forgotten (#80): clients drop its row.
    MachineForgotten {
        instance_slug: String,
        machine_id: String,
    },
    /// A handoff card changed (#82): a decision was recorded or a
    /// continuation finished. The card is derived from the record, no model text.
    HandoffUpdated {
        instance_slug: String,
        card: crate::domain::handoff::HandoffCard,
    },
    /// The resume ritual's offer changed (#83): a new suggestion, or none (`null`).
    ResumeUpdated {
        instance_slug: String,
        suggestion: Option<crate::services::resume_ritual::ResumeOffer>,
    },
    ContextCompacting {
        instance_slug: String,
        chat_id: String,
        messages_compacted: usize,
    },
    ChatStreamDelta {
        instance_slug: String,
        chat_id: String,
        message_id: String,
        delta: String,
    },
    SecretRequest {
        instance_slug: String,
        id: String,
        prompt: String,
        target: String,
    },
    ToolOutputChunk {
        instance_slug: String,
        chat_id: String,
        chunk: String,
    },
    /// Tool result arrived for an MCP App — viewer should send it to the iframe.
    McpAppResult {
        instance_slug: String,
        chat_id: String,
        /// The message id of the McpApp chat message to update.
        message_id: String,
        tool_output: String,
    },
    /// Streaming: an MCP App tool call is starting — show the iframe immediately.
    McpAppStart {
        instance_slug: String,
        chat_id: String,
        tool_name: String,
        html: String,
    },
    /// Server-generated TTS audio ready for playback.
    ChatAudioReady {
        instance_slug: String,
        chat_id: String,
        /// Base64-encoded MP3 audio bytes.
        audio_base64: String,
        /// Message IDs that this audio covers (for word reveal).
        message_ids: Vec<String>,
    },
    /// Streaming: partial tool arguments delta for an MCP App.
    McpAppInputDelta {
        instance_slug: String,
        chat_id: String,
        delta: String,
    },
    /// Automatic memory recall — the same entries the message's receipt persists (#84).
    MemoryRecall {
        instance_slug: String,
        chat_id: String,
        memories: Vec<RecalledMemory>,
    },
    /// Full chat state snapshot — client reconciles against this.
    ChatSnapshot {
        instance_slug: String,
        chat_id: String,
        messages: Vec<ChatMessage>,
        agent_running: bool,
    },
}
