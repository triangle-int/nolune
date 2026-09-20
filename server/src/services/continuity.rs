//! On-disk store for continuity records (#81): one bounded JSON file per task
//! under `instances/companion/continuity/`, written atomically and read
//! defensively. Corrupt or oversized files are skipped and reported through
//! [`ContinuityStore::list_errors`], never deleted or rewritten. Restarts
//! change nothing: whatever was active or waiting is still there.
//!
//! Every write is a read-modify-write of one file, and the API, the chat
//! tool, and the reference check on reads may all touch the same record at
//! once. They serialize on one lock per directory, shared by every handle in
//! the process, so nothing ever saves a copy that another writer has since
//! replaced.

use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use tokio::sync::Mutex;

use crate::domain::continuity::{
    Blocker, BlockerKind, ContinuityError, ContinuityRecord, ContinuityState, ContinuityUpdate,
    MAX_BLOCKERS, MAX_RECORD_BYTES, Origin, Provenance, ProvenanceSource, ResourceRef, is_valid_id,
};
use crate::services::{machine_registry::MachineRegistry, uploads};

const CONTINUITY_DIR: &str = "continuity";

/// A file in the continuity directory that is not a readable record.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RecordError {
    pub file: String,
    pub reason: String,
}

#[derive(Clone)]
pub struct ContinuityStore {
    workspace_dir: PathBuf,
    slug: String,
    /// Held across every read-modify-write. One per directory for the whole
    /// process (see [`directory_lock`]), so handles built independently by
    /// the routes and the tool still exclude each other.
    lock: Arc<Mutex<()>>,
}

/// The one lock for a continuity directory, whichever handle asks for it.
fn directory_lock(dir: &Path) -> Arc<Mutex<()>> {
    static LOCKS: OnceLock<std::sync::Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> = OnceLock::new();
    let mut locks = LOCKS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    Arc::clone(locks.entry(dir.to_path_buf()).or_default())
}

impl ContinuityStore {
    pub fn new(workspace_dir: &Path, slug: &str) -> Self {
        let dir = workspace_dir
            .join("instances")
            .join(slug)
            .join(CONTINUITY_DIR);
        Self {
            workspace_dir: workspace_dir.to_path_buf(),
            slug: slug.to_owned(),
            lock: directory_lock(&dir),
        }
    }

    fn dir(&self) -> PathBuf {
        self.workspace_dir
            .join("instances")
            .join(&self.slug)
            .join(CONTINUITY_DIR)
    }

    fn record_path(&self, id: &str) -> PathBuf {
        self.dir().join(format!("{id}.json"))
    }

    // ── writes: explicit task activity only ────────────────────────────────

    /// Start a record for a new task and persist it.
    pub async fn create(
        &self,
        goal: &str,
        origin: Origin,
        update: &ContinuityUpdate,
        provenance: Provenance,
        now: i64,
    ) -> Result<ContinuityRecord, ContinuityError> {
        let mut record =
            ContinuityRecord::new(new_record_id(now), goal, origin, provenance.clone(), now)?;
        if *update != ContinuityUpdate::default() {
            record.apply(update, provenance, now)?;
            // Creation and the initial details are one act, not two entries.
            record.provenance.truncate(1);
        }
        let _guard = self.lock.lock().await;
        self.write(&record)?;
        Ok(record)
    }

    /// Apply one explicit change to an existing record and persist it. The
    /// read and the write happen under the lock: the change lands on what is
    /// on disk now, never on a copy another writer has since replaced.
    pub async fn update(
        &self,
        id: &str,
        update: &ContinuityUpdate,
        provenance: Provenance,
        now: i64,
    ) -> Result<ContinuityRecord, ContinuityError> {
        let _guard = self.lock.lock().await;
        let mut record = self.get(id).ok_or(ContinuityError::NotFound)?;
        record.apply(update, provenance, now)?;
        self.write(&record)?;
        Ok(record)
    }

    pub async fn complete(
        &self,
        id: &str,
        provenance: Provenance,
        now: i64,
    ) -> Result<ContinuityRecord, ContinuityError> {
        self.set_state(id, ContinuityState::Completed, provenance, now)
            .await
    }

    pub async fn dismiss(
        &self,
        id: &str,
        provenance: Provenance,
        now: i64,
    ) -> Result<ContinuityRecord, ContinuityError> {
        self.set_state(id, ContinuityState::Dismissed, provenance, now)
            .await
    }

    async fn set_state(
        &self,
        id: &str,
        state: ContinuityState,
        provenance: Provenance,
        now: i64,
    ) -> Result<ContinuityRecord, ContinuityError> {
        self.update(
            id,
            &ContinuityUpdate {
                state: Some(state),
                ..Default::default()
            },
            provenance,
            now,
        )
        .await
    }

