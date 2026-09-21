//! Transactional companion restore from a backup archive (#74).
//!
//! [`restore_companion`] stages an archive under the top-level `imports/`
//! directory (never under `instances/`, whose siblings are reported as
//! obsolete companions), validates it there, and only then swaps it into
//! `instances/companion`: the live tree moves to `imports/previous-<id>`, the
//! staged tree moves into place, and a failure of the second rename moves the
//! previous tree back. Both renames and the rollback run on one blocking
//! thread, and the whole transaction from staging to the discard of the
//! previous tree runs on a task of its own that owns two gates: the
//! per-companion lifecycle gate every memory write holds, and the
//! process-wide [`ImportGate`](super::import_gate::ImportGate) every ambient writer of the companion tree
//! (an admitted mutating request, a saved chat message, a proactive run)
//! holds shared. A write that arrives during an import waits and lands in
//! the imported tree afterwards instead of racing the swap, and a caller
//! that stops waiting (an HTTP client that disconnects drops the handler
//! future) detaches from the import instead of aborting it half way.
//! Derived state (vectors, BM25, the catalog snapshot) is rebuilt from the
//! imported memory files before the previous tree is discarded, and a
//! provider that cannot embed leaves the collection marked for the startup
//! backfill rather than claiming a full rebuild. [`recover_on_startup`]
//! reconciles whatever a crash left under `imports/` before any writer
//! starts. See `docs/companion-storage.md` for the contract.

use std::{
    collections::HashMap,
    io::{self, Read},
    sync::Arc,
    time::Duration,
};

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use tokio_util::sync::CancellationToken;

use super::{
    media_text::{ImportEntryKind, MediaStore},
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

/// `imports/upload-<id>.companion.tar.gz`: where the import route streams a
/// request body before the restore reads it.
pub(crate) const UPLOAD_PREFIX: &str = "upload-";

/// How long an import waits for ambient writers in flight (a request being
/// handled, a proactive run) before it reports the companion busy. Requests
/// finish in milliseconds; a run that outlasts this is reported rather than
/// interrupted.
const AMBIENT_WRITER_WAIT: Duration = Duration::from_secs(10);

/// Largest identity marker re-read from staging before the swap.
const MAX_IDENTITY_BYTES: u64 = 4096;

/// State of the semantic index after an import.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivedIndex {
    /// Vectors and BM25 were rebuilt from the imported memory files.
    Rebuilt,
    /// The files are in place but the vectors were not rebuilt: the
    /// embedding provider could not embed, or the emptied collection could
    /// not be written and was discarded instead. Either way no record of the
    /// replaced tree is served, `VectorStore::needs_backfill` is `true` and
    /// the startup backfill retries. BM25 is rebuilt lazily on the next
    /// search.
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
    /// Ambient writers (a request being handled, a proactive run) still held
    /// the import gate after [`AMBIENT_WRITER_WAIT`]; nothing was staged.
    /// The route answers `409` too.
    WritersInFlight,
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
    /// The transaction task stopped without reporting (a panic, or the
    /// runtime shutting down). The gate is released and `imports/` holds
    /// whatever step it reached; the startup recovery reconciles it.
    Aborted(io::Error),
}

impl std::fmt::Display for RestoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy { tasks } => write!(
                f,
                "companion is busy: {tasks} agent task(s) are running; retry once they finish"
            ),
            Self::WritersInFlight => write!(
                f,
                "companion is busy: a request or a background routine is still writing to it; retry in a moment"
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
            Self::Aborted(error) => write!(f, "import did not run to completion: {error}"),
        }
    }
}

impl std::error::Error for RestoreError {}

/// Agent loops registered in `agent_tasks` for `slug`, `own_task` excepted:
/// they write the companion through ambient paths and hold no gate, so an
/// import is refused while any exists. The route asks before it reads the
/// request body; the restore asks again under the gates, where the answer
/// is authoritative.
pub(crate) async fn running_agent_tasks(
    agent_tasks: &tokio::sync::Mutex<HashMap<String, CancellationToken>>,
    slug: &str,
    own_task: Option<&str>,
) -> usize {
    let prefix = format!("{slug}/");
    let tasks = agent_tasks.lock().await;
    tasks
        .keys()
        .filter(|key| key.starts_with(&prefix) && own_task != Some(key.as_str()))
        .count()
}

