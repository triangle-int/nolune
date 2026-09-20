//! The commitments store (#85).
//!
//! One JSON file per commitment under `instances/{slug}/commitments/`. The
//! record is the source of truth: its `next_check`, waiting condition,
//! dependencies, and snooze survive a restart as plain fields, so the
//! evaluator never has to keep a schedule of its own and a restart cannot
//! create a duplicate one. The store is written from several places at once
//! (API handlers, dependents settled by a completion, the evaluator), so one
//! writer at a time holds `writes` across each read-modify-write, and every
//! write lands in its own temp file before it is renamed into place. Every
//! read of one record goes through the `load` path guard. Reads re-derive an
//! open status against the caller's clock and persist nothing. Callers pass
//! `now` so tests run against a fixed clock.

use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

use serde::Deserialize;

use crate::domain::commitment::{
    COMMITMENT_FORMAT_VERSION, Commitment, CommitmentStatus, CompletionEvidence, Deadline,
    MAX_LINKS, MAX_NOTE_CHARS, MAX_PROMISE_CHARS, Owner, Provenance, WaitCondition,
};

const COMMITMENTS_DIR: &str = "commitments";

/// Why the store refused a change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitmentError {
    NotFound(String),
    /// The input does not describe a keepable commitment.
    Invalid(String),
    /// Completed, dismissed, and failed commitments are history.
    Closed {
        id: String,
        status: CommitmentStatus,
    },
    /// Completion needs explicit user confirmation or recorded evidence.
    EvidenceRequired,
    Io(String),
}

impl CommitmentError {
    /// Stable machine-readable code for API responses.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "not_found",
            Self::Invalid(_) => "invalid",
            Self::Closed { .. } => "closed",
            Self::EvidenceRequired => "evidence_required",
            Self::Io(_) => "storage_error",
        }
    }
}

impl std::fmt::Display for CommitmentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(id) => write!(f, "unknown commitment {id}"),
            Self::Invalid(message) => f.write_str(message),
            Self::Closed { id, status } => {
                write!(f, "commitment {id} is already {}", status.as_str())
            }
            Self::EvidenceRequired => f.write_str(
                "completing a commitment needs the user's confirmation or recorded evidence",
            ),
            Self::Io(message) => f.write_str(message),
        }
    }
}

impl From<io::Error> for CommitmentError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

/// What a caller supplies to create a commitment.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct NewCommitment {
    pub promise: String,
    #[serde(default)]
    pub owner: Owner,
    #[serde(default)]
    pub deadline: Option<Deadline>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub waiting_on: Option<WaitCondition>,
    /// Defaults to the earliest of the waiting `until` and the deadline start.
    #[serde(default)]
    pub next_check: Option<i64>,
    #[serde(default)]
    pub continuity_ids: Vec<String>,
    #[serde(default)]
    pub provenance: Provenance,
}

/// What a caller may change on an open commitment. Absent fields are kept;
/// the `clear_*` flags remove optional ones.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CommitmentPatch {
    #[serde(default)]
    pub promise: Option<String>,
    #[serde(default)]
    pub owner: Option<Owner>,
    #[serde(default)]
    pub deadline: Option<Deadline>,
    #[serde(default)]
    pub clear_deadline: bool,
    #[serde(default)]
    pub dependencies: Option<Vec<String>>,
    #[serde(default)]
    pub waiting_on: Option<WaitCondition>,
    #[serde(default)]
    pub clear_waiting_on: bool,
    #[serde(default)]
    pub next_check: Option<i64>,
    #[serde(default)]
    pub clear_next_check: bool,
    #[serde(default)]
    pub continuity_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListFilter {
    #[default]
    Open,
    Closed,
    All,
}

#[derive(Clone)]
pub struct CommitmentStore {
    workspace_dir: PathBuf,
    slug: String,
    /// One writer at a time: every read-modify-write holds this, so two
    /// writers can never interleave on one record (mirrors `ProactiveLoop.active`).
    writes: Arc<Mutex<()>>,
    /// Record updates for connected clients. None in tests that do not care.
    events: Option<tokio::sync::broadcast::Sender<crate::domain::events::ServerEvent>>,
}

impl CommitmentStore {
    pub fn new(workspace_dir: &Path, slug: &str) -> Self {
        Self {
            workspace_dir: workspace_dir.to_path_buf(),
            slug: slug.to_owned(),
            writes: Arc::new(Mutex::new(())),
            events: None,
        }
    }

    /// Held for the whole of a read-modify-write. A poisoned lock only means
    /// a writer panicked; the files are still consistent, so keep going.
    fn write_guard(&self) -> MutexGuard<'_, ()> {
        self.writes.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Broadcast every record change as `commitment_updated`.
    pub fn with_events(
        mut self,
        events: tokio::sync::broadcast::Sender<crate::domain::events::ServerEvent>,
    ) -> Self {
        self.events = Some(events);
        self
    }

    // ── lifecycle ──────────────────────────────────────────────────────────

    pub fn create(&self, new: NewCommitment, now: i64) -> Result<Commitment, CommitmentError> {
        let _writes = self.write_guard();
        let mut commitment = Commitment {
            version: COMMITMENT_FORMAT_VERSION,
            id: new_commitment_id(now),
            promise: new.promise,
            owner: new.owner,
            status: CommitmentStatus::Active,
            deadline: new.deadline,
            dependencies: new.dependencies,
            waiting_on: new.waiting_on,
            next_check: new.next_check,
            continuity_ids: new.continuity_ids,
            provenance: new.provenance,
            completion: None,
            snoozed_until: None,
            snooze_count: 0,
            last_check: None,
            created_at: now,
            updated_at: now,
            status_changed_at: now,
        };
        self.normalize(&mut commitment)?;
        if commitment.next_check.is_none() {
            commitment.next_check = commitment.default_next_check();
        }
        commitment.status = self.derived_status(&commitment, now);
        self.save(&commitment)?;
        Ok(commitment)
    }

