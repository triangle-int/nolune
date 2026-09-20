//! Transactional companion restore from a backup archive (#74).
//!
//! [`restore_companion`] stages an archive under the top-level `imports/`
//! directory (never under `instances/`, whose siblings are reported as
//! obsolete companions), validates it there, and only then swaps it into
//! `instances/companion`: the live tree moves to `imports/previous-<id>`, the
//! staged tree moves into place, and a failure of the second rename moves the
//! previous tree back. The whole sequence runs under the same per-companion
//! lifecycle gate every memory write holds, so a write that arrives during an
//! import lands in the imported tree afterwards instead of racing the swap.
//! Derived state (vectors, BM25, the catalog snapshot) is rebuilt from the
//! imported memory files before the previous tree is discarded, and a
//! provider that cannot embed leaves the collection marked for the startup
//! backfill rather than claiming a full rebuild.
//! See `docs/companion-storage.md` for the contract.

use std::{
    collections::HashMap,
    io::{self, Read},
    sync::Arc,
};

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use tokio_util::sync::CancellationToken;

use super::{
    media_text::MediaStore,
    memory,
    profile_archive::{self, ArchiveError},
    vector::VectorStore,
};
use crate::domain::companion::IDENTITY_FILE;

/// `imports/staging-<id>`: where the archive is extracted.
const STAGING_PREFIX: &str = "staging-";

/// `imports/previous-<id>`: where the replaced companion waits until the
/// import has succeeded.
const PREVIOUS_PREFIX: &str = "previous-";

/// Largest identity marker re-read from staging before the swap.
const MAX_IDENTITY_BYTES: u64 = 4096;

/// State of the semantic index after an import.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivedIndex {
    /// Vectors and BM25 were rebuilt from the imported memory files.
    Rebuilt,
    /// The files are in place but the embedding provider could not rebuild
    /// the vectors; `VectorStore::needs_backfill` stays `true` and the
    /// startup backfill retries. BM25 is rebuilt lazily on the next search.
    Pending,
}

/// What a successful import published.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RestoreOutcome {
    /// Regular files extracted, the manifest included.
    pub files: usize,
    /// Explicit directory entries extracted.
    pub directories: usize,
    /// Payload bytes of regular files.
    pub bytes: u64,
    pub derived_index: DerivedIndex,
    /// Why the semantic index is still pending.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_reason: Option<String>,
    /// Chunks embedded by the rebuild (0 when pending).
    pub indexed_chunks: usize,
}

/// Why an import did not publish. Every variant except `PublishStranded`
/// leaves the previous companion exactly where it was.
#[derive(Debug)]
pub enum RestoreError {
    /// Chat or scheduler agents are writing the companion through ambient
    /// paths; the import is refused before anything is staged. The route
    /// answers `409`.
    Busy { tasks: usize },
    /// The archive was refused; the staging directory has been discarded.
    Archive(ArchiveError),
    /// Creating the staging directory failed; nothing was extracted.
    Staging(io::Error),
    /// A rename in the swap failed and the previous companion is in place.
    PublishFailed(io::Error),
    /// The second rename failed and moving the previous companion back
    /// failed too: it is intact at `imports/<previous>` and needs an
    /// operator to move it back.
    PublishStranded {
        error: io::Error,
        rollback: io::Error,
        previous: String,
    },
}

impl std::fmt::Display for RestoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy { tasks } => write!(
                f,
                "companion is busy: {tasks} agent task(s) are running; retry once they finish"
            ),
            Self::Archive(error) => write!(f, "archive refused: {error}"),
            Self::Staging(error) => write!(f, "import staging failed: {error}"),
            Self::PublishFailed(error) => {
                write!(
                    f,
                    "import failed and the previous companion was kept: {error}"
                )
            }
            Self::PublishStranded {
                error,
                rollback,
                previous,
            } => write!(
                f,
                "import failed ({error}) and the rollback failed ({rollback}); the previous companion is intact at imports/{previous}"
            ),
        }
    }
}

impl std::error::Error for RestoreError {}

