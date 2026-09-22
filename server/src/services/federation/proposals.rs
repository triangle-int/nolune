//! What paired companions proposed and this owner has yet to decide
//! (#111): the store behind `GET /api/federation/proposals` and the two
//! decisions on it. A meeting, a reminder, or a task handoff a peer sent
//! and the policy admitted is delivered into the conversation as data and
//! recorded here as one [`PeerProposal`] with everything the owner needs
//! to decide (who asked, on whose behalf, why, the typed details, when it
//! lapses) and nothing that acts on its own: receiving a proposal writes
//! this record and the chat message, and nothing else, anywhere.
//!
//! Accepting and dismissing are the owner's and go through one lock, so
//! two decisions on one proposal are taken one after the other and the
//! second finds the first's outcome: accepting twice runs the one write
//! once and says so; dismissing twice is one dismissal; a dismissed or
//! lapsed proposal cannot be accepted, and an accepted one cannot be
//! dismissed. The write itself (a commitment or a continuity record on
//! this server, in `services::peer_proposals`) is a seam the caller hands
//! in, run exactly once, after the owner's decision is recorded as an
//! audit receipt on the owner side and before the outcome is saved. A
//! request delivered twice is one proposal: the sender's companion id and
//! correlation id identify it.
//!
//! `federation/proposals.json` (`0600`, written through a temporary file
//! and a rename) holds the records. The details are the peer's words and
//! travel to the owner's client as plain text for review; nothing here
//! formats them, and no field of the record could hold a file. Decided
//! and lapsed proposals are kept for a while and bounded; a file this
//! build cannot load is never repaired and never overwritten: every read
//! and write fails closed.

use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use super::{
    audit::AuditLog,
    gate::FederationGate,
    identity,
    pairing::FederationState,
    peers::{Clock, system_clock},
};
use crate::domain::{
    events::ServerEvent,
    federation::FederationError,
    federation_intent::PeerLabel,
    federation_policy::{Decision, DecisionReason},
    federation_proposal::{
        PROPOSAL_VERSION, PeerProposal, ProposalDetails, ProposalOutcome, ProposalStatus,
    },
};

/// The store, under the keystore directory.
pub const PROPOSALS_FILE: &str = "proposals.json";
/// Version of the store file.
pub const PROPOSALS_FILE_VERSION: u32 = 1;
/// Proposals kept; past it the oldest decided or lapsed ones go first,
/// then the oldest open ones.
pub const MAX_PEER_PROPOSALS: usize = 200;
/// A decided or lapsed proposal is dropped this long after its decision
/// or its deadline.
pub const PROPOSAL_RETENTION_SECS: u64 = 30 * 24 * 60 * 60;

/// Upper bound for the store file; anything larger is not ours.
const MAX_PROPOSALS_FILE_BYTES: u64 = 8 * 1024 * 1024;

/// One proposal as the delivery hands it in: the sender and its pairing,
/// the request's id, the peer's two labels, the typed details, and the
/// chat message it was delivered as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Received {
    pub sender: String,
    pub pairing_id: String,
    pub correlation_id: String,
    pub represented_owner: PeerLabel,
    pub purpose: PeerLabel,
    pub details: ProposalDetails,
    pub message_id: String,
}

/// What accepting left: the proposal as it now stands and whether it had
/// been accepted before (in which case nothing was written this time).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Accepted {
    pub proposal: PeerProposal,
    pub already_accepted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposalsFile {
    version: u32,
    proposals: Vec<PeerProposal>,
}

#[derive(Deserialize)]
struct FileVersion {
    version: u32,
}

#[derive(Default)]
struct Inner {
    loaded: bool,
    unloadable: Option<String>,
    proposals: Vec<PeerProposal>,
}

