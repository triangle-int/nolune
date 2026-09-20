//! Reviewable handoffs (#82): review an unfinished task from another
//! computer and deliberately continue it on a chosen one.
//!
//! Nothing starts on its own. The card and the preview are reads. Accepting
//! is the one action: it re-runs every check against the destination at
//! that moment, refuses with the reasons when one blocks, binds the record
//! to the chosen stable machine id, admits one run through the proactive
//! loop under `Trigger::Handoff` (the dedupe key makes a second acceptance
//! return the same run instead of starting duplicate work), and hands the
//! task to the conversation it came from as an explicit request naming
//! that computer. The conversation's own tools do the work; when it stops,
//! the receipts land on the run and the outcome on the record, so the trail
//! is one place. Keeping or dismissing writes the decision and nothing
//! else.

use std::path::Path;

use chrono::Utc;
use tokio_util::sync::CancellationToken;

use crate::{
    app::state::AppState,
    domain::{
        companion::CANONICAL_SLUG,
        continuity::{
            ContinuityError, ContinuityRecord, HandoffDecision, HandoffOutcome,
            HandoffOutcomeStatus, MAX_NOTE_CHARS, Provenance, ProvenanceSource,
        },
        events::ServerEvent,
        handoff::{
            CheckKind, ComputerSummary, ContinuationCheck, ContinuationPreview, Environment,
            HandoffCard, Severity, build_card, computer_summary,
        },
        machine::KnownMachine,
        proactive::{ProactiveRun, RunOutcome, RunStatus, SkipReason, Target, Trigger},
    },
    services::{
        chat,
        continuity::{ContinuityStore, RecordError},
        llm::Message,
        proactive::{Admission, RunHandle, outcome_from_trace},
    },
};

/// How long a queued continuation waits for the conversation to stop
/// before the run is closed anyway (the chat loop itself is bounded by its
/// turn timeout and iteration cap well inside this).
const CONTINUATION_WAIT_SECS: u64 = 3600;
/// How long a cancelled continuation waits for the conversation to wind down.
const CANCEL_WAIT_SECS: u64 = 30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandoffError {
    /// No readable record with this id.
    NotFound,
    /// Continuation is refused; every reason is listed.
    NotReady(Vec<ContinuationCheck>),
    Invalid(String),
    Storage(String),
}

impl std::fmt::Display for HandoffError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => f.write_str("unknown continuity record"),
            Self::NotReady(checks) => {
                let reasons: Vec<&str> = checks.iter().map(|c| c.detail.as_str()).collect();
                write!(f, "cannot continue yet: {}", reasons.join("; "))
            }
            Self::Invalid(message) | Self::Storage(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for HandoffError {}

impl From<ContinuityError> for HandoffError {
    fn from(error: ContinuityError) -> Self {
        match error {
            ContinuityError::NotFound => Self::NotFound,
            ContinuityError::Invalid(message) => Self::Invalid(message),
            ContinuityError::TooLarge { .. } | ContinuityError::Io(_) => {
                Self::Storage(error.to_string())
            }
        }
    }
}

/// The cards offered right now, most recently updated first.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Listing {
    pub handoffs: Vec<HandoffCard>,
    /// Files under `continuity/` that could not be read. Surfaced, never deleted.
    pub errors: Vec<RecordError>,
}

/// An accepted handoff: the bound card and the run continuing it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Accepted {
    pub card: HandoffCard,
    pub run: ProactiveRun,
    /// A continuation was already running for this record; nothing new started.
    pub already_running: bool,
}

/// How a continuation ended, derived from the conversation's trace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContinuationResult {
    Completed(RunOutcome),
    Failed { error: String, retryable: bool },
    Cancelled,
}

fn store(state: &AppState) -> ContinuityStore {
    ContinuityStore::new(&state.workspace_dir, CANONICAL_SLUG)
}

/// Every known machine, or none when the machine list is unusable (the
/// card still renders; the origin is then just an id).
async fn machines(state: &AppState, now: i64) -> Vec<KnownMachine> {
    match state.machine_registry.known_at(now).await {
        Ok(machines) => machines,
        Err(error) => {
            log::warn!("[handoff] machine list unavailable: {error}");
            Vec::new()
        }
    }
}

