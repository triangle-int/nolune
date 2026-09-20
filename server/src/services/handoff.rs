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
            ComputerSummary, ContinuationCheck, ContinuationPreview, Environment, HandoffCard,
            build_card, computer_summary,
        },
        machine::KnownMachine,
        proactive::{ProactiveRun, RunOutcome, Target, Trigger},
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
    let _ = (state, now);
    todo!("#82: list the offered handoff cards")
}

/// One record's card, whether or not it is offered.
pub async fn card(state: &AppState, id: &str, now: i64) -> Result<HandoffCard, HandoffError> {
    let _ = (state, id, now);
    todo!("#82: build one card")
}

/// The pre-continuation preview for `machine_id`: the destination and
/// every check, nothing started.
pub async fn preview(
    state: &AppState,
    id: &str,
    machine_id: &str,
    now: i64,
) -> Result<ContinuationPreview, HandoffError> {
    let _ = (state, id, machine_id, now);
    todo!("#82: preview a continuation")
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
    let _ = (state, id, machine_id, now);
    todo!("#82: accept a handoff")
}

/// Retry a failed or cancelled continuation from the activity view: the
/// same checks, the bound computer, and a linked attempt (`retry_of`).
pub async fn retry(state: &AppState, run_id: &str, now: i64) -> Result<Accepted, HandoffError> {
    let _ = (state, run_id, now);
    todo!("#82: retry a continuation")
}

/// Leave the task on its origin computer and stop offering the card until
/// explicit work updates the record.
pub async fn keep(state: &AppState, id: &str, now: i64) -> Result<HandoffCard, HandoffError> {
    let _ = (state, id, now);
    todo!("#82: keep the task where it is")
}

/// Stop offering the card until explicit work updates the record. The
/// record itself stays resumable.
pub async fn dismiss(state: &AppState, id: &str, now: i64) -> Result<HandoffCard, HandoffError> {
    let _ = (state, id, now);
    todo!("#82: dismiss the card")
}

/// The explicit request the continuation puts into the task's conversation:
/// the record as it stands and the one computer to act on.
pub fn continuation_message(record: &ContinuityRecord, destination: &ComputerSummary) -> String {
    let _ = (record, destination);
    todo!("#82: state the handoff to the conversation")
}

/// The conversation's messages after `message_id`, for receipts.
pub fn trace_after(
    workspace_dir: &Path,
    slug: &str,
    chat_id: &str,
    message_id: &str,
) -> Vec<Message> {
    let _ = (workspace_dir, slug, chat_id, message_id);
    todo!("#82: read the trace after the handoff message")
}

/// What the trace says about the continuation: receipts when a turn ran,
/// the server's own `[system]` line when it stopped with an error, and a
/// retryable failure when no turn ran at all.
pub fn continuation_result(trace: &[Message], cancelled: bool) -> ContinuationResult {
    let _ = (trace, cancelled);
    todo!("#82: derive the continuation result")
}

/// The record's receipt of a finished run.
pub fn handoff_outcome(result: &ContinuationResult, finished_at: i64) -> HandoffOutcome {
    let _ = (result, finished_at);
    todo!("#82: summarize the result for the record")
}

#[allow(dead_code)]
fn unused(_: RunHandle, _: CancellationToken, _: i64) -> Option<(Target, Trigger, Admission)> {
    let _ = (Utc::now(), MAX_NOTE_CHARS, HandoffOutcomeStatus::Completed);
    let _ = (
        outcome_from_trace,
        chat::save_user_message,
        build_card,
        computer_summary,
        HandoffDecision::Dismissed { at: 0 },
        CONTINUATION_WAIT_SECS,
        by_user,
        by_server,
        broadcast,
        environment,
        machines,
        store,
    );
    None
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