/// Replace the companion `slug` with the contents of `archive`.
///
/// Holds `VectorStore::lifecycle_lock(slug)` from before the archive is
/// staged until derived state has been rebuilt and the previous tree removed.
/// The archive is read on a blocking thread through the validating extractor
/// in `profile_archive`, so nothing but the staging directory is written
/// before validation succeeds.
pub async fn restore_companion<R: Read + Send + 'static>(
    store: Arc<VectorStore>,
    agent_tasks: &tokio::sync::Mutex<HashMap<String, CancellationToken>>,
    slug: &str,
    archive: R,
) -> Result<RestoreOutcome, RestoreError> {
    let _gate = store.lifecycle_lock(slug).lock_owned().await;

    // Chat and scheduler agents write the companion through ambient paths
    // that the gate does not cover, so an import while one runs would race
    // the swap. Refuse before anything is staged; the route answers 409.
    let running = {
        let prefix = format!("{slug}/");
        let tasks = agent_tasks.lock().await;
        tasks.keys().filter(|key| key.starts_with(&prefix)).count()
    };
    if running > 0 {
        return Err(RestoreError::Busy { tasks: running });
    }

    let media = store.media_store();
    let id = uuid::Uuid::new_v4();
    let staging_name = format!("{STAGING_PREFIX}{id}");
    let previous_name = format!("{PREVIOUS_PREFIX}{id}");

    // Stage and validate. The extractor writes only through the staging
    // capability; the marker is re-read from what actually landed on disk.
    let staging = blocking({
        let media = media.clone();
        let name = staging_name.clone();
        move || media.create_import(&name)
    })
    .await
    .map_err(RestoreError::Staging)?;
    let extracted = tokio::task::spawn_blocking(move || {
        let summary = profile_archive::extract_into(archive, &staging)?;
        validate_staged(&staging)?;
        Ok::<_, ArchiveError>(summary)
    })
    .await
    .unwrap_or_else(|error| Err(ArchiveError::Io(task_error(error))));
    let summary = match extracted {
        Ok(summary) => summary,
        Err(error) => {
            discard(&media, &staging_name).await;
            return Err(RestoreError::Archive(error));
        }
    };

    // Swap: park the live tree, move the staged tree into place, and move
    // the parked tree back if that second rename fails.
    let had_previous = match blocking({
        let media = media.clone();
        let name = previous_name.clone();
        let slug = slug.to_owned();
        move || media.stash_companion(&slug, &name)
    })
    .await
    {
        Ok(had_previous) => had_previous,
        Err(error) => {
            discard(&media, &staging_name).await;
            return Err(RestoreError::PublishFailed(error));
        }
    };
    let published = blocking({
        let media = media.clone();
        let name = staging_name.clone();
        let slug = slug.to_owned();
        move || media.publish_import(&slug, &name)
    })
    .await;
    if let Err(error) = published {
        let rollback = if had_previous {
            blocking({
                let media = media.clone();
                let name = previous_name.clone();
                let slug = slug.to_owned();
                move || media.publish_import(&slug, &name)
            })
            .await
        } else {
            Ok(())
        };
        discard(&media, &staging_name).await;
        return Err(match rollback {
            Ok(()) => RestoreError::PublishFailed(error),
            Err(rollback) => {
                log::error!(
                    "[import] rollback failed; the previous companion is at imports/{previous_name}: {rollback}"
                );
                RestoreError::PublishStranded {
                    error,
                    rollback,
                    previous: previous_name,
                }
            }
        });
    }
    log::info!(
        "[import] published {} files ({} bytes) for {slug}",
        summary.files,
        summary.bytes
    );

    // Derived state: vectors and BM25 are emptied and rebuilt from the
    // imported memory files. A provider that cannot embed leaves the
    // collection marked for the startup backfill; BM25 rebuilds lazily.
    let (derived_index, pending_reason, indexed_chunks) =
        match store.rebuild_derived_no_lifecycle(slug).await {
            Ok(chunks) => (DerivedIndex::Rebuilt, None, chunks),
            Err(reason) => {
                log::warn!("[import] semantic index pending for {slug}: {reason}");
                (DerivedIndex::Pending, Some(reason), 0)
            }
        };
    // The catalog snapshot and the memory graph are read from the companion
    // directory on demand, so the swap already made them current; rebuild the
    // snapshot now and load the graph once so a malformed file shows up here.
    let _ = blocking({
        let media = media.clone();
        let slug = slug.to_owned();
        move || {
            memory::rebuild_catalog_snapshot(&slug, &media);
            let graph = memory::load_graph(&media, &slug);
            log::info!(
                "[import] memory graph reloaded for {slug}: {} edges",
                graph.edges.len()
            );
            Ok(())
        }
    })
    .await;

    if had_previous {
        discard(&media, &previous_name).await;
    }
    Ok(RestoreOutcome {
        files: summary.files,
        directories: summary.directories,
        bytes: summary.bytes,
        derived_index,
        pending_reason,
        indexed_chunks,
    })
}

