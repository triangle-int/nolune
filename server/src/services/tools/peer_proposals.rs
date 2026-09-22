//! Proposals to a paired companion (#111): the chat's bounded way to
//! propose a meeting to the other owner or to hand one of this owner's
//! unfinished tasks over to them. Like the sending tools of #110 in
//! `super::federation`, each is a thin wrapper that turns its arguments
//! into one typed [`OutboxRequest`] and hands it to the outbox, which asks
//! this owner's own policy gate before anything is queued and builds the
//! wire intent itself. The other companion records what arrives as a
//! proposal for its owner's review and writes nothing until they accept;
//! the tool answers with the queued request's id and never sees the
//! answer, let alone the other owner's decision.
//!
//! A handoff names a continuity record (#81) of this owner by id: the
//! tool reads the record and the outbox turns it into bounded references
//! and provenance (the goal, the most recent steps, the next step, the
//! blockers, a label per linked resource, the record's provenance
//! entries), never a file, a memory, or an upload's contents; a record
//! that is closed, or that does not exist, is refused before anything is
//! queued. There is no tool that takes a peer's proposal back as an
//! argument, and no peer can reach a file, an email, or a computer
//! through these.
//!
//! They are chat tools only: the check-in and reflection routines never
//! carry them, so nothing the companion does on its own can reach another
//! owner.

use std::path::Path;
use std::sync::Arc;

use schemars::JsonSchema;
use serde::Deserialize;

use crate::services::continuity::ContinuityStore;
use crate::services::federation::outbox::OutboxRequest;
use crate::services::tool::{Tool, ToolDefinition, ToolDyn};

use super::federation::{Context, PeerSending, SHARED_GUIDANCE};
use super::{ToolExecError, openai_schema};

/// The proposal tools, in registration order, beside the sending tools.
pub fn proposal_tools(
    sending: PeerSending,
    workspace_dir: &Path,
    instance_slug: &str,
    chat_id: &str,
) -> Vec<Box<dyn ToolDyn>> {
    let context = Arc::new(Context::new(sending, workspace_dir, instance_slug, chat_id));
    let continuity = ContinuityStore::new(workspace_dir, instance_slug);
    vec![
        Box::new(ProposePeerMeetingTool(context.clone())),
        Box::new(HandoffTaskToPeerTool {
            context,
            continuity,
        }),
    ]
}

// ---------------------------------------------------------------------------
// propose_peer_meeting
// ---------------------------------------------------------------------------

pub struct ProposePeerMeetingTool(Arc<Context>);

#[derive(Deserialize, JsonSchema)]
pub struct ProposePeerMeetingArgs {
    /// The paired companion to ask: its id, or a unique prefix of at least 8 characters.
    pub peer: String,
    /// What is proposed, in the user's words: what, where, with whom.
    pub description: String,
    /// Start of the window the meeting would fall in: RFC 3339, or a local date and time like 2026-01-06T09:00, or a date.
    pub from: String,
    /// End of the window (same formats), after `from` and at most 31 days later.
    pub to: String,
    /// The owner you speak for, by the name they go by.
    pub on_behalf_of: String,
    /// Why you propose it, in one line.
    pub purpose: String,
}

impl Tool for ProposePeerMeetingTool {
    const NAME: &'static str = "propose_peer_meeting";
    type Error = ToolExecError;
    type Args = ProposePeerMeetingArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.into(),
            description: format!(
                "Propose a meeting inside a window to a paired companion. Its owner reviews the proposal and decides; nothing is put on anyone's schedule until they accept, and this owner's own commitment is theirs to make. {SHARED_GUIDANCE}"
            ),
            parameters: openai_schema::<ProposePeerMeetingArgs>(),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let from = self.0.when(&args.from)?;
        let to = self.0.when(&args.to)?;
        self.0.send(
            args.peer,
            args.on_behalf_of,
            args.purpose,
            OutboxRequest::Proposal {
                description: args.description,
                from,
                to,
            },
        )
    }
}

// ---------------------------------------------------------------------------
// handoff_task_to_peer
// ---------------------------------------------------------------------------

pub struct HandoffTaskToPeerTool {
    context: Arc<Context>,
    continuity: ContinuityStore,
}

#[derive(Deserialize, JsonSchema)]
pub struct HandoffTaskToPeerArgs {
    /// The paired companion to hand the task to: its id, or a unique prefix of at least 8 characters.
    pub peer: String,
    /// The id of the unfinished task to hand over, as `task_continuity_update` reported it.
    pub record_id: String,
    /// The owner you speak for, by the name they go by.
    pub on_behalf_of: String,
    /// Why the task is handed over, in one line.
    pub purpose: String,
}

impl Tool for HandoffTaskToPeerTool {
    const NAME: &'static str = "handoff_task_to_peer";
    type Error = ToolExecError;
    type Args = HandoffTaskToPeerArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.into(),
            description: format!(
                "Hand one of the user's unfinished tasks over to a paired companion, by the task's id. What travels is the task's goal, its most recent steps, the next step, the blockers, a label for each linked resource, and where each entry came from; never a file, a memory, or an upload. The other owner reviews it and may take it over as a task of their own; the task here is left as it is. Only hand over a task the user asked you to hand over. {SHARED_GUIDANCE}"
            ),
            parameters: openai_schema::<HandoffTaskToPeerArgs>(),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let record = self.continuity.get(&args.record_id).ok_or_else(|| {
            ToolExecError(format!(
                "not queued: no task record has the id {:?}; task_continuity_update reports the id of each task",
                args.record_id
            ))
        })?;
        if !record.state.is_resumable() {
            return Err(ToolExecError(format!(
                "not queued: that task is closed ({:?}); only an unfinished task can be handed over",
                record.state
            )));
        }
        self.context.send(
            args.peer,
            args.on_behalf_of,
            args.purpose,
            OutboxRequest::Handoff {
                record: Box::new(record),
            },
        )
    }
}
