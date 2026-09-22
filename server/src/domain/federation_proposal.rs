//! What a paired companion proposed and this owner has yet to decide
//! (#111): a meeting, a reminder, or a task handoff, kept for review with
//! everything the owner needs to decide (who asked, on whose behalf, why,
//! and the typed details) and nothing that acts on its own. A proposal is
//! written by the delivery of an accepted intent and changes only by the
//! owner's own accept or dismiss, or by lapsing. Accepting writes exactly
//! one record on this server (a commitment for a meeting or a reminder, a
//! continuity record for a handoff) and names it in `outcome`; dismissing
//! writes nothing; a lapsed proposal cannot be accepted.
//!
//! The details are peer text: they serialize (that is how the owner's
//! client shows them, as plain text) but never format, and the one place
//! they reach the model is the untrusted block of the conversation message
//! the proposal was delivered as (`message_id`). The record that an
//! acceptance writes is built from this server's own ids, never from these
//! details (`services::peer_proposals`).
//!
//! Everything here is a shape; the store is `services::federation::proposals`.

use serde::{Deserialize, Serialize};

use super::federation_intent::{IntentPayload, PeerLabel, TaskHandoff, TimeWindow};
use super::federation_policy::{IntentClass, PeerText};

/// Version of a proposal record this server writes.
pub const PROPOSAL_VERSION: u32 = 1;

/// How long a task handoff waits for the owner's decision before it
/// lapses: a week. A meeting lapses with its window and a reminder at its
/// time.
pub const HANDOFF_REVIEW_SECS: u64 = 7 * 24 * 60 * 60;

/// Where a proposal stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    /// Waiting for the owner.
    Open,
    /// The owner accepted it and the one record was written.
    Accepted,
    /// The owner dismissed it; nothing was written and it stays dismissed.
    Dismissed,
    /// It lapsed before the owner decided; it can no longer be accepted.
    Expired,
}

impl ProposalStatus {
    pub fn name(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Accepted => "accepted",
            Self::Dismissed => "dismissed",
            Self::Expired => "expired",
        }
    }
}

/// The typed details the owner reviews, one shape per proposal class.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProposalDetails {
    /// Do something together inside `window`.
    Meeting {
        description: PeerText,
        window: TimeWindow,
    },
    /// Be reminded of `text` at `at` (Unix seconds).
    Reminder { text: PeerText, at: u64 },
    /// Take over the peer's owner's unfinished task.
    Handoff { task: TaskHandoff },
}

impl ProposalDetails {
    /// The details of `payload`, for the payloads that are proposals;
    /// `None` for a message, an availability query, or a decision.
    pub fn from_payload(payload: &IntentPayload) -> Option<Self> {
        match payload {
            IntentPayload::Proposal {
                description,
                window,
            } => Some(Self::Meeting {
                description: description.clone(),
                window: *window,
            }),
            IntentPayload::Reminder { text, at } => Some(Self::Reminder {
                text: text.clone(),
                at: *at,
            }),
            IntentPayload::Handoff { task } => Some(Self::Handoff { task: task.clone() }),
            IntentPayload::Message { .. }
            | IntentPayload::Availability { .. }
            | IntentPayload::Decision { .. } => None,
        }
    }

    /// The intent class the proposal came as.
    pub fn class(&self) -> IntentClass {
        match self {
            Self::Meeting { .. } => IntentClass::Proposal,
            Self::Reminder { .. } => IntentClass::Reminder,
            Self::Handoff { .. } => IntentClass::Handoff,
        }
    }

    /// When a proposal received at `received_at` can no longer be
    /// accepted: a meeting when its window ends, a reminder at its time,
    /// a handoff after [`HANDOFF_REVIEW_SECS`].
    pub fn review_deadline(&self, received_at: u64) -> u64 {
        match self {
            Self::Meeting { window, .. } => window.to,
            Self::Reminder { at, .. } => *at,
            Self::Handoff { .. } => received_at.saturating_add(HANDOFF_REVIEW_SECS),
        }
    }
}

/// What accepting wrote, or that dismissing wrote nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProposalOutcome {
    /// A commitment on this server, for a meeting or a reminder.
    Commitment { commitment_id: String },
    /// A continuity record on this server, for a handoff.
    Continuity { record_id: String },
    /// Nothing was written.
    Dismissed {},
}

/// One proposal as `federation/proposals.json` keeps it and
/// `GET /api/federation/proposals` lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeerProposal {
    pub version: u32,
    /// 16 hex characters, this server's own id for the proposal.
    pub id: String,
    /// `companion_id` that proposed.
    pub sender: String,
    /// The pairing the peer belongs to, which a key rotation does not change.
    pub pairing_id: String,
    /// The intent's own id; with `sender` it identifies the request, so a
    /// request delivered twice is one proposal.
    pub correlation_id: String,
    /// The owner the sender said it speaks for: its own words.
    pub represented_owner: PeerLabel,
    /// Why the sender said it asked: its own words.
    pub purpose: PeerLabel,
    pub intent: IntentClass,
    pub details: ProposalDetails,
    pub status: ProposalStatus,
    /// The conversation message the proposal was delivered as.
    pub message_id: String,
    /// Unix seconds by this server's clock.
    pub received_at: u64,
    /// Unix seconds; past it the proposal cannot be accepted.
    pub expires_at: u64,
    /// When the owner decided, or the proposal was found lapsed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<ProposalOutcome>,
    /// The owner-side audit receipt of the decision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_id: Option<String>,
}

