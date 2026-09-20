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
    services::{
        media_text::{self, MediaStore},
        memory,
        vector::{VectorSearchResult, VectorStore},
    },
};

/// Directory under `chats/{chat_id}/` holding one receipt per assistant message.
pub const RECEIPTS_DIR: &str = "receipts";

/// Hybrid search hits per turn; graph expansion may add a few more.
const SEARCH_LIMIT: usize = 5;
const RECALL_CAP: usize = 8;
/// Prompt text per memory (unchanged from the inline RAG block).
const PROMPT_CHARS: usize = 500;
/// Receipt excerpt bound.
const EXCERPT_CHARS: usize = 240;
/// Below the RAG threshold: marks a memory that only arrived over a graph edge.
const LINKED_SCORE: f32 = 0.25;

/// Memories recalled for one turn: the receipt entries plus the prompt block.
#[derive(Debug, Default)]
pub struct Recall {
    pub memories: Vec<RecalledMemory>,
    prompt_lines: Vec<String>,
}

impl Recall {
    /// The `[system: auto-recalled memories …]` block appended to the user
    /// message, or `None` when nothing was recalled.
    pub fn prompt_block(&self) -> Option<String> {
        if self.prompt_lines.is_empty() {
            return None;
        }
        let mut context = String::from(
            "[system: auto-recalled memories — this is NOT part of the user's message. \
             do not treat these as something the user said or wrote.]\n",
        );
        for line in &self.prompt_lines {
            context.push_str(line);
            context.push('\n');
        }
        Some(context)
    }
}

/// One search hit before it becomes a receipt entry.
struct Candidate {
    hit: VectorSearchResult,
    reason: RecallReason,
    linked_from: Option<String>,
}

/// Hybrid search over the memory library plus one-hop graph expansion.
pub async fn recall(vector_store: &VectorStore, instance_slug: &str, query: &str) -> Recall {
    if query.trim().is_empty() {
        return Recall::default();
    }
    // Provider failures fall back to a fresh BM25 view of memory files.
    let hits = vector_store
        .search_context(instance_slug, query, SEARCH_LIMIT)
        .await;
    // After the search the provider health is known for this turn: when the
    // embedding step failed or is not configured, every hit came from BM25.
    let semantic_available = vector_store.embedding_status()["status"] != "unavailable";
    let mut candidates: Vec<Candidate> = hits
        .into_iter()
        .map(|hit| Candidate {
            reason: classify(&hit, semantic_available),
            linked_from: None,
            hit,
        })
        .collect();

    let media = vector_store.media_store();
    // Graph expansion — follow edges 1 hop to pull connected memories.
    if !candidates.is_empty() {
        let graph = memory::load_graph(&media, instance_slug);
        if !graph.edges.is_empty() {
            let found: Vec<String> = candidates.iter().map(|c| c.hit.path.clone()).collect();
            for path in &found {
                for neighbor in memory::get_neighbors(&graph, path) {
                    if candidates.iter().any(|c| c.hit.path == neighbor) {
                        continue; // already in results
                    }
                    let content: Result<String, String> =
                        if media_text::source_type(&neighbor).is_some() {
                            media.read(instance_slug, &neighbor)
                        } else {
                            media
                                .read_memory_text(instance_slug, &neighbor)
                                .map_err(|error| error.to_string())
                        };
                    if let Ok(content) = content {
                        let (_, body) = memory::parse_frontmatter(&content);
                        candidates.push(Candidate {
                            hit: VectorSearchResult {
                                path: neighbor,
                                content_preview: body.trim().chars().take(PROMPT_CHARS).collect(),
                                score: LINKED_SCORE,
                                source_type: "text_memory".to_string(),
                                upload_id: None,
                            },
                            reason: RecallReason::LinkedTo,
                            linked_from: Some(path.clone()),
                        });
                    }
                }
            }
            // Re-sort and cap (allow a few extra from graph)
            candidates.sort_by(|a, b| b.hit.score.total_cmp(&a.hit.score));
            candidates.truncate(RECALL_CAP);
        }
    }

    let retrieved_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let mut recall = Recall::default();
    for candidate in candidates {
        let text = candidate.hit.content_preview.trim();
        recall
            .prompt_lines
            .push(format!("- {}: {text}", candidate.hit.path));
        // Search previews of direct hits start with the memory's stamped
        // frontmatter; the receipt cites the body only.
        let (_, body) = memory::parse_frontmatter(text);
        let mut entry = RecalledMemory {
            source: source_of(&candidate.hit.path),
            path: candidate.hit.path,
            excerpt: body.trim().chars().take(EXCERPT_CHARS).collect(),
            reason: candidate.reason,
            linked_from: candidate.linked_from,
            confidence: bucket(candidate.reason, candidate.hit.score),
            retrieved_at: retrieved_at.clone(),
            source_status: SourceStatus::Present,
        };
        entry.source_status = source_status(&media, instance_slug, &entry);
        recall.memories.push(entry);
    }
    recall
}