    /// The one write path: validate, bound, and atomically write one record.
    /// Callers hold the lock.
    fn write(&self, record: &ContinuityRecord) -> Result<(), ContinuityError> {
        record.validate()?;
        let json = serde_json::to_string_pretty(record).map_err(io_error)?;
        if json.len() > MAX_RECORD_BYTES {
            return Err(ContinuityError::TooLarge {
                bytes: json.len(),
                max: MAX_RECORD_BYTES,
            });
        }
        fs::create_dir_all(self.dir()).map_err(io_error)?;
        write_atomic(&self.record_path(&record.id), &json).map_err(io_error)
    }

    // ── reads ──────────────────────────────────────────────────────────────

    pub fn get(&self, id: &str) -> Option<ContinuityRecord> {
        if !is_valid_id(id) {
            return None;
        }
        read_record(&self.record_path(id)).ok()
    }

    /// Every readable record, most recently updated first. Production reads
    /// go through [`Self::list_validated`], which runs the reference check.
    #[cfg(test)]
    pub fn list(&self) -> Vec<ContinuityRecord> {
        self.scan().0
    }

    /// Files that were skipped, and why. Nothing is deleted.
    #[cfg(test)]
    pub fn list_errors(&self) -> Vec<RecordError> {
        self.scan().1
    }

    /// Records the user can pick up again: active, waiting, ready to resume.
    #[cfg(test)]
    pub fn resumable(&self) -> Vec<ContinuityRecord> {
        self.list()
            .into_iter()
            .filter(|record| record.state.is_resumable())
            .collect()
    }

    fn scan(&self) -> (Vec<ContinuityRecord>, Vec<RecordError>) {
        let Ok(entries) = fs::read_dir(self.dir()) else {
            return (Vec::new(), Vec::new());
        };
        let mut records = Vec::new();
        let mut errors = Vec::new();
        for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if name.starts_with('.')
                || path.extension().and_then(|ext| ext.to_str()) != Some("json")
            {
                continue;
            }
            match read_record(&path) {
                Ok(record) => records.push(record),
                Err(reason) => errors.push(RecordError {
                    file: name.to_owned(),
                    reason,
                }),
            }
        }
        records.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        (records, errors)
    }

    // ── reads with the reference check ─────────────────────────────────────

    /// One record after the reference check, or `None` when there is no
    /// readable record with this id.
    pub async fn validate_references(
        &self,
        id: &str,
        machines: &MachineRegistry,
        now: i64,
    ) -> Option<ContinuityRecord> {
        let connected = connected_machines(machines).await;
        let _guard = self.lock.lock().await;
        let mut record = self.get(id)?;
        self.check_references(&mut record, &connected, now);
        Some(record)
    }

    /// Every readable record (or only the resumable ones) after the
    /// reference check, most recently updated first, plus the files that
    /// could not be read.
    pub async fn list_validated(
        &self,
        machines: &MachineRegistry,
        now: i64,
        resumable_only: bool,
    ) -> (Vec<ContinuityRecord>, Vec<RecordError>) {
        let connected = connected_machines(machines).await;
        let _guard = self.lock.lock().await;
        let (mut records, errors) = self.scan();
        records.retain(|record| !resumable_only || record.state.is_resumable());
        for record in &mut records {
            self.check_references(record, &connected, now);
        }
        (records, errors)
    }

    /// Missing computers and resources become explicit `machine_unavailable`
    /// and `resource_missing` blockers; ones that came back are cleared.
    /// Nothing else on the record changes. Persists only when a blocker was
    /// added or removed. The record is what is on disk right now and the
    /// caller holds the lock, so the save can never overwrite another write.
    fn check_references(&self, record: &mut ContinuityRecord, connected: &[String], now: i64) {
        let mut expected: Vec<(BlockerKind, String)> = Vec::new();
        for machine_id in &record.machine_ids {
            if !connected.contains(machine_id) {
                expected.push((
                    BlockerKind::MachineUnavailable {
                        machine_id: machine_id.clone(),
                    },
                    format!("computer {machine_id} is not connected"),
                ));
            }
        }
        for link in &record.resources {
            let present = match &link.resource {
                ResourceRef::Upload { id } => {
                    uploads::get_upload_file_path(&self.workspace_dir, &self.slug, id).is_some()
                }
                ResourceRef::Memory { path } => self
                    .workspace_dir
                    .join("instances")
                    .join(&self.slug)
                    .join("memory")
                    .join(path)
                    .is_file(),
                ResourceRef::MachinePath { machine_id, .. } => connected.contains(machine_id),
            };
            if !present {
                expected.push((
                    BlockerKind::ResourceMissing {
                        resource: link.resource.clone(),
                    },
                    format!("{} cannot be found", link.resource.describe()),
                ));
            }
        }

        let before = record.blockers.clone();
        // Reference-check blockers whose reference is back are cleared.
        record.blockers.retain(|blocker| {
            !blocker.is_reference_check() || expected.iter().any(|(kind, _)| kind == &blocker.kind)
        });
        // Missing references gain one blocker each; existing ones are kept as they were.
        for (kind, detail) in expected {
            if record.blockers.len() >= MAX_BLOCKERS {
                break;
            }
            if !record.blockers.iter().any(|blocker| blocker.kind == kind) {
                record.blockers.push(Blocker {
                    kind,
                    detail,
                    provenance: Provenance {
                        source: ProvenanceSource::Server,
                        at: now,
                        note: "reference check".into(),
                    },
                });
            }
        }
        if record.blockers != before
            && let Err(error) = self.write(record)
        {
            log::warn!(
                "[continuity] could not persist reference check for {}: {error}",
                record.id
            );
        }
    }
}

