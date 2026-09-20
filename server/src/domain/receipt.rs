//! Memory recall receipts (#84): bounded provenance for one assistant message.
//!
//! A receipt answers "why did Nolune remember this?" without exposing model
//! reasoning: which canonical memory file was recalled, the excerpt that was
//! injected, how retrieval found it, a coarse confidence bucket, and when.
//! The same [`RecalledMemory`] shape rides on the transient `memory_recall`
//! event and is persisted next to the chat (see `docs/companion-storage.md`).

use serde::{Deserialize, Serialize};

/// How auto-recall surfaced a memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecallReason {
    /// Matched the meaning of the recent conversation (vector index).
    Semantic,
    /// Contains words from the recent conversation (BM25 index).
    Keyword,
    /// One graph hop away from another recalled memory (`linked_from`).
    LinkedTo,
    /// Surfaced by hybrid search, but the channel is not attributable from
    /// the search result. Currently only media memories while the embedding
    /// provider is available; never used for text memories.
    Matched,
}

/// Coarse confidence bucket. Raw scores never leave the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    High,
    Medium,
    Low,
}

/// Whether the cited canonical file still exists, resolved every time a
/// receipt is read so deleted sources are represented honestly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SourceStatus {
    #[default]
    Present,
    Missing,
}

/// One recalled memory as shown on a receipt and on the `memory_recall` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecalledMemory {
    /// Memory path as the library shows it (for media, the media file).
    pub path: String,
    /// Canonical file whose text was recalled. Equals `path` for text
    /// memories; media memories cite their bound text representation.
    pub source: String,
    /// Bounded excerpt of the injected text.
    pub excerpt: String,
    pub reason: RecallReason,
    /// The recalled memory this one was linked from (`linked_to` only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked_from: Option<String>,
    pub confidence: Confidence,
    /// RFC 3339 UTC timestamp of the retrieval.
    pub retrieved_at: String,
    #[serde(default)]
    pub source_status: SourceStatus,
}

/// Persisted provenance for one assistant message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryReceipt {
    pub message_id: String,
    pub chat_id: String,
    pub memories: Vec<RecalledMemory>,
}