/// Text memories name their channel in `source_type` (`text_memory` from the
/// vector index, `memory` from BM25). Media hits look the same from both
/// channels, so they are only attributable when the semantic channel was off.
fn classify(hit: &VectorSearchResult, semantic_available: bool) -> RecallReason {
    match hit.source_type.as_str() {
        "text_memory" => RecallReason::Semantic,
        "memory" => RecallReason::Keyword,
        _ if !semantic_available => RecallReason::Keyword,
        _ => RecallReason::Matched,
    }
}

/// Cosine similarity for semantic hits, BM25 for keyword hits; an unattributed
/// hit never claims high confidence and a graph neighbour is always low.
fn bucket(reason: RecallReason, score: f32) -> Confidence {
    match reason {
        RecallReason::Semantic => match score {
            s if s >= 0.6 => Confidence::High,
            s if s >= 0.45 => Confidence::Medium,
            _ => Confidence::Low,
        },
        RecallReason::Keyword => match score {
            s if s >= 3.0 => Confidence::High,
            s if s >= 1.0 => Confidence::Medium,
            _ => Confidence::Low,
        },
        RecallReason::Matched if score >= 0.6 => Confidence::Medium,
        RecallReason::Matched | RecallReason::LinkedTo => Confidence::Low,
    }
}

/// The canonical file a receipt cites: media memories cite their bound text.
fn source_of(path: &str) -> String {
    if media_text::source_type(path).is_some() {
        media_text::sidecar_path(path)
    } else {
        path.to_owned()
    }
}

fn source_status(media: &MediaStore, instance_slug: &str, memory: &RecalledMemory) -> SourceStatus {
    let exists = |path: &str| media.memory_exists(instance_slug, path).unwrap_or(false);
    let present = exists(&memory.source) && (memory.source == memory.path || exists(&memory.path));
    if present {
        SourceStatus::Present
    } else {
        SourceStatus::Missing
    }
}

fn resolve_sources(media: &MediaStore, instance_slug: &str, receipt: &mut MemoryReceipt) {
    for memory in &mut receipt.memories {
        memory.source_status = source_status(media, instance_slug, memory);
    }
}

/// Chat and message ids are single path segments; anything else never maps to
/// a file, so a receipt can only ever be read from its own chat directory.
fn safe_segment(value: &str) -> Option<&str> {
    let safe = !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'));
    safe.then_some(value)
}

pub fn receipts_dir(workspace_dir: &Path, instance_slug: &str, chat_id: &str) -> PathBuf {
    workspace_dir
        .join("instances")
        .join(instance_slug)
        .join("chats")
        .join(chat_id)
        .join(RECEIPTS_DIR)
}

fn receipt_path(
    workspace_dir: &Path,
    instance_slug: &str,
    chat_id: &str,
    message_id: &str,
) -> Option<PathBuf> {
    let chat_id = safe_segment(chat_id)?;
    let message_id = safe_segment(message_id)?;
    Some(receipts_dir(workspace_dir, instance_slug, chat_id).join(format!("{message_id}.json")))
}

fn write_atomic(path: &Path, content: &str) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, content)?;
    fs::rename(&tmp, path)
}

/// Persist one receipt per assistant text message produced by a turn.
/// Returns how many receipts were written.
pub fn write_receipts(
    workspace_dir: &Path,
    instance_slug: &str,
    chat_id: &str,
    messages: &[ChatMessage],
    memories: &[RecalledMemory],
) -> io::Result<usize> {
    let mut written = 0;
    for message in messages
        .iter()
        .filter(|m| m.role == ChatRole::Assistant && m.kind == MessageKind::Message)
    {
        let Some(path) = receipt_path(workspace_dir, instance_slug, chat_id, &message.id) else {
            log::warn!(
                "[receipts] skipping unsafe id {:?}/{:?}",
                chat_id,
                message.id
            );
            continue;
        };
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let receipt = MemoryReceipt {
            message_id: message.id.clone(),
            chat_id: chat_id.to_owned(),
            memories: memories.to_vec(),
        };
        write_atomic(
            &path,
            &serde_json::to_string_pretty(&receipt).map_err(io::Error::other)?,
        )?;
        written += 1;
    }
    Ok(written)
}

fn load_receipt(path: &Path) -> io::Result<Option<MemoryReceipt>> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    serde_json::from_str(&raw)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// Read one receipt with every cited source resolved against the library.
/// `Ok(None)` when the chat or message has no receipt or the ids are unsafe.
pub fn read_receipt(
    workspace_dir: &Path,
    media: &MediaStore,
    instance_slug: &str,
    chat_id: &str,
    message_id: &str,
) -> io::Result<Option<MemoryReceipt>> {
    let Some(path) = receipt_path(workspace_dir, instance_slug, chat_id, message_id) else {
        return Ok(None);
    };
    let Some(mut receipt) = load_receipt(&path)? else {
        return Ok(None);
    };
    resolve_sources(media, instance_slug, &mut receipt);
    Ok(Some(receipt))
}