impl PeerProposal {
    /// Whether the proposal has lapsed at `now`, whatever its status says.
    pub fn lapsed_at(&self, now: u64) -> bool {
        self.expires_at <= now
    }

    /// The status as it reads at `now`: an open proposal past its deadline
    /// reads as expired.
    pub fn status_at(&self, now: u64) -> ProposalStatus {
        if self.status == ProposalStatus::Open && self.lapsed_at(now) {
            ProposalStatus::Expired
        } else {
            self.status
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::federation_policy::DisclosureClass;

    const HANDOFF: &str = include_str!("../../tests/fixtures/federation/intents/handoff_v1.json");

    fn text(value: &str) -> PeerText {
        serde_json::from_value(serde_json::json!(value)).unwrap()
    }

    fn proposal(details: ProposalDetails, received_at: u64) -> PeerProposal {
        let intent = details.class();
        PeerProposal {
            version: PROPOSAL_VERSION,
            id: "0123456789abcdef".into(),
            sender: "TFccHElqXR1lkUBqQoQPWYxPSm1wPjCl8WXbgm_cQ7E".into(),
            pairing_id: "pair".into(),
            correlation_id: "req-1".into(),
            represented_owner: PeerLabel::new("Alice".into()).unwrap(),
            purpose: PeerLabel::new("catch up".into()).unwrap(),
            intent,
            expires_at: details.review_deadline(received_at),
            details,
            status: ProposalStatus::Open,
            message_id: "msg-1".into(),
            received_at,
            decided_at: None,
            outcome: None,
            receipt_id: None,
        }
    }

    #[test]
    fn details_come_from_the_proposal_payloads_and_lapse_by_kind() {
        let window = TimeWindow {
            from: 1_000,
            to: 2_000,
        };
        let meeting = ProposalDetails::from_payload(&IntentPayload::Proposal {
            description: text("lunch"),
            window,
        })
        .unwrap();
        assert_eq!(meeting.class(), IntentClass::Proposal);
        assert_eq!(
            meeting.review_deadline(10),
            2_000,
            "a meeting lapses with its window"
        );
        let reminder = ProposalDetails::from_payload(&IntentPayload::Reminder {
            text: text("water"),
            at: 5_000,
        })
        .unwrap();
        assert_eq!(reminder.class(), IntentClass::Reminder);
        assert_eq!(
            reminder.review_deadline(10),
            5_000,
            "a reminder lapses at its time"
        );
        let intent = crate::domain::federation_intent::FederationIntent::decode(
            HANDOFF.as_bytes(),
            1_800_000_060,
        )
        .unwrap();
        let handoff = ProposalDetails::from_payload(&intent.intent).unwrap();
        assert_eq!(handoff.class(), IntentClass::Handoff);
        assert_eq!(handoff.review_deadline(10), 10 + HANDOFF_REVIEW_SECS);
        assert_eq!(
            ProposalDetails::from_payload(&IntentPayload::Message { body: text("hi") }),
            None
        );
        assert_eq!(
            ProposalDetails::from_payload(&IntentPayload::Availability { window }),
            None
        );
        let _ = DisclosureClass::None;
    }

    #[test]
    fn a_proposal_reads_as_expired_once_lapsed_and_round_trips_strictly() {
        let open = proposal(
            ProposalDetails::Reminder {
                text: text("water the plants"),
                at: 5_000,
            },
            100,
        );
        assert_eq!(open.status_at(4_999), ProposalStatus::Open);
        assert_eq!(open.status_at(5_000), ProposalStatus::Expired);
        let decided = PeerProposal {
            status: ProposalStatus::Accepted,
            decided_at: Some(200),
            outcome: Some(ProposalOutcome::Commitment {
                commitment_id: "cmt_1".into(),
            }),
            receipt_id: Some("fedcba9876543210".into()),
            ..open.clone()
        };
        assert_eq!(
            decided.status_at(9_000),
            ProposalStatus::Accepted,
            "a decision stands past the deadline"
        );
        let json = serde_json::to_string(&decided).unwrap();
        assert_eq!(
            serde_json::from_str::<PeerProposal>(&json).unwrap(),
            decided
        );
        assert!(
            json.contains("water the plants"),
            "the details travel to the owner"
        );
        let with_extra = json.replacen("{", "{\"note\":\"x\",", 1);
        assert!(serde_json::from_str::<PeerProposal>(&with_extra).is_err());
        let bad_kind = json.replace("\"kind\":\"reminder\"", "\"kind\":\"email\"");
        assert!(serde_json::from_str::<PeerProposal>(&bad_kind).is_err());
        // Nothing about the details formats through Debug.
        assert!(!format!("{decided:?}").contains("water the plants"));
    }
}