    /// Edit an open commitment; its status is derived again from the result.
    /// Moving the deadline or the wait moves `next_check` with it unless the
    /// patch sets or clears `next_check` itself.
    pub fn update(
        &self,
        id: &str,
        patch: CommitmentPatch,
        now: i64,
    ) -> Result<Commitment, CommitmentError> {
        let _writes = self.write_guard();
        let mut commitment = self.open(id)?;
        if let Some(promise) = patch.promise {
            commitment.promise = promise;
        }
        if let Some(owner) = patch.owner {
            commitment.owner = owner;
        }
        let schedule_touched = patch.clear_deadline
            || patch.deadline.is_some()
            || patch.clear_waiting_on
            || patch.waiting_on.is_some();
        if patch.clear_deadline {
            commitment.deadline = None;
        } else if let Some(deadline) = patch.deadline {
            commitment.deadline = Some(deadline);
        }
        if let Some(dependencies) = patch.dependencies {
            commitment.dependencies = dependencies;
        }
        if patch.clear_waiting_on {
            commitment.waiting_on = None;
        } else if let Some(waiting_on) = patch.waiting_on {
            commitment.waiting_on = Some(waiting_on);
        }
        if patch.clear_next_check {
            commitment.next_check = None;
        } else if let Some(next_check) = patch.next_check {
            commitment.next_check = Some(next_check);
        } else if schedule_touched && let Some(next_check) = commitment.default_next_check() {
            commitment.next_check = Some(next_check);
        }
        if let Some(continuity_ids) = patch.continuity_ids {
            commitment.continuity_ids = continuity_ids;
        }
        self.normalize(&mut commitment)?;
        commitment.updated_at = now;
        self.refresh_status(&mut commitment, now);
        self.save(&commitment)?;
        Ok(commitment)
    }

    /// Hold an open commitment until `until`; a due one goes back to active
    /// and becomes due again when the snooze ends.
    pub fn snooze(&self, id: &str, until: i64, now: i64) -> Result<Commitment, CommitmentError> {
        let _writes = self.write_guard();
        let mut commitment = self.open(id)?;
        if until <= now {
            return Err(CommitmentError::Invalid(
                "a snooze must end in the future".into(),
            ));
        }
        commitment.snoozed_until = Some(until);
        commitment.snooze_count += 1;
        commitment.next_check = Some(until);
        commitment.updated_at = now;
        self.refresh_status(&mut commitment, now);
        self.save(&commitment)?;
        Ok(commitment)
    }

    /// Mark an open commitment completed. Refused without the user's
    /// confirmation or recorded evidence; unblocks commitments that depended
    /// on it. A `run_id` counts as evidence only when `run_exists` says the
    /// activity record is there: the store, not the caller, keeps a made-up
    /// run from completing anything, so every caller (the API now, the chat
    /// tool later) has to answer from the activity store.
    pub fn complete(
        &self,
        id: &str,
        mut evidence: CompletionEvidence,
        now: i64,
        run_exists: impl Fn(&str) -> bool,
    ) -> Result<Commitment, CommitmentError> {
        let _writes = self.write_guard();
        let mut commitment = self.open(id)?;
        evidence.summary = evidence.summary.as_deref().and_then(bounded_note);
        evidence.run_id = evidence.run_id.as_deref().and_then(bounded_note);
        if !evidence.is_sufficient() {
            return Err(CommitmentError::EvidenceRequired);
        }
        if let Some(run_id) = &evidence.run_id
            && !run_exists(run_id)
        {
            return Err(CommitmentError::Invalid(format!("unknown run {run_id}")));
        }
        evidence.at = now;
        commitment.completion = Some(evidence);
        commitment.status = CommitmentStatus::Completed;
        commitment.status_changed_at = now;
        commitment.updated_at = now;
        self.save(&commitment)?;
        self.settle_dependents(&commitment.id, now);
        Ok(commitment)
    }

    /// Dismiss an open commitment.
    pub fn cancel(&self, id: &str, now: i64) -> Result<Commitment, CommitmentError> {
        let _writes = self.write_guard();
        let mut commitment = self.open(id)?;
        commitment.status = CommitmentStatus::Dismissed;
        commitment.status_changed_at = now;
        commitment.updated_at = now;
        self.save(&commitment)?;
        Ok(commitment)
    }

    /// The persisted record, for a caller about to change it.
    fn open(&self, id: &str) -> Result<Commitment, CommitmentError> {
        let commitment = self
            .load(id)
            .ok_or_else(|| CommitmentError::NotFound(id.to_owned()))?;
        if !commitment.is_open() {
            return Err(CommitmentError::Closed {
                id: commitment.id,
                status: commitment.status,
            });
        }
        Ok(commitment)
    }

    /// Bound every field and refuse links that point nowhere.
    fn normalize(&self, commitment: &mut Commitment) -> Result<(), CommitmentError> {
        let promise: String = commitment
            .promise
            .trim()
            .chars()
            .take(MAX_PROMISE_CHARS)
            .collect();
        if promise.is_empty() {
            return Err(CommitmentError::Invalid("a promise cannot be empty".into()));
        }
        commitment.promise = promise;

        if let Some(Deadline::Window { start, end }) = commitment.deadline
            && end < start
        {
            return Err(CommitmentError::Invalid(
                "a deadline window cannot end before it starts".into(),
            ));
        }
        if let Some(WaitCondition::Event { event }) = &commitment.waiting_on {
            commitment.waiting_on = Some(WaitCondition::Event {
                event: bounded_note(event).ok_or_else(|| {
                    CommitmentError::Invalid("a waited-for event needs a name".into())
                })?,
            });
        }

        let dependencies = dedupe(std::mem::take(&mut commitment.dependencies));
        if dependencies.len() > MAX_LINKS {
            return Err(CommitmentError::Invalid(format!(
                "at most {MAX_LINKS} dependencies"
            )));
        }
        for dependency in &dependencies {
            if *dependency == commitment.id {
                return Err(CommitmentError::Invalid(
                    "a commitment cannot depend on itself".into(),
                ));
            }
            if self.load(dependency).is_none() {
                return Err(CommitmentError::Invalid(format!(
                    "unknown dependency {dependency}"
                )));
            }
        }
        commitment.dependencies = dependencies;

        let continuity_ids = dedupe(std::mem::take(&mut commitment.continuity_ids));
        if continuity_ids.len() > MAX_LINKS {
            return Err(CommitmentError::Invalid(format!(
                "at most {MAX_LINKS} linked continuity records"
            )));
        }
        commitment.continuity_ids = continuity_ids;
        Ok(())
    }

