//! The commitments store (#85).
//!
//! One JSON file per commitment under `instances/{slug}/commitments/`. The
//! record is the source of truth: its `next_check`, waiting condition,
//! dependencies, and snooze survive a restart as plain fields, so the
//! evaluator never has to keep a schedule of its own and a restart cannot
//! create a duplicate one. Every write goes through `write_atomic`; every
//! read of one record goes through the `get` path guard. Callers pass `now`
//! so tests run against a fixed clock.

use std::{
    fs, io,
    path::{Path, PathBuf},
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
                write!(f, "commitment {id} is already {}", status_word(*status))
            }
            Self::EvidenceRequired => f.write_str(
                "completing a commitment needs the user's confirmation or recorded evidence",
            ),
            Self::Io(message) => f.write_str(message),
        }
    }
}

fn status_word(status: CommitmentStatus) -> &'static str {
    match status {
        CommitmentStatus::Active => "active",
        CommitmentStatus::Waiting => "waiting",
        CommitmentStatus::Blocked => "blocked",
        CommitmentStatus::Due => "due",
        CommitmentStatus::Completed => "completed",
        CommitmentStatus::Dismissed => "dismissed",
        CommitmentStatus::Failed => "failed",
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
    /// Record updates for connected clients. None in tests that do not care.
    events: Option<tokio::sync::broadcast::Sender<crate::domain::events::ServerEvent>>,
}

impl CommitmentStore {
    pub fn new(workspace_dir: &Path, slug: &str) -> Self {
        Self {
            workspace_dir: workspace_dir.to_path_buf(),
            slug: slug.to_owned(),
            events: None,
        }
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
        let _ = (new, now);
        todo!("#85: create")
    }

    /// Edit an open commitment; its status is derived again from the result.
    pub fn update(
        &self,
        id: &str,
        patch: CommitmentPatch,
        now: i64,
    ) -> Result<Commitment, CommitmentError> {
        let _ = (id, patch, now);
        todo!("#85: update")
    }

    /// Hold an open commitment until `until`; a due one goes back to active
    /// and becomes due again when the snooze ends.
    pub fn snooze(&self, id: &str, until: i64, now: i64) -> Result<Commitment, CommitmentError> {
        let _ = (id, until, now);
        todo!("#85: snooze")
    }

    /// Mark an open commitment completed. Refused without the user's
    /// confirmation or recorded evidence; unblocks commitments that depended on it.
    pub fn complete(
        &self,
        id: &str,
        evidence: CompletionEvidence,
        now: i64,
    ) -> Result<Commitment, CommitmentError> {
        let _ = (id, evidence, now);
        todo!("#85: complete")
    }

    /// Dismiss an open commitment.
    pub fn cancel(&self, id: &str, now: i64) -> Result<Commitment, CommitmentError> {
        let _ = (id, now);
        todo!("#85: cancel")
    }

    // ── queries ────────────────────────────────────────────────────────────

    pub fn get(&self, id: &str) -> Option<Commitment> {
        let _ = id;
        todo!("#85: get")
    }

    /// Newest first.
    pub fn list(&self, filter: ListFilter) -> Vec<Commitment> {
        let _ = filter;
        todo!("#85: list")
    }

    /// Open, unsnoozed commitments whose `next_check` has passed or whose
    /// deadline has started. Reading never changes a record, so asking twice
    /// at the same instant answers the same records: the evaluator, not the
    /// store, decides what to do with them.
    pub fn due_for_check(&self, now: i64) -> Vec<Commitment> {
        let _ = now;
        todo!("#85: due_for_check")
    }
}

#[allow(dead_code)]
fn write_atomic(path: &Path, content: &str) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, content)?;
    fs::rename(&tmp, path)
}