async fn connected_machines(machines: &MachineRegistry) -> Vec<String> {
    machines
        .list()
        .await
        .into_iter()
        .map(|machine| machine.machine_id)
        .collect()
}

/// Read one file defensively: size cap first, then JSON, then invariants.
fn read_record(path: &Path) -> Result<ContinuityRecord, String> {
    let len = fs::metadata(path).map_err(|error| error.to_string())?.len();
    if len > MAX_RECORD_BYTES as u64 {
        return Err(format!(
            "record is {len} bytes; the limit is {MAX_RECORD_BYTES}"
        ));
    }
    let raw = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let record: ContinuityRecord =
        serde_json::from_str(&raw).map_err(|error| format!("not a continuity record: {error}"))?;
    record.validate().map_err(|error| error.to_string())?;
    let stem = path.file_stem().and_then(|stem| stem.to_str());
    if stem != Some(record.id.as_str()) {
        return Err(format!(
            "record id {:?} does not match its file name",
            record.id
        ));
    }
    Ok(record)
}

fn new_record_id(now: i64) -> String {
    let suffix: String = uuid::Uuid::new_v4().simple().to_string()[..8].to_owned();
    format!("task_{now}_{suffix}")
}

fn io_error(error: impl std::fmt::Display) -> ContinuityError {
    ContinuityError::Io(error.to_string())
}

