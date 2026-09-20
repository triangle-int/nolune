//! Memory correction controls (#84): correct, pin, exclude, and a conflict
//! ledger.
//!
//! Every action here rewrites the canonical memory file the user owns and
//! reconciles the derived search state through the vector store's existing
//! delete and re-index calls, so the next recall sees the corrected text and
//! never the old one. Corrections are recorded in a small versioned ledger
//! next to the memory store (`memory_corrections.json`, see
//! `docs/companion-storage.md`). A second, different correction of a memory
//! whose earlier correction is still in force is not merged: it is parked as
//! `needs_resolution` and the user chooses which statement stays
//! authoritative through [`resolve`]. Flags (`pinned`,
//! `exclude_from_proactive`) live in the memory's frontmatter, so they
//! survive the companion's own rewrites and a server restart.
//!
//! A text memory is read, rewritten, and re-indexed under the vector store's
//! per-companion lifecycle gate, the one its own `write_text_memory` and
//! `delete_memory` hold, so a correction or flag change never interleaves
//! with the companion's read-modify-write and never recreates a memory a
//! forget removed in the meantime. The ledger is bounded on the write side
//! (`MAX_ENTRIES`, `MAX_LEDGER_BYTES`) so it can always be read back.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};

use crate::{
    domain::{
        correction::{
            CorrectionConflict, CorrectionEntry, CorrectionLedger, CorrectionStatement,
            CorrectionStatus, Keep, LEDGER_VERSION,
        },
        memory::MemoryFlags,
    },
    services::{media_text, memory, vector::VectorStore},
};

/// Ledger file next to `memory/`, addressed through the workspace capability.
pub const LEDGER_FILE: &str = "memory_corrections.json";
/// A correction statement larger than this is refused (`TooLarge`).
pub const MAX_STATEMENT_BYTES: usize = 64 * 1024;
/// Excerpt of the previous text kept per entry.
const PREVIOUS_CHARS: usize = 240;
/// Entries kept in the ledger; the oldest droppable ones go beyond it.
const MAX_ENTRIES: usize = 500;
/// Bytes the ledger file may hold, enforced when it is written so a read
/// with the same bound never fails on a file this server wrote.
const MAX_LEDGER_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorrectionError {
    NotFound,
    Invalid(String),
    TooLarge,
    /// The ledger holds nothing but open questions and cannot record one
    /// more; resolve some first.
    LedgerFull,
    Io(String),
}

impl std::fmt::Display for CorrectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => f.write_str("memory not found"),
            Self::Invalid(message) | Self::Io(message) => f.write_str(message),
            Self::TooLarge => write!(f, "statement exceeds {MAX_STATEMENT_BYTES} bytes"),
            Self::LedgerFull => f.write_str(
                "the corrections ledger is full of open conflicts; resolve some before correcting more",
            ),
        }
    }
}

impl std::error::Error for CorrectionError {}

/// What a correction did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorrectionOutcome {
    /// The file was rewritten and re-indexed; the entry is `applied`.
    Applied(CorrectionEntry),
    /// The memory already said this; nothing was written or recorded.
    Unchanged,
    /// Parked: an earlier correction is still in force and differs.
    NeedsResolution(CorrectionConflict),
}

/// Flag changes; `None` leaves a flag as it is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
pub struct FlagUpdate {
    #[serde(default)]
    pub pinned: Option<bool>,
    #[serde(default)]
    pub exclude_from_proactive: Option<bool>,
}

/// The entry a resolution settled on, plus what the user kept.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Resolution {
    pub kept: Keep,
    pub path: String,
    pub entry: CorrectionEntry,
}

/// One lock per companion for the ledger read-modify-write and the file
/// rewrite around it, whichever store handle asks.
fn companion_lock(instance_slug: &str) -> Arc<tokio::sync::Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();
    let mut locks = LOCKS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    Arc::clone(locks.entry(instance_slug.to_owned()).or_default())
}

/// The ledger as persisted; missing reads as empty, another version or a
/// malformed file is reported and left alone.
pub fn load_ledger(
    media: &media_text::MediaStore,
    instance_slug: &str,
) -> Result<CorrectionLedger, CorrectionError> {
    let raw = match media.read_instance_text(instance_slug, LEDGER_FILE, MAX_LEDGER_BYTES) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CorrectionLedger::default());
        }
        Err(error) => return Err(CorrectionError::Io(format!("read {LEDGER_FILE}: {error}"))),
    };
    let ledger: CorrectionLedger = serde_json::from_str(&raw).map_err(|error| {
        CorrectionError::Invalid(format!("{LEDGER_FILE} is malformed: {error}"))
    })?;
    if ledger.version != LEDGER_VERSION {
        return Err(CorrectionError::Invalid(format!(
            "{LEDGER_FILE} version {} is unsupported (this server writes {LEDGER_VERSION})",
            ledger.version
        )));
    }
    Ok(ledger)
}

/// The entry to drop when the ledger is over a bound: the oldest settled
/// one, else the oldest applied one nothing open points at (its memory keeps
/// its text; the ledger just no longer remembers the correction, so the next
/// one applies as if it were the first). Open questions are never dropped.
fn droppable(ledger: &CorrectionLedger) -> Option<usize> {
    let settled = ledger.entries.iter().position(|entry| {
        matches!(
            entry.status,
            CorrectionStatus::Superseded | CorrectionStatus::Withdrawn
        )
    });
    settled.or_else(|| {
        ledger.entries.iter().position(|entry| {
            entry.status == CorrectionStatus::Applied
                && !ledger.entries.iter().any(|other| {
                    other.status == CorrectionStatus::NeedsResolution
                        && other.conflicts_with.as_deref() == Some(entry.id.as_str())
                })
        })
    })
}

/// Serialize the ledger within its bounds, dropping entries as
/// [`droppable`] says until it fits. `LedgerFull` when nothing can go.
fn encode_ledger(ledger: &mut CorrectionLedger) -> Result<String, CorrectionError> {
    loop {
        if ledger.entries.len() <= MAX_ENTRIES {
            let json = serde_json::to_string_pretty(ledger)
                .map_err(|error| CorrectionError::Io(error.to_string()))?;
            if json.len() <= MAX_LEDGER_BYTES {
                return Ok(json);
            }
        }
        let at = droppable(ledger).ok_or(CorrectionError::LedgerFull)?;
        ledger.entries.remove(at);
    }
}