/// The proposals from peers, over `federation/proposals.json`.
pub struct ProposalStore {
    root: PathBuf,
    inner: Mutex<Inner>,
    /// Held across a decision, from the lookup to the saved outcome, so
    /// two decisions on one proposal never interleave and the one write
    /// an acceptance runs is run once. Async because the write awaits.
    decisions: tokio::sync::Mutex<()>,
    clock: Clock,
    /// Every change to a proposal is broadcast under this slug.
    events: Option<(broadcast::Sender<ServerEvent>, String)>,
}

impl ProposalStore {
    /// A store over `workspace_root/federation/proposals.json` on the
    /// system clock. Nothing is read until the first access.
    pub fn new(workspace_root: &Path) -> Self {
        Self::with_clock(workspace_root, system_clock())
    }

    pub(crate) fn with_clock(workspace_root: &Path, clock: Clock) -> Self {
        Self {
            root: workspace_root.to_path_buf(),
            inner: Mutex::new(Inner::default()),
            decisions: tokio::sync::Mutex::new(()),
            clock,
            events: None,
        }
    }

    /// Broadcast every change as `peer_proposal_updated` under `slug`.
    pub fn with_events(mut self, events: broadcast::Sender<ServerEvent>, slug: &str) -> Self {
        self.events = Some((events, slug.to_owned()));
        self
    }

    /// `workspace/federation/proposals.json`
    pub fn path(&self) -> PathBuf {
        identity::federation_dir(&self.root).join(PROPOSALS_FILE)
    }

    /// Unix seconds by this store's clock.
    pub fn now(&self) -> u64 {
        (self.clock)()
    }

    /// Every kept proposal, newest first, each read as it stands now: an
    /// open one past its deadline reads as expired.
    pub fn list(&self) -> Result<Vec<PeerProposal>, FederationError> {
        let now = self.now();
        let mut inner = self.inner.lock().unwrap();
        self.ensure_loaded(&mut inner);
        self.refuse_if_unloadable(&inner)?;
        self.prune(&mut inner)?;
        let mut proposals: Vec<PeerProposal> = inner
            .proposals
            .iter()
            .cloned()
            .map(|proposal| as_of(proposal, now))
            .collect();
        proposals.reverse();
        Ok(proposals)
    }

    /// The proposal `id`, as it stands now.
    pub fn get(&self, id: &str) -> Result<Option<PeerProposal>, FederationError> {
        let now = self.now();
        let mut inner = self.inner.lock().unwrap();
        self.ensure_loaded(&mut inner);
        self.refuse_if_unloadable(&inner)?;
        Ok(inner
            .proposals
            .iter()
            .find(|proposal| proposal.id == id)
            .cloned()
            .map(|proposal| as_of(proposal, now)))
    }

    /// Records `received` as an open proposal and broadcasts it; a
    /// request already recorded (same sender and correlation id) is
    /// returned as it stands and recorded again by nothing.
    pub fn receive(&self, received: Received) -> Result<PeerProposal, FederationError> {
        let now = self.now();
        let mut inner = self.inner.lock().unwrap();
        self.ensure_loaded(&mut inner);
        self.refuse_if_unloadable(&inner)?;
        if let Some(existing) = inner.proposals.iter().find(|proposal| {
            proposal.sender == received.sender && proposal.correlation_id == received.correlation_id
        }) {
            return Ok(as_of(existing.clone(), now));
        }
        let proposal = PeerProposal {
            version: PROPOSAL_VERSION,
            id: AuditLog::new_id(),
            sender: received.sender,
            pairing_id: received.pairing_id,
            correlation_id: received.correlation_id,
            represented_owner: received.represented_owner,
            purpose: received.purpose,
            intent: received.details.class(),
            expires_at: received.details.review_deadline(now),
            details: received.details,
            status: ProposalStatus::Open,
            message_id: received.message_id,
            received_at: now,
            decided_at: None,
            outcome: None,
            receipt_id: None,
        };
        self.store(&mut inner, proposal.clone())?;
        drop(inner);
        self.broadcast(proposal.clone());
        Ok(proposal)
    }