/// Re-read the staged marker before the swap: the extractor already refused
/// an archive without a valid one, so this only guards the publication step
/// against a regression in the reader.
fn validate_staged(staging: &Dir) -> Result<(), ArchiveError> {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let mut file = match staging.open_with(IDENTITY_FILE, &options) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(ArchiveError::MissingIdentity);
        }
        Err(error) => return Err(ArchiveError::Io(error)),
    };
    if !file.metadata()?.is_file() {
        return Err(ArchiveError::MissingIdentity);
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_IDENTITY_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_IDENTITY_BYTES {
        return Err(ArchiveError::InvalidIdentity(
            "staged identity marker is too large".into(),
        ));
    }
    profile_archive::parse_identity(&bytes).map(|_| ())
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> io::Result<T> + Send + 'static,
) -> io::Result<T> {
    tokio::task::spawn_blocking(work)
        .await
        .unwrap_or_else(|error| Err(task_error(error)))
}

fn task_error(error: tokio::task::JoinError) -> io::Error {
    io::Error::other(format!("restore task failed: {error}"))
}

/// Remove `imports/<name>`; a failure is logged, never fatal, because the
/// companion itself is already in its final state by the time this runs.
async fn discard(media: &Arc<MediaStore>, name: &str) {
    let result = blocking({
        let media = media.clone();
        let name = name.to_owned();
        move || media.remove_import(&name)
    })
    .await;
    if let Err(error) = result {
        log::warn!("[import] could not remove imports/{name}: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::companion::{CANONICAL_SLUG, CompanionIdentity, IDENTITY_FILE};
    use crate::services::embedding::tests::{MockServer, response};
    use crate::services::media_text::StashPause;
    use std::{
        collections::BTreeMap,
        fs,
        io::Cursor,
        path::{Path, PathBuf},
    };

    type Tasks = Arc<tokio::sync::Mutex<HashMap<String, CancellationToken>>>;

    fn tasks() -> Tasks {
        Arc::new(tokio::sync::Mutex::new(HashMap::new()))
    }

    fn marker() -> Vec<u8> {
        serde_json::to_vec_pretty(&CompanionIdentity::canonical()).unwrap()
    }

    fn companion_dir(workspace: &Path) -> PathBuf {
        workspace.join("instances").join(CANONICAL_SLUG)
    }

    /// A companion tree at `dir`: the identity marker plus `files`.
    fn write_tree(dir: &Path, files: &[(&str, &[u8])]) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(IDENTITY_FILE), marker()).unwrap();
        for (path, bytes) in files {
            let full = dir.join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, bytes).unwrap();
        }
    }

    /// A valid archive of `files`, produced by the production writer.
    fn archive(files: &[(&str, &[u8])]) -> Cursor<Vec<u8>> {
        let scratch = tempfile::tempdir().unwrap();
        let source = scratch.path().join("companion");
        write_tree(&source, files);
        let source = profile_archive::open_companion_dir(&source).unwrap();
        let mut bytes = Vec::new();
        profile_archive::write_archive(&source, &mut bytes).unwrap();
        Cursor::new(bytes)
    }

    /// Every path under `root`, with file contents (`None` for directories).
    fn snapshot(root: &Path) -> BTreeMap<String, Option<Vec<u8>>> {
        fn visit(root: &Path, dir: &Path, out: &mut BTreeMap<String, Option<Vec<u8>>>) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                let relative = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                if path.symlink_metadata().unwrap().is_dir() {
                    out.insert(relative, None);
                    visit(root, &path, out);
                } else {
                    out.insert(relative, Some(fs::read(&path).unwrap()));
                }
            }
        }
        let mut out = BTreeMap::new();
        if root.is_dir() {
            visit(root, root, &mut out);
        }
        out
    }

    /// Names left under `imports/`; empty when the directory is absent.
    fn imports_entries(workspace: &Path) -> Vec<String> {
        let imports = workspace.join("imports");
        if !imports.is_dir() {
            return Vec::new();
        }
        let mut names: Vec<_> = fs::read_dir(imports)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// The committed index as `(path, source_type, preview)` triples.
    async fn listing(store: &VectorStore) -> Vec<(String, String, String)> {
        store
            .list_all(CANONICAL_SLUG, 100)
            .await
            .unwrap()
            .into_iter()
            .map(|record| (record.path, record.source_type, record.content_preview))
            .collect()
    }

    async fn hit_paths(store: &VectorStore, query: &str) -> Vec<String> {
        store
            .search_text(CANONICAL_SLUG, query, 10)
            .await
            .into_iter()
            .map(|hit| hit.path)
            .collect()
    }

    #[tokio::test]
    async fn a_failed_swap_leaves_the_companion_tree_and_index_byte_identical() {
        let mock = MockServer::new(vec![(200, response(vec![1., 0., 0.])); 2]).await;
        let workspace = tempfile::tempdir().unwrap();
        write_tree(
            &companion_dir(workspace.path()),
            &[
                ("memory/old.md", b"Orion nebula"),
                ("soul.md", b"old soul"),
                ("uploads/keep.txt", b"kept"),
            ],
        );
        let store =
            Arc::new(VectorStore::connect_with_config(workspace.path(), &mock.config).await);
        store
            .backfill_text_memories(workspace.path(), CANONICAL_SLUG)
            .await
            .unwrap();
        assert!(!store.needs_backfill(CANONICAL_SLUG).await.unwrap());
        let tree_before = snapshot(&companion_dir(workspace.path()));
        let index_before = listing(&store).await;
        assert_eq!(index_before.len(), 1);
        store.media_store().inject_next_import_publish_failure();

        let error = restore_companion(
            store.clone(),
            &tasks(),
            CANONICAL_SLUG,
            archive(&[("memory/new.md", b"Andromeda"), ("soul.md", b"new soul")]),
        )
        .await
        .unwrap_err();

        assert!(matches!(error, RestoreError::PublishFailed(_)), "{error}");
        assert_eq!(snapshot(&companion_dir(workspace.path())), tree_before);
        assert_eq!(listing(&store).await, index_before);
        assert!(!store.needs_backfill(CANONICAL_SLUG).await.unwrap());
        assert_eq!(hit_paths(&store, "Orion").await, vec!["old.md"]);
        assert_eq!(
            imports_entries(workspace.path()),
            Vec::<String>::new(),
            "staging and the parked tree are gone after the rollback"
        );
        // The gate is released again: a later write goes through.
        store
            .write_text_memory(CANONICAL_SLUG, "later.md", "after the failed import", false)
            .await
            .unwrap();
        assert!(
            companion_dir(workspace.path())
                .join("memory/later.md")
                .is_file()
        );
    }

    /// An HTTP client that disconnects drops the handler future. The swap
    /// must not be left half done: once the live tree is parked the
    /// transaction runs to completion on its own, so the companion is the
    /// imported tree (never missing, never a fresh bogus one) and `imports/`
    /// is clean.
    #[tokio::test]
    async fn dropping_the_restore_future_between_the_renames_completes_the_import() {
        let mock = MockServer::new(vec![(200, response(vec![1., 0., 0.])); 4]).await;
        let workspace = tempfile::tempdir().unwrap();
        write_tree(
            &companion_dir(workspace.path()),
            &[("memory/old.md", b"Orion nebula")],
        );
        let store =
            Arc::new(VectorStore::connect_with_config(workspace.path(), &mock.config).await);
        store
            .backfill_text_memories(workspace.path(), CANONICAL_SLUG)
            .await
            .unwrap();
        let (reached, reached_rx) = std::sync::mpsc::channel();
        let (resume, resume_rx) = std::sync::mpsc::channel();
        store.media_store().pause_next_stash(StashPause {
            reached,
            resume: resume_rx,
        });

        let tasks = tasks();
        let restore = restore_companion(
            store.clone(),
            &tasks,
            CANONICAL_SLUG,
            archive(&[("memory/new.md", b"Andromeda")]),
        );
        let mut restore = Box::pin(restore);
        let parked = tokio::task::spawn_blocking(move || reached_rx.recv());
        tokio::select! {
            outcome = &mut restore => panic!("restore finished before the swap paused: {outcome:?}"),
            parked = parked => parked.unwrap().unwrap(),
        }
        // The live tree is parked and the client goes away right now.
        assert!(!companion_dir(workspace.path()).exists());
        assert_eq!(
            imports_entries(workspace.path()).len(),
            2,
            "staging and previous"
        );
        drop(restore);
        resume.send(()).unwrap();

        // The gate is released only once the detached transaction is done.
        let gate = store.lifecycle_lock(CANONICAL_SLUG).lock_owned().await;
        drop(gate);
        let memory = companion_dir(workspace.path()).join("memory");
        assert!(memory.join("new.md").is_file(), "the imported tree is live");
        assert!(!memory.join("old.md").exists());
        assert_eq!(
            fs::read(companion_dir(workspace.path()).join(IDENTITY_FILE)).unwrap(),
            marker()
        );
        assert_eq!(
            imports_entries(workspace.path()),
            Vec::<String>::new(),
            "neither the staging nor the previous tree is left behind"
        );
        assert_eq!(
            listing(&store).await,
            vec![(
                "new.md".to_owned(),
                "text_memory".to_owned(),
                "Andromeda".to_owned()
            )],
            "derived state was rebuilt after the swap"
        );
        assert!(!store.needs_backfill(CANONICAL_SLUG).await.unwrap());
        store
            .write_text_memory(
                CANONICAL_SLUG,
                "later.md",
                "after the dropped import",
                false,
            )
            .await
            .unwrap();
        assert!(memory.join("later.md").is_file());
    }

    /// The reset of the vector collection is the one derived-state step that
    /// can fail after the swap. When its persist fails, nothing of the
    /// replaced tree may keep being served and the startup backfill must
    /// run: `derived_index: pending` always implies `needs_backfill()`.
    #[tokio::test]
    async fn a_reset_that_cannot_be_persisted_still_marks_the_collection_for_backfill() {
        let mock = MockServer::new(vec![(200, response(vec![1., 0., 0.])); 4]).await;
        let workspace = tempfile::tempdir().unwrap();
        write_tree(
            &companion_dir(workspace.path()),
            &[("memory/old.md", b"Orion nebula")],
        );
        let store =
            Arc::new(VectorStore::connect_with_config(workspace.path(), &mock.config).await);
        store
            .backfill_text_memories(workspace.path(), CANONICAL_SLUG)
            .await
            .unwrap();
        assert_eq!(listing(&store).await.len(), 1);
        let index_files = || {
            let vectors = workspace.path().join("vectors");
            let mut names: Vec<_> = fs::read_dir(&vectors)
                .map(|entries| {
                    entries
                        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                        .filter(|name| name.ends_with(".bin"))
                        .collect()
                })
                .unwrap_or_default();
            names.sort();
            names
        };
        assert_eq!(index_files().len(), 1);
        store.inject_next_persist_failure();

        let outcome = restore_companion(
            store.clone(),
            &tasks(),
            CANONICAL_SLUG,
            archive(&[("memory/new.md", b"Andromeda")]),
        )
        .await
        .unwrap();

        assert_eq!(outcome.derived_index, DerivedIndex::Pending);
        assert!(
            outcome
                .pending_reason
                .as_deref()
                .unwrap_or_default()
                .contains("injected failure"),
            "{outcome:?}"
        );
        assert!(
            store.needs_backfill(CANONICAL_SLUG).await.unwrap(),
            "pending always implies the startup backfill runs"
        );
        assert!(
            listing(&store).await.is_empty(),
            "no record of the replaced tree survives"
        );
        assert_eq!(
            index_files(),
            Vec::<String>::new(),
            "the stale index file is unlinked so a restart does not reload it"
        );
        assert!(
            !hit_paths(&store, "Orion")
                .await
                .contains(&"old.md".to_owned())
        );
        assert_eq!(hit_paths(&store, "Andromeda").await, vec!["new.md"]);
        assert_eq!(imports_entries(workspace.path()), Vec::<String>::new());
        // The next backfill repairs it in place.
        assert_eq!(
            store
                .backfill_text_memories(workspace.path(), CANONICAL_SLUG)
                .await
                .unwrap(),
            1
        );
        assert!(!store.needs_backfill(CANONICAL_SLUG).await.unwrap());
        assert_eq!(listing(&store).await[0].0, "new.md");
    }

    #[tokio::test]
    async fn a_concurrent_memory_write_lands_after_the_import() {
        let mock = MockServer::new(vec![(200, response(vec![1., 0., 0.])); 4]).await;
        let workspace = tempfile::tempdir().unwrap();
        write_tree(
            &companion_dir(workspace.path()),
            &[("memory/old.md", b"old")],
        );
        let store =
            Arc::new(VectorStore::connect_with_config(workspace.path(), &mock.config).await);
        let gate = store.lifecycle_lock(CANONICAL_SLUG);
        let held = gate.lock_owned().await;

        let import = {
            let store = store.clone();
            let tasks = tasks();
            let archive = archive(&[("memory/imported.md", b"imported")]);
            tokio::spawn(
                async move { restore_companion(store, &tasks, CANONICAL_SLUG, archive).await },
            )
        };
        tokio::task::yield_now().await;
        let write = {
            let store = store.clone();
            tokio::spawn(async move {
                store
                    .write_text_memory(CANONICAL_SLUG, "note.md", "written during import", false)
                    .await
            })
        };
        tokio::task::yield_now().await;
        let memory = companion_dir(workspace.path()).join("memory");
        assert!(
            memory.join("old.md").is_file(),
            "nothing moves while the gate is held"
        );
        assert!(!memory.join("note.md").exists());
        assert!(!memory.join("imported.md").exists());
        assert_eq!(imports_entries(workspace.path()), Vec::<String>::new());

        drop(held);
        let outcome = import.await.unwrap().unwrap();
        assert_eq!(outcome.derived_index, DerivedIndex::Rebuilt);
        write.await.unwrap().unwrap();

        assert!(memory.join("imported.md").is_file());
        assert!(
            memory.join("note.md").is_file(),
            "the write landed in the imported tree, after the swap"
        );
        assert!(!memory.join("old.md").exists());
        let paths: Vec<_> = listing(&store)
            .await
            .into_iter()
            .map(|(path, _, _)| path)
            .collect();
        assert_eq!(paths, vec!["imported.md", "note.md"]);
        assert_eq!(imports_entries(workspace.path()), Vec::<String>::new());
    }

    #[tokio::test]
    async fn stale_vector_and_bm25_entries_are_gone_after_a_successful_import() {
        let mock = MockServer::new(vec![(200, response(vec![1., 0., 0.])); 8]).await;
        let workspace = tempfile::tempdir().unwrap();
        write_tree(
            &companion_dir(workspace.path()),
            &[
                ("memory/old.md", b"Orion nebula"),
                ("memory_graph.json", br#"{"edges":[["gone.md","old.md"]]}"#),
            ],
        );
        let store =
            Arc::new(VectorStore::connect_with_config(workspace.path(), &mock.config).await);
        store
            .backfill_text_memories(workspace.path(), CANONICAL_SLUG)
            .await
            .unwrap();
        assert_eq!(hit_paths(&store, "Orion").await, vec!["old.md"]);
        let media = store.media_store();
        memory::rebuild_catalog_snapshot(CANONICAL_SLUG, &media);
        assert!(memory::load_catalog_snapshot(&media, CANONICAL_SLUG).contains("old.md"));

        let outcome = restore_companion(
            store.clone(),
            &tasks(),
            CANONICAL_SLUG,
            archive(&[
                ("memory/new.md", b"Andromeda galaxy"),
                ("memory_graph.json", br#"{"edges":[["new.md","other.md"]]}"#),
            ]),
        )
        .await
        .unwrap();

        assert_eq!(outcome.derived_index, DerivedIndex::Rebuilt);
        assert_eq!(outcome.pending_reason, None);
        assert_eq!(outcome.files, 3, "marker, memory, graph");
        assert_eq!(outcome.indexed_chunks, 1);
        assert!(!store.needs_backfill(CANONICAL_SLUG).await.unwrap());
        let records = listing(&store).await;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].0, "new.md");
        assert_eq!(records[0].2, "Andromeda galaxy");
        assert!(
            !hit_paths(&store, "Orion")
                .await
                .contains(&"old.md".to_owned())
        );
        assert_eq!(hit_paths(&store, "Andromeda").await, vec!["new.md"]);
        let catalog = memory::load_catalog_snapshot(&media, CANONICAL_SLUG);
        assert!(
            catalog.contains("new.md") && !catalog.contains("old.md"),
            "{catalog}"
        );
        assert_eq!(
            memory::load_graph(&media, CANONICAL_SLUG).edges,
            vec![["new.md".to_owned(), "other.md".to_owned()]]
        );
        let memory_dir = companion_dir(workspace.path()).join("memory");
        assert!(memory_dir.join("new.md").is_file());
        assert!(!memory_dir.join("old.md").exists());
        assert_eq!(
            imports_entries(workspace.path()),
            Vec::<String>::new(),
            "the previous tree is removed only after success, and it is"
        );
    }

    #[tokio::test]
    async fn an_unavailable_provider_reports_a_pending_index_instead_of_a_rebuild() {
        let workspace = tempfile::tempdir().unwrap();
        write_tree(
            &companion_dir(workspace.path()),
            &[("memory/old.md", b"old")],
        );
        let store = Arc::new(VectorStore::connect(workspace.path()).await);

        let outcome = restore_companion(
            store.clone(),
            &tasks(),
            CANONICAL_SLUG,
            archive(&[("memory/new.md", b"new")]),
        )
        .await
        .unwrap();

        assert_eq!(outcome.derived_index, DerivedIndex::Pending);
        assert!(outcome.pending_reason.is_some());
        assert_eq!(outcome.indexed_chunks, 0);
        assert!(store.needs_backfill(CANONICAL_SLUG).await.unwrap());
        assert!(listing(&store).await.is_empty());
        assert!(
            companion_dir(workspace.path())
                .join("memory/new.md")
                .is_file()
        );
        assert!(
            !companion_dir(workspace.path())
                .join("memory/old.md")
                .exists()
        );
        assert_eq!(
            hit_paths(&store, "new").await,
            vec!["new.md"],
            "BM25 still works"
        );
        assert_eq!(imports_entries(workspace.path()), Vec::<String>::new());
    }

    #[tokio::test]
    async fn the_upload_directory_cache_is_cleared_by_the_swap() {
        let workspace = tempfile::tempdir().unwrap();
        write_tree(&companion_dir(workspace.path()), &[]);
        let old = crate::services::uploads::save_upload(
            workspace.path(),
            CANONICAL_SLUG,
            "old.png",
            b"old bytes",
        )
        .unwrap();
        let store = Arc::new(VectorStore::connect(workspace.path()).await);
        let media = store.media_store();
        assert_eq!(
            media
                .upload_descriptor(CANONICAL_SLUG, &old.id)
                .unwrap()
                .size,
            9,
            "caches the uploads directory handle"
        );

        // Build the archive from a tree that carries its own upload.
        let scratch = tempfile::tempdir().unwrap();
        write_tree(&scratch.path().join("instances/companion"), &[]);
        let new = crate::services::uploads::save_upload(
            scratch.path(),
            CANONICAL_SLUG,
            "new.png",
            b"new upload bytes",
        )
        .unwrap();
        let source =
            profile_archive::open_companion_dir(&scratch.path().join("instances/companion"))
                .unwrap();
        let mut bytes = Vec::new();
        profile_archive::write_archive(&source, &mut bytes).unwrap();

        restore_companion(store.clone(), &tasks(), CANONICAL_SLUG, Cursor::new(bytes))
            .await
            .unwrap();

        assert_eq!(
            media
                .upload_descriptor(CANONICAL_SLUG, &new.id)
                .unwrap()
                .size,
            16,
            "the imported uploads directory is served, not the cached handle"
        );
        assert_eq!(
            media
                .upload_descriptor(CANONICAL_SLUG, &old.id)
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
    }

    #[tokio::test]
    async fn an_import_is_refused_while_agent_tasks_run_for_the_companion() {
        let workspace = tempfile::tempdir().unwrap();
        write_tree(
            &companion_dir(workspace.path()),
            &[("memory/old.md", b"old")],
        );
        let store = Arc::new(VectorStore::connect(workspace.path()).await);
        let tree_before = snapshot(&companion_dir(workspace.path()));
        let tasks = tasks();
        tasks.lock().await.insert(
            format!("{CANONICAL_SLUG}/default"),
            CancellationToken::new(),
        );
        tasks
            .lock()
            .await
            .insert("alice/default".to_owned(), CancellationToken::new());

        let error = restore_companion(
            store.clone(),
            &tasks,
            CANONICAL_SLUG,
            archive(&[("memory/new.md", b"new")]),
        )
        .await
        .unwrap_err();

        assert!(matches!(error, RestoreError::Busy { tasks: 1 }), "{error}");
        assert_eq!(snapshot(&companion_dir(workspace.path())), tree_before);
        assert_eq!(
            imports_entries(workspace.path()),
            Vec::<String>::new(),
            "nothing is staged while busy"
        );

        // Another companion's task does not block this one.
        tasks
            .lock()
            .await
            .remove(&format!("{CANONICAL_SLUG}/default"));
        restore_companion(
            store.clone(),
            &tasks,
            CANONICAL_SLUG,
            archive(&[("memory/new.md", b"new")]),
        )
        .await
        .unwrap();
        assert!(
            companion_dir(workspace.path())
                .join("memory/new.md")
                .is_file()
        );
    }

    #[tokio::test]
    async fn a_refused_archive_is_discarded_before_the_swap() {
        let workspace = tempfile::tempdir().unwrap();
        write_tree(
            &companion_dir(workspace.path()),
            &[("memory/old.md", b"old")],
        );
        let store = Arc::new(VectorStore::connect(workspace.path()).await);
        let tree_before = snapshot(&companion_dir(workspace.path()));

        // Wrong root: the writer refuses to build it, so hand-roll a tar.
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_mode(0o644);
        header.set_size(marker().len() as u64);
        builder
            .append_data(&mut header, "alice/companion.json", marker().as_slice())
            .unwrap();
        let tar = builder.into_inner().unwrap();
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut encoder, &tar).unwrap();
        let hostile = Cursor::new(encoder.finish().unwrap());

        let error = restore_companion(store.clone(), &tasks(), CANONICAL_SLUG, hostile)
            .await
            .unwrap_err();

        assert!(
            matches!(error, RestoreError::Archive(ArchiveError::WrongRoot(_))),
            "{error}"
        );
        assert_eq!(snapshot(&companion_dir(workspace.path())), tree_before);
        assert_eq!(imports_entries(workspace.path()), Vec::<String>::new());
        assert!(!workspace.path().join("instances/alice").exists());
    }

    #[tokio::test]
    async fn an_import_creates_the_companion_when_none_exists_yet() {
        let workspace = tempfile::tempdir().unwrap();
        let store = Arc::new(VectorStore::connect(workspace.path()).await);

        let outcome = restore_companion(
            store.clone(),
            &tasks(),
            CANONICAL_SLUG,
            archive(&[("memory/new.md", b"new"), ("soul.md", b"soul")]),
        )
        .await
        .unwrap();

        assert_eq!(outcome.files, 3);
        assert_eq!(
            fs::read(companion_dir(workspace.path()).join(IDENTITY_FILE)).unwrap(),
            marker()
        );
        assert_eq!(
            fs::read(companion_dir(workspace.path()).join("soul.md")).unwrap(),
            b"soul"
        );
        assert_eq!(imports_entries(workspace.path()), Vec::<String>::new());
    }

    #[test]
    fn restore_stages_under_imports_and_never_resolves_ambient_paths() {
        // `cap_std::fs::Dir` is the capability type; only ambient `std::fs` is forbidden.
        let production = include_str!("profile_import.rs")
            .split("#[cfg(test)]\nmod tests")
            .next()
            .unwrap()
            .replace("cap_std::fs::", "");
        for forbidden in [
            "std::fs::",
            "fs::read",
            "fs::write",
            "fs::rename",
            "fs::remove",
            "fs::create_dir",
            "Dir::open_ambient_dir(",
            "ambient_authority()",
            "Command::new",
            "\"instances\"",
            "\"instances/",
            "workspace_dir",
            ".instance_slugs()",
        ] {
            assert!(
                !production.contains(forbidden),
                "restore code contains ambient or instance-enumerating operation {forbidden}"
            );
        }
        for required in [
            "lifecycle_lock(",
            "profile_archive::extract_into(",
            "create_import(",
            "stash_companion(",
            "publish_import(",
            "remove_import(",
            "rebuild_derived_no_lifecycle(",
            "rebuild_catalog_snapshot(",
        ] {
            assert!(
                production.contains(required),
                "restore code does not go through {required}"
            );
        }

        let media = include_str!("media_text.rs")
            .split("#[cfg(test)]\nmod tests")
            .next()
            .unwrap();
        assert!(
            media.contains("const IMPORTS_DIR: &str = \"imports\";"),
            "staging must live under a top-level imports/ directory"
        );

        let doc = include_str!("../../../docs/companion-storage.md");
        for required in [
            "#74",
            "imports/",
            "staging-",
            "previous-",
            "lifecycle",
            "409",
            "derived_index",
        ] {
            assert!(
                doc.contains(required),
                "storage doc is missing {required:?}"
            );
        }
    }
}