fn write_ledger(
    media: &media_text::MediaStore,
    instance_slug: &str,
    json: &str,
) -> Result<(), CorrectionError> {
    media
        .write_instance_text(instance_slug, LEDGER_FILE, json)
        .map_err(|error| CorrectionError::Io(format!("write {LEDGER_FILE}: {error}")))
}

fn save_ledger(
    media: &media_text::MediaStore,
    instance_slug: &str,
    ledger: &mut CorrectionLedger,
) -> Result<(), CorrectionError> {
    let json = encode_ledger(ledger)?;
    write_ledger(media, instance_slug, &json)
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn new_id() -> String {
    format!(
        "corr_{}_{}",
        chrono::Utc::now().timestamp(),
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    )
}

fn excerpt(text: &str) -> String {
    text.trim().chars().take(PREVIOUS_CHARS).collect()
}

/// The memory as it is on disk: the text the user sees and, for a text
/// memory, the frontmatter around it.
struct Current {
    body: String,
    /// `None` for a media memory (its bound text carries no frontmatter).
    frontmatter: Option<memory::Frontmatter>,
}

fn io_error(error: std::io::Error) -> CorrectionError {
    match error.kind() {
        std::io::ErrorKind::NotFound => CorrectionError::NotFound,
        std::io::ErrorKind::InvalidInput => CorrectionError::Invalid(error.to_string()),
        _ => CorrectionError::Io(error.to_string()),
    }
}

/// The companion's lifecycle gate, held for a text memory from the read
/// through the re-index. A media memory's bound text goes through
/// `edit_media_text`, which takes the gate itself.
async fn text_gate(
    store: &VectorStore,
    instance_slug: &str,
    path: &str,
) -> Option<tokio::sync::OwnedMutexGuard<()>> {
    if media_text::source_type(path).is_some() {
        return None;
    }
    Some(store.lifecycle_lock(instance_slug).lock_owned().await)
}

fn read_current(
    media: &media_text::MediaStore,
    instance_slug: &str,
    path: &str,
) -> Result<Current, CorrectionError> {
    if media_text::source_type(path).is_some() {
        // A media memory: the owner must exist; missing bound text reads as empty.
        if !media.memory_exists(instance_slug, path).map_err(io_error)? {
            return Err(CorrectionError::NotFound);
        }
        return Ok(Current {
            body: media.read(instance_slug, path).unwrap_or_default(),
            frontmatter: None,
        });
    }
    if media_text::media_path(path).is_some() {
        return Err(CorrectionError::Invalid(
            "correct the media memory itself, not its bound text".into(),
        ));
    }
    let content = media
        .read_memory_text(instance_slug, path)
        .map_err(io_error)?;
    let (frontmatter, body) = memory::parse_frontmatter(&content);
    Ok(Current {
        body: body.to_owned(),
        frontmatter: Some(frontmatter),
    })
}

/// Write `statement` as the memory's text, keeping the frontmatter flags,
/// and reconcile the derived index. The caller holds the [`text_gate`].
async fn rewrite(
    store: &VectorStore,
    instance_slug: &str,
    path: &str,
    current: &Current,
    statement: &str,
) -> Result<(), CorrectionError> {
    let Some(frontmatter) = &current.frontmatter else {
        // Media: the bound text is replaced and re-indexed under the lifecycle gate.
        return store
            .edit_media_text(instance_slug, path, statement, false)
            .await
            .map_err(CorrectionError::Io);
    };
    let existing = memory::render_frontmatter(frontmatter, &current.body);
    let stamped = memory::stamp_content_with_flags(statement, Some(&existing), frontmatter.flags);
    write_text(store, instance_slug, path, &stamped).await
}

/// Write a text memory's file as given and reconcile the derived index under
/// the gate the caller holds. The vector index replaces the path's records in
/// one mutation, or removes them and marks the collection for backfill when
/// the provider fails, so the old text is never searchable after the file
/// changed; BM25 is invalidated with it and re-reads the file.
async fn write_text(
    store: &VectorStore,
    instance_slug: &str,
    path: &str,
    stamped: &str,
) -> Result<(), CorrectionError> {
    store
        .media_store()
        .write_memory_text(instance_slug, path, stamped)
        .map_err(io_error)?;
    if let Err(error) = store.index_text(instance_slug, path, stamped).await {
        log::warn!("[memory_corrections] semantic re-index of {path} pending: {error}");
    }
    Ok(())
}

/// The entry in force for `path`: the latest applied one, provided the
/// memory still reads exactly as it wrote it.
fn in_force<'a>(
    ledger: &'a CorrectionLedger,
    path: &str,
    body: &str,
) -> Option<&'a CorrectionEntry> {
    ledger
        .entries
        .iter()
        .rfind(|entry| entry.path == path && entry.status == CorrectionStatus::Applied)
        .filter(|entry| entry.statement.trim() == body.trim())
}

/// Let go of what the memory no longer holds. An applied entry whose text
/// the memory moved away from (the companion rewrote it, or it was forgotten
/// and recreated) is superseded, and so is any question parked against it:
/// the memory reads as neither statement, so there is nothing left to
/// choose between and the next correction applies directly. Returns whether
/// the ledger changed.
fn release_stale(ledger: &mut CorrectionLedger, path: &str, body: &str) -> bool {
    let in_force = in_force(ledger, path, body).map(|entry| entry.id.clone());
    let mut changed = false;
    for entry in ledger.entries.iter_mut().filter(|entry| entry.path == path) {
        let stale = match entry.status {
            CorrectionStatus::Applied => in_force.as_deref() != Some(entry.id.as_str()),
            CorrectionStatus::NeedsResolution => {
                entry.conflicts_with.as_deref() != in_force.as_deref()
            }
            CorrectionStatus::Superseded | CorrectionStatus::Withdrawn => false,
        };
        if stale {
            if entry.status == CorrectionStatus::NeedsResolution {
                entry.resolved_at = Some(now());
            }
            entry.status = CorrectionStatus::Superseded;
            changed = true;
        }
    }
    changed
}

