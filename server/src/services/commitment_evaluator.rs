//! The commitment evaluator (#85).
//!
//! Every 30 seconds the scheduler tick asks the store which commitments are
//! due for a check and offers each one to the proactive loop under
//! `Trigger::Commitment`; the loop's dedupe key is the duplicate
//! suppression, and the record's `next_check` is the only schedule, so a
//! restart or a repeated tick can never create a second one. An admitted
//! check runs the companion check-in with a task that states the commitment
//! and the exact trigger condition (what changed and why now); `reach_out`
//! inside it still asks `approve_side_effect`. Checks are held, not spent,
//! while initiative is off, during quiet hours, or once the attention budget
//! is used up. What each check concluded is written back on the record, so
//! a failed evaluation stays inspectable and is looked at again after a
//! backoff. Callers pass `now` so tests run against a fixed clock.

use std::path::Path;

use chrono::{TimeZone, Utc};

use crate::app::state::AppState;
use crate::domain::commitment::{Check, CheckOutcome, Commitment, Owner, WaitCondition};
use crate::domain::proactive::{ProactiveRun, RunOutcome, SideEffect, Target, Trigger};
use crate::services::commitments::CommitmentStore;
use crate::services::companion_routine::{self, Routine};
use crate::services::proactive::{Admission, ProactiveLoop, RunHandle, outcome_from_trace};
use crate::services::{companion, llm::LlmBackend};

/// Seconds before a failed or held check is looked at again.
pub const RETRY_BACKOFF_SECS: i64 = 600;

/// The event name observed for a completed dependency.
pub fn dependency_completed_event(commitment_id: &str) -> String {
    format!("commitment_completed:{commitment_id}")
}

/// The event name observed when a connected computer comes online.
pub fn machine_connected_event(machine_id: &str) -> String {
    format!("machine_connected:{machine_id}")
}

/// Why a commitment is being looked at now: the exact trigger condition,
/// stated on the run's reason and in the check-in's task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerCondition {
    /// A pending observation asked for this check: an event it waited for
    /// or the completion of a dependency.
    Observed { event: String },
    /// A snooze the user asked for ended.
    SnoozeEnded { until: i64 },
    /// The deadline (or the start of its window) arrived.
    DeadlineArrived { at: i64 },
    /// A timed wait ended.
    WaitEnded { until: i64 },
    /// The record's own `next_check` came up and nothing else changed.
    ScheduledCheck { at: i64 },
}

impl TriggerCondition {
    /// Derived from the record as persisted and the clock, most recent
    /// change first: a pending observation, then a snooze that ended since
    /// the last check, then a started deadline, then an ended timed wait.
    pub fn for_commitment(commitment: &Commitment, now: i64) -> Self {
        if let Some(Check {
            outcome: CheckOutcome::Observed { event },
            ..
        }) = &commitment.last_check
        {
            return Self::Observed {
                event: event.clone(),
            };
        }
        let last_check_at = commitment.last_check.as_ref().map(|check| check.at);
        if let Some(until) = commitment.snoozed_until
            && until <= now
            && last_check_at.is_none_or(|at| at < until)
        {
            return Self::SnoozeEnded { until };
        }
        if let Some(deadline) = commitment.deadline
            && deadline.has_started(now)
        {
            return Self::DeadlineArrived {
                at: deadline.starts_at(),
            };
        }
        if let Some(WaitCondition::Until { until }) = commitment.waiting_on
            && until <= now
        {
            return Self::WaitEnded { until };
        }
        Self::ScheduledCheck {
            at: commitment.next_check.unwrap_or(now),
        }
    }

    /// Short label for the run's stated reason.
    pub fn summary(&self) -> &'static str {
        match self {
            Self::Observed { event } if event.starts_with("commitment_completed:") => {
                "dependency completed"
            }
            Self::Observed { .. } => "event observed",
            Self::SnoozeEnded { .. } => "snooze ended",
            Self::DeadlineArrived { .. } => "deadline arrived",
            Self::WaitEnded { .. } => "wait ended",
            Self::ScheduledCheck { .. } => "scheduled check",
        }
    }

    /// One sentence for the check-in, with times in the companion's timezone.
    pub fn describe(&self, tz: chrono_tz::Tz) -> String {
        match self {
            Self::Observed { event } => match event.strip_prefix("commitment_completed:") {
                Some(id) => format!("the commitment it depended on ({id}) was completed"),
                None => format!("the event it waited for was observed: {event}"),
            },
            Self::SnoozeEnded { until } => {
                format!("the snooze until {} ended", format_at(*until, tz))
            }
            Self::DeadlineArrived { at } => {
                format!("its deadline arrived at {}", format_at(*at, tz))
            }
            Self::WaitEnded { until } => {
                format!("the wait until {} ended", format_at(*until, tz))
            }
            Self::ScheduledCheck { at } => format!(
                "the check scheduled for {} came up; nothing else changed",
                format_at(*at, tz)
            ),
        }
    }
}

/// One admitted check: the record as it read at admission, the condition,
/// and the loop's handle. Exactly one of `finish` or `cancel` ends it.
pub struct CommitmentRun {
    pub commitment: Commitment,
    pub condition: TriggerCondition,
    handle: RunHandle,
}