/// Write to a temp file named for this write alone, then rename it into
/// place. Two writers of the same record never share a temp file, and a
/// failed rename leaves nothing behind. Readers skip `.tmp` files.
fn write_atomic(path: &Path, content: &str) -> io::Result<()> {
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("record");
    let tmp = path.with_file_name(format!("{stem}.{}.tmp", uuid::Uuid::new_v4().simple()));
    fs::write(&tmp, content)?;
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::companion::CANONICAL_SLUG;
    use crate::domain::continuity::{
        BlockerKind, CONTINUITY_FORMAT_VERSION, MAX_NOTE_CHARS, MAX_PROVENANCE, MAX_RECORD_BYTES,
        MAX_STEPS, ResourceRef,
    };
    use crate::services::machine_registry::MachineInfo;

    /// 2026-01-05 (Monday) 09:00:00 UTC
    const T0: i64 = 1_767_603_600;

    fn harness() -> (tempfile::TempDir, ContinuityStore) {
        let ws = tempfile::tempdir().unwrap();
        fs::create_dir_all(ws.path().join("instances").join(CANONICAL_SLUG)).unwrap();
        let store = ContinuityStore::new(ws.path(), CANONICAL_SLUG);
        (ws, store)
    }

    fn continuity_dir(ws: &tempfile::TempDir) -> PathBuf {
        ws.path()
            .join("instances")
            .join(CANONICAL_SLUG)
            .join("continuity")
    }

    /// Leftover temp files from atomic writes; always empty after a write returns.
    fn temp_files(ws: &tempfile::TempDir) -> Vec<String> {
        let Ok(entries) = fs::read_dir(continuity_dir(ws)) else {
            return Vec::new();
        };
        entries
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect()
    }

    fn by(source: ProvenanceSource, note: &str) -> Provenance {
        Provenance {
            source,
            at: T0,
            note: note.into(),
        }
    }

    fn origin() -> Origin {
        Origin {
            chat_id: "default".into(),
            message_id: Some("msg_1".into()),
        }
    }

    async fn start(store: &ContinuityStore, goal: &str, now: i64) -> ContinuityRecord {
        store
            .create(
                goal,
                origin(),
                &ContinuityUpdate::default(),
                by(ProvenanceSource::Chat, "asked in chat"),
                now,
            )
            .await
            .unwrap()
    }

    async fn set_state(store: &ContinuityStore, id: &str, state: ContinuityState, now: i64) {
        store
            .update(
                id,
                &ContinuityUpdate {
                    state: Some(state),
                    ..Default::default()
                },
                by(ProvenanceSource::Tool, "state change"),
                now,
            )
            .await
            .unwrap();
    }

    async fn connect(registry: &MachineRegistry, machine_id: &str) {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        registry
            .register(
                MachineInfo {
                    machine_id: machine_id.into(),
                    os: "macos".into(),
                    hostname: "studio".into(),
                    screen_width: 1920,
                    screen_height: 1080,
                    last_seen: T0,
                    instance_slug: Some(CANONICAL_SLUG.into()),
                },
                tx,
            )
            .await;
    }

    #[tokio::test]
    async fn every_state_persists_under_the_companion_and_round_trips() {
        let (ws, store) = harness();
        for (i, state) in ContinuityState::ALL.into_iter().enumerate() {
            let now = T0 + i as i64;
            let created = start(&store, &format!("task {i}"), now).await;
            assert!(created.id.starts_with("task_"), "{}", created.id);
            let path = continuity_dir(&ws).join(format!("{}.json", created.id));
            assert!(path.is_file(), "{}", path.display());
            assert!(
                temp_files(&ws).is_empty(),
                "atomic write left a temp file: {:?}",
                temp_files(&ws)
            );

            set_state(&store, &created.id, state, now + 1).await;
            let stored = store.get(&created.id).unwrap();
            assert_eq!(stored.state, state);
            assert_eq!(stored.version, CONTINUITY_FORMAT_VERSION);
            assert_eq!(stored.goal, format!("task {i}"));
            assert_eq!(stored.origin, origin());
            assert_eq!(stored.updated_at, now + 1);
            assert_eq!(stored.provenance.len(), 2);
            let raw = fs::read_to_string(&path).unwrap();
            assert_eq!(
                serde_json::from_str::<ContinuityRecord>(&raw).unwrap(),
                stored
            );
        }
        let listed = store.list();
        assert_eq!(listed.len(), ContinuityState::ALL.len());
        assert!(
            listed
                .windows(2)
                .all(|w| w[0].updated_at >= w[1].updated_at),
            "most recently updated first"
        );
        assert!(store.list_errors().is_empty());
        assert!(temp_files(&ws).is_empty());
    }

    #[tokio::test]
    async fn restart_preserves_active_and_waiting_and_hides_completed_and_dismissed() {
        let (ws, store) = harness();
        let mut ids = Vec::new();
        for (i, state) in ContinuityState::ALL.into_iter().enumerate() {
            let record = start(&store, &format!("{state:?}"), T0 + i as i64).await;
            set_state(&store, &record.id, state, T0 + 10 + i as i64).await;
            ids.push((record.id, state));
        }
        drop(store);

        // A new process opens the same directory.
        let fresh = ContinuityStore::new(ws.path(), CANONICAL_SLUG);
        let resumable = fresh.resumable();
        let states: Vec<ContinuityState> = resumable.iter().map(|r| r.state).collect();
        assert_eq!(resumable.len(), 3, "{states:?}");
        for (id, state) in &ids {
            let present = resumable.iter().any(|r| &r.id == id);
            assert_eq!(present, state.is_resumable(), "{state:?}");
        }
        assert!(
            !states.contains(&ContinuityState::Completed)
                && !states.contains(&ContinuityState::Dismissed)
                && !states.contains(&ContinuityState::Failed)
        );
        assert_eq!(
            fresh.list().len(),
            ids.len(),
            "closed records stay inspectable"
        );

        // Explicitly reopening a dismissed task is allowed; it is the only way back.
        let (dismissed, _) = ids
            .iter()
            .find(|(_, state)| *state == ContinuityState::Dismissed)
            .unwrap();
        set_state(&fresh, dismissed, ContinuityState::ReadyToResume, T0 + 100).await;
        assert_eq!(fresh.resumable().len(), 4);
    }

    #[tokio::test]
    async fn missing_machines_and_resources_become_explicit_blockers_and_clear_when_back() {
        let (ws, store) = harness();
        let uploads = ws
            .path()
            .join("instances")
            .join(CANONICAL_SLUG)
            .join("uploads");
        fs::create_dir_all(&uploads).unwrap();
        fs::write(
            uploads.join("upload_ok.json"),
            serde_json::json!({
                "id": "upload_ok",
                "original_name": "trip.zip",
                "stored_name": "upload_ok.zip",
                "mime_type": "application/zip",
                "size": 3,
                "uploaded_at": "2026-01-05T09:00:00Z",
            })
            .to_string(),
        )
        .unwrap();
        fs::write(uploads.join("upload_ok.zip"), b"zip").unwrap();
        let memory = ws
            .path()
            .join("instances")
            .join(CANONICAL_SLUG)
            .join("memory")
            .join("notes");
        fs::create_dir_all(&memory).unwrap();
        fs::write(memory.join("trip.md"), "- photos").unwrap();

        let created = start(&store, "rename the photos", T0).await;
        let record = store
            .update(
                &created.id,
                &ContinuityUpdate {
                    completed_step: Some("listed the folder".into()),
                    next_step: Some("rename IMG_* files".into()),
                    machine_ids: vec!["mac-mini".into()],
                    resources: vec![
                        ResourceRef::Upload {
                            id: "upload_ok".into(),
                        },
                        ResourceRef::Upload {
                            id: "upload_gone".into(),
                        },
                        ResourceRef::Memory {
                            path: "notes/trip.md".into(),
                        },
                        ResourceRef::Memory {
                            path: "notes/missing.md".into(),
                        },
                        ResourceRef::MachinePath {
                            machine_id: "laptop".into(),
                            path: "/Volumes/Trip".into(),
                        },
                    ],
                    ..Default::default()
                },
                by(ProvenanceSource::Tool, "progress"),
                T0 + 1,
            )
            .await
            .unwrap();

        let registry = MachineRegistry::new();
        let checked = store
            .validate_references(&created.id, &registry, T0 + 2)
            .await
            .unwrap();
        let kinds: Vec<&BlockerKind> = checked.blockers.iter().map(|b| &b.kind).collect();
        assert_eq!(
            kinds,
            vec![
                &BlockerKind::MachineUnavailable {
                    machine_id: "mac-mini".into()
                },
                &BlockerKind::ResourceMissing {
                    resource: ResourceRef::Upload {
                        id: "upload_gone".into()
                    }
                },
                &BlockerKind::ResourceMissing {
                    resource: ResourceRef::Memory {
                        path: "notes/missing.md".into()
                    }
                },
                &BlockerKind::ResourceMissing {
                    resource: ResourceRef::MachinePath {
                        machine_id: "laptop".into(),
                        path: "/Volumes/Trip".into()
                    }
                },
            ],
            "{:#?}",
            checked.blockers
        );
        for blocker in &checked.blockers {
            assert_eq!(blocker.provenance.source, ProvenanceSource::Server);
            assert!(!blocker.detail.is_empty());
            assert!(blocker.is_reference_check());
        }
        // Nothing else moved.
        assert_eq!(checked.goal, record.goal);
        assert_eq!(checked.state, record.state);
        assert_eq!(checked.machine_ids, record.machine_ids);
        assert_eq!(checked.resources, record.resources);
        assert_eq!(checked.completed_steps, record.completed_steps);
        assert_eq!(checked.next_step, record.next_step);
        assert_eq!(checked.origin, record.origin);
        assert_eq!(checked.created_at, record.created_at);
        // The check is persisted so the blockers survive a restart, and idempotent.
        assert_eq!(store.get(&created.id).unwrap(), checked);
        let again = store
            .validate_references(&created.id, &registry, T0 + 3)
            .await
            .unwrap();
        assert_eq!(again, checked, "no duplicate blockers");
        // The listing runs the same check on every record it returns.
        let (listed, errors) = store.list_validated(&registry, T0 + 3, true).await;
        assert_eq!(listed, vec![checked.clone()]);
        assert!(errors.is_empty());

        // The computer reconnects: its blockers clear; the missing files stay.
        connect(&registry, "mac-mini").await;
        connect(&registry, "laptop").await;
        let back = store
            .validate_references(&created.id, &registry, T0 + 4)
            .await
            .unwrap();
        let kinds: Vec<&BlockerKind> = back.blockers.iter().map(|b| &b.kind).collect();
        assert_eq!(
            kinds,
            vec![
                &BlockerKind::ResourceMissing {
                    resource: ResourceRef::Upload {
                        id: "upload_gone".into()
                    }
                },
                &BlockerKind::ResourceMissing {
                    resource: ResourceRef::Memory {
                        path: "notes/missing.md".into()
                    }
                },
            ]
        );
        assert_eq!(store.get(&created.id).unwrap(), back);

        // A blocker the tool stated is never touched by the reference check.
        store
            .update(
                &created.id,
                &ContinuityUpdate {
                    blocker: Some("waiting for the user to pick a naming scheme".into()),
                    ..Default::default()
                },
                by(ProvenanceSource::Tool, "asked"),
                T0 + 5,
            )
            .await
            .unwrap();
        let stated = store
            .validate_references(&created.id, &registry, T0 + 6)
            .await
            .unwrap();
        assert_eq!(stated.blockers.len(), 3);
        assert_eq!(stated.blockers[2].kind, BlockerKind::Other);

        // Unknown or unreadable ids are simply absent.
        assert!(
            store
                .validate_references("task_missing", &registry, T0 + 7)
                .await
                .is_none()
        );
        assert!(
            store
                .validate_references("../companion", &registry, T0 + 7)
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn a_dismiss_between_a_read_and_the_reference_check_is_not_reverted() {
        let (_ws, store) = harness();
        let created = start(&store, "rename the photos", T0).await;
        store
            .update(
                &created.id,
                &ContinuityUpdate {
                    machine_ids: vec!["mac-mini".into()],
                    ..Default::default()
                },
                by(ProvenanceSource::Tool, "progress"),
                T0 + 1,
            )
            .await
            .unwrap();

        // A GET reads the record, then awaits the machine registry. Meanwhile
        // the user dismisses the task from the card.
        let stale = store.get(&created.id).unwrap();
        assert_eq!(stale.state, ContinuityState::Active);
        store
            .dismiss(
                &created.id,
                by(ProvenanceSource::User, "not needed"),
                T0 + 2,
            )
            .await
            .unwrap();

        // The reference check finishes: it adds its blocker to what is on
        // disk now, never writing back the copy the GET started from.
        let checked = store
            .validate_references(&created.id, &MachineRegistry::new(), T0 + 3)
            .await
            .unwrap();
        assert_eq!(checked.state, ContinuityState::Dismissed, "{checked:#?}");
        assert_eq!(checked.provenance.last().unwrap().note, "not needed");
        assert_eq!(checked.blockers.len(), 1);
        let on_disk = store.get(&created.id).unwrap();
        assert_eq!(on_disk.state, ContinuityState::Dismissed);
        assert_eq!(on_disk.provenance.len(), 3);
        assert!(
            store.resumable().is_empty(),
            "a dismissed task never comes back"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_writers_and_reference_checks_never_lose_an_update() {
        let (ws, store) = harness();
        let created = start(&store, "shared", T0).await;
        store
            .update(
                &created.id,
                &ContinuityUpdate {
                    machine_ids: vec!["mac-mini".into()],
                    ..Default::default()
                },
                by(ProvenanceSource::Tool, "progress"),
                T0 + 1,
            )
            .await
            .unwrap();
        let registry = MachineRegistry::new();

        // The tool in a running chat, PUTs from the card, and GETs running the
        // reference check all hit the same file at once. Each one gets its own
        // handle, the way the routes and the tool do.
        let mut tasks = tokio::task::JoinSet::new();
        for i in 0..24u32 {
            let store = ContinuityStore::new(ws.path(), CANONICAL_SLUG);
            let registry = registry.clone();
            let id = created.id.clone();
            tasks.spawn(async move {
                if i % 3 == 2 {
                    store
                        .validate_references(&id, &registry, T0 + 10 + i as i64)
                        .await
                        .unwrap();
                } else {
                    store
                        .update(
                            &id,
                            &ContinuityUpdate {
                                completed_step: Some(format!("step {i}")),
                                ..Default::default()
                            },
                            by(ProvenanceSource::Tool, &format!("write {i}")),
                            T0 + 10 + i as i64,
                        )
                        .await
                        .unwrap();
                }
            });
        }
        while let Some(joined) = tasks.join_next().await {
            joined.unwrap();
        }

        let record = store.get(&created.id).unwrap();
        let mut steps: Vec<&str> = record
            .completed_steps
            .iter()
            .map(|step| step.summary.as_str())
            .collect();
        steps.sort();
        let mut expected: Vec<String> = (0..24u32)
            .filter(|i| i % 3 != 2)
            .map(|i| format!("step {i}"))
            .collect();
        expected.sort();
        assert_eq!(steps, expected, "every write landed");
        assert_eq!(record.provenance.len(), 2 + expected.len());
        assert_eq!(
            record.blockers.len(),
            1,
            "one blocker for the missing computer"
        );
        assert!(store.list_errors().is_empty());
        assert!(temp_files(&ws).is_empty(), "{:?}", temp_files(&ws));
    }

    #[tokio::test]
    async fn corrupt_and_oversized_files_are_skipped_and_reported_never_deleted() {
        let (ws, store) = harness();
        let good = start(&store, "good", T0).await;
        let dir = continuity_dir(&ws);
        fs::write(dir.join("task_garbage.json"), "{not json").unwrap();
        let mut future = store.get(&good.id).unwrap();
        future.version = CONTINUITY_FORMAT_VERSION + 1;
        future.id = "task_future".into();
        fs::write(
            dir.join("task_future.json"),
            serde_json::to_string(&future).unwrap(),
        )
        .unwrap();
        let mut invalid = store.get(&good.id).unwrap();
        invalid.provenance.clear();
        invalid.id = "task_noprov".into();
        fs::write(
            dir.join("task_noprov.json"),
            serde_json::to_string(&invalid).unwrap(),
        )
        .unwrap();
        let big = format!(
            "{{\"version\":1,\"id\":\"task_big\",\"goal\":\"{}\"}}",
            "g".repeat(MAX_RECORD_BYTES)
        );
        fs::write(dir.join("task_big.json"), &big).unwrap();
        let mut renamed = store.get(&good.id).unwrap();
        renamed.id = "task_elsewhere".into();
        fs::write(
            dir.join("task_renamed.json"),
            serde_json::to_string(&renamed).unwrap(),
        )
        .unwrap();
        fs::write(dir.join("task_stale.0123abcd.tmp"), "half written").unwrap();
        fs::write(dir.join(".ledger.json"), "{}").unwrap();

        let listed = store.list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, good.id);
        assert!(store.get("task_garbage").is_none());
        assert!(store.get("task_future").is_none());
        assert!(store.get("task_noprov").is_none());
        assert!(store.get("task_big").is_none());

        let mut errors = store.list_errors();
        errors.sort_by(|a, b| a.file.cmp(&b.file));
        let files: Vec<&str> = errors.iter().map(|e| e.file.as_str()).collect();
        assert_eq!(
            files,
            [
                "task_big.json",
                "task_future.json",
                "task_garbage.json",
                "task_noprov.json",
                "task_renamed.json",
            ],
            "{errors:#?}"
        );
        assert!(errors[0].reason.contains("bytes"), "{}", errors[0].reason);
        assert!(errors[1].reason.contains("version"), "{}", errors[1].reason);
        assert!(
            errors[3].reason.contains("provenance"),
            "{}",
            errors[3].reason
        );
        assert!(
            errors[4].reason.contains("file name"),
            "{}",
            errors[4].reason
        );
        assert!(store.get("task_renamed").is_none());
        for file in files {
            assert!(dir.join(file).is_file(), "{file} must not be deleted");
        }
        assert_eq!(
            fs::read_to_string(dir.join("task_big.json")).unwrap(),
            big,
            "oversized files are never rewritten"
        );
        assert!(store.resumable().len() == 1);
        let (listed, errors) = store
            .list_validated(&MachineRegistry::new(), T0 + 1, false)
            .await;
        assert_eq!(listed.len(), 1);
        assert_eq!(errors.len(), 5, "the listing surfaces the same files");
    }

    #[tokio::test]
    async fn writes_are_bounded_and_require_provenance() {
        let (ws, store) = harness();
        let record = start(&store, "bounded", T0).await;
        let path = continuity_dir(&ws).join(format!("{}.json", record.id));
        let before = fs::read_to_string(&path).unwrap();

        assert!(matches!(
            store
                .update(
                    &record.id,
                    &ContinuityUpdate {
                        state: Some(ContinuityState::Waiting),
                        ..Default::default()
                    },
                    by(ProvenanceSource::User, "   "),
                    T0 + 1,
                )
                .await,
            Err(ContinuityError::Invalid(_))
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), before);

        let mut stripped = record.clone();
        stripped.provenance.clear();
        assert!(matches!(
            store.write(&stripped),
            Err(ContinuityError::Invalid(_))
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), before);

        // Every field at its own cap still fits in one file, so a record can
        // always take one more provenance entry to be closed.
        use crate::domain::continuity::{MAX_PATH_BYTES, MAX_RESOURCES, ResourceLink, Step};
        let mut huge = record.clone();
        huge.completed_steps = (0..MAX_STEPS)
            .map(|_| Step {
                summary: "s".repeat(MAX_NOTE_CHARS),
                provenance: by(ProvenanceSource::Tool, &"n".repeat(MAX_NOTE_CHARS)),
            })
            .collect();
        huge.resources = (0..MAX_RESOURCES)
            .map(|i| ResourceLink {
                resource: ResourceRef::MachinePath {
                    machine_id: "mac-mini".into(),
                    path: format!("/{i}/{}", "p".repeat(MAX_PATH_BYTES - 8)),
                },
                provenance: by(ProvenanceSource::Tool, "link"),
            })
            .collect();
        huge.provenance = (0..MAX_PROVENANCE)
            .map(|_| by(ProvenanceSource::Tool, &"p".repeat(MAX_NOTE_CHARS)))
            .collect();
        huge.validate().unwrap();
        store.write(&huge).unwrap();
        assert!(fs::metadata(&path).unwrap().len() <= MAX_RECORD_BYTES as u64);
        assert_eq!(store.get(&record.id).unwrap(), huge);
        assert!(temp_files(&ws).is_empty());

        // A file over the cap is refused on the way out as well as on the way in.
        let mut oversized = huge.clone();
        oversized.origin.chat_id = "c".repeat(MAX_RECORD_BYTES);
        assert!(
            matches!(store.write(&oversized), Err(ContinuityError::Invalid(_))),
            "an open-ended field would let a record outgrow its file"
        );

        assert_eq!(
            store
                .create(
                    "   ",
                    origin(),
                    &ContinuityUpdate::default(),
                    by(ProvenanceSource::Chat, "asked"),
                    T0,
                )
                .await,
            Err(ContinuityError::Invalid("goal is required".into()))
        );
        assert_eq!(
            store
                .update(
                    "task_missing",
                    &ContinuityUpdate::default(),
                    by(ProvenanceSource::User, "x"),
                    T0,
                )
                .await,
            Err(ContinuityError::NotFound)
        );
        assert_eq!(store.get("../companion"), None);
        assert_eq!(store.get(".hidden"), None);
    }

    #[tokio::test]
    async fn a_record_grown_to_the_caps_can_still_be_completed_and_dismissed() {
        let (ws, store) = harness();
        let record = start(&store, "fill it up", T0).await;
        let long = |c: char| c.to_string().repeat(MAX_NOTE_CHARS);
        let mut now = T0;
        for _ in 0..MAX_STEPS {
            now += 1;
            store
                .update(
                    &record.id,
                    &ContinuityUpdate {
                        completed_step: Some(long('s')),
                        ..Default::default()
                    },
                    by(ProvenanceSource::Tool, &long('n')),
                    now,
                )
                .await
                .unwrap();
        }
        for i in 0..MAX_PROVENANCE {
            now += 1;
            let state = if i % 2 == 0 {
                ContinuityState::Waiting
            } else {
                ContinuityState::Active
            };
            store
                .update(
                    &record.id,
                    &ContinuityUpdate {
                        state: Some(state),
                        ..Default::default()
                    },
                    by(ProvenanceSource::Tool, &long('t')),
                    now,
                )
                .await
                .unwrap();
        }
        let full = store.get(&record.id).unwrap();
        assert_eq!(full.completed_steps.len(), MAX_STEPS);
        assert_eq!(full.provenance.len(), MAX_PROVENANCE);
        let path = continuity_dir(&ws).join(format!("{}.json", record.id));
        assert!(fs::metadata(&path).unwrap().len() <= MAX_RECORD_BYTES as u64);

        // One more step is over the cap and is refused without a partial write ...
        assert!(matches!(
            store
                .update(
                    &record.id,
                    &ContinuityUpdate {
                        completed_step: Some("one too many".into()),
                        ..Default::default()
                    },
                    by(ProvenanceSource::Tool, "step"),
                    now + 1,
                )
                .await,
            Err(ContinuityError::Invalid(_))
        ));
        assert_eq!(store.get(&record.id).unwrap(), full);

        // ... but closing the task always works.
        let done = store
            .complete(&record.id, by(ProvenanceSource::User, &long('d')), now + 1)
            .await
            .unwrap();
        assert_eq!(done.state, ContinuityState::Completed);
        assert_eq!(done.provenance.len(), MAX_PROVENANCE);
        assert_eq!(done.provenance[0].note, "asked in chat");
        let gone = store
            .dismiss(&record.id, by(ProvenanceSource::User, &long('x')), now + 2)
            .await
            .unwrap();
        assert_eq!(gone.state, ContinuityState::Dismissed);
        assert_eq!(store.get(&record.id).unwrap(), gone);
        assert!(store.resumable().is_empty());
        assert!(store.list_errors().is_empty());
        assert!(temp_files(&ws).is_empty());
    }

    #[tokio::test]
    async fn complete_and_dismiss_close_the_record_with_provenance() {
        let (_ws, store) = harness();
        let a = start(&store, "a", T0).await;
        let b = start(&store, "b", T0 + 1).await;
        let done = store
            .complete(&a.id, by(ProvenanceSource::User, "marked done"), T0 + 2)
            .await
            .unwrap();
        assert_eq!(done.state, ContinuityState::Completed);
        assert_eq!(done.updated_at, T0 + 2);
        assert_eq!(done.provenance.last().unwrap().note, "marked done");
        let gone = store
            .dismiss(&b.id, by(ProvenanceSource::User, "not needed"), T0 + 3)
            .await
            .unwrap();
        assert_eq!(gone.state, ContinuityState::Dismissed);
        assert!(store.resumable().is_empty());
        assert_eq!(store.list().len(), 2);
        assert_eq!(
            store
                .complete("task_missing", by(ProvenanceSource::User, "x"), T0)
                .await,
            Err(ContinuityError::NotFound)
        );
    }
}