fn pending<'a>(ledger: &'a CorrectionLedger, path: &str) -> Option<&'a CorrectionEntry> {
    ledger
        .entries
        .iter()
        .find(|entry| entry.path == path && entry.status == CorrectionStatus::NeedsResolution)
}

fn conflict_of(ledger: &CorrectionLedger, proposed: &CorrectionEntry) -> CorrectionConflict {
    let current = proposed
        .conflicts_with
        .as_deref()
        .and_then(|id| ledger.entries.iter().find(|entry| entry.id == id))
        .map(CorrectionStatement::from)
        .unwrap_or_else(|| CorrectionStatement {
            id: String::new(),
            statement: String::new(),
            corrected_at: String::new(),
        });
    CorrectionConflict {
        conflict_id: proposed.id.clone(),
        path: proposed.path.clone(),
        current,
        proposed: CorrectionStatement::from(proposed),
    }
}

/// Rewrite a memory with the user's statement and record it.
pub async fn correct(
    store: &VectorStore,
    instance_slug: &str,
    path: &str,
    statement: &str,
) -> Result<CorrectionOutcome, CorrectionError> {
    if statement.len() > MAX_STATEMENT_BYTES {
        return Err(CorrectionError::TooLarge);
    }
    let statement = statement.trim();
    if statement.is_empty() {
        return Err(CorrectionError::Invalid("statement cannot be empty".into()));
    }
    let _guard = companion_lock(instance_slug).lock_owned().await;
    let _gate = text_gate(store, instance_slug, path).await;
    let media = store.media_store();
    let current = read_current(&media, instance_slug, path)?;
    if current.body.trim() == statement {
        return Ok(CorrectionOutcome::Unchanged);
    }
    let mut ledger = load_ledger(&media, instance_slug)?;
    let released = release_stale(&mut ledger, path, &current.body);

    // One open question per memory: further statements point at it. Its
    // `current` side is the entry in force, so it always reads as the file.
    if let Some(parked) = pending(&ledger, path) {
        let conflict = conflict_of(&ledger, parked);
        if released {
            save_ledger(&media, instance_slug, &mut ledger)?;
        }
        return Ok(CorrectionOutcome::NeedsResolution(conflict));
    }
    if let Some(current_entry) = in_force(&ledger, path, &current.body) {
        let proposed = CorrectionEntry {
            id: new_id(),
            path: path.to_owned(),
            statement: statement.to_owned(),
            previous: excerpt(&current.body),
            status: CorrectionStatus::NeedsResolution,
            corrected_at: now(),
            resolved_at: None,
            conflicts_with: Some(current_entry.id.clone()),
        };
        let conflict = conflict_of(&ledger, &proposed);
        ledger.entries.push(proposed);
        save_ledger(&media, instance_slug, &mut ledger)?;
        return Ok(CorrectionOutcome::NeedsResolution(conflict));
    }

    let entry = CorrectionEntry {
        id: new_id(),
        path: path.to_owned(),
        statement: statement.to_owned(),
        previous: excerpt(&current.body),
        status: CorrectionStatus::Applied,
        corrected_at: now(),
        resolved_at: None,
        conflicts_with: None,
    };
    // The ledger must be able to record the correction before the memory
    // changes; a full ledger refuses without touching the file.
    ledger.entries.push(entry.clone());
    let json = encode_ledger(&mut ledger)?;
    rewrite(store, instance_slug, path, &current, statement).await?;
    write_ledger(&media, instance_slug, &json)?;
    Ok(CorrectionOutcome::Applied(entry))
}

/// Settle a `needs_resolution` entry.
pub async fn resolve(
    store: &VectorStore,
    instance_slug: &str,
    conflict_id: &str,
    keep: Keep,
) -> Result<Resolution, CorrectionError> {
    let _guard = companion_lock(instance_slug).lock_owned().await;
    let media = store.media_store();
    let mut ledger = load_ledger(&media, instance_slug)?;
    let at = ledger
        .entries
        .iter()
        .position(|entry| {
            entry.id == conflict_id && entry.status == CorrectionStatus::NeedsResolution
        })
        .ok_or(CorrectionError::NotFound)?;
    let path = ledger.entries[at].path.clone();
    let resolved_at = now();
    match keep {
        Keep::Current => {
            let entry = &mut ledger.entries[at];
            entry.status = CorrectionStatus::Withdrawn;
            entry.resolved_at = Some(resolved_at);
        }
        Keep::Proposed => {
            let _gate = text_gate(store, instance_slug, &path).await;
            let current = read_current(&media, instance_slug, &path)?;
            let statement = ledger.entries[at].statement.clone();
            rewrite(store, instance_slug, &path, &current, &statement).await?;
            for entry in &mut ledger.entries {
                if entry.path == path && entry.status == CorrectionStatus::Applied {
                    entry.status = CorrectionStatus::Superseded;
                }
            }
            let entry = &mut ledger.entries[at];
            entry.status = CorrectionStatus::Applied;
            entry.previous = excerpt(&current.body);
            entry.resolved_at = Some(resolved_at);
        }
    }
    let entry = ledger.entries[at].clone();
    save_ledger(&media, instance_slug, &mut ledger)?;
    Ok(Resolution {
        kept: keep,
        path,
        entry,
    })
}