impl CommitmentRun {
    pub fn id(&self) -> &str {
        self.handle.id()
    }
}

#[derive(Clone)]
pub struct CommitmentEvaluator {
    store: CommitmentStore,
    proactive: ProactiveLoop,
}

impl CommitmentEvaluator {
    pub fn new(store: CommitmentStore, proactive: ProactiveLoop) -> Self {
        Self { store, proactive }
    }

    /// One tick: every commitment due for a check is offered to the loop
    /// once. Admission never moves `next_check`; the loop's dedupe key keeps
    /// a second tick from starting a second run, and `finish` moves the
    /// schedule when the check is over. Nothing is offered while initiative
    /// is off, during quiet hours, or with the attention budget spent: the
    /// records stay as they are and are picked up when contact is possible.
    pub fn admit_due(&self, now: i64) -> Vec<CommitmentRun> {
        let due = self.store.due_for_check(now);
        if due.is_empty() {
            return Vec::new();
        }
        if !self.proactive.policy().enabled {
            log::debug!("[commitments] {} due, held: initiative is off", due.len());
            return Vec::new();
        }
        if let Err(denied) = self.proactive.reach_out_allowed(now) {
            log::debug!("[commitments] {} due, held: {denied}", due.len());
            return Vec::new();
        }
        let mut admitted = Vec::new();
        for commitment in due {
            let condition = TriggerCondition::for_commitment(&commitment, now);
            let reason = format!("{}: {}", condition.summary(), commitment.promise);
            match self.proactive.begin_at(
                Trigger::Commitment {
                    commitment_id: commitment.id.clone(),
                },
                &reason,
                Target::Companion,
                now,
            ) {
                Admission::Admitted(handle) => admitted.push(CommitmentRun {
                    commitment,
                    condition,
                    handle,
                }),
                Admission::Skipped(run) => {
                    log::info!("[commitments] {} skipped ({:?})", commitment.id, run.status);
                }
            }
        }
        admitted
    }

    /// The check-in's task: the commitment, what changed, and why now.
    pub fn check_in_task(&self, run: &CommitmentRun, tz: chrono_tz::Tz, now: i64) -> String {
        let commitment = &run.commitment;
        let mut task = format!(
            "current time: {}\ntriggered by: commitment {}\n\n",
            format_at(now, tz),
            commitment.id
        );
        task.push_str(
            "you are following through on a commitment you track. this is a check, \
             not a conversation: decide whether the user needs to hear from you \
             about it right now.\n\n",
        );
        task.push_str("## the commitment\n");
        task.push_str(&format!("- promise: {}\n", commitment.promise));
        task.push_str(match commitment.owner {
            Owner::Companion => "- owner: you promised the user this\n",
            Owner::User => "- owner: the user asked you to hold them to this\n",
        });
        task.push_str(&format!("- status: {}\n", commitment.status.as_str()));
        if let Some(deadline) = commitment.deadline {
            task.push_str(&format!(
                "- deadline: {}\n",
                describe_deadline(deadline, tz)
            ));
        }
        if let Some(waiting_on) = &commitment.waiting_on {
            task.push_str(&format!(
                "- waiting on: {}\n",
                describe_wait(waiting_on, tz)
            ));
        }
        if !commitment.dependencies.is_empty() {
            let open = commitment
                .dependencies
                .iter()
                .filter(|id| {
                    self.store
                        .get(id, now)
                        .is_none_or(|dependency| dependency.is_open())
                })
                .count();
            task.push_str(&format!(
                "- depends on {} other commitment(s), {open} still open\n",
                commitment.dependencies.len()
            ));
        }
        if commitment.snooze_count > 0 {
            task.push_str(&format!(
                "- snoozed {} time{}\n",
                commitment.snooze_count,
                if commitment.snooze_count == 1 {
                    ""
                } else {
                    "s"
                }
            ));
        }
        task.push_str("\n## what changed and why now\n");
        let mut condition = run.condition.describe(tz);
        if let TriggerCondition::Observed { event } = &run.condition
            && let Some(id) = event.strip_prefix("commitment_completed:")
            && let Some(dependency) = self.store.get(id, now)
        {
            condition.push_str(&format!(": \"{}\"", dependency.promise));
        }
        task.push_str(&condition);
        task.push_str(".\n\n");
        task.push_str(
            "## how to respond\n\
             - if the user should hear from you, call reach_out once: say which \
             commitment this is about, what changed, and why you are bringing it up \
             now. keep it short.\n\
             - reach_out may be declined (quiet hours, or today's budget is spent); \
             accept that, it will come up again.\n\
             - never claim the commitment is done and never close it: only the user \
             can complete, snooze, or cancel it, in a conversation.\n\
             - if there is nothing the user needs right now, do nothing.\n",
        );
        task
    }