async fn environment(state: &AppState) -> Environment {
    Environment {
        model_ready: state.config.read().await.llm.setup_required().is_none(),
        initiative_on: state.proactive.policy().enabled,
    }
}

fn by_user(note: String, now: i64) -> Provenance {
    Provenance {
        source: ProvenanceSource::User,
        at: now,
        note,
    }
}

fn by_server(note: String, now: i64) -> Provenance {
    Provenance {
        source: ProvenanceSource::Server,
        at: now,
        note,
    }
}

fn broadcast(state: &AppState, card: &HandoffCard) {
    let _ = state.events.send(ServerEvent::HandoffUpdated {
        instance_slug: CANONICAL_SLUG.to_owned(),
        card: card.clone(),
    });
}

/// The cards offered right now: resumable records the user has not kept or
/// dismissed since their last explicit update, after the reference check.
pub async fn list(state: &AppState, now: i64) -> Listing {
    let machines = machines(state, now).await;
    let (records, errors) = store(state)
        .list_validated(&state.machine_registry, now, true)
        .await;
    Listing {
        handoffs: records
            .iter()
            .filter(|record| record.handoff_offered())
            .map(|record| build_card(record, &machines))
            .collect(),
        errors,
    }
}

/// The record after the reference check, so the card and the checks see
/// the same blockers the continuity API reports.
async fn validated(state: &AppState, id: &str, now: i64) -> Result<ContinuityRecord, HandoffError> {
    store(state)
        .validate_references(id, &state.machine_registry, now)
        .await
        .ok_or(HandoffError::NotFound)
}

/// One record's card, whether or not it is offered.
pub async fn card(state: &AppState, id: &str, now: i64) -> Result<HandoffCard, HandoffError> {
    let record = validated(state, id, now).await?;
    Ok(build_card(&record, &machines(state, now).await))
}

/// The pre-continuation preview for `machine_id`: the destination and
/// every check, nothing started.
pub async fn preview(
    state: &AppState,
    id: &str,
    machine_id: &str,
    now: i64,
) -> Result<ContinuationPreview, HandoffError> {
    let (record, preview) = checked(state, id, machine_id, now).await?;
    drop(record);
    Ok(preview)
}

async fn checked(
    state: &AppState,
    id: &str,
    machine_id: &str,
    now: i64,
) -> Result<(ContinuityRecord, ContinuationPreview), HandoffError> {
    let record = validated(state, id, now).await?;
    let machines = machines(state, now).await;
    let environment = environment(state).await;
    let preview = crate::domain::handoff::preview(&record, machine_id, &machines, environment);
    Ok((record, preview))
}

/// Continue the task on `machine_id`. Re-validates at this moment, binds
/// the record, admits one run, and hands the task to its conversation.
/// Accepting again while that run is still going returns the same run.
pub async fn accept(
    state: &AppState,
    id: &str,
    machine_id: &str,
    now: i64,
) -> Result<Accepted, HandoffError> {
    continue_on(state, id, machine_id, now, None).await
}

/// Retry a failed or cancelled continuation from the activity view: the
/// same checks, the bound computer, and a linked attempt (`retry_of`).
pub async fn retry(state: &AppState, run_id: &str, now: i64) -> Result<Accepted, HandoffError> {
    let previous = state
        .proactive
        .get(run_id)
        .ok_or_else(|| HandoffError::Invalid(format!("unknown run {run_id}")))?;
    let Trigger::Handoff { handoff_id } = &previous.trigger else {
        return Err(HandoffError::Invalid(format!(
            "run {run_id} is not a handoff continuation"
        )));
    };
    let record = store(state).get(handoff_id).ok_or(HandoffError::NotFound)?;
    let Some(machine_id) = record
        .handoff
        .as_ref()
        .and_then(HandoffDecision::bound_machine)
    else {
        return Err(HandoffError::Invalid(
            "the task is no longer bound to a computer; accept it again from its card".into(),
        ));
    };
    continue_on(state, handoff_id, machine_id, now, Some(run_id)).await
}

