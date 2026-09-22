//! The owner's decisions on what paired companions proposed (#111): the
//! one place a proposal from a peer becomes a record on this server, and
//! only on this server, only when this owner accepts it. Accepting a
//! meeting or a reminder writes one commitment (#85) due at the proposed
//! time; accepting a task handoff writes one continuity record (#81),
//! active, from which the owner picks the work up as they would any task
//! of their own. Dismissing writes nothing. The proposal store
//! (`services::federation::proposals`) holds the decision lock, records
//! the owner's word as an audit receipt, runs the write here exactly
//! once, and keeps what it wrote, so accepting twice creates one record
//! and a dismissed or lapsed proposal is never written.
//!
//! What is written is built from this server's own ids alone: the
//! sender's verified companion id and the conversation message the
//! proposal was delivered as. The peer's words (the description, the
//! reminder text, the goal, the steps, the labels) stay in that message's
//! untrusted block and in the proposal record the owner reviewed, and
//! never become a promise, a goal, a provenance note, or a resource link,
//! because those reach the model outside any untrusted block. A handed
//! over task starts with no resources: the peer's references are labels
//! on another owner's server and name nothing here. Like the delivery,
//! this lives beside the federation modules, not among them: the
//! federation state never reaches into the companion's directory.

use crate::{
    app::state::AppState,
    domain::{
        commitment::{Deadline, Owner, Provenance as CommitmentProvenance},
        companion::CANONICAL_SLUG,
        continuity::{ContinuityUpdate, Origin, Provenance, ProvenanceSource},
        federation::FederationError,
        federation_proposal::{PeerProposal, ProposalDetails, ProposalOutcome},
    },
    services::{
        commitments::NewCommitment, continuity::ContinuityStore, federation::proposals::Accepted,
        peer_delivery::INBOUND_CHAT_ID,
    },
};

/// The owner accepts the proposal `id`: the one record it stands for is
/// written on this server and the proposal keeps its id. A proposal
/// already accepted is returned with `already_accepted` and nothing is
/// written again.
pub async fn accept(state: &AppState, id: &str) -> Result<Accepted, FederationError> {
    state
        .federation_proposals
        .accept(
            &state.federation,
            &state.federation_gate,
            id,
            async |proposal| write(state, proposal).await,
        )
        .await
}

/// The owner dismisses the proposal `id`: nothing is written, and it stays
/// dismissed.
pub async fn dismiss(state: &AppState, id: &str) -> Result<PeerProposal, FederationError> {
    state
        .federation_proposals
        .dismiss(&state.federation, &state.federation_gate, id)
        .await
}

/// The one write an acceptance runs, on this owner's own stores.
async fn write(
    state: &AppState,
    proposal: &PeerProposal,
) -> Result<ProposalOutcome, FederationError> {
    let now = i64::try_from(state.federation_proposals.now()).unwrap_or(i64::MAX);
    let io = |message: String| FederationError::Io {
        path: state.workspace_dir.clone(),
        message,
    };
    let sender = proposal.sender.as_str();
    let message_id = proposal.message_id.as_str();
    let provenance = CommitmentProvenance::Chat {
        chat_id: INBOUND_CHAT_ID.to_owned(),
        message_id: Some(message_id.to_owned()),
    };
    match &proposal.details {
        ProposalDetails::Meeting { window, .. } => {
            let commitment = state
                .commitments
                .create(
                    NewCommitment {
                        promise: meeting_promise(sender, message_id),
                        owner: Owner::User,
                        deadline: Some(Deadline::Window {
                            start: i64::try_from(window.from).unwrap_or(i64::MAX),
                            end: i64::try_from(window.to).unwrap_or(i64::MAX),
                        }),
                        provenance,
                        ..NewCommitment::default()
                    },
                    now,
                )
                .map_err(|error| io(error.to_string()))?;
            Ok(ProposalOutcome::Commitment {
                commitment_id: commitment.id,
            })
        }
        ProposalDetails::Reminder { at, .. } => {
            let commitment = state
                .commitments
                .create(
                    NewCommitment {
                        promise: reminder_promise(sender, message_id),
                        owner: Owner::Companion,
                        deadline: Some(Deadline::At {
                            at: i64::try_from(*at).unwrap_or(i64::MAX),
                        }),
                        provenance,
                        ..NewCommitment::default()
                    },
                    now,
                )
                .map_err(|error| io(error.to_string()))?;
            Ok(ProposalOutcome::Commitment {
                commitment_id: commitment.id,
            })
        }
        ProposalDetails::Handoff { .. } => {
            let record = ContinuityStore::new(&state.workspace_dir, CANONICAL_SLUG)
                .create(
                    &handoff_goal(sender, message_id),
                    Origin {
                        chat_id: INBOUND_CHAT_ID.to_owned(),
                        message_id: Some(message_id.to_owned()),
                    },
                    &ContinuityUpdate::default(),
                    Provenance {
                        source: ProvenanceSource::User,
                        at: now,
                        note: handoff_note(sender, message_id),
                    },
                    now,
                )
                .await
                .map_err(|error| io(error.to_string()))?;
            Ok(ProposalOutcome::Continuity {
                record_id: record.id,
            })
        }
    }
}

/// The promise of the commitment a meeting becomes: this server's ids
/// only, because a promise reaches the check-in prompt outside any
/// untrusted block.
fn meeting_promise(sender: &str, message_id: &str) -> String {
    format!(
        "Keep the meeting companion {sender} proposed and you accepted (chat message {message_id})"
    )
}

/// The promise of the commitment a reminder becomes, likewise.
fn reminder_promise(sender: &str, message_id: &str) -> String {
    format!("Remind the owner of what companion {sender} sent (chat message {message_id})")
}

/// The goal of the continuity record a handoff becomes, likewise: the
/// owner rewrites it in their own words when they pick the task up.
fn handoff_goal(sender: &str, message_id: &str) -> String {
    format!("Take over the task companion {sender} handed over (chat message {message_id})")
}

/// The creating provenance entry of that record.
fn handoff_note(sender: &str, message_id: &str) -> String {
    format!("Accepted a task handoff from companion {sender} (chat message {message_id})")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SENDER: &str = "TFccHElqXR1lkUBqQoQPWYxPSm1wPjCl8WXbgm_cQ7E";

    #[test]
    fn what_is_written_names_this_servers_ids_and_nothing_the_peer_chose() {
        for line in [
            meeting_promise(SENDER, "msg-1"),
            reminder_promise(SENDER, "msg-1"),
            handoff_goal(SENDER, "msg-1"),
            handoff_note(SENDER, "msg-1"),
        ] {
            assert!(line.contains(SENDER) && line.contains("msg-1"), "{line}");
            assert!(
                line.chars().count() <= 300,
                "fits every record's bound: {line}"
            );
        }
    }
}
