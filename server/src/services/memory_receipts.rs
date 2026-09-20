//! Auto-recall provenance (#84).
//!
//! The chat turn asks [`recall`] which memories to inject for the recent
//! conversation. The result carries the receipt shape
//! ([`RecalledMemory`]: canonical path, excerpt, retrieval reason, confidence
//! bucket, retrieval time) and the prompt block built from it, so the
//! `memory_recall` event and the persisted receipt can never disagree with what
//! the model saw. After the turn, one receipt file per assistant message is
//! written atomically next to the chat history
//! (`chats/{chat_id}/receipts/{message_id}.json`). Reading a receipt resolves
//! every cited source again, so a deleted memory is reported as missing instead
//! of failing the receipt.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use crate::{
    domain::{
        chat::{ChatMessage, ChatRole, MessageKind},
        receipt::{Confidence, MemoryReceipt, RecallReason, RecalledMemory, SourceStatus},
    },
    services::{media_text::MediaStore, vector::VectorStore},
};

/// Directory under `chats/{chat_id}/` holding one receipt per assistant message.
pub const RECEIPTS_DIR: &str = "receipts";

/// Memories recalled for one turn: the receipt entries plus the prompt block.
#[derive(Debug, Default)]
pub struct Recall {
    pub memories: Vec<RecalledMemory>,
    prompt_lines: Vec<String>,
}

impl Recall {
    pub fn is_empty(&self) -> bool {
        self.memories.is_empty()
    }

    /// The `[system: auto-recalled memories …]` block appended to the user
    /// message, or `None` when nothing was recalled.
    pub fn prompt_block(&self) -> Option<String> {
        let _ = &self.prompt_lines;
        todo!("PR A of #84")
    }
}

/// Hybrid search over the memory library plus one-hop graph expansion.
pub async fn recall(_vector_store: &VectorStore, _instance_slug: &str, _query: &str) -> Recall {
    todo!("PR A of #84")
}

pub fn receipts_dir(_workspace_dir: &Path, _instance_slug: &str, _chat_id: &str) -> PathBuf {
    todo!("PR A of #84")
}

/// Persist one receipt per assistant text message produced by a turn.
/// Returns how many receipts were written.
pub fn write_receipts(
    _workspace_dir: &Path,
    _instance_slug: &str,
    _chat_id: &str,
    _messages: &[ChatMessage],
    _memories: &[RecalledMemory],
) -> io::Result<usize> {
    let _ = (
        ChatRole::Assistant,
        MessageKind::Message,
        fs::read_dir::<&Path>,
    );
    todo!("PR A of #84")
}

/// Read one receipt with every cited source resolved against the library.
/// `Ok(None)` when the chat or message has no receipt or the ids are unsafe.
pub fn read_receipt(
    _workspace_dir: &Path,
    _media: &MediaStore,
    _instance_slug: &str,
    _chat_id: &str,
    _message_id: &str,
) -> io::Result<Option<MemoryReceipt>> {
    let _ = (
        SourceStatus::Missing,
        Confidence::Low,
        RecallReason::Matched,
    );
    todo!("PR A of #84")
}