/// The one path that starts work: checks, admission, binding, hand-over.
async fn continue_on(
    state: &AppState,
    id: &str,
    machine_id: &str,
    now: i64,
    retry_of: Option<&str>,
) -> Result<Accepted, HandoffError> {
    let (record, preview) = checked(state, id, machine_id, now).await?;

    // Idempotent: a continuation that is still running is the answer, whatever
    // computer this acceptance named.
    if let Some(HandoffDecision::Accepted { run_id, .. }) = &record.handoff
        && let Some(run) = state.proactive.get(run_id)
        && !run.status.is_finished()
    {
        return Ok(Accepted {
            card: preview.card,
            run,
            already_running: true,
        });
    }
    if !preview.ready {
        return Err(HandoffError::NotReady(preview.checks));
    }

    let destination = preview.destination;
    let trigger = Trigger::Handoff {
        handoff_id: id.to_owned(),
    };
    let reason = format!("continue on {}: {}", destination.display_name, record.goal);
    let target = Target::Machine {
        machine_id: machine_id.to_owned(),
    };
    let admission = match retry_of {
        Some(previous) => state
            .proactive
            .retry(previous, now)
            .map_err(HandoffError::Invalid)?,
        None => state.proactive.begin_at(trigger, &reason, target, now),
    };
    let handle = match admission {
        Admission::Admitted(handle) => handle,
        Admission::Skipped(skipped) => {
            // The loop's dedupe key is the backstop for two acceptances that
            // raced past the record: the first one's run is the answer.
            if let RunStatus::Skipped {
                reason: SkipReason::Duplicate { of },
            } = &skipped.status
                && let Some(run) = state.proactive.get(of)
            {
                return Ok(Accepted {
                    card: card(state, id, now).await?,
                    run,
                    already_running: true,
                });
            }
            return Err(HandoffError::NotReady(vec![ContinuationCheck {
                kind: CheckKind::InitiativeOff,
                severity: Severity::Blocking,
                detail: "the companion loop did not admit the continuation; initiative may be off"
                    .into(),
            }]));
        }
    };
    let run_id = handle.id().to_owned();

    // Bind the record to the chosen computer and this run. The write is the
    // record's own validation; a failure closes the run and starts nothing.
    let record = match store(state)
        .decide_handoff(
            id,
            HandoffDecision::Accepted {
                machine_id: machine_id.to_owned(),
                run_id: run_id.clone(),
                at: now,
                outcome: None,
            },
            by_user(
                format!("continue on {} ({})", destination.display_name, run_id),
                now,
            ),
            now,
        )
        .await
    {
        Ok(record) => record,
        Err(error) => {
            handle.fail_at(&format!("could not bind the handoff: {error}"), false, now);
            return Err(error.into());
        }
    };

    // Hand the task to the conversation it came from, as the user's explicit
    // request naming the one computer. Its tools do the work from here.
    let chat_id = record.origin.chat_id.clone();
    let message = continuation_message(&record, &destination);
    let saved =
        match chat::save_user_message(&state.workspace_dir, CANONICAL_SLUG, &chat_id, &message) {
            Ok(saved) => saved,
            Err(error) => {
                handle.fail_at(
                    &format!("could not reach the task's conversation: {error}"),
                    true,
                    now,
                );
                return Err(HandoffError::Storage(error.to_string()));
            }
        };
    let _ = state.events.send(ServerEvent::ChatMessageCreated {
        instance_slug: CANONICAL_SLUG.to_owned(),
        chat_id: chat_id.clone(),
        message: saved.clone(),
    });
    // The acceptance itself decides whether the conversation needs a turn
    // started or is already running and will pick the request up; the
    // follower spawned below only waits for it to stop.
    let own_loop = ensure_agent_loop(state, &chat_id).await;
    log::info!(
        "[handoff] {id}: continuing on '{}' as {run_id} in {chat_id} ({})",
        destination.machine_id,
        if own_loop.is_some() {
            "turn started"
        } else {
            "queued on the running conversation"
        }
    );
    spawn_continuation(
        state.clone(),
        Continuation {
            record_id: id.to_owned(),
            chat_id,
            destination_name: destination.display_name.clone(),
            message_id: saved.id,
        },
        handle,
        own_loop,
    );

    let card = build_card(&record, &machines(state, now).await);
    broadcast(state, &card);
    let run = state
        .proactive
        .get(&run_id)
        .ok_or_else(|| HandoffError::Storage(format!("run {run_id} vanished")))?;
    Ok(Accepted {
        card,
        run,
        already_running: false,
    })
}