    /// The owner accepts `id`: under the decision lock, an open and
    /// unexpired proposal gets the owner's approval recorded as an audit
    /// receipt, then `write` is run once for the one record it creates on
    /// this server, then the outcome is saved and broadcast. One already
    /// accepted is returned with `already_accepted` and nothing is run;
    /// one dismissed or lapsed is refused as not open.
    pub async fn accept(
        &self,
        federation: &FederationState,
        gate: &FederationGate,
        id: &str,
        write: impl AsyncFnOnce(&PeerProposal) -> Result<ProposalOutcome, FederationError>,
    ) -> Result<Accepted, FederationError> {
        let _one_decision = self.decisions.lock().await;
        let now = self.now();
        let proposal = self.get(id)?.ok_or(FederationError::UnknownProposal)?;
        match proposal.status {
            ProposalStatus::Open => {}
            ProposalStatus::Accepted => {
                return Ok(Accepted {
                    proposal,
                    already_accepted: true,
                });
            }
            status @ (ProposalStatus::Dismissed | ProposalStatus::Expired) => {
                return Err(FederationError::ProposalNotOpen { status });
            }
        }
        let me = federation.identity()?.companion_id().to_owned();
        let receipt_id = gate.record_owner_decision(
            &proposal.pairing_id,
            &proposal.sender,
            &me,
            proposal.intent,
            &Decision::allow(DecisionReason::OwnerApproved),
        )?;
        let outcome = write(&proposal).await?;
        let decided = PeerProposal {
            status: ProposalStatus::Accepted,
            decided_at: Some(now),
            outcome: Some(outcome),
            receipt_id: Some(receipt_id),
            ..proposal
        };
        self.save(decided.clone())?;
        Ok(Accepted {
            proposal: decided,
            already_accepted: false,
        })
    }

    /// The owner dismisses `id`: under the decision lock, an open or
    /// lapsed proposal gets the owner's denial recorded as an audit
    /// receipt and is saved as dismissed; nothing else is written. One
    /// already dismissed is returned as it stands; an accepted one is
    /// refused as not open.
    pub async fn dismiss(
        &self,
        federation: &FederationState,
        gate: &FederationGate,
        id: &str,
    ) -> Result<PeerProposal, FederationError> {
        let _one_decision = self.decisions.lock().await;
        let now = self.now();
        let proposal = self.get(id)?.ok_or(FederationError::UnknownProposal)?;
        match proposal.status {
            ProposalStatus::Open | ProposalStatus::Expired => {}
            ProposalStatus::Dismissed => return Ok(proposal),
            status @ ProposalStatus::Accepted => {
                return Err(FederationError::ProposalNotOpen { status });
            }
        }
        let me = federation.identity()?.companion_id().to_owned();
        let receipt_id = gate.record_owner_decision(
            &proposal.pairing_id,
            &proposal.sender,
            &me,
            proposal.intent,
            &Decision::deny(DecisionReason::OwnerDenied),
        )?;
        let decided = PeerProposal {
            status: ProposalStatus::Dismissed,
            decided_at: Some(now),
            outcome: Some(ProposalOutcome::Dismissed {}),
            receipt_id: Some(receipt_id),
            ..proposal
        };
        self.save(decided.clone())?;
        Ok(decided)
    }

    /// Writes `proposal` in place of the one with its id, enforces
    /// retention, and broadcasts it.
    fn save(&self, proposal: PeerProposal) -> Result<(), FederationError> {
        let mut inner = self.inner.lock().unwrap();
        self.ensure_loaded(&mut inner);
        self.refuse_if_unloadable(&inner)?;
        self.store(&mut inner, proposal.clone())?;
        drop(inner);
        self.broadcast(proposal);
        Ok(())
    }