/// Every receipt of a chat, ordered by message id, with sources resolved.
pub fn list_receipts(
    _workspace_dir: &Path,
    _media: &MediaStore,
    _instance_slug: &str,
    _chat_id: &str,
) -> io::Result<Vec<MemoryReceipt>> {
    todo!("PR A of #84")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{
        embedding::tests::{MockServer, response},
        media_text, memory,
    };

    fn memory_dir(workspace: &Path) -> PathBuf {
        let dir = workspace.join("instances/one/memory");
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn assistant(id: &str, content: &str, kind: MessageKind) -> ChatMessage {
        ChatMessage {
            id: id.into(),
            role: ChatRole::Assistant,
            content: content.into(),
            created_at: "1".into(),
            kind,
            tool_name: None,
            mcp_app_html: None,
            mcp_app_input: None,
            model: None,
        }
    }

    fn user(id: &str) -> ChatMessage {
        ChatMessage {
            role: ChatRole::User,
            ..assistant(id, "hi", MessageKind::Message)
        }
    }

    fn recalled(path: &str, reason: RecallReason) -> RecalledMemory {
        RecalledMemory {
            path: path.into(),
            source: path.into(),
            excerpt: format!("excerpt of {path}"),
            reason,
            linked_from: None,
            confidence: Confidence::Medium,
            retrieved_at: "2026-09-20T12:00:00Z".into(),
            source_status: SourceStatus::Present,
        }
    }

    #[tokio::test]
    async fn recall_labels_semantic_keyword_and_linked_memories() {
        let mock = MockServer::new(vec![(200, response(vec![1., 0., 0.]))]).await;
        let workspace = tempfile::tempdir().unwrap();
        let dir = memory_dir(workspace.path());
        fs::write(dir.join("note.md"), "Orion nebula").unwrap();
        fs::write(dir.join("words.md"), "Pleiades cluster").unwrap();
        fs::write(
            dir.join("linked.md"),
            "---\ncreated: 2026-01-01\nupdated: 2026-01-01\n---\nMoon phases",
        )
        .unwrap();
        let store = VectorStore::connect_with_config(workspace.path(), &mock.config).await;
        store
            .upsert_text_memory(
                "one",
                "note.md",
                vec![("Orion nebula".into(), vec![1., 0., 0.])],
            )
            .await
            .unwrap();
        memory::add_edge(&store.media_store(), "one", "note.md", "linked.md");

        let recall = recall(&store, "one", "Orion Pleiades").await;

        let by_path = |path: &str| {
            recall
                .memories
                .iter()
                .find(|m| m.path == path)
                .unwrap_or_else(|| panic!("{path} was not recalled: {:?}", recall.memories))
        };
        let note = by_path("note.md");
        assert_eq!(note.reason, RecallReason::Semantic);
        assert_eq!(note.confidence, Confidence::High);
        assert_eq!(note.excerpt, "Orion nebula");
        let words = by_path("words.md");
        assert_eq!(words.reason, RecallReason::Keyword);
        assert_eq!(words.excerpt, "Pleiades cluster");
        let linked = by_path("linked.md");
        assert_eq!(linked.reason, RecallReason::LinkedTo);
        assert_eq!(linked.linked_from.as_deref(), Some("note.md"));
        assert_eq!(linked.confidence, Confidence::Low);
        assert_eq!(linked.excerpt, "Moon phases", "frontmatter is stripped");
        assert_eq!(recall.memories.len(), 3);
        assert_eq!(
            recall.memories[0].path, "note.md",
            "ordered by relevance: {:?}",
            recall.memories
        );
        for memory in &recall.memories {
            assert_eq!(memory.source, memory.path, "text memories cite themselves");
            assert_eq!(memory.source_status, SourceStatus::Present);
            chrono::DateTime::parse_from_rfc3339(&memory.retrieved_at)
                .unwrap_or_else(|e| panic!("{}: {e}", memory.retrieved_at));
        }

        let block = recall.prompt_block().unwrap();
        assert!(
            block.starts_with("[system: auto-recalled memories"),
            "{block}"
        );
        assert!(block.contains("- note.md: Orion nebula\n"), "{block}");
        assert!(block.contains("- linked.md: Moon phases\n"), "{block}");
        assert!(!block.contains("semantic"), "the prompt stays as before");
    }

    #[tokio::test]
    async fn empty_query_recalls_nothing() {
        let workspace = tempfile::tempdir().unwrap();
        memory_dir(workspace.path());
        let store = VectorStore::connect(workspace.path()).await;
        let recall = recall(&store, "one", "   ").await;
        assert!(recall.is_empty());
        assert_eq!(recall.prompt_block(), None);
    }

    #[tokio::test]
    async fn media_memories_cite_their_bound_text_path() {
        let workspace = tempfile::tempdir().unwrap();
        let dir = memory_dir(workspace.path());
        fs::write(dir.join("photo.png"), [0xff]).unwrap();
        media_text::write(&dir, "photo.png", "sky over Lisbon").unwrap();
        // No embedding provider: every hit is a keyword hit.
        let store = VectorStore::connect(workspace.path()).await;

        let recall = recall(&store, "one", "Lisbon").await;
        assert_eq!(recall.memories.len(), 1, "{:?}", recall.memories);
        let photo = &recall.memories[0];
        assert_eq!(photo.path, "photo.png");
        assert_eq!(photo.source, "photo.png.md");
        assert_eq!(photo.reason, RecallReason::Keyword);
        assert_eq!(photo.excerpt, "sky over Lisbon");
        assert_eq!(photo.source_status, SourceStatus::Present);

        let written = write_receipts(
            workspace.path(),
            "one",
            "default",
            &[assistant("msg_1", "nice sky", MessageKind::Message)],
            &recall.memories,
        )
        .unwrap();
        assert_eq!(written, 1);
        fs::remove_file(dir.join("photo.png.md")).unwrap();
        let receipt = read_receipt(
            workspace.path(),
            &store.media_store(),
            "one",
            "default",
            "msg_1",
        )
        .unwrap()
        .unwrap();
        assert_eq!(receipt.memories[0].source, "photo.png.md");
        assert_eq!(receipt.memories[0].source_status, SourceStatus::Missing);

        // With a working embedding provider the channel of a media hit is not
        // attributable from the search result, so it is reported as matched.
        let mock = MockServer::new(vec![(200, response(vec![1., 0., 0.]))]).await;
        media_text::write(&dir, "photo.png", "sky over Lisbon").unwrap();
        let store = VectorStore::connect_with_config(workspace.path(), &mock.config).await;
        let matched = super::recall(&store, "one", "Lisbon").await;
        assert_eq!(matched.memories.len(), 1, "{:?}", matched.memories);
        assert_eq!(matched.memories[0].reason, RecallReason::Matched);
        assert_ne!(matched.memories[0].confidence, Confidence::High);
    }

    #[test]
    fn receipt_round_trips_and_reports_deleted_sources_as_missing() {
        let workspace = tempfile::tempdir().unwrap();
        let dir = memory_dir(workspace.path());
        fs::write(dir.join("note.md"), "Orion nebula").unwrap();
        fs::write(dir.join("gone.md"), "soon deleted").unwrap();
        let media = MediaStore::open(workspace.path()).unwrap();
        let mut linked = recalled("gone.md", RecallReason::LinkedTo);
        linked.linked_from = Some("note.md".into());
        linked.confidence = Confidence::Low;
        let memories = vec![recalled("note.md", RecallReason::Semantic), linked];

        let written = write_receipts(
            workspace.path(),
            "one",
            "default",
            &[assistant("msg_7_0", "remembered", MessageKind::Message)],
            &memories,
        )
        .unwrap();
        assert_eq!(written, 1);
        let path = receipts_dir(workspace.path(), "one", "default").join("msg_7_0.json");
        assert!(path.is_file(), "{}", path.display());
        assert!(
            !path.with_extension("tmp").exists(),
            "atomic write leaves no temp file"
        );
        let raw: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw["message_id"], "msg_7_0");
        assert_eq!(raw["chat_id"], "default");
        assert_eq!(raw["memories"][1]["reason"], "linked_to");
        assert_eq!(raw["memories"][1]["linked_from"], "note.md");
        assert_eq!(raw["memories"][0]["confidence"], "medium");
        assert!(raw["memories"][0].get("score").is_none(), "no raw scores");

        let receipt = read_receipt(workspace.path(), &media, "one", "default", "msg_7_0")
            .unwrap()
            .unwrap();
        assert_eq!(
            receipt,
            MemoryReceipt {
                message_id: "msg_7_0".into(),
                chat_id: "default".into(),
                memories: memories.clone(),
            }
        );

        fs::remove_file(dir.join("gone.md")).unwrap();
        let receipt = read_receipt(workspace.path(), &media, "one", "default", "msg_7_0")
            .unwrap()
            .unwrap();
        assert_eq!(receipt.memories[0].source_status, SourceStatus::Present);
        assert_eq!(receipt.memories[1].source_status, SourceStatus::Missing);
        assert_eq!(receipt.memories[1].excerpt, "excerpt of gone.md");

        let listed = list_receipts(workspace.path(), &media, "one", "default").unwrap();
        assert_eq!(listed, vec![receipt]);
        assert_eq!(
            read_receipt(workspace.path(), &media, "one", "default", "msg_none").unwrap(),
            None
        );
        assert_eq!(
            list_receipts(workspace.path(), &media, "one", "other").unwrap(),
            Vec::new()
        );
    }

    #[test]
    fn one_receipt_per_assistant_text_message_and_unsafe_ids_are_skipped() {
        let workspace = tempfile::tempdir().unwrap();
        let dir = memory_dir(workspace.path());
        fs::write(dir.join("note.md"), "Orion nebula").unwrap();
        let media = MediaStore::open(workspace.path()).unwrap();
        let memories = vec![recalled("note.md", RecallReason::Keyword)];
        let messages = vec![
            user("msg_1"),
            assistant("msg_2", "[tool: run_command]", MessageKind::ToolCall),
            assistant("msg_2_1", "output", MessageKind::ToolOutput),
            assistant("msg_3", "first bubble", MessageKind::Message),
            assistant("msg_3_1", "second bubble", MessageKind::Message),
            assistant("../escape", "unsafe id", MessageKind::Message),
        ];

        let written =
            write_receipts(workspace.path(), "one", "default", &messages, &memories).unwrap();
        assert_eq!(written, 2);
        let mut names: Vec<String> = fs::read_dir(receipts_dir(workspace.path(), "one", "default"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, vec!["msg_3.json", "msg_3_1.json"]);
        assert!(
            !workspace
                .path()
                .join("instances/one/chats/default/escape.json")
                .exists()
        );

        // Ids that could leave the chat directory never resolve to a file.
        for (chat_id, message_id) in [
            ("default", "../../other/chats/default/receipts/msg_3"),
            ("../other/chats/default", "msg_3"),
            ("default", ""),
            ("default", "msg_3.json"),
        ] {
            assert_eq!(
                read_receipt(workspace.path(), &media, "one", chat_id, message_id).unwrap(),
                None,
                "{chat_id}/{message_id}"
            );
        }
        assert!(
            list_receipts(workspace.path(), &media, "one", "../other")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn receipt_layout_is_documented() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let doc = fs::read_to_string(repo.join("docs/companion-storage.md")).unwrap();
        for required in ["#84", "receipts/", "source_status", "missing", "linked_to"] {
            assert!(
                doc.contains(required),
                "storage doc is missing {required:?}"
            );
        }
    }
}