#[allow(dead_code)]
const _: (u32, usize, usize, usize) = (
    COMMITMENT_FORMAT_VERSION,
    MAX_PROMISE_CHARS,
    MAX_NOTE_CHARS,
    MAX_LINKS,
);

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
        assert!(
            !path.with_extension("tmp").exists(),
            "atomic write leaves no temp file"
        );
        assert_eq!(store.get(&created.id).unwrap(), created);
        assert_eq!(store.list(ListFilter::Open), vec![created]);
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
            store.list(ListFilter::Open).len(),
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
        assert_eq!(fresh.get(&waiting.id).unwrap(), waiting);
        assert_eq!(fresh.get(&blocked.id).unwrap(), blocked);
        assert_eq!(fresh.list(ListFilter::Open).len(), 2);

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
            fresh.list(ListFilter::All).len(),
            2,
            "no record was created by asking"
        );
        assert_eq!(
            fresh.get(&blocked.id).unwrap().status,
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
                store.complete(&a.id, empty, T0 + 5),
                Err(CommitmentError::EvidenceRequired),
                "{label}"
            );
        }
        assert_eq!(
            store.get(&a.id).unwrap(),
            a,
            "a refused completion changes nothing"
        );

        let done = store.complete(&a.id, confirmed(), T0 + 10).unwrap();
        assert_eq!(done.status, CommitmentStatus::Completed);
        assert_eq!(done.status_changed_at, T0 + 10);
        assert_eq!(done.updated_at, T0 + 10);
        let evidence = done.completion.clone().unwrap();
        assert!(evidence.confirmed_by_user);
        assert_eq!(evidence.at, T0 + 10, "the store stamps the evidence");
        assert_eq!(
            store.complete(&a.id, confirmed(), T0 + 11),
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
            )
            .unwrap();
        assert_eq!(
            by_run.completion.unwrap().run_id.as_deref(),
            Some("run_1_abcdef01")
        );
        assert_eq!(store.list(ListFilter::Open).len(), 0);
        assert_eq!(store.list(ListFilter::Closed).len(), 3);
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
            store.get(&c.id).unwrap().status,
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
        assert_eq!(store.get(&c.id).unwrap().status, CommitmentStatus::Blocked);

        store.complete(&a.id, confirmed(), T0 + 10).unwrap();
        let unblocked = store.get(&c.id).unwrap();
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
        assert_eq!(fresh.get(&due.id).unwrap(), dismissed);
        assert_eq!(fresh.list(ListFilter::Open).len(), 0);
        assert_eq!(fresh.list(ListFilter::Closed).len(), 1);
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
            due.next_check, None,
            "an explicit edit does not invent a clock"
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
        assert_eq!(cleared.next_check, Some(T0 + 900));
        assert_eq!(
            cleared.status_changed_at,
            T0 + 6,
            "unchanged status keeps its timestamp"
        );

        assert!(matches!(
            store.update(
                &a.id,
                CommitmentPatch {
                    dependencies: Some(vec![a.id.clone()]),
                    ..Default::default()
                },
                T0 + 8,
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
                T0 + 8,
            ),
            Err(CommitmentError::Invalid(_))
        ));
        assert_eq!(
            store.update("cmt_missing", CommitmentPatch::default(), T0 + 8),
            Err(CommitmentError::NotFound("cmt_missing".into()))
        );
        assert_eq!(
            store.get(&a.id).unwrap(),
            cleared,
            "refused edits change nothing"
        );

        for hostile in ["../soul", "..\\soul", ".ledger", "a/b"] {
            assert!(store.get(hostile).is_none(), "{hostile} must not be read");
        }

        let b = store.create(promise("newer"), T0 + 100).unwrap();
        let ids: Vec<String> = store
            .list(ListFilter::All)
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(ids, vec![b.id.clone(), a.id.clone()], "newest first");
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
                .complete(&a.id, CompletionEvidence::default(), T0 + 2)
                .is_err()
        );
        store.complete(&a.id, confirmed(), T0 + 3).unwrap();

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
            commitment: store.get(&a.id).unwrap(),
        })
        .unwrap();
        assert!(json.contains("\"type\":\"commitment_updated\""));
        assert!(json.contains("\"status\":\"completed\""));

        // The evaluator's check record round-trips as part of the same file.
        let mut with_check = store.get(&a.id).unwrap();
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