    fn derived_status(&self, commitment: &Commitment, now: i64) -> CommitmentStatus {
        commitment.derive_open_status(now, |dependency| self.unfinished(dependency))
    }

    /// A dependency counts as unfinished until it is completed; a dismissed
    /// or missing one never finishes. Reads the persisted record: open or
    /// closed never depends on the clock, and a dependency cycle must not
    /// turn one read into an endless chain of derivations.
    fn unfinished(&self, id: &str) -> bool {
        self.load(id)
            .is_none_or(|dependency| dependency.status != CommitmentStatus::Completed)
    }

    fn refresh_status(&self, commitment: &mut Commitment, now: i64) {
        let status = self.derived_status(commitment, now);
        if status != commitment.status {
            commitment.status = status;
            commitment.status_changed_at = now;
        }
    }

    /// Re-derive every open commitment that depended on `of`. Runs under the
    /// caller's write guard and compares against the persisted status, so a
    /// transition the clock already implied is written down too.
    fn settle_dependents(&self, of: &str, now: i64) {
        for mut dependent in self.load_all() {
            if !dependent.is_open() || !dependent.dependencies.iter().any(|id| id == of) {
                continue;
            }
            let before = dependent.status;
            self.refresh_status(&mut dependent, now);
            if dependent.status != before {
                dependent.updated_at = now;
                let _ = self.save(&dependent);
            }
        }
    }

    // ── queries ────────────────────────────────────────────────────────────

    /// One record as it reads at `now`: an open status is re-derived from the
    /// record's own fields, so a passed deadline, an ended timed wait, or an
    /// expired snooze shows without waiting for the next write. Nothing is
    /// persisted; `status_changed_at` stays the last written transition.
    pub fn get(&self, id: &str, now: i64) -> Option<Commitment> {
        self.load(id).map(|commitment| self.fresh(commitment, now))
    }

    /// Newest first, each record as it reads at `now` (see `get`).
    pub fn list(&self, filter: ListFilter, now: i64) -> Vec<Commitment> {
        self.load_all()
            .into_iter()
            .filter(|commitment| match filter {
                ListFilter::Open => commitment.is_open(),
                ListFilter::Closed => !commitment.is_open(),
                ListFilter::All => true,
            })
            .map(|commitment| self.fresh(commitment, now))
            .collect()
    }

    fn fresh(&self, mut commitment: Commitment, now: i64) -> Commitment {
        if commitment.is_open() {
            commitment.status = self.derived_status(&commitment, now);
        }
        commitment
    }

    /// The persisted record behind the path guard; None when it is missing.
    /// A record that no longer parses is reported, not hidden: it is a
    /// storage fault, and this is where it would otherwise vanish silently.
    fn load(&self, id: &str) -> Option<Commitment> {
        if id.contains('/') || id.contains('\\') || id.starts_with('.') {
            return None;
        }
        let path = self.record_path(id);
        let raw = fs::read_to_string(&path).ok()?;
        parse_record(&path, &raw)
    }

    /// Every persisted record, newest first.
    fn load_all(&self) -> Vec<Commitment> {
        let Ok(entries) = fs::read_dir(self.commitments_dir()) else {
            return Vec::new();
        };
        let mut commitments: Vec<Commitment> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension().and_then(|ext| ext.to_str()) == Some("json")
                    && !path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with('.'))
            })
            .filter_map(|path| {
                let raw = fs::read_to_string(&path).ok()?;
                parse_record(&path, &raw)
            })
            .collect();
        commitments.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        commitments
    }

    /// Open, unsnoozed commitments whose `next_check` has passed, oldest
    /// first. Reading never changes a record, so asking twice at the same
    /// instant answers the same records: the evaluator, not the store,
    /// decides what to do with them.
    #[allow(dead_code)] // Foundation for the commitment evaluator (#85, PR B).
    pub fn due_for_check(&self, now: i64) -> Vec<Commitment> {
        let mut due: Vec<Commitment> = self
            .list(ListFilter::Open, now)
            .into_iter()
            .filter(|commitment| commitment.needs_check(now))
            .collect();
        due.reverse();
        due
    }

    // ── storage ────────────────────────────────────────────────────────────

    fn instance_dir(&self) -> PathBuf {
        self.workspace_dir.join("instances").join(&self.slug)
    }

    fn commitments_dir(&self) -> PathBuf {
        self.instance_dir().join(COMMITMENTS_DIR)
    }

    fn record_path(&self, id: &str) -> PathBuf {
        self.commitments_dir().join(format!("{id}.json"))
    }

    fn save(&self, commitment: &Commitment) -> io::Result<()> {
        fs::create_dir_all(self.commitments_dir())?;
        write_atomic(
            &self.record_path(&commitment.id),
            &serde_json::to_string_pretty(commitment).map_err(io::Error::other)?,
        )?;
        if let Some(events) = &self.events {
            let _ = events.send(crate::domain::events::ServerEvent::CommitmentUpdated {
                instance_slug: self.slug.clone(),
                commitment: commitment.clone(),
            });
        }
        Ok(())
    }
}