/// Leave the task on its origin computer and stop offering the card until
/// explicit work updates the record.
pub async fn keep(state: &AppState, id: &str, now: i64) -> Result<HandoffCard, HandoffError> {
    let record = validated(state, id, now).await?;
    let machines = machines(state, now).await;
    let origin = record
        .machine_ids
        .first()
        .map(|machine_id| computer_summary(machine_id, &machines));
    let note = match &origin {
        Some(origin) => format!("kept on {}", origin.display_name),
        None => "kept where it is".to_owned(),
    };
    let record = store(state)
        .decide_handoff(
            id,
            HandoffDecision::Kept {
                machine_id: origin.map(|origin| origin.machine_id),
                at: now,
            },
            by_user(note, now),
            now,
        )
        .await?;
    let card = build_card(&record, &machines);
    broadcast(state, &card);
    Ok(card)
}

/// Stop offering the card until explicit work updates the record. The
/// record itself stays resumable.
pub async fn dismiss(state: &AppState, id: &str, now: i64) -> Result<HandoffCard, HandoffError> {
    let record = store(state)
        .decide_handoff(
            id,
            HandoffDecision::Dismissed { at: now },
            by_user("handoff dismissed".into(), now),
            now,
        )
        .await?;
    let card = build_card(&record, &machines(state, now).await);
    broadcast(state, &card);
    Ok(card)
}

/// The explicit request the continuation puts into the task's conversation:
/// the record as it stands and the one computer to act on.
pub fn continuation_message(record: &ContinuityRecord, destination: &ComputerSummary) -> String {
    let mut message = format!(
        "[handoff] continue the task \"{}\" on {} (machine_id {}).\n",
        record.goal, destination.display_name, destination.machine_id
    );
    if !record.completed_steps.is_empty() {
        message.push_str("done so far:\n");
        for step in &record.completed_steps {
            message.push_str(&format!("- {}\n", step.summary));
        }
    }
    if let Some(next_step) = &record.next_step {
        message.push_str(&format!("next step: {next_step}\n"));
    }
    if !record.blockers.is_empty() {
        message.push_str("known blockers:\n");
        for blocker in &record.blockers {
            message.push_str(&format!("- {}\n", blocker.detail));
        }
    }
    if !record.resources.is_empty() {
        message.push_str("resources (links, nothing was copied):\n");
        for link in &record.resources {
            message.push_str(&format!("- {}\n", link.resource.describe()));
        }
    }
    message.push_str(&format!(
        "rules: use machine_id \"{}\" for every computer_use, remote_bash, and remote file call; \
         do not act on any other computer. record progress with task_continuity_update \
         (id {}). ask before anything that needs approval or goes beyond the task above.",
        destination.machine_id, record.id
    ));
    message
}

/// One accepted continuation being followed to its end.
struct Continuation {
    record_id: String,
    chat_id: String,
    destination_name: String,
    /// The handoff request in the conversation; receipts come from what follows it.
    message_id: String,
}

/// Follow the conversation until it stops (or the run is cancelled), then
/// close the run with receipts and put the outcome on the record. `own_loop`
/// is the turn the acceptance started, or `None` when the conversation was
/// already running and the request is queued on it.
fn spawn_continuation(
    state: AppState,
    continuation: Continuation,
    handle: RunHandle,
    own_loop: Option<tokio::task::JoinHandle<()>>,
) {
    tokio::spawn(async move {
        let key = crate::routes::chat::task_key(CANONICAL_SLUG, &continuation.chat_id);
        let cancelled = handle.token();
        let follow = async {
            match own_loop {
                Some(join) => {
                    let _ = join.await;
                }
                None => wait_for_conversation(&state, &key, CONTINUATION_WAIT_SECS).await,
            }
        };
        let was_cancelled = tokio::select! {
            _ = follow => false,
            _ = cancelled.cancelled() => {
                if let Some(token) = state.agent_tasks.lock().await.get(&key) {
                    token.cancel();
                }
                wait_for_conversation(&state, &key, CANCEL_WAIT_SECS).await;
                true
            }
        };
        finalize(&state, &continuation, handle, was_cancelled).await;
    });
}