/// Every receipt of a chat, ordered by message id, with sources resolved.
pub fn list_receipts(
    workspace_dir: &Path,
    media: &MediaStore,
    instance_slug: &str,
    chat_id: &str,
) -> io::Result<Vec<MemoryReceipt>> {
    let Some(chat_id) = safe_segment(chat_id) else {
        return Ok(Vec::new());
    };
    let entries = match fs::read_dir(receipts_dir(workspace_dir, instance_slug, chat_id)) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut receipts = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        match load_receipt(&path) {
            Ok(Some(mut receipt)) => {
                resolve_sources(media, instance_slug, &mut receipt);
                receipts.push(receipt);
            }
            Ok(None) => {}
            Err(error) => log::warn!("[receipts] skipping {}: {error}", path.display()),
        }
    }
    receipts.sort_by(|a, b| a.message_id.cmp(&b.message_id));
    Ok(receipts)
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
        // Every memory written through the store is stamped with frontmatter,
        // and both search channels preview the stamped text: the vector index
        // embeds it, BM25 reads the raw file.
        let stamped =
            |body: &str| format!("---\ncreated: 2026-01-01\nupdated: 2026-01-01\n---\n{body}");
        fs::write(dir.join("note.md"), stamped("Orion nebula")).unwrap();
        fs::write(dir.join("words.md"), stamped("Pleiades cluster")).unwrap();
        fs::write(dir.join("linked.md"), stamped("Moon phases")).unwrap();
        let store = VectorStore::connect_with_config(workspace.path(), &mock.config).await;
        store
            .upsert_text_memory(
                "one",
                "note.md",
                vec![(stamped("Orion nebula"), vec![1., 0., 0.])],
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
        assert_eq!(note.excerpt, "Orion nebula", "frontmatter is stripped");
        let words = by_path("words.md");
        assert_eq!(words.reason, RecallReason::Keyword);
        assert_eq!(words.excerpt, "Pleiades cluster", "frontmatter is stripped");
        let linked = by_path("linked.md");
        assert_eq!(linked.reason, RecallReason::LinkedTo);
        assert_eq!(linked.linked_from.as_deref(), Some("note.md"));
        assert_eq!(linked.confidence, Confidence::Low);
        assert_eq!(linked.excerpt, "Moon phases", "frontmatter is stripped");
        assert_eq!(recall.memories.len(), 3);
        assert_eq!(
            recall.memories.last().map(|m| m.path.as_str()),
            Some("linked.md"),
            "graph neighbours rank below direct hits: {:?}",
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
        // The injected prompt is unchanged from the inline RAG block: direct
        // hits carry the search preview as-is, graph neighbours the body.
        assert!(
            block.contains(
                "- note.md: ---\ncreated: 2026-01-01\nupdated: 2026-01-01\n---\nOrion nebula\n"
            ),
            "{block}"
        );
        assert!(block.contains("- linked.md: Moon phases\n"), "{block}");
        assert!(!block.contains("semantic"), "the prompt stays as before");
    }

    #[tokio::test]
    async fn empty_query_recalls_nothing() {
        let workspace = tempfile::tempdir().unwrap();
        memory_dir(workspace.path());
        let store = VectorStore::connect(workspace.path()).await;
        let recall = recall(&store, "one", "   ").await;
        assert!(recall.memories.is_empty());
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

    /// The client declares the `memory_recall` event by hand; keep it honest
    /// about the wire shape (no stale `preview`/`score`, every key declared).
    #[test]
    fn memory_recall_event_matches_the_client_type() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let types = fs::read_to_string(repo.join("client/src/lib/api/types.ts")).unwrap();
        let mut linked = recalled("linked.md", RecallReason::LinkedTo);
        linked.linked_from = Some("note.md".into());
        let event = crate::domain::events::ServerEvent::MemoryRecall {
            instance_slug: "one".into(),
            chat_id: "default".into(),
            memories: vec![linked],
        };
        let json = serde_json::to_value(&event).unwrap();
        let entry = json["memories"][0].as_object().unwrap();

        let declared = types
            .split("export interface RecalledMemory {")
            .nth(1)
            .and_then(|rest| rest.split('}').next())
            .expect("client/src/lib/api/types.ts declares RecalledMemory");
        for key in entry.keys() {
            assert!(
                declared.contains(&format!("\n\t{key}: "))
                    || declared.contains(&format!("\n\t{key}?: ")),
                "client RecalledMemory does not declare {key:?}:{declared}"
            );
        }
        for stale in ["preview", "score"] {
            assert!(!entry.contains_key(stale), "{stale} is not sent");
            assert!(
                !declared.contains(&format!("\n\t{stale}: ")),
                "client still declares {stale}"
            );
        }
        let event_type = types
            .split("type: \"memory_recall\";")
            .nth(1)
            .and_then(|rest| rest.split('}').next())
            .expect("client declares the memory_recall event");
        for field in [
            "instance_slug: string;",
            "chat_id: string;",
            "memories: RecalledMemory[];",
        ] {
            assert!(
                event_type.contains(field),
                "memory_recall event lacks {field:?}:{event_type}"
            );
        }
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