/// Trimmed and bounded to `MAX_NOTE_CHARS`; None when nothing is left.
fn bounded_note(text: &str) -> Option<String> {
    let note: String = text.trim().chars().take(MAX_NOTE_CHARS).collect();
    (!note.is_empty()).then_some(note)
}

/// Trimmed, non-empty, first occurrence wins.
fn dedupe(items: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(items.len());
    for item in items {
        let item = item.trim();
        if !item.is_empty() && !out.iter().any(|seen| seen == item) {
            out.push(item.to_owned());
        }
    }
    out
}

fn new_commitment_id(now: i64) -> String {
    let suffix: String = uuid::Uuid::new_v4().simple().to_string()[..8].to_owned();
    format!("cmt_{now}_{suffix}")
}

fn parse_record(path: &Path, raw: &str) -> Option<Commitment> {
    match serde_json::from_str(raw) {
        Ok(commitment) => Some(commitment),
        Err(error) => {
            log::warn!(
                "commitments: skipping unparseable record {}: {error}",
                path.display()
            );
            None
        }
    }
}

/// Write to a temp file of its own in the same directory, then rename it
/// over `path`: a reader sees the old record or the new one, never a partial
/// one, and two writers can never share a temp file.
fn write_atomic(path: &Path, content: &str) -> io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::other("record path has no directory"))?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(content.as_bytes())?;
    tmp.persist(path).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::commitment::{CheckOutcome, Owner};
    use crate::domain::companion::CANONICAL_SLUG;

    /// 2026-01-05 (Monday) 09:00:00 UTC
    const T0: i64 = 1_767_603_600;

    fn harness() -> (tempfile::TempDir, CommitmentStore) {
        let ws = tempfile::tempdir().unwrap();
        fs::create_dir_all(ws.path().join("instances").join(CANONICAL_SLUG)).unwrap();
        let store = CommitmentStore::new(ws.path(), CANONICAL_SLUG);
        (ws, store)
    }

    fn promise(text: &str) -> NewCommitment {
        NewCommitment {
            promise: text.into(),
            ..Default::default()
        }
    }

    fn confirmed() -> CompletionEvidence {
        CompletionEvidence {
            confirmed_by_user: true,
            ..Default::default()
        }
    }

    #[test]
    fn every_commitment_has_one_versioned_bounded_record() {
        let (ws, store) = harness();
        let long_promise = "x".repeat(MAX_PROMISE_CHARS + 50);
        let created = store
            .create(
                NewCommitment {
                    promise: long_promise,
                    provenance: Provenance::Chat {
                        chat_id: "default".into(),
                        message_id: Some("m1".into()),
                    },
                    ..Default::default()
                },
                T0,
            )
            .unwrap();
        assert!(created.id.starts_with("cmt_"));
        assert_eq!(created.version, COMMITMENT_FORMAT_VERSION);
        assert_eq!(created.status, CommitmentStatus::Active);
        assert_eq!(created.owner, Owner::Companion);
        assert_eq!(created.promise.chars().count(), MAX_PROMISE_CHARS);
        assert_eq!(created.created_at, T0);
        assert_eq!(created.updated_at, T0);
        assert_eq!(created.status_changed_at, T0);
        assert_eq!(created.next_check, None, "nothing to wait for yet");
        assert_eq!(created.completion, None);
        assert_eq!(created.snooze_count, 0);

        let path = ws
            .path()
            .join("instances")
            .join(CANONICAL_SLUG)
            .join("commitments")
            .join(format!("{}.json", created.id));
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains("\"kind\": \"chat\""));
        for forbidden in ["raw", "thought", "monologue", "trace"] {
            assert!(
                !raw.contains(forbidden),
                "record leaks model text: {forbidden}"
            );
        }
        assert_eq!(
            fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1,
            "atomic write leaves no temp file"
        );
        assert_eq!(store.get(&created.id, T0).unwrap(), created);
        assert_eq!(store.list(ListFilter::Open, T0), vec![created]);
    }

    #[test]
    fn status_follows_deadline_dependencies_and_waiting_condition() {
        let (_ws, store) = harness();
        let later = store
            .create(
                NewCommitment {
                    deadline: Some(Deadline::At { at: T0 + 3600 }),
                    ..promise("send the draft")
                },
                T0,
            )
            .unwrap();
        assert_eq!(later.status, CommitmentStatus::Active);
        assert_eq!(
            later.next_check,
            Some(T0 + 3600),
            "the deadline is the default next check"
        );

        let blocked = store
            .create(
                NewCommitment {
                    dependencies: vec![later.id.clone(), later.id.clone()],
                    ..promise("ask for feedback on the draft")
                },
                T0,
            )
            .unwrap();
        assert_eq!(blocked.status, CommitmentStatus::Blocked);
        assert_eq!(blocked.dependencies, vec![later.id.clone()], "deduplicated");

        let waiting = store
            .create(
                NewCommitment {
                    waiting_on: Some(WaitCondition::Event {
                        event: "email_reply:thread-1".into(),
                    }),
                    ..promise("follow up once they answer")
                },
                T0,
            )
            .unwrap();
        assert_eq!(waiting.status, CommitmentStatus::Waiting);
        assert_eq!(waiting.next_check, None, "event waits have no clock");

        let timed_wait = store
            .create(
                NewCommitment {
                    waiting_on: Some(WaitCondition::Until { until: T0 + 600 }),
                    deadline: Some(Deadline::Window {
                        start: T0 + 900,
                        end: T0 + 1800,
                    }),
                    ..promise("check the build")
                },
                T0,
            )
            .unwrap();
        assert_eq!(timed_wait.status, CommitmentStatus::Waiting);
        assert_eq!(
            timed_wait.next_check,
            Some(T0 + 600),
            "earliest of the wait and the deadline"
        );
        assert_eq!(
            timed_wait.derive_open_status(T0 + 600, |_| false),
            CommitmentStatus::Active,
            "a timed wait settles itself"
        );
        assert_eq!(
            timed_wait.derive_open_status(T0 + 900, |_| false),
            CommitmentStatus::Due
        );

        let overdue = store
            .create(
                NewCommitment {
                    deadline: Some(Deadline::At { at: T0 - 1 }),
                    waiting_on: Some(WaitCondition::UserReply),
                    ..promise("already late")
                },
                T0,
            )
            .unwrap();
        assert_eq!(
            overdue.status,
            CommitmentStatus::Due,
            "a started deadline wins over waiting"
        );

        for (label, bad) in [
            ("empty promise", promise("   ")),
            (
                "window ends before it starts",
                NewCommitment {
                    deadline: Some(Deadline::Window {
                        start: T0 + 10,
                        end: T0,
                    }),
                    ..promise("x")
                },
            ),
            (
                "unknown dependency",
                NewCommitment {
                    dependencies: vec!["cmt_missing".into()],
                    ..promise("x")
                },
            ),
        ] {
            assert!(
                matches!(store.create(bad, T0), Err(CommitmentError::Invalid(_))),
                "{label} must be invalid"
            );
        }
        assert_eq!(
            store.list(ListFilter::Open, T0).len(),
            5,
            "rejected input is not stored"
        );
    }

    #[test]
    fn waiting_conditions_and_dependencies_survive_restart_without_duplicate_schedules() {
        let (ws, store) = harness();
        let waiting = store
            .create(
                NewCommitment {
                    waiting_on: Some(WaitCondition::Until { until: T0 + 600 }),
                    continuity_ids: vec!["cont_1".into(), "cont_1".into(), "cont_2".into()],
                    ..promise("reconnect and finish the export")
                },
                T0,
            )
            .unwrap();
        let blocked = store
            .create(
                NewCommitment {
                    dependencies: vec![waiting.id.clone()],
                    ..promise("tell them the export is ready")
                },
                T0 + 1,
            )
            .unwrap();
        assert_eq!(waiting.continuity_ids, vec!["cont_1", "cont_2"]);

        let fresh = CommitmentStore::new(ws.path(), CANONICAL_SLUG);
        assert_eq!(fresh.get(&waiting.id, T0).unwrap(), waiting);
        assert_eq!(fresh.get(&blocked.id, T0).unwrap(), blocked);
        assert_eq!(fresh.list(ListFilter::Open, T0).len(), 2);

        assert!(fresh.due_for_check(T0 + 599).is_empty(), "not yet");
        let first: Vec<String> = fresh
            .due_for_check(T0 + 600)
            .into_iter()
            .map(|c| c.id)
            .collect();
        let second: Vec<String> = fresh
            .due_for_check(T0 + 600)
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(first, vec![waiting.id.clone()]);
        assert_eq!(first, second, "asking twice does not schedule twice");
        assert_eq!(
            fresh.list(ListFilter::All, T0).len(),
            2,
            "no record was created by asking"
        );
        assert_eq!(
            fresh.get(&blocked.id, T0).unwrap().status,
            CommitmentStatus::Blocked,
            "the dependency is still open"
        );
    }

    #[test]
    fn completion_requires_user_confirmation_or_recorded_evidence() {
        let (_ws, store) = harness();
        let a = store.create(promise("water the plants"), T0).unwrap();
        for (label, empty) in [
            ("no evidence", CompletionEvidence::default()),
            (
                "blank summary",
                CompletionEvidence {
                    summary: Some("   ".into()),
                    ..Default::default()
                },
            ),
        ] {
            assert_eq!(
                store.complete(&a.id, empty, T0 + 5, |_| true),
                Err(CommitmentError::EvidenceRequired),
                "{label}"
            );
        }
        assert_eq!(
            store.get(&a.id, T0).unwrap(),
            a,
            "a refused completion changes nothing"
        );

        let done = store
            .complete(&a.id, confirmed(), T0 + 10, |_| true)
            .unwrap();
        assert_eq!(done.status, CommitmentStatus::Completed);
        assert_eq!(done.status_changed_at, T0 + 10);
        assert_eq!(done.updated_at, T0 + 10);
        let evidence = done.completion.clone().unwrap();
        assert!(evidence.confirmed_by_user);
        assert_eq!(evidence.at, T0 + 10, "the store stamps the evidence");
        assert_eq!(
            store.complete(&a.id, confirmed(), T0 + 11, |_| true),
            Err(CommitmentError::Closed {
                id: a.id.clone(),
                status: CommitmentStatus::Completed
            })
        );

        let b = store.create(promise("send the invoice"), T0).unwrap();
        let long_summary = "y".repeat(MAX_NOTE_CHARS + 20);
        let observed = store
            .complete(
                &b.id,
                CompletionEvidence {
                    summary: Some(long_summary),
                    ..Default::default()
                },
                T0 + 20,
                |_| true,
            )
            .unwrap();
        assert_eq!(
            observed
                .completion
                .as_ref()
                .unwrap()
                .summary
                .as_ref()
                .unwrap()
                .chars()
                .count(),
            MAX_NOTE_CHARS
        );

        let c = store.create(promise("summarise the thread"), T0).unwrap();
        let by_run = store
            .complete(
                &c.id,
                CompletionEvidence {
                    run_id: Some("run_1_abcdef01".into()),
                    ..Default::default()
                },
                T0 + 30,
                |_| true,
            )
            .unwrap();
        assert_eq!(
            by_run.completion.unwrap().run_id.as_deref(),
            Some("run_1_abcdef01")
        );
        assert_eq!(store.list(ListFilter::Open, T0).len(), 0);
        assert_eq!(store.list(ListFilter::Closed, T0).len(), 3);
    }

    #[test]
    fn completing_a_dependency_unblocks_dependents_but_dismissing_does_not() {
        let (_ws, store) = harness();
        let a = store.create(promise("finish the export"), T0).unwrap();
        let b = store.create(promise("archive the old one"), T0).unwrap();
        let c = store
            .create(
                NewCommitment {
                    dependencies: vec![a.id.clone(), b.id.clone()],
                    ..promise("announce it")
                },
                T0,
            )
            .unwrap();
        assert_eq!(c.status, CommitmentStatus::Blocked);

        store.cancel(&b.id, T0 + 5).unwrap();
        assert_eq!(
            store.get(&c.id, T0).unwrap().status,
            CommitmentStatus::Blocked,
            "a dismissed dependency never finished"
        );
        store
            .update(
                &c.id,
                CommitmentPatch {
                    dependencies: Some(vec![a.id.clone()]),
                    ..Default::default()
                },
                T0 + 6,
            )
            .unwrap();
        assert_eq!(
            store.get(&c.id, T0).unwrap().status,
            CommitmentStatus::Blocked
        );

        store
            .complete(&a.id, confirmed(), T0 + 10, |_| true)
            .unwrap();
        let unblocked = store.get(&c.id, T0).unwrap();
        assert_eq!(unblocked.status, CommitmentStatus::Active);
        assert_eq!(unblocked.status_changed_at, T0 + 10);
        assert_eq!(unblocked.updated_at, T0 + 10);
    }

    #[test]
    fn snooze_and_cancel_persist_and_only_open_commitments_change() {
        let (ws, store) = harness();
        let due = store
            .create(
                NewCommitment {
                    deadline: Some(Deadline::At { at: T0 - 60 }),
                    ..promise("call the dentist")
                },
                T0,
            )
            .unwrap();
        assert_eq!(due.status, CommitmentStatus::Due);
        assert!(due.needs_check(T0));

        assert!(matches!(
            store.snooze(&due.id, T0 - 1, T0),
            Err(CommitmentError::Invalid(_))
        ));
        let snoozed = store.snooze(&due.id, T0 + 7200, T0).unwrap();
        assert_eq!(
            snoozed.status,
            CommitmentStatus::Active,
            "deferred, not due"
        );
        assert_eq!(snoozed.snoozed_until, Some(T0 + 7200));
        assert_eq!(snoozed.snooze_count, 1);
        assert_eq!(snoozed.next_check, Some(T0 + 7200));
        assert!(!snoozed.needs_check(T0 + 10));
        assert!(snoozed.needs_check(T0 + 7200));
        assert_eq!(
            snoozed.derive_open_status(T0 + 7200, |_| false),
            CommitmentStatus::Due,
            "due again once the snooze ends"
        );
        assert!(store.due_for_check(T0 + 10).is_empty());
        assert_eq!(store.due_for_check(T0 + 7200).len(), 1);

        let dismissed = store.cancel(&due.id, T0 + 20).unwrap();
        assert_eq!(dismissed.status, CommitmentStatus::Dismissed);
        assert_eq!(dismissed.status_changed_at, T0 + 20);
        let closed = CommitmentError::Closed {
            id: due.id.clone(),
            status: CommitmentStatus::Dismissed,
        };
        assert_eq!(store.cancel(&due.id, T0 + 21), Err(closed.clone()));
        assert_eq!(
            store.snooze(&due.id, T0 + 9000, T0 + 21),
            Err(closed.clone())
        );
        assert_eq!(
            store.update(&due.id, CommitmentPatch::default(), T0 + 21),
            Err(closed)
        );
        assert!(store.due_for_check(T0 + 7200).is_empty());

        let fresh = CommitmentStore::new(ws.path(), CANONICAL_SLUG);
        assert_eq!(fresh.get(&due.id, T0).unwrap(), dismissed);
        assert_eq!(fresh.list(ListFilter::Open, T0).len(), 0);
        assert_eq!(fresh.list(ListFilter::Closed, T0).len(), 1);
    }

    #[test]
    fn edits_keep_the_record_the_source_of_truth() {
        let (_ws, store) = harness();
        let a = store
            .create(
                NewCommitment {
                    waiting_on: Some(WaitCondition::UserReply),
                    ..promise("pick a restaurant")
                },
                T0,
            )
            .unwrap();
        assert_eq!(a.status, CommitmentStatus::Waiting);

        let edited = store
            .update(
                &a.id,
                CommitmentPatch {
                    promise: Some("  book the restaurant  ".into()),
                    owner: Some(Owner::User),
                    clear_waiting_on: true,
                    continuity_ids: Some(vec!["cont_9".into()]),
                    ..Default::default()
                },
                T0 + 5,
            )
            .unwrap();
        assert_eq!(edited.promise, "book the restaurant");
        assert_eq!(edited.owner, Owner::User);
        assert_eq!(edited.waiting_on, None);
        assert_eq!(edited.status, CommitmentStatus::Active);
        assert_eq!(edited.status_changed_at, T0 + 5);
        assert_eq!(edited.continuity_ids, vec!["cont_9"]);
        assert_eq!(edited.created_at, T0);

        let due = store
            .update(
                &a.id,
                CommitmentPatch {
                    deadline: Some(Deadline::At { at: T0 + 2 }),
                    ..Default::default()
                },
                T0 + 6,
            )
            .unwrap();
        assert_eq!(due.status, CommitmentStatus::Due);
        assert_eq!(
            due.next_check,
            Some(T0 + 2),
            "a moved deadline moves the clock with it"
        );

        let cleared = store
            .update(
                &a.id,
                CommitmentPatch {
                    clear_deadline: true,
                    next_check: Some(T0 + 900),
                    ..Default::default()
                },
                T0 + 7,
            )
            .unwrap();
        assert_eq!(cleared.deadline, None);
        assert_eq!(cleared.status, CommitmentStatus::Active);
        assert_eq!(cleared.status_changed_at, T0 + 7);
        assert_eq!(cleared.next_check, Some(T0 + 900), "an explicit clock wins");

        let relinked = store
            .update(
                &a.id,
                CommitmentPatch {
                    continuity_ids: Some(vec!["cont_9".into(), "cont_10".into()]),
                    ..Default::default()
                },
                T0 + 8,
            )
            .unwrap();
        assert_eq!(relinked.updated_at, T0 + 8);
        assert_eq!(
            relinked.status_changed_at,
            T0 + 7,
            "an unchanged status keeps its timestamp"
        );
        assert_eq!(
            relinked.next_check,
            Some(T0 + 900),
            "no schedule was touched"
        );

        assert!(matches!(
            store.update(
                &a.id,
                CommitmentPatch {
                    dependencies: Some(vec![a.id.clone()]),
                    ..Default::default()
                },
                T0 + 9,
            ),
            Err(CommitmentError::Invalid(_))
        ));
        assert!(matches!(
            store.update(
                &a.id,
                CommitmentPatch {
                    promise: Some(String::new()),
                    ..Default::default()
                },
                T0 + 9,
            ),
            Err(CommitmentError::Invalid(_))
        ));
        assert_eq!(
            store.update("cmt_missing", CommitmentPatch::default(), T0 + 9),
            Err(CommitmentError::NotFound("cmt_missing".into()))
        );
        assert_eq!(
            store.get(&a.id, T0).unwrap(),
            relinked,
            "refused edits change nothing"
        );

        for hostile in ["../soul", "..\\soul", ".ledger", "a/b"] {
            assert!(
                store.get(hostile, T0).is_none(),
                "{hostile} must not be read"
            );
        }

        let b = store.create(promise("newer"), T0 + 100).unwrap();
        let ids: Vec<String> = store
            .list(ListFilter::All, T0)
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(ids, vec![b.id.clone(), a.id.clone()], "newest first");
    }

    #[test]
    fn concurrent_writers_never_corrupt_a_record_or_lose_a_completion() {
        use std::sync::{Arc, Barrier};

        let (ws, store) = harness();
        let dir = ws
            .path()
            .join("instances")
            .join(CANONICAL_SLUG)
            .join("commitments");
        // A long edit and a short completion racing on one record: without
        // one writer at a time and one temp file per write, the shorter body
        // keeps the longer one's tail, or the completion is overwritten.
        let filler = "z".repeat(MAX_PROMISE_CHARS);
        const ROUNDS: i64 = 48;
        for round in 0..ROUNDS {
            let created = store.create(promise("ship it"), T0 + round).unwrap();
            let id = created.id.clone();
            let start = Arc::new(Barrier::new(2));
            let completer = {
                let (store, id, start) = (store.clone(), id.clone(), start.clone());
                std::thread::spawn(move || {
                    start.wait();
                    store.complete(&id, confirmed(), T0 + 100, |_| true)
                })
            };
            let editor = {
                let (store, id, start, filler) =
                    (store.clone(), id.clone(), start.clone(), filler.clone());
                std::thread::spawn(move || {
                    start.wait();
                    store.update(
                        &id,
                        CommitmentPatch {
                            promise: Some(filler),
                            ..Default::default()
                        },
                        T0 + 100,
                    )
                })
            };
            let completed = completer.join().unwrap();
            let edited = editor.join().unwrap();

            let raw = fs::read_to_string(dir.join(format!("{id}.json"))).unwrap();
            let on_disk: Commitment = serde_json::from_str(&raw).unwrap_or_else(|error| {
                panic!("round {round}: record unparseable: {error}\n{raw}")
            });
            assert!(
                completed.is_ok(),
                "round {round}: an edit cannot close a commitment: {completed:?}"
            );
            assert_eq!(
                on_disk.status,
                CommitmentStatus::Completed,
                "round {round}: an acknowledged completion was reverted"
            );
            assert!(on_disk.completion.is_some(), "round {round}");
            match edited {
                Ok(_) => assert_eq!(on_disk.promise, filler, "round {round}: edit ran first"),
                Err(CommitmentError::Closed { .. }) => {
                    assert_eq!(on_disk.promise, "ship it", "round {round}: edit ran second");
                }
                Err(other) => panic!("round {round}: unexpected refusal {other:?}"),
            }
            assert_eq!(
                store.list(ListFilter::All, T0 + 100).len(),
                (round + 1) as usize,
                "round {round}: a record vanished from the list"
            );
            assert_eq!(
                fs::read_dir(&dir).unwrap().count(),
                (round + 1) as usize,
                "round {round}: a temp file was left behind"
            );
        }
    }

    #[test]
    fn unparseable_records_are_skipped_not_fatal() {
        let (ws, store) = harness();
        let good = store.create(promise("keep this"), T0).unwrap();
        let dir = ws
            .path()
            .join("instances")
            .join(CANONICAL_SLUG)
            .join("commitments");
        fs::write(
            dir.join("cmt_broken.json"),
            "{\"version\": 1, \"id\": \"cmt_bro",
        )
        .unwrap();
        assert_eq!(store.list(ListFilter::All, T0), vec![good]);
        assert_eq!(store.get("cmt_broken", T0), None);
    }

    #[test]
    fn run_evidence_must_name_a_recorded_run() {
        let (_ws, store) = harness();
        let a = store.create(promise("summarise the thread"), T0).unwrap();
        let dangling = CompletionEvidence {
            run_id: Some("run_does_not_exist".into()),
            ..Default::default()
        };
        assert_eq!(
            store.complete(&a.id, dangling, T0 + 1, |_| false),
            Err(CommitmentError::Invalid(
                "unknown run run_does_not_exist".into()
            )),
            "a run id is evidence only when the activity record exists"
        );
        assert!(
            matches!(
                store.complete(
                    &a.id,
                    CompletionEvidence {
                        confirmed_by_user: true,
                        run_id: Some("run_does_not_exist".into()),
                        ..Default::default()
                    },
                    T0 + 1,
                    |_| false,
                ),
                Err(CommitmentError::Invalid(_))
            ),
            "confirmation does not make a dangling run link valid"
        );
        assert_eq!(store.get(&a.id, T0 + 1).unwrap(), a, "nothing was written");
        assert_eq!(
            store.complete(&a.id, CompletionEvidence::default(), T0 + 1, |_| true),
            Err(CommitmentError::EvidenceRequired),
            "the lookup never replaces the evidence rule"
        );

        let by_run = store
            .complete(
                &a.id,
                CompletionEvidence {
                    run_id: Some("run_1_abcdef01".into()),
                    ..Default::default()
                },
                T0 + 2,
                |run| run == "run_1_abcdef01",
            )
            .unwrap();
        assert_eq!(by_run.status, CommitmentStatus::Completed);
        assert_eq!(
            by_run.completion.unwrap().run_id.as_deref(),
            Some("run_1_abcdef01")
        );
    }

    #[test]
    fn reads_report_the_status_the_clock_implies() {
        let (ws, store) = harness();
        let path = |id: &str| {
            ws.path()
                .join("instances")
                .join(CANONICAL_SLUG)
                .join("commitments")
                .join(format!("{id}.json"))
        };
        let later = store
            .create(
                NewCommitment {
                    deadline: Some(Deadline::At { at: T0 + 60 }),
                    ..promise("send the draft")
                },
                T0,
            )
            .unwrap();
        assert_eq!(later.status, CommitmentStatus::Active);
        assert_eq!(
            store.get(&later.id, T0 + 59).unwrap().status,
            CommitmentStatus::Active
        );
        let due = store.get(&later.id, T0 + 60).unwrap();
        assert_eq!(
            due.status,
            CommitmentStatus::Due,
            "a passed deadline shows on read"
        );
        assert_eq!(
            due.status_changed_at, T0,
            "reading persists nothing: the last written transition stands"
        );
        assert!(
            fs::read_to_string(path(&later.id))
                .unwrap()
                .contains("\"status\": \"active\""),
            "the file is untouched by reads"
        );
        assert_eq!(
            store.list(ListFilter::Open, T0 + 60)[0].status,
            CommitmentStatus::Due
        );

        let waiting = store
            .create(
                NewCommitment {
                    waiting_on: Some(WaitCondition::Until { until: T0 + 30 }),
                    ..promise("check the build")
                },
                T0,
            )
            .unwrap();
        assert_eq!(waiting.status, CommitmentStatus::Waiting);
        assert_eq!(
            store.get(&waiting.id, T0 + 30).unwrap().status,
            CommitmentStatus::Active,
            "an ended timed wait shows on read"
        );

        let snoozed = store.snooze(&later.id, T0 + 120, T0 + 61).unwrap();
        assert_eq!(snoozed.status, CommitmentStatus::Active);
        assert_eq!(
            store.get(&later.id, T0 + 119).unwrap().status,
            CommitmentStatus::Active
        );
        assert_eq!(
            store.get(&later.id, T0 + 120).unwrap().status,
            CommitmentStatus::Due,
            "an expired snooze shows on read"
        );

        let persisted = store
            .update(&later.id, CommitmentPatch::default(), T0 + 121)
            .unwrap();
        assert_eq!(persisted.status, CommitmentStatus::Due);
        assert_eq!(
            persisted.status_changed_at,
            T0 + 121,
            "the next write persists the transition"
        );

        store
            .complete(&later.id, confirmed(), T0 + 130, |_| true)
            .unwrap();
        assert_eq!(
            store.get(&later.id, T0 + 9_999).unwrap().status,
            CommitmentStatus::Completed,
            "closed records are history and never re-derived"
        );
    }

    #[test]
    fn every_record_change_is_broadcast_as_a_commitment_update() {
        let (ws, _) = harness();
        let (events, mut rx) = tokio::sync::broadcast::channel(16);
        let store = CommitmentStore::new(ws.path(), CANONICAL_SLUG).with_events(events);
        let a = store.create(promise("send the photos"), T0).unwrap();
        store.snooze(&a.id, T0 + 600, T0 + 1).unwrap();
        assert!(
            store
                .complete(&a.id, CompletionEvidence::default(), T0 + 2, |_| true)
                .is_err()
        );
        store
            .complete(&a.id, confirmed(), T0 + 3, |_| true)
            .unwrap();

        let mut statuses = Vec::new();
        while let Ok(event) = rx.try_recv() {
            let crate::domain::events::ServerEvent::CommitmentUpdated {
                instance_slug,
                commitment,
            } = event
            else {
                panic!("unexpected event");
            };
            assert_eq!(instance_slug, CANONICAL_SLUG);
            statuses.push(commitment.status);
        }
        assert_eq!(
            statuses,
            vec![
                CommitmentStatus::Active,
                CommitmentStatus::Active,
                CommitmentStatus::Completed
            ],
            "one event per write; refused writes send nothing"
        );
        let json = serde_json::to_string(&crate::domain::events::ServerEvent::CommitmentUpdated {
            instance_slug: CANONICAL_SLUG.into(),
            commitment: store.get(&a.id, T0).unwrap(),
        })
        .unwrap();
        assert!(json.contains("\"type\":\"commitment_updated\""));
        assert!(json.contains("\"status\":\"completed\""));

        // The evaluator's check record round-trips as part of the same file.
        let mut with_check = store.get(&a.id, T0).unwrap();
        with_check.last_check = Some(crate::domain::commitment::Check {
            at: T0 + 4,
            outcome: CheckOutcome::Failed {
                error: "provider offline".into(),
                retryable: true,
            },
            run_id: Some("run_1_abcdef01".into()),
        });
        let json = serde_json::to_string(&with_check).unwrap();
        let back: Commitment = serde_json::from_str(&json).unwrap();
        assert_eq!(back, with_check);
    }
}