    /// `proposal` replaces the entry with its id, retention is enforced,
    /// and the file is written; the caller holds `inner`.
    fn store(&self, inner: &mut Inner, proposal: PeerProposal) -> Result<(), FederationError> {
        let now = self.now();
        let mut proposals: Vec<PeerProposal> = inner
            .proposals
            .iter()
            .filter(|kept| kept.id != proposal.id)
            .cloned()
            .collect();
        proposals.push(proposal);
        enforce_retention(&mut proposals, now);
        self.persist(&proposals)?;
        inner.proposals = proposals;
        Ok(())
    }

    fn broadcast(&self, proposal: PeerProposal) {
        if let Some((events, slug)) = &self.events {
            let _ = events.send(ServerEvent::PeerProposalUpdated {
                instance_slug: slug.clone(),
                proposal,
            });
        }
    }

    /// Drops what retention says goes from memory and, when anything
    /// went, from the file.
    fn prune(&self, inner: &mut Inner) -> Result<(), FederationError> {
        let now = self.now();
        let mut proposals = inner.proposals.clone();
        if !enforce_retention(&mut proposals, now) {
            return Ok(());
        }
        self.persist(&proposals)?;
        inner.proposals = proposals;
        Ok(())
    }

    /// Reads the file once. A missing file is an empty store. A file that
    /// cannot be read, is not a store of this version, or has a record of
    /// another version leaves the store marked unloadable: reported, never
    /// repaired, never overwritten.
    fn ensure_loaded(&self, inner: &mut Inner) {
        if inner.loaded {
            return;
        }
        inner.loaded = true;
        let path = self.path();
        let contents = match std::fs::metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => {
                return self.mark_unloadable(inner, format!("cannot be read ({error})"));
            }
            Ok(metadata) if metadata.len() > MAX_PROPOSALS_FILE_BYTES => {
                return self.mark_unloadable(inner, "is larger than the store can be".to_owned());
            }
            Ok(_) => match std::fs::read_to_string(&path) {
                Ok(contents) => contents,
                Err(error) => {
                    return self.mark_unloadable(inner, format!("cannot be read ({error})"));
                }
            },
        };
        let version = serde_json::from_str::<FileVersion>(&contents)
            .ok()
            .map(|file| file.version);
        match serde_json::from_str::<ProposalsFile>(&contents) {
            Ok(file)
                if file.version == PROPOSALS_FILE_VERSION
                    && file
                        .proposals
                        .iter()
                        .all(|proposal| proposal.version == PROPOSAL_VERSION) =>
            {
                inner.proposals = file.proposals;
            }
            _ => {
                let reason = match version {
                    Some(version) if version != PROPOSALS_FILE_VERSION => {
                        format!("has unsupported version {version}")
                    }
                    _ => "does not have the expected shape".to_owned(),
                };
                self.mark_unloadable(inner, reason);
            }
        }
    }

    fn mark_unloadable(&self, inner: &mut Inner, reason: String) {
        log::warn!(
            "[federation] peer proposal store {} {reason}; no proposal will be received or decided and nothing will be written until it is repaired or moved aside and the server restarted",
            self.path().display()
        );
        inner.unloadable = Some(reason);
    }

    fn persist(&self, proposals: &[PeerProposal]) -> Result<(), FederationError> {
        let path = self.path();
        let file = ProposalsFile {
            version: PROPOSALS_FILE_VERSION,
            proposals: proposals.to_vec(),
        };
        let mut json =
            serde_json::to_string_pretty(&file).map_err(|error| FederationError::Io {
                path: path.clone(),
                message: error.to_string(),
            })?;
        json.push('\n');
        identity::replace_private(&path, json.as_bytes()).map_err(|error| FederationError::Io {
            path,
            message: error.to_string(),
        })
    }

    fn refuse_if_unloadable(&self, inner: &Inner) -> Result<(), FederationError> {
        match &inner.unloadable {
            Some(reason) => Err(FederationError::Io {
                path: self.path(),
                message: format!(
                    "federation peer proposal store {reason}; repair or move it aside and restart before federation is used"
                ),
            }),
            None => Ok(()),
        }
    }
}