/// Start the conversation's agent loop when none is running, exactly as a
/// sent message does; `None` when one is already running and will pick the
/// handoff up on its next turn.
async fn ensure_agent_loop(state: &AppState, chat_id: &str) -> Option<tokio::task::JoinHandle<()>> {
    let key = crate::routes::chat::task_key(CANONICAL_SLUG, chat_id);
    let cancel = CancellationToken::new();
    {
        let mut tasks = state.agent_tasks.lock().await;
        if tasks.contains_key(&key) {
            return None;
        }
        tasks.insert(key, cancel.clone());
    }
    Some(tokio::spawn(crate::routes::chat::run_agent_loop(
        state.clone(),
        CANONICAL_SLUG.to_owned(),
        chat_id.to_owned(),
        cancel,
        false,
    )))
}

/// Wait until no agent loop runs for `key`, or `max_secs` pass.
async fn wait_for_conversation(state: &AppState, key: &str, max_secs: u64) {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(max_secs);
    loop {
        if !state.agent_tasks.lock().await.contains_key(key) {
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            log::warn!("[handoff] {key}: the conversation did not stop in time");
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

async fn finalize(
    state: &AppState,
    continuation: &Continuation,
    handle: RunHandle,
    cancelled: bool,
) {
    let trace = trace_after(
        &state.workspace_dir,
        CANONICAL_SLUG,
        &continuation.chat_id,
        &continuation.message_id,
    );
    let result = continuation_result(&trace, cancelled);
    let now = Utc::now().timestamp();
    let run_id = handle.id().to_owned();
    match &result {
        ContinuationResult::Completed(outcome) => {
            handle.complete_at(outcome.clone(), now);
        }
        ContinuationResult::Failed { error, retryable } => {
            handle.fail_at(error, *retryable, now);
        }
        ContinuationResult::Cancelled => {
            handle.cancel_at(now);
        }
    }
    let outcome = handoff_outcome(&result, now);
    let verb = match outcome.status {
        HandoffOutcomeStatus::Completed => "completed",
        HandoffOutcomeStatus::Failed => "failed",
        HandoffOutcomeStatus::Cancelled => "cancelled",
    };
    let note = format!(
        "continuation on {} {verb}: {}",
        continuation.destination_name, outcome.summary
    );
    log::info!("[handoff] {}: {run_id} {note}", continuation.record_id);
    match store(state)
        .record_handoff_outcome(
            &continuation.record_id,
            &run_id,
            outcome,
            by_server(note, now),
            now,
        )
        .await
    {
        Ok(record) => broadcast(state, &build_card(&record, &machines(state, now).await)),
        Err(error) => log::warn!(
            "[handoff] {}: could not record the outcome of {run_id}: {error}",
            continuation.record_id
        ),
    }
}

/// The conversation's messages after `message_id`, for receipts.
pub fn trace_after(
    workspace_dir: &Path,
    slug: &str,
    chat_id: &str,
    message_id: &str,
) -> Vec<Message> {
    let entries = chat::load_rig_history(&chat::rig_history_path(workspace_dir, slug, chat_id))
        .unwrap_or_default();
    let Some(at) = entries
        .iter()
        .position(|entry| entry.id.as_deref() == Some(message_id))
    else {
        return Vec::new();
    };
    entries
        .into_iter()
        .skip(at + 1)
        .map(|entry| entry.message)
        .collect()
}

/// What the trace says about the continuation: receipts when a turn ran,
/// the server's own `[system]` line when it stopped with an error, and a
/// retryable failure when no turn ran at all.
pub fn continuation_result(trace: &[Message], cancelled: bool) -> ContinuationResult {
    use crate::services::llm::ContentBlock;
    if cancelled {
        return ContinuationResult::Cancelled;
    }
    let last_reply = trace.iter().rev().find_map(|message| match message {
        Message::Assistant { content } => Some(content),
        Message::User { .. } => None,
    });
    let Some(last_reply) = last_reply else {
        return ContinuationResult::Failed {
            error: "the conversation stopped before taking the task up".into(),
            retryable: true,
        };
    };
    let system_line = last_reply.iter().rev().find_map(|block| match block {
        ContentBlock::Text { text } => text.strip_prefix("[system] "),
        _ => None,
    });
    if let Some(error) = system_line {
        return ContinuationResult::Failed {
            error: error.trim().to_owned(),
            retryable: true,
        };
    }
    ContinuationResult::Completed(outcome_from_trace(trace, 0))
}

/// The record's receipt of a finished run.
pub fn handoff_outcome(result: &ContinuationResult, finished_at: i64) -> HandoffOutcome {
    let (status, summary) = match result {
        ContinuationResult::Completed(outcome) => (
            HandoffOutcomeStatus::Completed,
            match outcome.actions.len() {
                0 => "no actions".to_owned(),
                1 => "1 action".to_owned(),
                n => format!("{n} actions"),
            },
        ),
        ContinuationResult::Failed { error, .. } => (
            HandoffOutcomeStatus::Failed,
            error.chars().take(MAX_NOTE_CHARS).collect(),
        ),
        ContinuationResult::Cancelled => (HandoffOutcomeStatus::Cancelled, "cancelled".to_owned()),
    };
    HandoffOutcome {
        status,
        finished_at,
        summary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::continuity::{ContinuityUpdate, HandoffOutcomeStatus, Origin, ResourceRef};
    use crate::domain::proactive::ActionReceipt;
    use crate::services::llm::{ContentBlock, HistoryEntry};
    use cua_protocol::MachineHealth;

    const T0: i64 = 1_767_603_600;

    fn record() -> ContinuityRecord {
        let by = |note: &str| Provenance {
            source: ProvenanceSource::Chat,
            at: T0,
            note: note.into(),
        };
        let mut record = ContinuityRecord::new(
            "task_1767603600_0badcafe".into(),
            "rename the trip photos",
            Origin {
                chat_id: "chat_1".into(),
                message_id: None,
            },
            by("asked in chat"),
            T0,
        )
        .unwrap();
        record
            .apply(
                &ContinuityUpdate {
                    completed_step: Some("listed the folder".into()),
                    blocker: Some("needs the external drive".into()),
                    next_step: Some("rename IMG_* files".into()),
                    machine_ids: vec!["mac-a".into()],
                    resources: vec![
                        ResourceRef::Upload {
                            id: "upload_1".into(),
                        },
                        ResourceRef::MachinePath {
                            machine_id: "mac-a".into(),
                            path: "/Volumes/Trip".into(),
                        },
                    ],
                    ..Default::default()
                },
                by("progress"),
                T0 + 1,
            )
            .unwrap();
        record
    }

    fn destination() -> ComputerSummary {
        ComputerSummary {
            machine_id: "4f3c1c2e-9b5e-4d2b-8f0a-1c2d3e4f5a6b".into(),
            display_name: "Studio Mac".into(),
            known: true,
            online: true,
            health: MachineHealth::Healthy,
            platform: Some(cua_protocol::Platform::Macos),
            last_seen: Some(T0),
        }
    }

    #[test]
    fn the_continuation_message_states_the_task_and_the_one_computer_to_act_on() {
        let message = continuation_message(&record(), &destination());
        for required in [
            "[handoff]",
            "rename the trip photos",
            "Studio Mac",
            "4f3c1c2e-9b5e-4d2b-8f0a-1c2d3e4f5a6b",
            "listed the folder",
            "rename IMG_* files",
            "needs the external drive",
            "upload upload_1",
            "/Volumes/Trip on mac-a",
            "task_1767603600_0badcafe",
            "task_continuity_update",
        ] {
            assert!(
                message.contains(required),
                "missing {required:?} in:\n{message}"
            );
        }
        // The one computer is stated as a rule, not a suggestion.
        assert!(
            message.contains("do not act on any other computer"),
            "{message}"
        );
        assert!(message.contains("ask before"), "{message}");
    }

    #[test]
    fn the_trace_after_the_handoff_message_is_what_the_receipts_come_from() {
        let ws = tempfile::tempdir().unwrap();
        let path = chat::rig_history_path(ws.path(), CANONICAL_SLUG, "chat_1");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let entry = |id: &str, message: Message| HistoryEntry::new(message, "0".into(), id.into());
        let tool_call = Message::Assistant {
            content: vec![ContentBlock::ToolCall {
                id: "call_1".into(),
                name: "computer_use".into(),
                arguments: serde_json::json!({"machine_id": "mac-b", "action": "screenshot"}),
            }],
        };
        chat::save_rig_history(
            &path,
            &[
                entry("m1", Message::user("earlier")),
                entry("m2", Message::assistant("earlier reply")),
                entry("m3", Message::user("[handoff] continue")),
                entry("m4", tool_call.clone()),
                entry("m5", Message::assistant("done")),
            ],
        );
        let trace = trace_after(ws.path(), CANONICAL_SLUG, "chat_1", "m3");
        assert_eq!(
            serde_json::to_value(&trace).unwrap(),
            serde_json::to_value(vec![tool_call, Message::assistant("done")]).unwrap()
        );
        assert!(trace_after(ws.path(), CANONICAL_SLUG, "chat_1", "m5").is_empty());
        assert!(
            trace_after(ws.path(), CANONICAL_SLUG, "chat_1", "missing").is_empty(),
            "an unknown anchor yields no receipts rather than the whole history"
        );
        assert!(trace_after(ws.path(), CANONICAL_SLUG, "chat_none", "m3").is_empty());
    }

    #[test]
    fn the_continuation_result_comes_from_receipts_never_from_model_text() {
        let tool_call = Message::Assistant {
            content: vec![
                ContentBlock::text("let me look"),
                ContentBlock::ToolCall {
                    id: "call_1".into(),
                    name: "computer_use".into(),
                    arguments: serde_json::json!({"machine_id": "mac-b", "action": "screenshot"}),
                },
            ],
        };
        let trace = vec![
            tool_call,
            Message::user("tool result"),
            Message::Assistant {
                content: vec![ContentBlock::ToolCall {
                    id: "call_2".into(),
                    name: "task_continuity_update".into(),
                    arguments: serde_json::json!({"id": "task_1", "completed_step": "renamed", "note": "progress"}),
                }],
            },
            Message::assistant("renamed the files."),
        ];
        let ContinuationResult::Completed(outcome) = continuation_result(&trace, false) else {
            panic!("a trace with tool calls completes");
        };
        assert_eq!(outcome.actions.len(), 2);
        assert_eq!(
            outcome.actions[0],
            ActionReceipt {
                tool: "computer_use".into(),
                summary: crate::services::tools::tool_summary(
                    "computer_use",
                    r#"{"action":"screenshot","machine_id":"mac-b"}"#
                ),
            }
        );
        assert_eq!(outcome.messages_sent, 0);
        let receipt = handoff_outcome(&ContinuationResult::Completed(outcome), T0 + 200);
        assert_eq!(receipt.status, HandoffOutcomeStatus::Completed);
        assert_eq!(receipt.finished_at, T0 + 200);
        assert_eq!(receipt.summary, "2 actions");

        // A reply without actions still completed; it just did nothing.
        let quiet = continuation_result(&[Message::assistant("nothing to do")], false);
        assert!(matches!(quiet, ContinuationResult::Completed(ref o) if o.actions.is_empty()));
        assert_eq!(handoff_outcome(&quiet, T0).summary, "no actions");

        // The server's own error line is a failure the user can retry.
        let failed =
            continuation_result(&[Message::assistant("[system] request timed out")], false);
        assert_eq!(
            failed,
            ContinuationResult::Failed {
                error: "request timed out".into(),
                retryable: true,
            }
        );
        assert_eq!(
            handoff_outcome(&failed, T0).status,
            HandoffOutcomeStatus::Failed
        );
        assert_eq!(handoff_outcome(&failed, T0).summary, "request timed out");

        // No turn at all (no model, or the conversation never picked it up).
        let none = continuation_result(&[], false);
        assert!(matches!(
            none,
            ContinuationResult::Failed {
                retryable: true,
                ..
            }
        ));
        assert!(!handoff_outcome(&none, T0).summary.is_empty());

        // Cancellation wins over whatever the trace says.
        assert_eq!(
            continuation_result(&[Message::assistant("done")], true),
            ContinuationResult::Cancelled
        );
        assert_eq!(
            handoff_outcome(&ContinuationResult::Cancelled, T0).status,
            HandoffOutcomeStatus::Cancelled
        );

        // Summaries stay within a note.
        let long = ContinuationResult::Failed {
            error: "x".repeat(MAX_NOTE_CHARS * 2),
            retryable: false,
        };
        assert!(handoff_outcome(&long, T0).summary.chars().count() <= MAX_NOTE_CHARS);
    }
}