    /// End an admitted check and write what it concluded on the record. A
    /// failure is recorded `failed` (retryable) on both the run and the
    /// commitment and looked at again after `RETRY_BACKOFF_SECS`; a run whose
    /// reach-out was denied is looked at again the same way. An observation
    /// that arrived while the check ran is left in place: it asks for a
    /// check of its own.
    pub fn finish(
        &self,
        run: CommitmentRun,
        result: Result<RunOutcome, String>,
        now: i64,
    ) -> ProactiveRun {
        let (finished, outcome, retry_at) = match result {
            Ok(outcome) => {
                let finished = run.handle.complete_at(outcome, now);
                let denied = finished.approvals.iter().any(|approval| {
                    approval.side_effect == SideEffect::ReachOut && !approval.allowed
                });
                let retry_at = denied.then_some(now + RETRY_BACKOFF_SECS);
                (finished, CheckOutcome::Triggered, retry_at)
            }
            Err(error) => {
                let finished = run.handle.fail_at(&error, true, now);
                let outcome = CheckOutcome::Failed {
                    error: error.chars().take(200).collect(),
                    retryable: true,
                };
                (finished, outcome, Some(now + RETRY_BACKOFF_SECS))
            }
        };
        self.record(&run.commitment, &finished.id, outcome, retry_at, now);
        finished
    }

    /// The user cancelled the run: nothing changed for the commitment.
    pub fn cancel(&self, run: CommitmentRun, now: i64) -> ProactiveRun {
        let finished = run.handle.cancel_at(now);
        self.record(
            &run.commitment,
            &finished.id,
            CheckOutcome::Unchanged,
            None,
            now,
        );
        finished
    }

    fn record(
        &self,
        seen: &Commitment,
        run_id: &str,
        outcome: CheckOutcome,
        retry_at: Option<i64>,
        now: i64,
    ) {
        let fresh_observation = self.store.get(&seen.id, now).is_some_and(|current| {
            matches!(
                &current.last_check,
                Some(Check {
                    outcome: CheckOutcome::Observed { .. },
                    ..
                })
            ) && current.last_check != seen.last_check
        });
        if fresh_observation {
            log::info!(
                "[commitments] {}: an observation arrived during the check; it asks for its own",
                seen.id
            );
            return;
        }
        let check = Check {
            at: now,
            outcome,
            run_id: Some(run_id.to_owned()),
        };
        match self.store.record_check(&seen.id, check, retry_at, now) {
            Ok(_) => {}
            Err(error) => log::info!("[commitments] {}: check not recorded: {error}", seen.id),
        }
    }
}

/// One scheduler tick under the canonical companion: admit every due
/// commitment and run its check-in in the background. Held until the
/// companion is onboarded and a background model is configured.
pub(crate) async fn tick(state: &AppState, now: i64) {
    let instance_dir = companion::companion_dir(&state.workspace_dir);
    if !instance_dir.join("soul.md").exists() {
        return;
    }
    let llm = match state.background_llm.read().await.as_ref() {
        Some(llm) => llm.clone(),
        None => {
            log::debug!("[commitments] held: background model preset not configured");
            return;
        }
    };
    let evaluator = CommitmentEvaluator::new(state.commitments.clone(), state.proactive.clone());
    for run in evaluator.admit_due(now) {
        let evaluator = evaluator.clone();
        let instance_dir = instance_dir.clone();
        let ws = state.workspace_dir.clone();
        let events = state.events.clone();
        let vector_store = state.vector_store.clone();
        let resources = state.resources.clone();
        let proactive = state.proactive.clone();
        let llm = llm.clone();
        tokio::spawn(async move {
            run_check_in(
                evaluator,
                run,
                &ws,
                &instance_dir,
                &llm,
                &events,
                &vector_store,
                &resources,
                &proactive,
                now,
            )
            .await;
        });
    }
}

/// One admitted check: the companion check-in with the commitment as its task.
#[allow(clippy::too_many_arguments)]
async fn run_check_in(
    evaluator: CommitmentEvaluator,
    run: CommitmentRun,
    workspace_dir: &Path,
    instance_dir: &Path,
    llm: &LlmBackend,
    events: &tokio::sync::broadcast::Sender<crate::domain::events::ServerEvent>,
    vector_store: &std::sync::Arc<crate::services::vector::VectorStore>,
    resources: &crate::services::resource_access::ResourceAccess,
    proactive: &ProactiveLoop,
    now: i64,
) {
    let slug = crate::domain::companion::CANONICAL_SLUG;
    let task = evaluator.check_in_task(&run, instance_timezone(instance_dir), now);
    let run_id = run.id().to_owned();
    let cancelled = run.handle.token();
    log::info!(
        "[commitments] checking {} ({run_id}): {}",
        run.commitment.id,
        run.condition.summary()
    );
    let work = companion_routine::run(
        workspace_dir,
        slug,
        instance_dir,
        llm,
        events,
        vector_store,
        resources,
        Routine::CheckIn,
        Some(&task),
        "commitment",
        (proactive, run_id.as_str()),
    );
    let result = tokio::select! {
        result = work => result,
        _ = cancelled.cancelled() => {
            log::info!("[commitments] {run_id}: cancelled");
            evaluator.cancel(run, Utc::now().timestamp());
            return;
        }
    };
    let finished_at = Utc::now().timestamp();
    match result {
        Ok(r) => {
            log::info!("[commitments] {run_id}: done ({} tokens)", r.tokens);
            evaluator.finish(run, Ok(outcome_from_trace(&r.trace, r.tokens)), finished_at);
        }
        Err(e) => {
            log::warn!("[commitments] {run_id}: failed: {e}");
            evaluator.finish(run, Err(e.to_string()), finished_at);
        }
    }
}