/// Replace the companion `slug` with the contents of `archive`.
///
/// Holds `VectorStore::lifecycle_lock(slug)` and the exclusive side of the
/// [`ImportGate`](super::import_gate::ImportGate), taken together and
/// holding neither while ambient writers finish, from before the archive is
/// staged until derived state has been rebuilt and the previous tree
/// removed. Once the busy check has passed the transaction runs on a task
/// of its own that owns both gates,
/// so dropping this future (an HTTP client that disconnects drops the axum
/// handler future) detaches from the import rather than stopping it between
/// two steps; the import then finishes on its own and logs its result. The
/// archive is read on a blocking thread through the validating extractor in
/// `profile_archive`, so nothing but the staging directory is written before
/// validation succeeds.
pub async fn restore_companion<R: Read + Send + 'static>(
    store: Arc<VectorStore>,
    agent_tasks: &tokio::sync::Mutex<HashMap<String, CancellationToken>>,
    slug: &str,
    archive: R,
) -> Result<RestoreOutcome, RestoreError> {
    restore(store, agent_tasks, None, slug, archive).await
}

/// `restore_companion` for a caller that runs inside the agent loop `own_task`
/// (an `agent_tasks` key): that loop is blocked on this call and cannot write
/// during the swap, so it does not count as busy. The `restore_backup` tool
/// passes its own conversation; every other loop still refuses the import.
pub async fn restore_companion_from_agent<R: Read + Send + 'static>(
    store: Arc<VectorStore>,
    agent_tasks: &tokio::sync::Mutex<HashMap<String, CancellationToken>>,
    own_task: &str,
    slug: &str,
    archive: R,
) -> Result<RestoreOutcome, RestoreError> {
    restore(store, agent_tasks, Some(own_task), slug, archive).await
}

async fn restore<R: Read + Send + 'static>(
    store: Arc<VectorStore>,
    agent_tasks: &tokio::sync::Mutex<HashMap<String, CancellationToken>>,
    own_task: Option<&str>,
    slug: &str,
    archive: R,
) -> Result<RestoreOutcome, RestoreError> {
    // The lifecycle gate first: an import queues behind a backfill or a
    // memory write like any other lifecycle change, holding nothing while it
    // waits. Then the import gate, exclusively, and without holding the
    // lifecycle gate while ambient writers in flight (a request, a proactive
    // run) finish: one of them may need the lifecycle gate for a memory
    // write, and queueing it behind the import would stall both. So the
    // import lets the lifecycle gate go, waits a bounded time for the
    // writers, and takes both again; while it holds the import gate no new
    // writer starts. Nothing has been written up to here, so dropping the
    // future while it waits on any of them is harmless.
    let import_gate = store.media_store().import_gate();
    let deadline = tokio::time::Instant::now() + AMBIENT_WRITER_WAIT;
    let (gate, exclusive) = loop {
        let gate = store.lifecycle_lock(slug).lock_owned().await;
        if let Some(exclusive) = import_gate.try_import() {
            break (gate, exclusive);
        }
        drop(gate);
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero()
            || tokio::time::timeout(remaining, import_gate.idle())
                .await
                .is_err()
        {
            return Err(RestoreError::WritersInFlight);
        }
    };

    // Agent loops write the companion through ambient paths and hold no
    // gate, so an import while one runs would race the swap. Refuse before
    // anything is staged; the route answers 409.
    let running = running_agent_tasks(agent_tasks, slug, own_task).await;
    if running > 0 {
        return Err(RestoreError::Busy { tasks: running });
    }

    let slug = slug.to_owned();
    tokio::spawn(async move {
        let result = transaction(&store, &slug, archive).await;
        match &result {
            Ok(outcome) => log::info!(
                "[import] restored {slug}: {} files, {} bytes, index {:?}",
                outcome.files,
                outcome.bytes,
                outcome.derived_index
            ),
            Err(error) => log::warn!("[import] restore of {slug} failed: {error}"),
        }
        drop(exclusive);
        drop(gate);
        result
    })
    .await
    .unwrap_or_else(|error| Err(RestoreError::Aborted(task_error(error))))
}