/// `proposal` as it stands at `now`: an open one past its deadline reads
/// as expired.
fn as_of(mut proposal: PeerProposal, now: u64) -> PeerProposal {
    proposal.status = proposal.status_at(now);
    proposal
}

/// The moment a proposal stops being worth keeping from: its decision,
/// or its deadline when it was never decided.
fn settled_at(proposal: &PeerProposal) -> u64 {
    proposal.decided_at.unwrap_or(proposal.expires_at)
}

/// Drops what retention says goes: a proposal decided or lapsed more than
/// [`PROPOSAL_RETENTION_SECS`] ago, and past [`MAX_PEER_PROPOSALS`] the
/// oldest settled ones, then the oldest open ones. The list is kept
/// oldest first. Returns whether anything went.
fn enforce_retention(proposals: &mut Vec<PeerProposal>, now: u64) -> bool {
    let before = proposals.len();
    let oldest_allowed = now.saturating_sub(PROPOSAL_RETENTION_SECS);
    proposals.retain(|proposal| {
        proposal.status_at(now) == ProposalStatus::Open || settled_at(proposal) > oldest_allowed
    });
    while proposals.len() > MAX_PEER_PROPOSALS {
        let settled = proposals
            .iter()
            .position(|proposal| proposal.status_at(now) != ProposalStatus::Open)
            .unwrap_or(0);
        proposals.remove(settled);
    }
    proposals.len() != before
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        federation_intent::TimeWindow,
        federation_policy::{IntentClass, PeerText},
    };
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };

    const T0: u64 = 1_800_000_000;
    const SENDER: &str = "TFccHElqXR1lkUBqQoQPWYxPSm1wPjCl8WXbgm_cQ7E";

    fn open(root: &Path, now: &Arc<AtomicU64>) -> ProposalStore {
        let read = now.clone();
        ProposalStore::with_clock(root, Arc::new(move || read.load(Ordering::SeqCst)))
    }

    fn text(value: &str) -> PeerText {
        serde_json::from_value(serde_json::json!(value)).unwrap()
    }

    fn received(correlation_id: &str, at: u64) -> Received {
        Received {
            sender: SENDER.into(),
            pairing_id: "pair".into(),
            correlation_id: correlation_id.into(),
            represented_owner: PeerLabel::new("Alice".into()).unwrap(),
            purpose: PeerLabel::new("catch up".into()).unwrap(),
            details: ProposalDetails::Reminder {
                text: text("water the plants"),
                at,
            },
            message_id: format!("msg-{correlation_id}"),
        }
    }

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn nothing_is_written_until_a_proposal_is_received_and_a_redelivery_is_one_proposal() {
        let dir = tempfile::tempdir().unwrap();
        let now = Arc::new(AtomicU64::new(T0));
        let store = open(dir.path(), &now);
        assert!(store.list().unwrap().is_empty());
        assert!(!store.path().exists(), "listing writes nothing");
        let first = store.receive(received("req-1", T0 + 3_600)).unwrap();
        assert_eq!(first.status, ProposalStatus::Open);
        assert_eq!(first.intent, IntentClass::Reminder);
        assert_eq!(
            first.expires_at,
            T0 + 3_600,
            "a reminder lapses at its time"
        );
        assert_eq!(first.received_at, T0);
        assert_eq!(first.id.len(), 16);
        #[cfg(unix)]
        assert_eq!(mode(&store.path()), 0o600);
        now.store(T0 + 10, Ordering::SeqCst);
        let again = store.receive(received("req-1", T0 + 3_600)).unwrap();
        assert_eq!(again, first, "the same request is the same proposal");
        assert_eq!(store.list().unwrap().len(), 1);
        // Newest first, and read back after a restart.
        let second = store.receive(received("req-2", T0 + 7_200)).unwrap();
        let listed = store.list().unwrap();
        assert_eq!(
            listed.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            [second.id.as_str(), first.id.as_str()]
        );
        let reopened = open(dir.path(), &now);
        assert_eq!(reopened.list().unwrap(), listed);
        assert_eq!(reopened.get(&first.id).unwrap(), Some(first.clone()));
        assert_eq!(reopened.get("0000000000000000").unwrap(), None);
        // Past its deadline an open proposal reads as expired, in the
        // listing and by id, without a write.
        now.store(T0 + 3_600, Ordering::SeqCst);
        assert_eq!(
            reopened.get(&first.id).unwrap().unwrap().status,
            ProposalStatus::Expired
        );
        assert_eq!(reopened.list().unwrap()[1].status, ProposalStatus::Expired);
        assert_eq!(reopened.list().unwrap()[0].status, ProposalStatus::Open);
    }

    #[test]
    fn settled_proposals_lapse_and_the_list_is_bounded() {
        let now = T0;
        let mut proposals: Vec<PeerProposal> = (0..(MAX_PEER_PROPOSALS + 5))
            .map(|index| {
                let received_at = T0 - 10_000 + index as u64;
                PeerProposal {
                    version: PROPOSAL_VERSION,
                    id: format!("{index:016x}"),
                    sender: SENDER.into(),
                    pairing_id: "pair".into(),
                    correlation_id: format!("req-{index}"),
                    represented_owner: PeerLabel::new("Alice".into()).unwrap(),
                    purpose: PeerLabel::new("catch up".into()).unwrap(),
                    intent: IntentClass::Proposal,
                    details: ProposalDetails::Meeting {
                        description: text("lunch"),
                        window: TimeWindow {
                            from: T0 + 100,
                            to: T0 + 200,
                        },
                    },
                    // Every other one is dismissed already.
                    status: if index % 2 == 0 {
                        ProposalStatus::Dismissed
                    } else {
                        ProposalStatus::Open
                    },
                    message_id: "msg".into(),
                    received_at,
                    expires_at: T0 + 200,
                    decided_at: (index % 2 == 0).then_some(received_at),
                    outcome: (index % 2 == 0).then_some(ProposalOutcome::Dismissed {}),
                    receipt_id: None,
                }
            })
            .collect();
        assert!(enforce_retention(&mut proposals, now));
        assert_eq!(proposals.len(), MAX_PEER_PROPOSALS);
        assert_eq!(
            proposals
                .iter()
                .filter(|p| p.status == ProposalStatus::Open)
                .count(),
            (MAX_PEER_PROPOSALS + 5) / 2,
            "settled proposals go before open ones"
        );
        assert_eq!(
            proposals[0].id, "0000000000000001",
            "the oldest settled ones went"
        );
        // Long after, the decided ones are gone and the open ones lapse too.
        let mut later = proposals.clone();
        assert!(enforce_retention(
            &mut later,
            T0 + 200 + PROPOSAL_RETENTION_SECS + 1
        ));
        assert!(later.is_empty());
        let mut kept = proposals.clone();
        assert!(
            !enforce_retention(&mut kept, T0 + 1),
            "nothing more goes at once"
        );
    }

    #[test]
    fn an_unloadable_store_fails_every_operation_closed_and_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let now = Arc::new(AtomicU64::new(T0));
        let store = open(dir.path(), &now);
        std::fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        std::fs::write(store.path(), "{\"version\": 7}\n").unwrap();
        assert!(matches!(store.list(), Err(FederationError::Io { .. })));
        assert!(matches!(
            store.receive(received("req-1", T0 + 60)),
            Err(FederationError::Io { .. })
        ));
        assert!(matches!(store.get("x"), Err(FederationError::Io { .. })));
        assert_eq!(
            std::fs::read_to_string(store.path()).unwrap(),
            "{\"version\": 7}\n",
            "never overwritten"
        );
    }
}