/// Set `pinned` / `exclude_from_proactive` on a text memory and re-index it.
/// Returns the flags now on the file.
pub async fn set_flags(
    store: &VectorStore,
    instance_slug: &str,
    path: &str,
    update: FlagUpdate,
) -> Result<MemoryFlags, CorrectionError> {
    let _guard = companion_lock(instance_slug).lock_owned().await;
    let _gate = text_gate(store, instance_slug, path).await;
    let media = store.media_store();
    let current = read_current(&media, instance_slug, path)?;
    let Some(mut frontmatter) = current.frontmatter else {
        return Err(CorrectionError::Invalid(
            "pinned and exclude_from_proactive apply to text memories only".into(),
        ));
    };
    let flags = MemoryFlags {
        pinned: update.pinned.unwrap_or(frontmatter.flags.pinned),
        exclude_from_proactive: update
            .exclude_from_proactive
            .unwrap_or(frontmatter.flags.exclude_from_proactive),
    };
    if flags == frontmatter.flags {
        return Ok(flags);
    }
    frontmatter.flags = flags;
    // A flag is not a content change: created/updated stay as they are, and
    // a legacy file without frontmatter is stamped as of today.
    let stamped = if frontmatter.created.is_some() || frontmatter.updated.is_some() {
        memory::render_frontmatter(&frontmatter, &current.body)
    } else {
        memory::stamp_content_with_flags(&current.body, None, flags)
    };
    write_text(store, instance_slug, path, &stamped).await?;
    Ok(flags)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{
        embedding::tests::{MockServer, response},
        media_text::MediaStore,
    };
    use std::{fs, path::Path};

    const STAMPED: &str = "---\ncreated: 2026-01-01\nupdated: 2026-01-02\nexclude_from_proactive: true\n---\nlikes tea\n";

    fn seed(workspace: &Path) -> std::path::PathBuf {
        let dir = workspace.join("instances/one/memory/about");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("tea.md"), STAMPED).unwrap();
        dir
    }

    fn body_of(workspace: &Path, path: &str) -> String {
        let raw = fs::read_to_string(workspace.join("instances/one/memory").join(path)).unwrap();
        memory::parse_frontmatter(&raw).1.to_owned()
    }

    fn ledger(workspace: &Path) -> CorrectionLedger {
        let media = MediaStore::open(workspace).unwrap();
        load_ledger(&media, "one").unwrap()
    }

    #[tokio::test]
    async fn correction_rewrites_the_canonical_file_and_reindexes() {
        // One document embedding for the re-index, one query embedding for
        // the search that must see the corrected text.
        let mock = MockServer::new(vec![
            (200, response(vec![1., 0., 0.])),
            (200, response(vec![1., 0., 0.])),
        ])
        .await;
        let ws = tempfile::tempdir().unwrap();
        seed(ws.path());
        let store = VectorStore::connect_with_config(ws.path(), &mock.config).await;
        store
            .upsert_text_memory(
                "one",
                "about/tea.md",
                vec![(STAMPED.to_owned(), vec![1., 0., 0.])],
            )
            .await
            .unwrap();
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

        let outcome = correct(&store, "one", "about/tea.md", "likes oolong")
            .await
            .unwrap();
        let CorrectionOutcome::Applied(entry) = outcome else {
            panic!("expected an applied correction, got {outcome:?}");
        };
        assert_eq!(entry.path, "about/tea.md");
        assert_eq!(entry.statement, "likes oolong");
        assert_eq!(entry.previous, "likes tea");
        assert_eq!(entry.status, CorrectionStatus::Applied);
        assert!(entry.id.starts_with("corr_"), "{}", entry.id);
        assert!(entry.corrected_at.ends_with('Z'), "{}", entry.corrected_at);

        // The canonical file: new body, created kept, updated bumped, flags kept.
        let raw = fs::read_to_string(ws.path().join("instances/one/memory/about/tea.md")).unwrap();
        assert_eq!(
            raw,
            format!(
                "---\ncreated: 2026-01-01\nupdated: {today}\nexclude_from_proactive: true\n---\nlikes oolong"
            )
        );

        // Derived state: the semantic hit previews the corrected text, never the old.
        let hits = store.search_text("one", "oolong", 5).await;
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].path, "about/tea.md");
        assert_eq!(
            hits[0].source_type, "text_memory",
            "served by the re-indexed vector"
        );
        assert!(hits[0].content_preview.contains("likes oolong"), "{hits:?}");
        assert!(!hits[0].content_preview.contains("likes tea"), "{hits:?}");
        assert_eq!(mock.requests.lock().unwrap().len(), 2);
        let listed = store.list_all("one", 10).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert!(listed[0].content_preview.contains("likes oolong"));

        // The ledger recorded exactly this.
        let recorded = ledger(ws.path());
        assert_eq!(recorded.version, LEDGER_VERSION);
        assert_eq!(recorded.entries, vec![entry.clone()]);
        assert!(ws.path().join("instances/one").join(LEDGER_FILE).exists());

        // Saying the same thing again changes nothing and records nothing.
        assert_eq!(
            correct(&store, "one", "about/tea.md", " likes oolong \n")
                .await
                .unwrap(),
            CorrectionOutcome::Unchanged
        );
        assert_eq!(ledger(ws.path()).entries.len(), 1);

        // Refusals.
        assert_eq!(
            correct(&store, "one", "about/missing.md", "x")
                .await
                .unwrap_err(),
            CorrectionError::NotFound
        );
        assert!(matches!(
            correct(&store, "one", "../outside.md", "x")
                .await
                .unwrap_err(),
            CorrectionError::Invalid(_)
        ));
        assert!(matches!(
            correct(&store, "one", "about/tea.md", "  \n")
                .await
                .unwrap_err(),
            CorrectionError::Invalid(_)
        ));
        assert_eq!(
            correct(
                &store,
                "one",
                "about/tea.md",
                &"x".repeat(MAX_STATEMENT_BYTES + 1)
            )
            .await
            .unwrap_err(),
            CorrectionError::TooLarge
        );
        assert_eq!(body_of(ws.path(), "about/tea.md"), "likes oolong");
        assert_eq!(ledger(ws.path()).entries.len(), 1);
    }

    #[tokio::test]
    async fn conflicting_corrections_need_explicit_resolution_and_survive_restart() {
        let ws = tempfile::tempdir().unwrap();
        seed(ws.path());
        // No embedding provider: the file and BM25 are the whole truth here.
        let store = VectorStore::connect(ws.path()).await;

        let first = match correct(&store, "one", "about/tea.md", "drinks oolong")
            .await
            .unwrap()
        {
            CorrectionOutcome::Applied(entry) => entry,
            other => panic!("{other:?}"),
        };

        // A different second statement is parked, not merged.
        let conflict = match correct(&store, "one", "about/tea.md", "drinks matcha")
            .await
            .unwrap()
        {
            CorrectionOutcome::NeedsResolution(conflict) => conflict,
            other => panic!("{other:?}"),
        };
        assert_eq!(conflict.path, "about/tea.md");
        assert_eq!(conflict.current.id, first.id);
        assert_eq!(conflict.current.statement, "drinks oolong");
        assert_eq!(conflict.proposed.statement, "drinks matcha");
        assert_ne!(conflict.proposed.id, first.id);
        assert_eq!(conflict.conflict_id, conflict.proposed.id);
        assert_eq!(body_of(ws.path(), "about/tea.md"), "drinks oolong");
        assert!(
            store.search_text("one", "matcha", 5).await.is_empty(),
            "a parked statement is not retrievable"
        );
        let ledger_now = ledger(ws.path());
        assert_eq!(ledger_now.entries.len(), 2);
        assert_eq!(ledger_now.entries[0].status, CorrectionStatus::Applied);
        assert_eq!(
            ledger_now.entries[1].status,
            CorrectionStatus::NeedsResolution
        );
        assert_eq!(
            ledger_now.entries[1].conflicts_with.as_deref(),
            Some(first.id.as_str())
        );

        // While one conflict is pending, further statements point at it.
        match correct(&store, "one", "about/tea.md", "drinks sencha")
            .await
            .unwrap()
        {
            CorrectionOutcome::NeedsResolution(again) => {
                assert_eq!(again.conflict_id, conflict.conflict_id);
                assert_eq!(again.proposed.statement, "drinks matcha");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(ledger(ws.path()).entries.len(), 2, "conflicts do not stack");

        // Restart: a fresh store over the same workspace sees the same ledger
        // and the user resolves in favour of the proposed statement.
        drop(store);
        let store = VectorStore::connect(ws.path()).await;
        assert_eq!(
            load_ledger(&store.media_store(), "one").unwrap(),
            ledger_now
        );
        let resolution = resolve(&store, "one", &conflict.conflict_id, Keep::Proposed)
            .await
            .unwrap();
        assert_eq!(resolution.kept, Keep::Proposed);
        assert_eq!(resolution.path, "about/tea.md");
        assert_eq!(resolution.entry.id, conflict.conflict_id);
        assert_eq!(resolution.entry.status, CorrectionStatus::Applied);
        assert!(resolution.entry.resolved_at.is_some());
        assert_eq!(body_of(ws.path(), "about/tea.md"), "drinks matcha");
        let hits = store.search_text("one", "matcha", 5).await;
        assert_eq!(hits.len(), 1, "{hits:?}");
        let ledger_now = ledger(ws.path());
        assert_eq!(ledger_now.entries[0].status, CorrectionStatus::Superseded);
        assert_eq!(ledger_now.entries[1], resolution.entry);
        assert_eq!(
            resolve(&store, "one", &conflict.conflict_id, Keep::Proposed)
                .await
                .unwrap_err(),
            CorrectionError::NotFound,
            "a settled conflict cannot be resolved twice"
        );
        assert_eq!(
            resolve(&store, "one", "corr_nope", Keep::Current)
                .await
                .unwrap_err(),
            CorrectionError::NotFound
        );

        // Keeping the current statement withdraws the proposed one.
        let conflict = match correct(&store, "one", "about/tea.md", "drinks sencha")
            .await
            .unwrap()
        {
            CorrectionOutcome::NeedsResolution(conflict) => conflict,
            other => panic!("{other:?}"),
        };
        assert_eq!(conflict.current.statement, "drinks matcha");
        let resolution = resolve(&store, "one", &conflict.conflict_id, Keep::Current)
            .await
            .unwrap();
        assert_eq!(resolution.kept, Keep::Current);
        assert_eq!(resolution.entry.status, CorrectionStatus::Withdrawn);
        assert_eq!(resolution.entry.statement, "drinks sencha");
        assert_eq!(body_of(ws.path(), "about/tea.md"), "drinks matcha");
        let ledger_now = ledger(ws.path());
        assert_eq!(ledger_now.entries.len(), 3);
        assert_eq!(ledger_now.entries[1].status, CorrectionStatus::Applied);
        assert_eq!(ledger_now.entries[2].status, CorrectionStatus::Withdrawn);

        // Once the companion rewrote the memory itself, the earlier
        // correction is no longer in force: the next one applies directly.
        store
            .write_text_memory("one", "about/tea.md", "drinks matcha and coffee", false)
            .await
            .unwrap();
        let entry = match correct(&store, "one", "about/tea.md", "only water")
            .await
            .unwrap()
        {
            CorrectionOutcome::Applied(entry) => entry,
            other => panic!("{other:?}"),
        };
        assert_eq!(entry.previous, "drinks matcha and coffee");
        assert_eq!(body_of(ws.path(), "about/tea.md"), "only water");
        let ledger_now = ledger(ws.path());
        assert_eq!(ledger_now.entries.len(), 4);
        assert_eq!(ledger_now.entries[1].status, CorrectionStatus::Superseded);
        assert_eq!(ledger_now.entries[3].status, CorrectionStatus::Applied);
        // The user's flags rode along through every rewrite.
        assert!(
            memory::memory_flags(&store.media_store(), "one", "about/tea.md")
                .exclude_from_proactive
        );
    }

    #[tokio::test]
    async fn flags_rewrite_the_file_and_survive_restart() {
        let ws = tempfile::tempdir().unwrap();
        let dir = seed(ws.path());
        fs::write(dir.join("plain.md"), "no frontmatter yet").unwrap();
        let store = VectorStore::connect(ws.path()).await;
        let pin = FlagUpdate {
            pinned: Some(true),
            exclude_from_proactive: None,
        };

        let flags = set_flags(&store, "one", "about/tea.md", pin).await.unwrap();
        assert!(flags.pinned && flags.exclude_from_proactive);
        assert_eq!(
            fs::read_to_string(dir.join("tea.md")).unwrap(),
            "---\ncreated: 2026-01-01\nupdated: 2026-01-02\npinned: true\nexclude_from_proactive: true\n---\nlikes tea\n",
            "flags are not a content change: updated stays"
        );
        let flags = set_flags(&store, "one", "about/plain.md", pin)
            .await
            .unwrap();
        assert_eq!(
            flags,
            MemoryFlags {
                pinned: true,
                exclude_from_proactive: false
            }
        );
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        assert_eq!(
            fs::read_to_string(dir.join("plain.md")).unwrap(),
            format!(
                "---\ncreated: {today}\nupdated: {today}\npinned: true\n---\nno frontmatter yet"
            )
        );

        // Restart, then clear and set through a fresh store.
        drop(store);
        let store = VectorStore::connect(ws.path()).await;
        let media = store.media_store();
        assert!(memory::memory_flags(&media, "one", "about/tea.md").pinned);
        assert!(memory::memory_flags(&media, "one", "about/plain.md").pinned);
        let flags = set_flags(
            &store,
            "one",
            "about/tea.md",
            FlagUpdate {
                pinned: Some(false),
                exclude_from_proactive: Some(false),
            },
        )
        .await
        .unwrap();
        assert_eq!(flags, MemoryFlags::default());
        assert_eq!(
            fs::read_to_string(dir.join("tea.md")).unwrap(),
            STAMPED.replace("exclude_from_proactive: true\n", "")
        );
        let before = fs::read_to_string(dir.join("tea.md")).unwrap();
        assert_eq!(
            set_flags(&store, "one", "about/tea.md", FlagUpdate::default())
                .await
                .unwrap(),
            MemoryFlags::default()
        );
        assert_eq!(fs::read_to_string(dir.join("tea.md")).unwrap(), before);
        assert_eq!(
            memory::scan_library_for(&media, "one", memory::MemoryAccess::Proactive)
                .iter()
                .map(|entry| entry.path.as_str())
                .collect::<Vec<_>>(),
            ["about/plain.md", "about/tea.md"]
        );
        // The BM25 view was refreshed with every rewrite.
        assert_eq!(store.search_text("one", "tea", 5).await.len(), 1);

        assert_eq!(
            set_flags(&store, "one", "about/missing.md", pin)
                .await
                .unwrap_err(),
            CorrectionError::NotFound
        );
        fs::write(dir.join("photo.png"), [0xff]).unwrap();
        assert!(matches!(
            set_flags(&store, "one", "about/photo.png", pin)
                .await
                .unwrap_err(),
            CorrectionError::Invalid(_)
        ));
    }

    #[tokio::test]
    async fn media_memories_are_corrected_through_their_bound_text() {
        let mock = MockServer::new(vec![
            (200, response(vec![1., 0., 0.])),
            (200, response(vec![1., 0., 0.])),
        ])
        .await;
        let ws = tempfile::tempdir().unwrap();
        let dir = ws.path().join("instances/one/memory");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("photo.png"), [0xff, 0x81]).unwrap();
        media_text::write(&dir, "photo.png", "sky over Lisbon").unwrap();
        let store = VectorStore::connect_with_config(ws.path(), &mock.config).await;

        let entry = match correct(&store, "one", "photo.png", "sky over Porto")
            .await
            .unwrap()
        {
            CorrectionOutcome::Applied(entry) => entry,
            other => panic!("{other:?}"),
        };
        assert_eq!(entry.path, "photo.png");
        assert_eq!(entry.previous, "sky over Lisbon");
        assert_eq!(
            store.media_store().read("one", "photo.png").unwrap(),
            "sky over Porto"
        );
        let hits = store.search_text("one", "Porto", 5).await;
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].path, "photo.png");
        assert!(hits[0].content_preview.contains("Porto"));
        assert!(!hits[0].content_preview.contains("Lisbon"));
        assert_eq!(ledger(ws.path()).entries, vec![entry]);

        // A second, different statement conflicts like a text memory would.
        assert!(matches!(
            correct(&store, "one", "photo.png", "sky over Faro")
                .await
                .unwrap(),
            CorrectionOutcome::NeedsResolution(_)
        ));
        assert_eq!(
            store.media_store().read("one", "photo.png").unwrap(),
            "sky over Porto"
        );
    }

    #[tokio::test]
    async fn ledger_file_is_versioned_and_left_alone_when_unreadable() {
        let ws = tempfile::tempdir().unwrap();
        seed(ws.path());
        let store = VectorStore::connect(ws.path()).await;
        let media = store.media_store();
        assert_eq!(
            load_ledger(&media, "one").unwrap(),
            CorrectionLedger::default()
        );

        let file = ws.path().join("instances/one").join(LEDGER_FILE);
        for raw in [r#"{"version":2,"entries":[]}"#, "{not json"] {
            fs::write(&file, raw).unwrap();
            assert!(
                matches!(load_ledger(&media, "one"), Err(CorrectionError::Invalid(_))),
                "{raw}"
            );
            assert!(matches!(
                correct(&store, "one", "about/tea.md", "drinks oolong")
                    .await
                    .unwrap_err(),
                CorrectionError::Invalid(_)
            ));
            assert_eq!(fs::read_to_string(&file).unwrap(), raw, "never rewritten");
            assert_eq!(body_of(ws.path(), "about/tea.md"), "likes tea\n");
        }
    }

    #[tokio::test]
    async fn a_stale_conflict_is_released_by_the_companions_own_rewrite() {
        let ws = tempfile::tempdir().unwrap();
        seed(ws.path());
        let store = VectorStore::connect(ws.path()).await;
        let path = "about/tea.md";
        let applied = |outcome: CorrectionOutcome| match outcome {
            CorrectionOutcome::Applied(entry) => entry,
            other => panic!("expected applied, got {other:?}"),
        };
        let parked = |outcome: CorrectionOutcome| match outcome {
            CorrectionOutcome::NeedsResolution(conflict) => conflict,
            other => panic!("expected needs_resolution, got {other:?}"),
        };

        let first = applied(correct(&store, "one", path, "drinks oolong").await.unwrap());
        let conflict = parked(correct(&store, "one", path, "drinks matcha").await.unwrap());
        assert_eq!(conflict.current.statement, body_of(ws.path(), path));

        // The companion rewrites the memory while the question is open: the
        // memory holds neither statement any more, so the next correction
        // applies directly instead of answering with a conflict that
        // describes text the memory no longer holds.
        store
            .write_text_memory("one", path, "drinks coffee now", false)
            .await
            .unwrap();
        let third = applied(correct(&store, "one", path, "only water").await.unwrap());
        assert_eq!(third.previous, "drinks coffee now");
        assert_eq!(body_of(ws.path(), path), "only water");
        let ledger_now = ledger(ws.path());
        assert_eq!(ledger_now.entries.len(), 3);
        assert_eq!(ledger_now.entries[0].id, first.id);
        assert_eq!(ledger_now.entries[0].status, CorrectionStatus::Superseded);
        assert_eq!(ledger_now.entries[1].id, conflict.conflict_id);
        assert_eq!(
            ledger_now.entries[1].status,
            CorrectionStatus::Superseded,
            "the parked statement was overtaken by the companion's rewrite"
        );
        assert!(ledger_now.entries[1].resolved_at.is_some());
        assert_eq!(ledger_now.entries[2], third);
        assert_eq!(
            resolve(&store, "one", &conflict.conflict_id, Keep::Current)
                .await
                .unwrap_err(),
            CorrectionError::NotFound,
            "a released question cannot be resolved"
        );

        // Forget and re-create: the same release, from a fresh file.
        let conflict = parked(correct(&store, "one", path, "drinks sencha").await.unwrap());
        assert_eq!(conflict.current.id, third.id);
        store.delete_memory("one", path).await.unwrap();
        assert_eq!(
            correct(&store, "one", path, "drinks kombucha")
                .await
                .unwrap_err(),
            CorrectionError::NotFound,
            "a forgotten memory cannot be corrected"
        );
        assert_eq!(
            ledger(ws.path()).entries[3].status,
            CorrectionStatus::NeedsResolution,
            "nothing was decided while the memory is gone"
        );
        store
            .write_text_memory("one", path, "fresh start", false)
            .await
            .unwrap();
        let fifth = applied(
            correct(&store, "one", path, "drinks kombucha")
                .await
                .unwrap(),
        );
        assert_eq!(fifth.previous, "fresh start");
        assert_eq!(body_of(ws.path(), path), "drinks kombucha");
        let ledger_now = ledger(ws.path());
        assert_eq!(ledger_now.entries.len(), 5);
        assert_eq!(ledger_now.entries[2].status, CorrectionStatus::Superseded);
        assert_eq!(ledger_now.entries[3].status, CorrectionStatus::Superseded);
        assert_eq!(ledger_now.entries[4], fifth);

        // A conflict is only ever reported against the text on disk.
        let conflict = parked(correct(&store, "one", path, "drinks mate").await.unwrap());
        assert_eq!(conflict.current.id, fifth.id);
        assert_eq!(conflict.current.statement, body_of(ws.path(), path));
    }

    #[tokio::test]
    async fn text_rewrites_wait_for_the_companion_lifecycle_gate() {
        // Its own companion: the correction lock is per slug across the
        // process, so no other test can park these tasks on it.
        const SLUG: &str = "gated";
        let ws = tempfile::tempdir().unwrap();
        let dir = ws.path().join("instances").join(SLUG).join("memory/about");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("tea.md"), STAMPED).unwrap();
        let store = std::sync::Arc::new(VectorStore::connect(ws.path()).await);
        let path = "about/tea.md";

        // While the companion holds the gate (its own write or forget in
        // flight), neither a correction nor a flag change touches the file.
        let guard = store.lifecycle_lock(SLUG).lock_owned().await;
        let correction = {
            let store = store.clone();
            tokio::spawn(async move { correct(&store, SLUG, path, "likes oolong").await })
        };
        tokio::task::yield_now().await;
        let flag = {
            let store = store.clone();
            tokio::spawn(async move {
                set_flags(
                    &store,
                    SLUG,
                    path,
                    FlagUpdate {
                        pinned: Some(true),
                        exclude_from_proactive: None,
                    },
                )
                .await
            })
        };
        tokio::task::yield_now().await;
        assert!(!correction.is_finished() && !flag.is_finished());
        assert_eq!(
            fs::read_to_string(dir.join("tea.md")).unwrap(),
            STAMPED,
            "nothing is written before the gate is held"
        );
        drop(guard);
        assert!(matches!(
            correction.await.unwrap().unwrap(),
            CorrectionOutcome::Applied(_)
        ));
        assert!(flag.await.unwrap().unwrap().pinned);
        let raw = fs::read_to_string(dir.join("tea.md")).unwrap();
        let (frontmatter, body) = memory::parse_frontmatter(&raw);
        assert_eq!(body, "likes oolong");
        assert!(frontmatter.flags.pinned && frontmatter.flags.exclude_from_proactive);

        // A forget already queued on the gate goes first, and the correction
        // behind it finds nothing to correct instead of recreating the file.
        let guard = store.lifecycle_lock(SLUG).lock_owned().await;
        let forget = {
            let store = store.clone();
            tokio::spawn(async move { store.delete_memory(SLUG, path).await })
        };
        tokio::task::yield_now().await;
        let correction = {
            let store = store.clone();
            tokio::spawn(async move { correct(&store, SLUG, path, "likes sencha").await })
        };
        tokio::task::yield_now().await;
        assert!(!forget.is_finished() && !correction.is_finished());
        drop(guard);
        forget.await.unwrap().unwrap();
        assert_eq!(
            correction.await.unwrap().unwrap_err(),
            CorrectionError::NotFound
        );
        assert!(
            !dir.join("tea.md").exists(),
            "a forgotten memory stays forgotten"
        );
        assert!(store.search_text(SLUG, "sencha", 5).await.is_empty());
        let media = store.media_store();
        assert_eq!(
            load_ledger(&media, SLUG).unwrap().entries.len(),
            1,
            "nothing recorded for it"
        );
    }

    #[tokio::test]
    async fn a_full_ledger_never_locks_out_further_corrections() {
        let ws = tempfile::tempdir().unwrap();
        let dir = seed(ws.path());
        let store = VectorStore::connect(ws.path()).await;
        let media = store.media_store();
        // Enough max-size statements to pass the ledger's byte bound many
        // times over if nothing were dropped.
        let count = MAX_LEDGER_BYTES / MAX_STATEMENT_BYTES + 8;
        let statement = |i: usize| format!("{i:04}{}", "x".repeat(MAX_STATEMENT_BYTES - 4));
        for i in 0..count {
            fs::write(dir.join(format!("{i}.md")), STAMPED).unwrap();
        }

        for i in 0..count {
            let path = format!("about/{i}.md");
            let outcome = correct(&store, "one", &path, &statement(i)).await;
            assert!(
                matches!(outcome, Ok(CorrectionOutcome::Applied(_))),
                "correction {i}: {outcome:?}"
            );
            assert_eq!(body_of(ws.path(), &path), statement(i));
            let recorded = load_ledger(&media, "one")
                .unwrap_or_else(|error| panic!("ledger unreadable after {i}: {error}"));
            assert!(
                recorded.entries.iter().any(|entry| entry.path == path),
                "the latest correction is always recorded"
            );
            assert!(
                fs::metadata(ws.path().join("instances/one").join(LEDGER_FILE))
                    .unwrap()
                    .len() as usize
                    <= MAX_LEDGER_BYTES
            );
        }
        let recorded = load_ledger(&media, "one").unwrap();
        assert!(recorded.entries.len() < count, "the oldest were dropped");
        assert!(
            recorded
                .entries
                .iter()
                .all(|entry| entry.status == CorrectionStatus::Applied)
        );
        assert_eq!(
            recorded.entries.last().unwrap().path,
            format!("about/{}.md", count - 1)
        );

        // The memory whose entry was dropped keeps its text; the ledger just
        // no longer remembers the correction, so the next one applies as if
        // it were the first.
        assert_eq!(body_of(ws.path(), "about/0.md"), statement(0));
        assert!(matches!(
            correct(&store, "one", "about/0.md", "short again")
                .await
                .unwrap(),
            CorrectionOutcome::Applied(_)
        ));
        assert_eq!(body_of(ws.path(), "about/0.md"), "short again");
    }

    #[test]
    fn ledger_encoding_drops_settled_then_applied_entries_but_never_open_ones() {
        let entry = |i: usize, status: CorrectionStatus| CorrectionEntry {
            id: format!("corr_{i}"),
            path: format!("about/{i}.md"),
            statement: "x".repeat(MAX_STATEMENT_BYTES),
            previous: String::new(),
            status,
            corrected_at: String::new(),
            resolved_at: None,
            conflicts_with: None,
        };
        let over = MAX_LEDGER_BYTES / MAX_STATEMENT_BYTES + 2;

        // Settled entries go first, oldest first, and only as many as needed.
        let mut ledger = CorrectionLedger::default();
        ledger.entries.push(entry(0, CorrectionStatus::Applied));
        ledger
            .entries
            .extend((1..over).map(|i| entry(i, CorrectionStatus::Withdrawn)));
        ledger
            .entries
            .extend((over..2 * over).map(|i| entry(i, CorrectionStatus::Superseded)));
        ledger
            .entries
            .push(entry(2 * over, CorrectionStatus::Applied));
        let json = encode_ledger(&mut ledger).unwrap();
        assert!(json.len() <= MAX_LEDGER_BYTES);
        assert_eq!(ledger.entries[0].id, "corr_0");
        assert_eq!(
            ledger.entries.last().unwrap().id,
            format!("corr_{}", 2 * over)
        );
        assert!(
            ledger.entries.len() > 2,
            "only as many as needed were dropped"
        );
        assert!(
            ledger.entries[1..]
                .iter()
                .all(|entry| entry.id > ledger.entries[0].id),
            "{:?}",
            ledger.entries.iter().map(|e| &e.id).collect::<Vec<_>>()
        );

        // Then the oldest applied entries, except one an open question still
        // points at; open questions themselves are never dropped.
        let mut ledger = CorrectionLedger::default();
        ledger
            .entries
            .extend((0..over).map(|i| entry(i, CorrectionStatus::Applied)));
        let mut open = entry(over, CorrectionStatus::NeedsResolution);
        open.conflicts_with = Some("corr_0".into());
        ledger.entries.push(open);
        let json = encode_ledger(&mut ledger).unwrap();
        assert!(json.len() <= MAX_LEDGER_BYTES);
        assert_eq!(ledger.entries[0].id, "corr_0", "still pointed at");
        let kept: Vec<usize> = ledger.entries[1..ledger.entries.len() - 1]
            .iter()
            .map(|entry| entry.id["corr_".len()..].parse().unwrap())
            .collect();
        assert!(kept.len() > 1 && kept.len() < over - 1, "{kept:?}");
        assert_eq!(
            kept,
            (over - kept.len()..over).collect::<Vec<_>>(),
            "the oldest free applied entries went, and only those"
        );
        assert_eq!(
            ledger.entries.last().unwrap().status,
            CorrectionStatus::NeedsResolution
        );

        let mut ledger = CorrectionLedger::default();
        ledger
            .entries
            .extend((0..over).map(|i| entry(i, CorrectionStatus::NeedsResolution)));
        assert_eq!(
            encode_ledger(&mut ledger).unwrap_err(),
            CorrectionError::LedgerFull
        );
        assert_eq!(ledger.entries.len(), over, "nothing open was dropped");

        // The entry cap is enforced the same way.
        let mut ledger = CorrectionLedger::default();
        ledger
            .entries
            .extend((0..MAX_ENTRIES + 3).map(|i| CorrectionEntry {
                statement: "short".into(),
                ..entry(i, CorrectionStatus::Applied)
            }));
        encode_ledger(&mut ledger).unwrap();
        assert_eq!(ledger.entries.len(), MAX_ENTRIES);
        assert_eq!(ledger.entries[0].id, "corr_3");
    }

    #[test]
    fn memory_flags_and_ledger_are_documented() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let doc = fs::read_to_string(repo.join("docs/companion-storage.md")).unwrap();
        for required in [
            LEDGER_FILE,
            "pinned: true",
            "exclude_from_proactive",
            "needs_resolution",
            "/memory-corrections",
            "resolve",
        ] {
            assert!(
                doc.contains(required),
                "storage doc is missing {required:?}"
            );
        }
    }
}