fn describe_deadline(deadline: crate::domain::commitment::Deadline, tz: chrono_tz::Tz) -> String {
    use crate::domain::commitment::Deadline;
    match deadline {
        Deadline::At { at } => format_at(at, tz),
        Deadline::Window { start, end } => {
            format!(
                "between {} and {}",
                format_at(start, tz),
                format_at(end, tz)
            )
        }
    }
}

fn describe_wait(waiting_on: &WaitCondition, tz: chrono_tz::Tz) -> String {
    match waiting_on {
        WaitCondition::Until { until } => format!("the clock reaching {}", format_at(*until, tz)),
        WaitCondition::Event { event } => format!("the event {event}"),
        WaitCondition::UserReply => "the user's reply".into(),
    }
}

/// The companion's timezone for stated times, UTC when none is set.
pub fn instance_timezone(instance_dir: &Path) -> chrono_tz::Tz {
    crate::routes::instances::read_timezone(instance_dir)
        .and_then(|value| value.parse().ok())
        .unwrap_or(chrono_tz::UTC)
}

fn format_at(at: i64, tz: chrono_tz::Tz) -> String {
    Utc.timestamp_opt(at, 0)
        .single()
        .map(|utc| {
            utc.with_timezone(&tz)
                .format("%A, %B %-d, %Y %H:%M %Z")
                .to_string()
        })
        .unwrap_or_else(|| at.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::commitment::{CommitmentStatus, CompletionEvidence, Deadline};
    use crate::domain::companion::CANONICAL_SLUG;
    use crate::domain::proactive::{ProactivePolicy, QuietHours, RunStatus, SkipReason};
    use crate::services::commitments::NewCommitment;
    use crate::services::proactive::Denied;
    use std::fs;

    /// 2026-01-05 (Monday) 09:00:00 UTC
    const T0: i64 = 1_767_603_600;

    struct Harness {
        ws: std::sync::Arc<tempfile::TempDir>,
        store: CommitmentStore,
        proactive: ProactiveLoop,
        evaluator: CommitmentEvaluator,
    }

    impl Harness {
        fn new() -> Self {
            let ws = tempfile::tempdir().unwrap();
            fs::create_dir_all(ws.path().join("instances").join(CANONICAL_SLUG)).unwrap();
            Self::over(std::sync::Arc::new(ws))
        }

        fn over(ws: std::sync::Arc<tempfile::TempDir>) -> Self {
            let store = CommitmentStore::new(ws.path(), CANONICAL_SLUG);
            let proactive = ProactiveLoop::new(ws.path(), CANONICAL_SLUG);
            let evaluator = CommitmentEvaluator::new(store.clone(), proactive.clone());
            Self {
                ws,
                store,
                proactive,
                evaluator,
            }
        }

        /// A new process over the same files: in-memory state is gone.
        fn restart(&self) -> Self {
            Self::over(self.ws.clone())
        }

        fn instance_dir(&self) -> std::path::PathBuf {
            self.ws.path().join("instances").join(CANONICAL_SLUG)
        }

        fn commitment_runs(&self) -> Vec<ProactiveRun> {
            self.proactive
                .list(usize::MAX)
                .into_iter()
                .filter(|run| matches!(run.trigger, Trigger::Commitment { .. }))
                .collect()
        }
    }

    fn promise(text: &str) -> NewCommitment {
        NewCommitment {
            promise: text.into(),
            ..Default::default()
        }
    }

    fn deadline_at(text: &str, at: i64) -> NewCommitment {
        NewCommitment {
            deadline: Some(Deadline::At { at }),
            ..promise(text)
        }
    }

    fn confirmed() -> CompletionEvidence {
        CompletionEvidence {
            confirmed_by_user: true,
            ..Default::default()
        }
    }

    fn one(mut runs: Vec<CommitmentRun>) -> CommitmentRun {
        assert_eq!(runs.len(), 1, "exactly one run expected");
        runs.pop().unwrap()
    }

    #[test]
    fn two_ticks_at_the_same_instant_admit_exactly_one_run() {
        let h = Harness::new();
        let draft = h
            .store
            .create(deadline_at("send the draft", T0 + 60), T0)
            .unwrap();
        assert!(h.evaluator.admit_due(T0).is_empty(), "not due yet");
        assert!(h.evaluator.admit_due(T0 + 59).is_empty());

        let first = one(h.evaluator.admit_due(T0 + 60));
        assert_eq!(first.commitment.id, draft.id);
        let second = h.evaluator.admit_due(T0 + 60);
        assert!(second.is_empty(), "the loop suppresses the duplicate");

        let runs = h.commitment_runs();
        assert_eq!(runs.len(), 2, "the skip is recorded, not hidden");
        let skipped = runs
            .iter()
            .find(|run| run.id != first.id())
            .expect("the second tick's record");
        assert_eq!(
            skipped.status,
            RunStatus::Skipped {
                reason: SkipReason::Duplicate {
                    of: first.id().to_owned()
                }
            }
        );
        assert_eq!(
            skipped.dedupe_key,
            format!("commitment:{}", draft.id),
            "one key per commitment"
        );
        assert_eq!(
            h.store.get(&draft.id, T0 + 60).unwrap().next_check,
            Some(T0 + 60),
            "admission never moves the schedule; the loop owns duplicate suppression"
        );

        h.evaluator
            .finish(first, Ok(RunOutcome::default()), T0 + 90);
        assert!(h.evaluator.admit_due(T0 + 90).is_empty());
        assert_eq!(h.commitment_runs().len(), 2);
    }

    #[test]
    fn a_deadline_commitment_becomes_due_exactly_once_and_its_run_names_it() {
        let h = Harness::new();
        let draft = h
            .store
            .create(deadline_at("send the draft", T0 + 60), T0)
            .unwrap();

        let run = one(h.evaluator.admit_due(T0 + 60));
        assert_eq!(
            run.condition,
            TriggerCondition::DeadlineArrived { at: T0 + 60 }
        );
        let record = h.proactive.get(run.id()).unwrap();
        assert_eq!(
            record.trigger,
            Trigger::Commitment {
                commitment_id: draft.id.clone()
            },
            "the run links back to the exact commitment"
        );
        assert_eq!(record.target, Target::Companion);
        assert_eq!(record.reason, "deadline arrived: send the draft");
        assert_eq!(record.status, RunStatus::Running);

        let task = h
            .evaluator
            .check_in_task(&run, chrono_tz::Asia::Tokyo, T0 + 60);
        for required in [
            draft.id.as_str(),
            "send the draft",
            "what changed",
            "why now",
            "deadline arrived",
            "Monday, January 5, 2026 18:01 JST",
            "reach_out",
            "you promised the user",
        ] {
            assert!(
                task.contains(required),
                "task is missing {required:?}:\n{task}"
            );
        }
        for forbidden in ["commitment_complete", "mark it complete"] {
            assert!(
                !task.contains(forbidden),
                "the check-in must not be told to complete anything: {forbidden}"
            );
        }

        let finished = h.evaluator.finish(run, Ok(RunOutcome::default()), T0 + 75);
        assert_eq!(finished.status, RunStatus::Completed);
        let after = h.store.get(&draft.id, T0 + 75).unwrap();
        assert_eq!(
            after.last_check,
            Some(Check {
                at: T0 + 75,
                outcome: CheckOutcome::Triggered,
                run_id: Some(finished.id.clone()),
            })
        );
        assert_eq!(after.next_check, None, "a deadline is checked exactly once");
        assert_eq!(
            after.status,
            CommitmentStatus::Due,
            "only the user completes a commitment"
        );
        assert_eq!(after.completion, None);

        for later in [T0 + 76, T0 + 3600, T0 + 86_400] {
            assert!(h.evaluator.admit_due(later).is_empty(), "at {later}");
        }
        assert_eq!(h.commitment_runs().len(), 1);
    }

    #[test]
    fn a_timed_wait_ends_once_and_hands_over_to_the_deadline() {
        let h = Harness::new();
        let build = h
            .store
            .create(
                NewCommitment {
                    waiting_on: Some(WaitCondition::Until { until: T0 + 600 }),
                    deadline: Some(Deadline::At { at: T0 + 900 }),
                    ..promise("check the build")
                },
                T0,
            )
            .unwrap();
        assert_eq!(build.status, CommitmentStatus::Waiting);
        assert!(h.evaluator.admit_due(T0 + 599).is_empty());

        let run = one(h.evaluator.admit_due(T0 + 600));
        assert_eq!(
            run.condition,
            TriggerCondition::WaitEnded { until: T0 + 600 }
        );
        assert_eq!(
            h.proactive.get(run.id()).unwrap().reason,
            "wait ended: check the build"
        );
        assert!(
            h.evaluator
                .check_in_task(&run, chrono_tz::UTC, T0 + 600)
                .contains("wait until Monday, January 5, 2026 09:10 UTC ended")
        );
        h.evaluator.finish(run, Ok(RunOutcome::default()), T0 + 620);
        let between = h.store.get(&build.id, T0 + 620).unwrap();
        assert_eq!(between.status, CommitmentStatus::Active);
        assert_eq!(between.next_check, Some(T0 + 900), "the deadline is next");
        assert!(h.evaluator.admit_due(T0 + 899).is_empty());

        let run = one(h.evaluator.admit_due(T0 + 900));
        assert_eq!(
            run.condition,
            TriggerCondition::DeadlineArrived { at: T0 + 900 }
        );
        h.evaluator.finish(run, Ok(RunOutcome::default()), T0 + 910);
        assert_eq!(h.store.get(&build.id, T0 + 910).unwrap().next_check, None);
        assert!(h.evaluator.admit_due(T0 + 7200).is_empty());
        assert_eq!(h.commitment_runs().len(), 2);
    }

    #[test]
    fn a_completed_dependency_is_observed_and_checked_once() {
        let h = Harness::new();
        let export = h.store.create(promise("finish the export"), T0).unwrap();
        let announce = h
            .store
            .create(
                NewCommitment {
                    dependencies: vec![export.id.clone()],
                    ..promise("tell them the export is ready")
                },
                T0 + 1,
            )
            .unwrap();
        assert_eq!(announce.status, CommitmentStatus::Blocked);
        assert!(
            h.evaluator.admit_due(T0 + 5).is_empty(),
            "blocked with no schedule of its own"
        );

        h.store
            .complete(&export.id, confirmed(), T0 + 10, |_| true)
            .unwrap();
        let run = one(h.evaluator.admit_due(T0 + 10));
        assert_eq!(run.commitment.id, announce.id);
        assert_eq!(
            run.condition,
            TriggerCondition::Observed {
                event: dependency_completed_event(&export.id)
            }
        );
        assert_eq!(
            h.proactive.get(run.id()).unwrap().reason,
            "dependency completed: tell them the export is ready"
        );
        let task = h.evaluator.check_in_task(&run, chrono_tz::UTC, T0 + 10);
        assert!(
            task.contains(&format!(
                "the commitment it depended on ({}) was completed",
                export.id
            )),
            "{task}"
        );
        assert!(
            task.contains("finish the export"),
            "the dependency is named"
        );

        h.evaluator.finish(run, Ok(RunOutcome::default()), T0 + 20);
        let after = h.store.get(&announce.id, T0 + 20).unwrap();
        assert_eq!(after.status, CommitmentStatus::Active);
        assert_eq!(after.next_check, None);
        assert!(matches!(
            after.last_check,
            Some(Check {
                outcome: CheckOutcome::Triggered,
                ..
            })
        ));
        assert!(h.evaluator.admit_due(T0 + 3600).is_empty());
        assert_eq!(h.commitment_runs().len(), 1);
    }

    #[test]
    fn an_observed_event_clears_the_wait_and_is_checked_once() {
        let h = Harness::new();
        let on_mac = h
            .store
            .create(
                NewCommitment {
                    waiting_on: Some(WaitCondition::Event {
                        event: machine_connected_event("mac"),
                    }),
                    ..promise("resume the photo export on the mac")
                },
                T0,
            )
            .unwrap();
        assert_eq!(on_mac.status, CommitmentStatus::Waiting);
        assert!(h.evaluator.admit_due(T0 + 60).is_empty());

        assert!(
            h.store
                .observe_event(&machine_connected_event("other"), T0 + 100)
                .is_empty()
        );
        assert!(h.evaluator.admit_due(T0 + 100).is_empty());
        let observed = h
            .store
            .observe_event(&machine_connected_event("mac"), T0 + 120);
        assert_eq!(observed.len(), 1);

        let run = one(h.evaluator.admit_due(T0 + 120));
        assert_eq!(
            run.condition,
            TriggerCondition::Observed {
                event: "machine_connected:mac".into()
            }
        );
        assert_eq!(
            h.proactive.get(run.id()).unwrap().reason,
            "event observed: resume the photo export on the mac"
        );
        assert!(
            h.evaluator
                .check_in_task(&run, chrono_tz::UTC, T0 + 120)
                .contains("the event it waited for was observed: machine_connected:mac")
        );
        assert!(h.evaluator.admit_due(T0 + 120).is_empty());

        // A second observation while the check runs asks for a check of its own.
        h.store
            .update(
                &on_mac.id,
                crate::services::commitments::CommitmentPatch {
                    waiting_on: Some(WaitCondition::Event {
                        event: machine_connected_event("mac"),
                    }),
                    ..Default::default()
                },
                T0 + 131,
            )
            .unwrap();
        let again = h
            .store
            .observe_event(&machine_connected_event("mac"), T0 + 132);
        assert_eq!(again.len(), 1);
        h.evaluator.finish(run, Ok(RunOutcome::default()), T0 + 140);
        let after = h.store.get(&on_mac.id, T0 + 140).unwrap();
        assert_eq!(
            after.last_check,
            Some(Check {
                at: T0 + 132,
                outcome: CheckOutcome::Observed {
                    event: "machine_connected:mac".into()
                },
                run_id: None,
            }),
            "the fresh observation is kept"
        );
        assert_eq!(after.next_check, Some(T0 + 132));
        let run = one(h.evaluator.admit_due(T0 + 140));
        h.evaluator.finish(run, Ok(RunOutcome::default()), T0 + 150);
        assert_eq!(h.store.get(&on_mac.id, T0 + 150).unwrap().next_check, None);
        let completed = h
            .commitment_runs()
            .into_iter()
            .filter(|run| run.status == RunStatus::Completed)
            .count();
        assert_eq!(
            completed, 2,
            "one check per observation, plus one recorded skip"
        );
        assert_eq!(h.commitment_runs().len(), 3);
    }

    #[test]
    fn a_failed_evaluation_is_recorded_retryable_and_looked_at_again() {
        let h = Harness::new();
        let call = h
            .store
            .create(deadline_at("call the dentist", T0 + 60), T0)
            .unwrap();
        let run = one(h.evaluator.admit_due(T0 + 60));
        let run_id = run.id().to_owned();
        let failed = h
            .evaluator
            .finish(run, Err("provider offline".into()), T0 + 65);
        assert_eq!(
            failed.status,
            RunStatus::Failed {
                error: "provider offline".into(),
                retryable: true
            }
        );

        let after = h.store.get(&call.id, T0 + 65).unwrap();
        assert_eq!(
            after.last_check,
            Some(Check {
                at: T0 + 65,
                outcome: CheckOutcome::Failed {
                    error: "provider offline".into(),
                    retryable: true,
                },
                run_id: Some(run_id.clone()),
            }),
            "the failure is inspectable on the record"
        );
        assert_eq!(after.next_check, Some(T0 + 65 + RETRY_BACKOFF_SECS));
        assert!(
            h.evaluator.admit_due(T0 + 66).is_empty(),
            "no retry before the backoff"
        );

        // The run is retryable through the loop like any other failure.
        let retried = match h.proactive.retry(&run_id, T0 + 70) {
            Ok(Admission::Admitted(handle)) => handle,
            other => panic!(
                "a failed evaluation must be retryable: {:?}",
                other.as_ref().err()
            ),
        };
        assert_eq!(h.proactive.get(retried.id()).unwrap().attempt, 2);
        retried.complete_at(RunOutcome::default(), T0 + 71);

        // And the evaluator looks again after the backoff on its own.
        let run = one(h.evaluator.admit_due(T0 + 65 + RETRY_BACKOFF_SECS));
        assert_eq!(run.commitment.id, call.id);
        assert_eq!(
            run.condition,
            TriggerCondition::DeadlineArrived { at: T0 + 60 }
        );
        let done = h.evaluator.finish(run, Ok(RunOutcome::default()), T0 + 700);
        assert_eq!(done.status, RunStatus::Completed);
        assert_eq!(h.store.get(&call.id, T0 + 700).unwrap().next_check, None);
    }

    #[test]
    fn quiet_hours_hold_checks_and_deny_the_check_ins_reach_out() {
        let h = Harness::new();
        fs::write(
            h.instance_dir().join("project_state.json"),
            r#"{"timezone":"Asia/Tokyo"}"#,
        )
        .unwrap();
        h.proactive
            .set_policy(&ProactivePolicy {
                quiet_hours: Some(QuietHours {
                    start_hour: 22,
                    end_hour: 7,
                }),
                ..ProactivePolicy::default()
            })
            .unwrap();
        // 14:00 UTC is 23:00 in Tokyo: quiet. 22:00 UTC is 07:00: not.
        let night = T0 + 5 * 3600;
        let morning = T0 + 13 * 3600;
        let laundry = h
            .store
            .create(deadline_at("move the laundry", night), T0)
            .unwrap();

        for tick in [night, night + 30, night + 3600] {
            assert!(
                h.evaluator.admit_due(tick).is_empty(),
                "held during quiet hours at {tick}"
            );
        }
        assert!(
            h.commitment_runs().is_empty(),
            "a hold spends no run and records nothing"
        );
        assert_eq!(
            h.store.get(&laundry.id, T0).unwrap(),
            laundry,
            "the record is untouched"
        );

        let run = one(h.evaluator.admit_due(morning));
        assert_eq!(
            run.condition,
            TriggerCondition::DeadlineArrived { at: night },
            "the condition is the deadline, however long it was held"
        );
        // The check-in's reach_out goes through the same gate as every run;
        // a denial (here: the clock moved back into quiet hours) is recorded.
        assert_eq!(
            h.proactive
                .approve_side_effect(Some(run.id()), SideEffect::ReachOut, night),
            Err(Denied::QuietHours)
        );
        let finished = h
            .evaluator
            .finish(run, Ok(RunOutcome::default()), morning + 30);
        assert_eq!(finished.approvals.len(), 1);
        assert!(!finished.approvals[0].allowed);
        assert_eq!(
            h.store.get(&laundry.id, morning + 30).unwrap().next_check,
            Some(morning + 30 + RETRY_BACKOFF_SECS),
            "a check whose reach-out was denied is looked at again"
        );
    }

    #[test]
    fn the_attention_budget_holds_checks_and_denies_reach_out() {
        let h = Harness::new();
        h.proactive
            .set_policy(&ProactivePolicy {
                daily_reach_out_budget: 1,
                ..ProactivePolicy::default()
            })
            .unwrap();
        let invoice = h
            .store
            .create(deadline_at("send the invoice", T0 + 60), T0)
            .unwrap();

        // The one message of the day was spent by a heartbeat.
        h.proactive
            .approve_side_effect(None, SideEffect::ReachOut, T0)
            .unwrap();
        assert!(
            h.evaluator.admit_due(T0 + 60).is_empty(),
            "held: budget spent"
        );
        assert!(h.commitment_runs().is_empty());
        assert_eq!(h.store.get(&invoice.id, T0).unwrap(), invoice, "untouched");

        // The budget frees after 24 hours and the check is admitted.
        let tomorrow = T0 + 86_400;
        let run = one(h.evaluator.admit_due(tomorrow));
        assert_eq!(run.commitment.id, invoice.id);
        assert_eq!(
            h.proactive
                .approve_side_effect(Some(run.id()), SideEffect::ReachOut, tomorrow),
            Ok(()),
            "the check-in's message counts against the budget"
        );
        assert_eq!(
            h.proactive
                .approve_side_effect(Some(run.id()), SideEffect::ReachOut, tomorrow + 1),
            Err(Denied::AttentionBudget)
        );
        h.evaluator
            .finish(run, Ok(RunOutcome::default()), tomorrow + 10);

        // Initiative off holds everything the same way.
        h.proactive
            .set_policy(&ProactivePolicy {
                enabled: false,
                ..ProactivePolicy::default()
            })
            .unwrap();
        let plants = h
            .store
            .create(deadline_at("water the plants", tomorrow + 20), tomorrow)
            .unwrap();
        assert!(h.evaluator.admit_due(tomorrow + 20).is_empty());
        assert_eq!(
            h.store.get(&plants.id, tomorrow).unwrap(),
            plants,
            "untouched"
        );
        assert_eq!(h.commitment_runs().len(), 1);
    }

    #[test]
    fn repeated_ticks_and_restarts_never_duplicate_a_schedule() {
        let h = Harness::new();
        let due_at = T0 + 3600;
        let dentist = h
            .store
            .create(deadline_at("call the dentist", due_at), T0)
            .unwrap();
        for tick in (T0..due_at).step_by(30).take(120) {
            assert!(h.evaluator.admit_due(tick).is_empty(), "at {tick}");
        }
        assert!(h.commitment_runs().is_empty(), "heartbeats record nothing");
        assert_eq!(h.store.get(&dentist.id, T0).unwrap(), dentist);

        // Restart: the record is the only schedule.
        let h2 = h.restart();
        let interrupted = one(h2.evaluator.admit_due(due_at));
        assert_eq!(interrupted.commitment.id, dentist.id);
        let interrupted_id = interrupted.id().to_owned();
        drop(interrupted); // the process dies mid-check

        // Restart again: the dead run is recovered, the check is made once.
        let h3 = h2.restart();
        assert_eq!(h3.proactive.recover_on_restart(due_at + 30), 1);
        assert_eq!(
            h3.proactive.get(&interrupted_id).unwrap().status,
            RunStatus::Failed {
                error: "interrupted by server restart".into(),
                retryable: true
            }
        );
        let run = one(h3.evaluator.admit_due(due_at + 30));
        assert_ne!(run.id(), interrupted_id);
        h3.evaluator
            .finish(run, Ok(RunOutcome::default()), due_at + 60);

        let h4 = h3.restart();
        for tick in [due_at + 60, due_at + 90, due_at + 86_400] {
            assert!(h4.evaluator.admit_due(tick).is_empty(), "at {tick}");
        }
        let completed: Vec<ProactiveRun> = h4
            .commitment_runs()
            .into_iter()
            .filter(|run| run.status == RunStatus::Completed)
            .collect();
        assert_eq!(completed.len(), 1, "exactly one check was made");
        assert_eq!(
            h4.store
                .list(crate::services::commitments::ListFilter::All, due_at + 60)
                .len(),
            1,
            "no record was created by ticking"
        );
    }

    #[test]
    fn a_snooze_ending_is_its_own_condition_and_a_cancelled_check_changes_nothing() {
        let h = Harness::new();
        let plants = h
            .store
            .create(deadline_at("water the plants", T0 - 60), T0)
            .unwrap();
        assert_eq!(plants.status, CommitmentStatus::Due);
        h.store.snooze(&plants.id, T0 + 600, T0).unwrap();
        assert!(h.evaluator.admit_due(T0 + 599).is_empty(), "snoozed");

        let run = one(h.evaluator.admit_due(T0 + 600));
        assert_eq!(
            run.condition,
            TriggerCondition::SnoozeEnded { until: T0 + 600 }
        );
        assert_eq!(
            h.proactive.get(run.id()).unwrap().reason,
            "snooze ended: water the plants"
        );
        let task = h.evaluator.check_in_task(&run, chrono_tz::UTC, T0 + 600);
        assert!(task.contains("snooze until Monday, January 5, 2026 09:10 UTC ended"));
        assert!(task.contains("snoozed 1 time"), "{task}");
        assert!(task.contains("status: due"), "{task}");

        let cancelled = h.evaluator.cancel(run, T0 + 610);
        assert_eq!(cancelled.status, RunStatus::Cancelled);
        let after = h.store.get(&plants.id, T0 + 610).unwrap();
        assert_eq!(
            after.last_check,
            Some(Check {
                at: T0 + 610,
                outcome: CheckOutcome::Unchanged,
                run_id: Some(cancelled.id.clone()),
            })
        );
        assert_eq!(after.next_check, None);
        assert!(h.evaluator.admit_due(T0 + 700).is_empty());
    }

    #[tokio::test]
    async fn the_tick_holds_when_no_background_model_is_configured() {
        let ws = tempfile::tempdir().unwrap();
        let state =
            AppState::new_in(crate::config::Config::default(), ws.path().to_path_buf()).await;
        let instance_dir = companion::companion_dir(&state.workspace_dir);
        fs::create_dir_all(&instance_dir).unwrap();
        fs::write(instance_dir.join("soul.md"), "i am little moon").unwrap();
        let draft = state
            .commitments
            .create(deadline_at("send the draft", T0 + 60), T0)
            .unwrap();

        tick(&state, T0 + 60).await;
        assert!(
            state.proactive.list(usize::MAX).is_empty(),
            "no model, no run"
        );
        assert_eq!(
            state.commitments.get(&draft.id, T0).unwrap(),
            draft,
            "the record is untouched"
        );
    }
}