/// The import proper, run with the lifecycle gate held by the caller.
async fn transaction<R: Read + Send + 'static>(
    store: &VectorStore,
    slug: &str,
    archive: R,
) -> Result<RestoreOutcome, RestoreError> {
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

    // Swap: both renames and the rollback on one blocking thread, so no
    // other task runs between them and, once started, they run to
    // completion whatever happens to the futures waiting on them.
    let swapped = tokio::task::spawn_blocking({
        let media = media.clone();
        let slug = slug.to_owned();
        let staging = staging_name.clone();
        let previous = previous_name.clone();
        move || swap(&media, &slug, &staging, &previous)
    })
    .await
    .unwrap_or_else(|error| Err(RestoreError::Aborted(task_error(error))));
    let had_previous = match swapped {
        Ok(had_previous) => had_previous,
        Err(error) => {
            discard(&media, &staging_name).await;
            return Err(error);
        }
    };
    log::info!(
        "[import] published {} files ({} bytes) for {slug}",
        summary.files,
        summary.bytes
    );

    // Derived state: vectors and BM25 are emptied and rebuilt from the
    // imported memory files. A provider that cannot embed, or a reset that
    // cannot be written (the collection is discarded then), leaves it marked
    // for the startup backfill; BM25 rebuilds lazily.
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

/// Park the live tree, move the staged tree into place, and move the parked
/// tree back if that second rename fails. `Ok(true)` when a previous tree
/// was replaced. Runs on one blocking thread: there is no point between the
/// renames at which another task is scheduled or a dropped future stops it.
fn swap(
    media: &MediaStore,
    slug: &str,
    staging: &str,
    previous: &str,
) -> Result<bool, RestoreError> {
    let had_previous = media
        .stash_companion(slug, previous)
        .map_err(RestoreError::PublishFailed)?;
    let Err(error) = media.publish_import(slug, staging) else {
        return Ok(had_previous);
    };
    let rollback = if had_previous {
        media.publish_import(slug, previous)
    } else {
        Ok(())
    };
    Err(match rollback {
        Ok(()) => RestoreError::PublishFailed(error),
        Err(rollback) => {
            log::error!(
                "[import] rollback failed; the previous companion is at imports/{previous}: {rollback}"
            );
            RestoreError::PublishStranded {
                error,
                rollback,
                previous: previous.to_owned(),
            }
        }
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

/// What [`recover_on_startup`] found under `imports/` and did about it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Recovery {
    /// The parked tree moved back to `instances/companion`: the process died
    /// between the two renames of a swap and the companion was missing.
    pub restored: Option<String>,
    /// Parked trees found next to a live companion: the swap had published
    /// the import before the process died, so they are the replaced data and
    /// were discarded; the derived index is reset for the startup backfill
    /// because the rebuild may not have run.
    pub discarded_previous: Vec<String>,
    /// Parked trees left in place because the companion was missing and more
    /// than one candidate exists; an operator has to choose.
    pub ambiguous: Vec<String>,
    /// Staging directories and uploaded archives removed.
    pub swept: Vec<String>,
    /// Names left alone: not an import artifact, not a regular file or
    /// directory, or one that could not be removed.
    pub ignored: Vec<String>,
}

impl Recovery {
    pub fn is_noop(&self) -> bool {
        *self == Self::default()
    }
}

/// Reconcile `imports/` after a crash (#74), before any writer starts.
///
/// A process that dies between the two renames of a swap leaves no
/// `instances/companion` and one `imports/previous-<id>`: that tree is moved
/// back, so the companion is exactly what it was before the import. One that
/// dies after the second rename leaves the imported tree live and a
/// `previous-*` tree beside it: the tree is the replaced data and is
/// discarded, and because the derived rebuild may not have run the vector
/// collection is reset so the startup backfill re-indexes the live tree.
/// `staging-*` directories and `upload-*` archives are always leftovers and
/// are removed. Anything else under `imports/`, and a `previous-*` tree that
/// is not the one candidate for a missing companion, is left in place and
/// reported.
pub async fn recover_on_startup(store: &VectorStore, slug: &str) -> io::Result<Recovery> {
    let media = store.media_store();
    let (recovery, reset_index) = blocking({
        let slug = slug.to_owned();
        move || reconcile_imports(&media, &slug)
    })
    .await?;
    if reset_index {
        match store.reset_collection(slug).await {
            Ok(()) => log::info!(
                "[import] derived index of {slug} reset for the startup backfill after an interrupted import"
            ),
            Err(error) => log::warn!(
                "[import] could not reset the derived index of {slug} after an interrupted import: {error}"
            ),
        }
    }
    Ok(recovery)
}

/// The filesystem half of [`recover_on_startup`]; `true` when the derived
/// index must be reset.
fn reconcile_imports(media: &MediaStore, slug: &str) -> io::Result<(Recovery, bool)> {
    let entries = media.list_imports()?;
    let mut recovery = Recovery::default();
    if entries.is_empty() {
        return Ok((recovery, false));
    }
    let mut previous = Vec::new();
    let mut staging = Vec::new();
    let mut uploads = Vec::new();
    for entry in entries {
        match entry.kind {
            ImportEntryKind::Directory if entry.name.starts_with(PREVIOUS_PREFIX) => {
                previous.push(entry.name);
            }
            ImportEntryKind::Directory if entry.name.starts_with(STAGING_PREFIX) => {
                staging.push(entry.name);
            }
            ImportEntryKind::File if entry.name.starts_with(UPLOAD_PREFIX) => {
                uploads.push(entry.name);
            }
            _ => recovery.ignored.push(entry.name),
        }
    }

    let mut reset_index = false;
    if media.has_companion_tree(slug)? {
        for name in previous {
            match media.remove_import(&name) {
                Ok(()) => {
                    log::warn!(
                        "[import] discarded imports/{name}: the import it belonged to was published before the process stopped"
                    );
                    recovery.discarded_previous.push(name);
                    reset_index = true;
                }
                Err(error) => {
                    log::warn!("[import] could not remove imports/{name}: {error}");
                    recovery.ignored.push(name);
                }
            }
        }
    } else if previous.len() == 1 {
        let name = previous.remove(0);
        media.publish_import(slug, &name)?;
        log::warn!(
            "[import] moved imports/{name} back into place: the companion was missing after an interrupted import"
        );
        recovery.restored = Some(name);
    } else if !previous.is_empty() {
        log::error!(
            "[import] the companion is missing and {} parked trees are under imports/ ({}); none was moved back, choose one by hand",
            previous.len(),
            previous.join(", ")
        );
        recovery.ambiguous = previous;
    }

    for name in staging {
        match media.remove_import(&name) {
            Ok(()) => recovery.swept.push(name),
            Err(error) => {
                log::warn!("[import] could not remove imports/{name}: {error}");
                recovery.ignored.push(name);
            }
        }
    }
    for name in uploads {
        match media.remove_import_upload(&name) {
            Ok(()) => recovery.swept.push(name),
            Err(error) => {
                log::warn!("[import] could not remove imports/{name}: {error}");
                recovery.ignored.push(name);
            }
        }
    }
    if !recovery.ignored.is_empty() {
        log::warn!(
            "[import] left alone under imports/: {}",
            recovery.ignored.join(", ")
        );
    }
    Ok((recovery, reset_index))
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

        // Build the archive from a tree that carries its own upload. Upload ids
        // are minted from the millisecond clock, so let it move past the first
        // upload's: a fast runner can otherwise mint both uploads as one id.
        let minted: u128 = old.uploaded_at.parse().unwrap();
        while std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
            <= minted
        {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let scratch = tempfile::tempdir().unwrap();
        write_tree(&scratch.path().join("instances/companion"), &[]);
        let new = crate::services::uploads::save_upload(
            scratch.path(),
            CANONICAL_SLUG,
            "new.png",
            b"new upload bytes",
        )
        .unwrap();
        assert_ne!(new.id, old.id, "the two uploads must be distinct records");
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

    /// The `restore_backup` tool runs inside an agent loop of its own, which
    /// is blocked on the tool call and cannot write during the swap: that one
    /// conversation is not busy, every other one still is.
    #[tokio::test]
    async fn an_import_from_a_conversation_discounts_that_conversation_only() {
        let workspace = tempfile::tempdir().unwrap();
        write_tree(
            &companion_dir(workspace.path()),
            &[("memory/old.md", b"old")],
        );
        let store = Arc::new(VectorStore::connect(workspace.path()).await);
        let tree_before = snapshot(&companion_dir(workspace.path()));
        let own = format!("{CANONICAL_SLUG}/default");
        let tasks = tasks();
        tasks
            .lock()
            .await
            .insert(own.clone(), CancellationToken::new());
        tasks
            .lock()
            .await
            .insert(format!("{CANONICAL_SLUG}/other"), CancellationToken::new());

        let error = restore_companion_from_agent(
            store.clone(),
            &tasks,
            &own,
            CANONICAL_SLUG,
            archive(&[("memory/new.md", b"new")]),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, RestoreError::Busy { tasks: 1 }), "{error}");
        assert_eq!(snapshot(&companion_dir(workspace.path())), tree_before);
        assert_eq!(imports_entries(workspace.path()), Vec::<String>::new());

        tasks
            .lock()
            .await
            .remove(&format!("{CANONICAL_SLUG}/other"));
        let outcome = restore_companion_from_agent(
            store.clone(),
            &tasks,
            &own,
            CANONICAL_SLUG,
            archive(&[("memory/new.md", b"new")]),
        )
        .await
        .unwrap();
        assert_eq!(outcome.files, 2);
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
        // The plain entry point still counts the caller's own conversation.
        let error = restore_companion(
            store,
            &tasks,
            CANONICAL_SLUG,
            archive(&[("memory/new.md", b"new")]),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, RestoreError::Busy { tasks: 1 }), "{error}");
    }

    /// Poll `done` until it holds, yielding to other tasks in between.
    async fn wait_until(done: impl Fn() -> bool) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "condition never held"
            );
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    }

    /// An admitted request or a proactive run holds the import gate shared:
    /// the import stages nothing until it is released, and a writer that
    /// arrives once the import holds the gate waits and then sees the
    /// imported tree.
    #[tokio::test]
    async fn ambient_writers_and_the_import_take_turns_on_the_import_gate() {
        let workspace = tempfile::tempdir().unwrap();
        write_tree(
            &companion_dir(workspace.path()),
            &[("memory/old.md", b"old")],
        );
        let store = Arc::new(VectorStore::connect(workspace.path()).await);
        let gate = store.media_store().import_gate();
        let writer = gate.writer().await;

        let import = {
            let store = store.clone();
            let tasks = tasks();
            let archive = archive(&[("memory/new.md", b"new")]);
            tokio::spawn(
                async move { restore_companion(store, &tasks, CANONICAL_SLUG, archive).await },
            )
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!import.is_finished(), "the import waits for the writer");
        assert!(!gate.importing());
        assert_eq!(
            imports_entries(workspace.path()),
            Vec::<String>::new(),
            "nothing is staged while a writer is in flight"
        );
        assert!(
            companion_dir(workspace.path())
                .join("memory/old.md")
                .is_file()
        );
        drop(writer);

        wait_until(|| gate.importing()).await;
        let late = {
            let gate = gate.clone();
            let memory = companion_dir(workspace.path()).join("memory");
            tokio::spawn(async move {
                let _writer = gate.writer().await;
                (
                    memory.join("new.md").is_file(),
                    memory.join("old.md").exists(),
                )
            })
        };
        tokio::task::yield_now().await;
        assert!(
            !late.is_finished(),
            "a writer arriving now waits for the import"
        );
        let outcome = import.await.unwrap().unwrap();
        assert_eq!(outcome.files, 2);
        assert_eq!(
            late.await.unwrap(),
            (true, false),
            "the late writer ran after the import, against the imported tree"
        );
        assert!(!gate.importing());
        assert_eq!(imports_entries(workspace.path()), Vec::<String>::new());
    }

    /// While the import waits for a writer to finish it holds nothing: a
    /// memory write that arrives then (another admitted request) goes
    /// through the lifecycle gate at once instead of queueing behind the
    /// import and stalling it for the whole wait, and the import follows.
    #[tokio::test]
    async fn a_memory_write_arriving_while_the_import_waits_for_writers_is_not_stalled() {
        let workspace = tempfile::tempdir().unwrap();
        write_tree(
            &companion_dir(workspace.path()),
            &[("memory/old.md", b"old")],
        );
        let store = Arc::new(VectorStore::connect(workspace.path()).await);
        let gate = store.media_store().import_gate();
        let first = gate.writer().await;

        let import = {
            let store = store.clone();
            let tasks = tasks();
            let archive = archive(&[("memory/new.md", b"new")]);
            tokio::spawn(
                async move { restore_companion(store, &tasks, CANONICAL_SLUG, archive).await },
            )
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!import.is_finished(), "the import waits for the writer");

        // A second request: admitted (the import is only waiting), and its
        // memory write must not wait for the import.
        let second = gate
            .try_writer()
            .expect("a writer is admitted while the import waits");
        tokio::time::timeout(
            Duration::from_secs(3),
            store.write_text_memory(CANONICAL_SLUG, "during.md", "written while waiting", false),
        )
        .await
        .expect("the write waited for the import instead of going first")
        .unwrap();
        assert!(
            companion_dir(workspace.path())
                .join("memory/during.md")
                .is_file()
        );
        drop(second);
        assert!(!import.is_finished(), "the first writer is still in flight");
        drop(first);

        let outcome = import.await.unwrap().unwrap();
        assert_eq!(outcome.files, 2);
        assert!(
            companion_dir(workspace.path())
                .join("memory/new.md")
                .is_file()
        );
        assert!(
            !companion_dir(workspace.path())
                .join("memory/during.md")
                .exists(),
            "the write landed before the import and was replaced by it"
        );
        assert_eq!(imports_entries(workspace.path()), Vec::<String>::new());
    }

    /// A writer that outlasts the import's bounded wait (a proactive run in
    /// the middle of a model call) is reported, never interrupted, and the
    /// companion is untouched.
    #[tokio::test(start_paused = true)]
    async fn an_import_reports_busy_when_ambient_writers_outlast_its_wait() {
        let workspace = tempfile::tempdir().unwrap();
        write_tree(
            &companion_dir(workspace.path()),
            &[("memory/old.md", b"old")],
        );
        let store = Arc::new(VectorStore::connect(workspace.path()).await);
        let tree_before = snapshot(&companion_dir(workspace.path()));
        let gate = store.media_store().import_gate();
        let writer = gate.writer().await;

        let error = restore_companion(
            store.clone(),
            &tasks(),
            CANONICAL_SLUG,
            archive(&[("memory/new.md", b"new")]),
        )
        .await
        .unwrap_err();

        assert!(matches!(error, RestoreError::WritersInFlight), "{error}");
        assert!(error.to_string().contains("busy"), "{error}");
        assert_eq!(snapshot(&companion_dir(workspace.path())), tree_before);
        assert_eq!(imports_entries(workspace.path()), Vec::<String>::new());
        assert!(!gate.importing(), "a refused import holds nothing");
        // The gate is free again: a later write and a later import go through.
        store
            .write_text_memory(CANONICAL_SLUG, "later.md", "after the refusal", false)
            .await
            .unwrap();
        drop(writer);
        let outcome = restore_companion(
            store.clone(),
            &tasks(),
            CANONICAL_SLUG,
            archive(&[("memory/new.md", b"new")]),
        )
        .await
        .unwrap();
        assert_eq!(outcome.files, 2);
        assert!(
            companion_dir(workspace.path())
                .join("memory/new.md")
                .is_file()
        );
    }

    /// The process died between the two renames: the companion is missing
    /// and its tree is parked. Startup moves it back byte for byte and
    /// sweeps the staging directory and the uploaded archive, leaving
    /// anything else under `imports/` alone.
    #[tokio::test]
    async fn startup_recovery_moves_a_lone_parked_tree_back_and_sweeps_leftovers() {
        let mock = MockServer::new(vec![(200, response(vec![1., 0., 0.])); 1]).await;
        let workspace = tempfile::tempdir().unwrap();
        write_tree(
            &companion_dir(workspace.path()),
            &[("memory/old.md", b"Orion"), ("soul.md", b"old soul")],
        );
        let store =
            Arc::new(VectorStore::connect_with_config(workspace.path(), &mock.config).await);
        store
            .backfill_text_memories(workspace.path(), CANONICAL_SLUG)
            .await
            .unwrap();
        let expected = snapshot(&companion_dir(workspace.path()));
        // The process died right after the first rename of a swap.
        let imports = workspace.path().join("imports");
        fs::create_dir_all(&imports).unwrap();
        fs::rename(
            companion_dir(workspace.path()),
            imports.join("previous-abc"),
        )
        .unwrap();
        write_tree(&imports.join("staging-abc"), &[("memory/new.md", b"new")]);
        fs::write(imports.join("upload-abc.companion.tar.gz"), b"partial").unwrap();
        fs::write(imports.join("notes.txt"), b"operator note").unwrap();
        assert!(!companion_dir(workspace.path()).exists());

        let recovery = recover_on_startup(&store, CANONICAL_SLUG).await.unwrap();

        assert_eq!(recovery.restored.as_deref(), Some("previous-abc"));
        assert_eq!(
            recovery.swept,
            vec!["staging-abc", "upload-abc.companion.tar.gz"]
        );
        assert_eq!(recovery.ignored, vec!["notes.txt"]);
        assert!(recovery.discarded_previous.is_empty());
        assert!(recovery.ambiguous.is_empty());
        assert_eq!(snapshot(&companion_dir(workspace.path())), expected);
        assert_eq!(imports_entries(workspace.path()), vec!["notes.txt"]);
        assert!(
            !store.needs_backfill(CANONICAL_SLUG).await.unwrap(),
            "the index still describes the tree that is back in place"
        );
        assert_eq!(listing(&store).await[0].0, "old.md");
        // Running again finds nothing to move.
        let again = recover_on_startup(&store, CANONICAL_SLUG).await.unwrap();
        assert_eq!(again.restored, None);
        assert_eq!(again.ignored, vec!["notes.txt"]);
        assert_eq!(snapshot(&companion_dir(workspace.path())), expected);
    }

    /// The process died after the second rename: the imported tree is live
    /// and the replaced one is still parked. Startup discards the parked
    /// tree and, because the derived rebuild may not have run, resets the
    /// collection so the startup backfill re-indexes the live tree.
    #[tokio::test]
    async fn startup_recovery_discards_a_parked_tree_beside_a_published_import_and_reindexes() {
        let mock = MockServer::new(vec![(200, response(vec![1., 0., 0.])); 2]).await;
        let workspace = tempfile::tempdir().unwrap();
        write_tree(
            &companion_dir(workspace.path()),
            &[("memory/new.md", b"Andromeda")],
        );
        let store =
            Arc::new(VectorStore::connect_with_config(workspace.path(), &mock.config).await);
        // The index still describes the replaced tree.
        let mut vector = vec![0.; 3];
        vector[0] = 1.;
        store
            .upsert_text_memory(CANONICAL_SLUG, "old.md", vec![("Orion".into(), vector)])
            .await
            .unwrap();
        store
            .backfill_text_memories(workspace.path(), CANONICAL_SLUG)
            .await
            .unwrap();
        let before = snapshot(&companion_dir(workspace.path()));
        let imports = workspace.path().join("imports");
        write_tree(
            &imports.join("previous-abc"),
            &[("memory/old.md", b"Orion")],
        );

        let recovery = recover_on_startup(&store, CANONICAL_SLUG).await.unwrap();

        assert_eq!(recovery.discarded_previous, vec!["previous-abc"]);
        assert_eq!(recovery.restored, None);
        assert!(recovery.swept.is_empty());
        assert_eq!(snapshot(&companion_dir(workspace.path())), before);
        assert_eq!(imports_entries(workspace.path()), Vec::<String>::new());
        assert!(
            store.needs_backfill(CANONICAL_SLUG).await.unwrap(),
            "the derived index is rebuilt by the startup backfill"
        );
        assert!(listing(&store).await.is_empty());
        assert_eq!(
            store
                .backfill_text_memories(workspace.path(), CANONICAL_SLUG)
                .await
                .unwrap(),
            1
        );
        assert_eq!(listing(&store).await[0].0, "new.md");
    }

    /// Two parked trees and no companion: nothing is chosen for the operator.
    #[tokio::test]
    async fn startup_recovery_leaves_several_parked_trees_for_the_operator() {
        let workspace = tempfile::tempdir().unwrap();
        let store = Arc::new(VectorStore::connect(workspace.path()).await);
        let imports = workspace.path().join("imports");
        write_tree(&imports.join("previous-one"), &[("soul.md", b"one")]);
        write_tree(&imports.join("previous-two"), &[("soul.md", b"two")]);
        write_tree(&imports.join("staging-one"), &[]);
        let before = snapshot(&imports);

        let recovery = recover_on_startup(&store, CANONICAL_SLUG).await.unwrap();

        assert_eq!(recovery.ambiguous, vec!["previous-one", "previous-two"]);
        assert_eq!(recovery.restored, None);
        assert_eq!(recovery.swept, vec!["staging-one"]);
        assert!(!companion_dir(workspace.path()).exists());
        let mut expected = before;
        expected.retain(|path, _| !path.starts_with("staging-one"));
        assert_eq!(snapshot(&imports), expected);
    }

    /// Nothing under `imports/` means nothing happens, and a link parked
    /// there is never followed, moved, or removed.
    #[tokio::test]
    async fn startup_recovery_is_a_noop_without_leftovers_and_never_follows_links() {
        let workspace = tempfile::tempdir().unwrap();
        let store = Arc::new(VectorStore::connect(workspace.path()).await);
        assert!(
            recover_on_startup(&store, CANONICAL_SLUG)
                .await
                .unwrap()
                .is_noop()
        );
        assert!(!workspace.path().join("imports").exists());

        #[cfg(unix)]
        {
            let elsewhere = workspace.path().join("elsewhere");
            write_tree(&elsewhere, &[("soul.md", b"elsewhere")]);
            let imports = workspace.path().join("imports");
            fs::create_dir_all(&imports).unwrap();
            std::os::unix::fs::symlink(&elsewhere, imports.join("previous-link")).unwrap();
            std::os::unix::fs::symlink(&elsewhere, imports.join("staging-link")).unwrap();

            let recovery = recover_on_startup(&store, CANONICAL_SLUG).await.unwrap();

            assert_eq!(recovery.ignored, vec!["previous-link", "staging-link"]);
            assert_eq!(recovery.restored, None);
            assert!(!companion_dir(workspace.path()).exists());
            assert!(
                imports
                    .join("previous-link")
                    .symlink_metadata()
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
            assert!(elsewhere.join("soul.md").is_file());
        }
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
            "import_gate()",
            "profile_archive::extract_into(",
            "create_import(",
            "stash_companion(",
            "publish_import(",
            "remove_import(",
            "rebuild_derived_no_lifecycle(",
            "rebuild_catalog_snapshot(",
            "list_imports(",
            "has_companion_tree(",
            "reset_collection(",
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
