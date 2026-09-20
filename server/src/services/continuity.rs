//! On-disk store for continuity records (#81): one bounded JSON file per task
//! under `instances/companion/continuity/`, written atomically and read
//! defensively. Corrupt or oversized files are skipped and reported through
//! [`ContinuityStore::list_errors`], never deleted or rewritten. Restarts
//! change nothing: whatever was active or waiting is still there.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

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
}

impl ContinuityStore {
    pub fn new(workspace_dir: &Path, slug: &str) -> Self {
        Self {
            workspace_dir: workspace_dir.to_path_buf(),
            slug: slug.to_owned(),
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
    pub fn create(
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
        self.save(&record)?;
        Ok(record)
    }

    /// Apply one explicit change to an existing record and persist it.
    pub fn update(
        &self,
        id: &str,
        update: &ContinuityUpdate,
        provenance: Provenance,
        now: i64,
    ) -> Result<ContinuityRecord, ContinuityError> {
        let mut record = self.get(id).ok_or(ContinuityError::NotFound)?;
        record.apply(update, provenance, now)?;
        self.save(&record)?;
        Ok(record)
    }

    pub fn complete(
        &self,
        id: &str,
        provenance: Provenance,
        now: i64,
    ) -> Result<ContinuityRecord, ContinuityError> {
        self.set_state(id, ContinuityState::Completed, provenance, now)
    }

    pub fn dismiss(
        &self,
        id: &str,
        provenance: Provenance,
        now: i64,
    ) -> Result<ContinuityRecord, ContinuityError> {
        self.set_state(id, ContinuityState::Dismissed, provenance, now)
    }

    fn set_state(
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
    }

    /// Validate, bound, and atomically write one record.
    pub fn save(&self, record: &ContinuityRecord) -> Result<(), ContinuityError> {
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

    /// Every readable record, most recently updated first.
    pub fn list(&self) -> Vec<ContinuityRecord> {
        self.scan().0
    }

    /// Files that were skipped, and why. Nothing is deleted.
    pub fn list_errors(&self) -> Vec<RecordError> {
        self.scan().1
    }

    /// Records the user can pick up again: active, waiting, ready to resume.
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

    /// Missing computers and resources become explicit `machine_unavailable`
    /// and `resource_missing` blockers; ones that came back are cleared.
    /// Nothing else on the record changes. Persists only when a blocker
    /// was added or removed.
    pub async fn validate_references(
        &self,
        mut record: ContinuityRecord,
        machines: &MachineRegistry,
        now: i64,
    ) -> ContinuityRecord {
        let connected: Vec<String> = machines
            .list()
            .await
            .into_iter()
            .map(|machine| machine.machine_id)
            .collect();
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
            && let Err(error) = self.save(&record)
        {
            log::warn!(
                "[continuity] could not persist reference check for {}: {error}",
                record.id
            );
        }
        record
    }
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
    Ok(record)
}

fn new_record_id(now: i64) -> String {
    let suffix: String = uuid::Uuid::new_v4().simple().to_string()[..8].to_owned();
    format!("task_{now}_{suffix}")
}

fn io_error(error: impl std::fmt::Display) -> ContinuityError {
    ContinuityError::Io(error.to_string())
}

fn write_atomic(path: &Path, content: &str) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, content)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::companion::CANONICAL_SLUG;
    use crate::domain::continuity::{
        BlockerKind, CONTINUITY_FORMAT_VERSION, MAX_RECORD_BYTES, ResourceRef,
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

    fn start(store: &ContinuityStore, goal: &str, now: i64) -> ContinuityRecord {
        store
            .create(
                goal,
                origin(),
                &ContinuityUpdate::default(),
                by(ProvenanceSource::Chat, "asked in chat"),
                now,
            )
            .unwrap()
    }

    fn set_state(store: &ContinuityStore, id: &str, state: ContinuityState, now: i64) {
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

    #[test]
    fn every_state_persists_under_the_companion_and_round_trips() {
        let (ws, store) = harness();
        for (i, state) in ContinuityState::ALL.into_iter().enumerate() {
            let now = T0 + i as i64;
            let created = start(&store, &format!("task {i}"), now);
            assert!(created.id.starts_with("task_"), "{}", created.id);
            let path = ws
                .path()
                .join("instances")
                .join(CANONICAL_SLUG)
                .join("continuity")
                .join(format!("{}.json", created.id));
            assert!(path.is_file(), "{}", path.display());
            assert!(
                !path.with_extension("tmp").exists(),
                "atomic write left a temp file"
            );

            set_state(&store, &created.id, state, now + 1);
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
    }

    #[test]
    fn restart_preserves_active_and_waiting_and_hides_completed_and_dismissed() {
        let (ws, store) = harness();
        let mut ids = Vec::new();
        for (i, state) in ContinuityState::ALL.into_iter().enumerate() {
            let record = start(&store, &format!("{state:?}"), T0 + i as i64);
            set_state(&store, &record.id, state, T0 + 10 + i as i64);
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
        set_state(&fresh, dismissed, ContinuityState::ReadyToResume, T0 + 100);
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

        let created = start(&store, "rename the photos", T0);
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
            .unwrap();

        let registry = MachineRegistry::new();
        let checked = store
            .validate_references(record.clone(), &registry, T0 + 2)
            .await;
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
            .validate_references(checked.clone(), &registry, T0 + 3)
            .await;
        assert_eq!(again, checked, "no duplicate blockers");

        // The computer reconnects: its blockers clear; the missing files stay.
        connect(&registry, "mac-mini").await;
        connect(&registry, "laptop").await;
        let back = store.validate_references(again, &registry, T0 + 4).await;
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
            .unwrap();
        let stated = store
            .validate_references(store.get(&created.id).unwrap(), &registry, T0 + 6)
            .await;
        assert_eq!(stated.blockers.len(), 3);
        assert_eq!(stated.blockers[2].kind, BlockerKind::Other);
    }

    #[test]
    fn corrupt_and_oversized_files_are_skipped_and_reported_never_deleted() {
        let (ws, store) = harness();
        let good = start(&store, "good", T0);
        let dir = ws
            .path()
            .join("instances")
            .join(CANONICAL_SLUG)
            .join("continuity");
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
        fs::write(dir.join("task_stale.tmp"), "half written").unwrap();
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
                "task_noprov.json"
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
        for file in files {
            assert!(dir.join(file).is_file(), "{file} must not be deleted");
        }
        assert_eq!(
            fs::read_to_string(dir.join("task_big.json")).unwrap(),
            big,
            "oversized files are never rewritten"
        );
        assert!(store.resumable().len() == 1);
    }

    #[test]
    fn writes_are_bounded_and_require_provenance() {
        let (ws, store) = harness();
        let record = start(&store, "bounded", T0);
        let path = ws
            .path()
            .join("instances")
            .join(CANONICAL_SLUG)
            .join("continuity")
            .join(format!("{}.json", record.id));
        let before = fs::read_to_string(&path).unwrap();

        assert!(matches!(
            store.update(
                &record.id,
                &ContinuityUpdate {
                    state: Some(ContinuityState::Waiting),
                    ..Default::default()
                },
                by(ProvenanceSource::User, "   "),
                T0 + 1,
            ),
            Err(ContinuityError::Invalid(_))
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), before);

        let mut stripped = record.clone();
        stripped.provenance.clear();
        assert!(matches!(
            store.save(&stripped),
            Err(ContinuityError::Invalid(_))
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), before);

        // Every field at its own cap adds up to more than one file may hold.
        use crate::domain::continuity::{
            MAX_NOTE_CHARS, MAX_PATH_BYTES, MAX_PROVENANCE, MAX_RESOURCES, MAX_STEPS, ResourceLink,
            Step,
        };
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
        match store.save(&huge) {
            Err(ContinuityError::TooLarge { bytes, max }) => {
                assert!(bytes > max);
                assert_eq!(max, MAX_RECORD_BYTES);
            }
            other => panic!("expected TooLarge, got {other:?}"),
        }
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
        assert!(!path.with_extension("tmp").exists());

        assert_eq!(
            store.create(
                "   ",
                origin(),
                &ContinuityUpdate::default(),
                by(ProvenanceSource::Chat, "asked"),
                T0,
            ),
            Err(ContinuityError::Invalid("goal is required".into()))
        );
        assert_eq!(
            store.update(
                "task_missing",
                &ContinuityUpdate::default(),
                by(ProvenanceSource::User, "x"),
                T0,
            ),
            Err(ContinuityError::NotFound)
        );
        assert_eq!(store.get("../companion"), None);
        assert_eq!(store.get(".hidden"), None);
    }

    #[test]
    fn complete_and_dismiss_close_the_record_with_provenance() {
        let (_ws, store) = harness();
        let a = start(&store, "a", T0);
        let b = start(&store, "b", T0 + 1);
        let done = store
            .complete(&a.id, by(ProvenanceSource::User, "marked done"), T0 + 2)
            .unwrap();
        assert_eq!(done.state, ContinuityState::Completed);
        assert_eq!(done.updated_at, T0 + 2);
        assert_eq!(done.provenance.last().unwrap().note, "marked done");
        let gone = store
            .dismiss(&b.id, by(ProvenanceSource::User, "not needed"), T0 + 3)
            .unwrap();
        assert_eq!(gone.state, ContinuityState::Dismissed);
        assert!(store.resumable().is_empty());
        assert_eq!(store.list().len(), 2);
        assert_eq!(
            store.complete("task_missing", by(ProvenanceSource::User, "x"), T0),
            Err(ContinuityError::NotFound)
        );
    }
}
