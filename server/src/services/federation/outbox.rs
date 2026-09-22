//! Outbound structured intents (#110, PR 3): the persisted outbox that
//! carries what this companion asks a paired peer for, the sender loop
//! that delivers it with bounded retries, and the receipts of what was
//! asked and what the peer disclosed.
//!
//! The chat tools in `services::tools::federation` and
//! `services::tools::peer_proposals` build a typed [`Outgoing`] request (a
//! message, an availability query, a reminder, a meeting proposal, or a
//! task handoff built here from one of this owner's continuity records by
//! [`super::handoffs`]), and the proposal store (`super::proposals`) builds
//! one for the decision this owner made on a peer's proposal (#111), and
//! hand it to [`Outbox::enqueue`], which resolves the peer, asks this
//! owner's own policy gate ([`FederationGate::admit_outbound`]: the peer
//! must be paired, the intent must be able to disclose at that class, and
//! no live rule the owner wrote may deny the pair), builds the
//! [`FederationIntent`] with a fresh correlation id, this companion as the
//! sender, and a lifetime of [`OUTBOX_INTENT_LIFETIME_SECS`], validates it,
//! and persists one [`OutboxEntry`]. A tool call for a peer that is
//! unknown, pending, revoked, or denied is refused before anything is
//! queued.
//!
//! The sender loop ([`Outbox::start`], one pass at a time through
//! [`Outbox::run_due`]) seals each due intent for its peer, posts it to
//! the peer's approved origins at the intent route, opens the sealed
//! answer, decodes the typed response fail-closed, and checks it against
//! the intent it answers. `accepted` and `denied` settle the entry;
//! `needs_owner` leaves it waiting and asks again every
//! [`OWNER_RETRY_SECS`] until the peer's owner decides or the intent
//! expires. A peer that could not be reached, that answered something that
//! did not verify or decode, that refused with a transient code, that
//! answered `denied` with its rate limit (the entry waits out the larger
//! of the peer's window and the backoff), or in front of which something
//! else answered with an error status (a server error, `429`, or a status
//! whose body names no code: a proxy's own page, never a verdict on the
//! request) is tried again after [`backoff_secs`] (doubling from
//! [`RETRY_BASE_SECS`], capped at [`RETRY_MAX_SECS`]); after
//! [`MAX_DELIVERY_ATTEMPTS`] such failures, about nine hours of trying,
//! the entry is visibly `failed`. A `4xx` from the peer's route that
//! names a lasting condition fails it at once. An intent that expires
//! before it was delivered is `expired`. Every change to an entry is
//! broadcast as `ServerEvent::OutboxUpdated`, so the activity page shows
//! where each request stands.
//!
//! Delivery is idempotent end to end: the correlation id never changes
//! across retries or a restart, and the receiving side answers a request it
//! already settled with the same response again ([`super::inbound`]).
//! An attempt is written as in flight before the envelope leaves, so a
//! process that dies mid-delivery leaves a mark; [`Outbox::recover_on_restart`]
//! turns it into `interrupted` and the entry is retried on the same id,
//! never queued a second time.
//!
//! A peer's decision on a reminder, a meeting, or a handoff this companion
//! sent arrives later as its own `decision` intent, and the delivery notes
//! it on the delivered entry it answers through [`Outbox::note_decision`]
//! (#111): the entry keeps a [`PeerDecision`], its status stays
//! `delivered`, and a decision naming no such entry is refused, typed.
//!
//! Every settled outcome, and the first `needs_owner`, writes a
//! requesting-side audit receipt through the gate and an [`IntentReceipt`]
//! naming what was requested and what the peer disclosed, read off the
//! typed response only; a request that lapsed unanswered is recorded as
//! denied, `unreachable`, and one that lapsed after the peer did answer
//! (its owner never decided, its rate limit never lifted) with the peer's
//! last reason. `federation/outbox.json` (`0600`, written through
//! a temporary file and a rename) holds the entries and the receipts. An
//! entry carries the intent it sends (this owner's own words, which the
//! sender needs again for every retry), its attempt history, and the
//! peer's typed response, never anything else the peer said. Settled
//! entries and receipts are bounded and lapse; a file this build cannot
//! load is never repaired and never overwritten: every read and write
//! fails closed.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tokio::sync::{Notify, broadcast};
use tokio_util::sync::CancellationToken;

use super::{
    audit::AuditLog,
    gate::FederationGate,
    handoffs, identity,
    inbound::INTENT_PATH,
    pairing::{FederationState, HttpTransport, Overview, PeerTransport},
    peers::{Clock, system_clock},
};
use crate::domain::{
    continuity::ContinuityRecord,
    events::ServerEvent,
    federation::{FederationError, PeerState, PeerSummary},
    federation_intent::{
        FederationIntent, INTENT_VERSION, IntentError, IntentOutcome, IntentPayload, IntentReceipt,
        IntentResponse, LabelField, MAX_INTENT_CLOCK_SKEW_SECS, MAX_INTENT_LIFETIME_SECS,
        PeerLabel, ProposalDecision, ReceiptBasis, TimeWindow,
    },
    federation_policy::{
        Decision, DecisionReason, DisclosureClass, IntentRequest, PeerText, ReceiptSide, Verdict,
    },
};

/// The store, under the keystore directory.
pub const OUTBOX_FILE: &str = "outbox.json";
/// Version of the store file and of every entry in it.
pub const OUTBOX_VERSION: u32 = 1;
/// How long a queued intent stands: a day. The retry ladder below (about
/// nine hours of trying) fits inside it, so an outage or a peer asleep
/// for a night is waited out, and a request the peer's owner has to
/// answer is asked again for the rest of the day, but not forever. Below
/// the decoder's week-long maximum, which the line after it pins at
/// compile time.
pub const OUTBOX_INTENT_LIFETIME_SECS: u64 = 24 * 60 * 60;
const _: () = assert!(OUTBOX_INTENT_LIFETIME_SECS <= MAX_INTENT_LIFETIME_SECS);
/// Failed attempts (unreachable, undecodable, transiently refused, or
/// answered with the peer's rate limit) before an entry is given up on
/// and shown as failed. With [`RETRY_BASE_SECS`] doubling to
/// [`RETRY_MAX_SECS`], the waits before the last attempt add up to about
/// nine hours: a peer that is away for a night is reached in the morning,
/// and a failure is visible the same day.
pub const MAX_DELIVERY_ATTEMPTS: u32 = 16;
/// Wait after the first failed attempt; doubled after each one after it.
pub const RETRY_BASE_SECS: u64 = 30;
/// Longest wait between two attempts.
pub const RETRY_MAX_SECS: u64 = 60 * 60;
/// How often a request the peer's owner has to answer is asked again.
pub const OWNER_RETRY_SECS: u64 = 15 * 60;
/// Entries kept; past it the oldest go.
pub const MAX_OUTBOX_ENTRIES: usize = 1000;
/// Settled entries and receipts older than this are dropped.
pub const OUTBOX_RETENTION_SECS: u64 = 30 * 24 * 60 * 60;
/// Receipts kept overall.
pub const MAX_OUTBOX_RECEIPTS: usize = 1000;
/// Receipts kept per pairing, so one peer's traffic only ever pushes out
/// its own history.
pub const MAX_OUTBOX_RECEIPTS_PER_PAIRING: usize = 200;
/// Shortest prefix of a companion id a tool may name a peer by.
pub const MIN_PEER_PREFIX_CHARS: usize = 8;

/// Upper bound for the store file; anything larger is not ours.
const MAX_OUTBOX_FILE_BYTES: u64 = 8 * 1024 * 1024;
/// The loop looks again after this long when nothing is due.
const IDLE_POLL_SECS: u64 = 60;
/// Random bytes behind a correlation id, as hex.
const CORRELATION_ID_BYTES: usize = 16;

/// Where an outgoing request stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutboxStatus {
    /// Not delivered yet; tried again at `next_attempt_at`.
    Queued,
    /// The peer answered `needs_owner`; asked again at `next_attempt_at`.
    WaitingOwner,
    /// The peer answered `accepted`.
    Delivered,
    /// The peer answered `denied`.
    Denied,
    /// Given up on: too many failed attempts, or a refusal that will not
    /// change.
    Failed,
    /// The intent expired before it was delivered.
    Expired,
}

impl OutboxStatus {
    /// Whether nothing more happens to an entry in this state.
    pub fn is_settled(self) -> bool {
        matches!(
            self,
            Self::Delivered | Self::Denied | Self::Failed | Self::Expired
        )
    }
}

/// How one attempt ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AttemptOutcome {
    /// Written before the envelope leaves; replaced when the answer is in.
    InFlight {},
    /// The process died while this attempt was in flight.
    Interrupted {},
    /// No approved origin answered.
    Unreachable {},
    /// An error status came back: from the peer's route with a typed
    /// `code`, or from whatever answers in front of the peer (a proxy's
    /// or a tunnel's own error page carries no code and is `unknown`).
    /// `status` is absent for a refusal this side settled on because the
    /// peer is gone or no longer paired.
    Refused {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<u16>,
        code: String,
    },
    /// The peer's answer did not verify or decode as a response to this
    /// intent.
    Malformed {},
    /// The peer answered with a typed response.
    Answered {
        outcome: IntentOutcome,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<DecisionReason>,
    },
}

impl AttemptOutcome {
    /// The outcome's wire name, for a log line.
    pub fn name(&self) -> &'static str {
        match self {
            Self::InFlight {} => "in_flight",
            Self::Interrupted {} => "interrupted",
            Self::Unreachable {} => "unreachable",
            Self::Refused { .. } => "refused",
            Self::Malformed {} => "malformed",
            Self::Answered { .. } => "answered",
        }
    }
}

/// One delivery attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attempt {
    /// Unix seconds by this server's clock.
    pub at: u64,
    pub outcome: AttemptOutcome,
}

/// One request this companion sends a peer, as the store keeps it: the
/// intent itself (which every retry sends again, so it has to be here),
/// where it stands, every attempt, and the peer's typed response once
/// there is one. Nothing the peer said beyond that response is kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutboxEntry {
    pub version: u32,
    /// `companion_id` of the peer when the request was queued; the entry
    /// follows the pairing if the peer rotates its key.
    pub recipient: String,
    pub pairing_id: String,
    /// The intent as sent: its `correlation_id` is the entry's key and its
    /// `expires_at` is the entry's deadline.
    pub intent: FederationIntent,
    pub status: OutboxStatus,
    pub attempts: Vec<Attempt>,
    /// When the next attempt is due; `None` once settled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_attempt_at: Option<u64>,
    /// The peer's last typed response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<IntentResponse>,
    /// The latest receipt written for this request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_id: Option<String>,
    /// The conversation the request was made in.
    pub chat_id: String,
    pub created_at: u64,
    pub updated_at: u64,
    /// What the peer's owner decided on a delivered reminder, proposal, or
    /// handoff, once their companion said (#111); absent until then, and
    /// for every other kind of request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<PeerDecision>,
}

/// The peer owner's decision as noted on the entry: what they decided and
/// when this server was told (Unix seconds by this server's clock).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeerDecision {
    pub decision: ProposalDecision,
    pub at: u64,
}

impl OutboxEntry {
    pub fn correlation_id(&self) -> &str {
        &self.intent.correlation_id
    }

    /// Attempts that count towards giving up: everything but the in-flight
    /// mark and an answer, except that an answer of `denied, rate_limited`
    /// counts too (the peer was reached but took nothing, and a peer over
    /// its budget for good has to bound the entry like one that is away).
    pub fn failures(&self) -> u32 {
        self.attempts
            .iter()
            .filter(|attempt| match &attempt.outcome {
                AttemptOutcome::InFlight {} => false,
                AttemptOutcome::Answered { reason, .. } => {
                    *reason == Some(DecisionReason::RateLimited)
                }
                _ => true,
            })
            .count() as u32
    }
}

/// What a tool asks for, typed per intent class; the outbox turns it into
/// the wire intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutboxRequest {
    Message {
        text: String,
    },
    /// Unix seconds, `from` before `to`.
    Availability {
        from: u64,
        to: u64,
    },
    /// Unix seconds.
    Reminder {
        text: String,
        at: u64,
    },
    /// A meeting inside a window (#111), Unix seconds, `from` before `to`.
    Proposal {
        description: String,
        from: u64,
        to: u64,
    },
    /// One of this owner's unfinished tasks (#111), handed over as the
    /// bounded references and provenance [`handoffs::handoff_from_record`]
    /// reads off the record, never its contents.
    Handoff {
        record: Box<ContinuityRecord>,
    },
    /// This owner's decision on a proposal the peer sent (#111):
    /// `correlation_id` is the peer's own id for that request.
    Decision {
        correlation_id: String,
        decision: ProposalDecision,
    },
}

/// One request to queue: the peer (a companion id, or its first
/// [`MIN_PEER_PREFIX_CHARS`] or more characters when unique), the two
/// labels this companion declares about itself, the typed request, and
/// the conversation it was made in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    pub peer: String,
    pub represented_owner: String,
    pub purpose: String,
    pub request: OutboxRequest,
    pub chat_id: String,
}

/// What the owner listing shows: every entry and every kept receipt,
/// newest first.
#[derive(Debug, Clone, Serialize)]
pub struct OutboxView {
    pub entries: Vec<OutboxEntry>,
    pub receipts: Vec<IntentReceipt>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OutboxFile {
    version: u32,
    entries: Vec<OutboxEntry>,
    receipts: Vec<IntentReceipt>,
}

#[derive(Deserialize)]
struct FileVersion {
    version: u32,
}

#[derive(Default)]
struct Inner {
    loaded: bool,
    unloadable: Option<String>,
    entries: Vec<OutboxEntry>,
    receipts: Vec<IntentReceipt>,
}

/// The persisted outbox and its sender loop, over `federation/outbox.json`.
pub struct Outbox {
    root: PathBuf,
    inner: Mutex<Inner>,
    transport: Arc<dyn PeerTransport>,
    clock: Clock,
    /// Every change to an entry is broadcast under this slug.
    events: Option<(broadcast::Sender<ServerEvent>, String)>,
    /// Woken by `enqueue`, so a fresh request goes out at once.
    wake: Notify,
    /// The running loop, until `shutdown`.
    running: Mutex<Option<(CancellationToken, tokio::task::JoinHandle<()>)>>,
    /// One delivery pass at a time, whoever drives it.
    passes: tokio::sync::Mutex<()>,
}

impl Outbox {
    /// An outbox over `workspace_root/federation/outbox.json` on the system
    /// clock, reaching peers through `client`. Nothing is read until the
    /// first access.
    pub fn new(workspace_root: &Path, client: reqwest::Client) -> Self {
        Self::with_transport_and_clock(
            workspace_root,
            Arc::new(HttpTransport::new(client)),
            system_clock(),
        )
    }

    pub(crate) fn with_transport_and_clock(
        workspace_root: &Path,
        transport: Arc<dyn PeerTransport>,
        clock: Clock,
    ) -> Self {
        Self {
            root: workspace_root.to_path_buf(),
            inner: Mutex::new(Inner::default()),
            transport,
            clock,
            events: None,
            wake: Notify::new(),
            running: Mutex::new(None),
            passes: tokio::sync::Mutex::new(()),
        }
    }

    /// Broadcast every entry change as `outbox_updated` under `slug`.
    pub fn with_events(mut self, events: broadcast::Sender<ServerEvent>, slug: &str) -> Self {
        self.events = Some((events, slug.to_owned()));
        self
    }

    /// `workspace/federation/outbox.json`
    pub fn path(&self) -> PathBuf {
        identity::federation_dir(&self.root).join(OUTBOX_FILE)
    }

    /// Unix seconds by this outbox's clock.
    pub fn now(&self) -> u64 {
        (self.clock)()
    }

    /// Queues `outgoing` for delivery: the peer is resolved, this owner's
    /// own policy is asked, the intent is built and validated, and the
    /// entry is persisted and broadcast. Refused, with nothing queued, for
    /// a peer that is unknown, pending, revoked, or denied by an owner
    /// rule, and for a label or a text the wire would not take.
    pub fn enqueue(
        &self,
        federation: &FederationState,
        gate: &FederationGate,
        outgoing: Outgoing,
    ) -> Result<OutboxEntry, FederationError> {
        let now = self.now();
        let overview = federation.overview()?;
        let named = resolve_peer(&overview, &outgoing.peer)?;
        let (disclosure, payload) = match outgoing.request {
            OutboxRequest::Message { text } => (
                DisclosureClass::None,
                IntentPayload::Message {
                    body: peer_text(text)?,
                },
            ),
            OutboxRequest::Availability { from, to } => (
                DisclosureClass::Availability,
                IntentPayload::Availability {
                    window: TimeWindow { from, to },
                },
            ),
            OutboxRequest::Reminder { text, at } => (
                DisclosureClass::None,
                IntentPayload::Reminder {
                    text: peer_text(text)?,
                    at,
                },
            ),
            OutboxRequest::Proposal {
                description,
                from,
                to,
            } => (
                DisclosureClass::None,
                IntentPayload::Proposal {
                    description: peer_text(description)?,
                    window: TimeWindow { from, to },
                },
            ),
            OutboxRequest::Handoff { record } => (
                DisclosureClass::None,
                IntentPayload::Handoff {
                    task: handoffs::handoff_from_record(&record)
                        .map_err(FederationError::Intent)?,
                },
            ),
            OutboxRequest::Decision {
                correlation_id,
                decision,
            } => (
                DisclosureClass::None,
                IntentPayload::Decision {
                    correlation_id,
                    decision,
                },
            ),
        };
        let request = IntentRequest::new(payload.class(), disclosure);
        let peer = gate.admit_outbound(federation, &named.companion_id, request)?;
        let intent = FederationIntent {
            version: INTENT_VERSION,
            correlation_id: new_correlation_id(),
            sender: overview.companion_id,
            represented_owner: label(outgoing.represented_owner, LabelField::RepresentedOwner)?,
            purpose: label(outgoing.purpose, LabelField::Purpose)?,
            disclosure,
            issued_at: now,
            expires_at: now.saturating_add(OUTBOX_INTENT_LIFETIME_SECS),
            intent: payload,
        };
        intent.validate(now).map_err(FederationError::Intent)?;
        let entry = OutboxEntry {
            version: OUTBOX_VERSION,
            recipient: peer.companion_id,
            pairing_id: peer.pairing_id,
            intent,
            status: OutboxStatus::Queued,
            attempts: Vec::new(),
            next_attempt_at: Some(now),
            response: None,
            receipt_id: None,
            chat_id: outgoing.chat_id,
            created_at: now,
            updated_at: now,
            decision: None,
        };
        self.save(entry.clone(), None)?;
        self.wake.notify_one();
        Ok(entry)
    }

    /// Notes what the peer under `pairing_id` decided on the request
    /// `correlation_id` (#111): the entry must be one this companion sent
    /// that pairing, delivered, and a reminder, a proposal, or a handoff
    /// (the kinds an owner decides on); anything else, and a decision that
    /// contradicts one already noted, is [`FederationError::UnknownRequest`]
    /// and notes nothing. The same decision told again is noted once. The
    /// entry stays `delivered`; the decision and when it was noted are
    /// kept on it, and the change is broadcast.
    pub fn note_decision(
        &self,
        pairing_id: &str,
        correlation_id: &str,
        decision: ProposalDecision,
    ) -> Result<OutboxEntry, FederationError> {
        let now = self.now();
        let mut entry = self
            .get(correlation_id)?
            .filter(|entry| entry.pairing_id == pairing_id)
            .filter(|entry| entry.status == OutboxStatus::Delivered)
            .filter(|entry| {
                matches!(
                    entry.intent.intent,
                    IntentPayload::Reminder { .. }
                        | IntentPayload::Proposal { .. }
                        | IntentPayload::Handoff { .. }
                )
            })
            .ok_or(FederationError::UnknownRequest)?;
        match entry.decision {
            Some(noted) if noted.decision == decision => return Ok(entry),
            Some(_) => return Err(FederationError::UnknownRequest),
            None => {}
        }
        entry.decision = Some(PeerDecision { decision, at: now });
        entry.updated_at = now;
        self.save(entry.clone(), None)?;
        Ok(entry)
    }

    /// Writes `entry` (replacing the entry with its correlation id, if
    /// any) and appends `receipt`, in one write, enforces retention, and
    /// broadcasts the entry. Fails closed over a file this build could not
    /// load or write.
    fn save(
        &self,
        entry: OutboxEntry,
        receipt: Option<IntentReceipt>,
    ) -> Result<(), FederationError> {
        let now = self.now();
        let mut inner = self.inner.lock().unwrap();
        self.ensure_loaded(&mut inner);
        self.refuse_if_unloadable(&inner)?;
        let mut entries: Vec<OutboxEntry> = inner
            .entries
            .iter()
            .filter(|kept| kept.correlation_id() != entry.correlation_id())
            .cloned()
            .collect();
        entries.push(entry.clone());
        let mut receipts = inner.receipts.clone();
        receipts.extend(receipt);
        enforce_retention(&mut entries, &mut receipts, now);
        self.persist(&entries, &receipts)?;
        inner.entries = entries;
        inner.receipts = receipts;
        drop(inner);
        self.broadcast(entry);
        Ok(())
    }

    /// Every entry and every kept receipt, newest first.
    pub fn view(&self) -> Result<OutboxView, FederationError> {
        let mut inner = self.inner.lock().unwrap();
        self.ensure_loaded(&mut inner);
        self.refuse_if_unloadable(&inner)?;
        self.prune(&mut inner)?;
        let mut entries = inner.entries.clone();
        entries.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| b.created_at.cmp(&a.created_at))
        });
        let mut receipts = inner.receipts.clone();
        receipts.reverse();
        Ok(OutboxView { entries, receipts })
    }

    /// The entry keyed by `correlation_id`, if any.
    pub fn get(&self, correlation_id: &str) -> Result<Option<OutboxEntry>, FederationError> {
        let mut inner = self.inner.lock().unwrap();
        self.ensure_loaded(&mut inner);
        self.refuse_if_unloadable(&inner)?;
        self.prune(&mut inner)?;
        Ok(inner
            .entries
            .iter()
            .find(|entry| entry.correlation_id() == correlation_id)
            .cloned())
    }

    /// When the earliest unsettled entry is due, if any.
    pub fn next_due(&self) -> Result<Option<u64>, FederationError> {
        let mut inner = self.inner.lock().unwrap();
        self.ensure_loaded(&mut inner);
        self.refuse_if_unloadable(&inner)?;
        Ok(inner
            .entries
            .iter()
            .filter(|entry| !entry.status.is_settled())
            .filter_map(|entry| entry.next_attempt_at)
            .min())
    }

    /// Attempts left in flight by a previous process can never finish:
    /// they are marked interrupted, and the entry is tried again on the
    /// same correlation id when it is due. Returns how many were marked.
    pub fn recover_on_restart(&self) -> Result<usize, FederationError> {
        let interrupted: Vec<OutboxEntry> = {
            let mut inner = self.inner.lock().unwrap();
            self.ensure_loaded(&mut inner);
            self.refuse_if_unloadable(&inner)?;
            inner
                .entries
                .iter()
                .filter(|entry| !entry.status.is_settled())
                .filter(|entry| {
                    entry
                        .attempts
                        .last()
                        .is_some_and(|attempt| attempt.outcome == AttemptOutcome::InFlight {})
                })
                .cloned()
                .collect()
        };
        let now = self.now();
        for mut entry in interrupted.iter().cloned() {
            if let Some(attempt) = entry.attempts.last_mut() {
                attempt.outcome = AttemptOutcome::Interrupted {};
            }
            entry.updated_at = now;
            self.save(entry, None)?;
        }
        Ok(interrupted.len())
    }

    /// One delivery pass: every unsettled entry that is due is attempted
    /// once, and every one whose intent expired is settled as such.
    /// Returns how many entries were touched.
    pub async fn run_due(
        &self,
        federation: &FederationState,
        gate: &FederationGate,
    ) -> Result<usize, FederationError> {
        let _pass = self.passes.lock().await;
        let now = self.now();
        let due: Vec<String> = {
            let mut inner = self.inner.lock().unwrap();
            self.ensure_loaded(&mut inner);
            self.refuse_if_unloadable(&inner)?;
            let mut due: Vec<&OutboxEntry> = inner
                .entries
                .iter()
                .filter(|entry| !entry.status.is_settled())
                .filter(|entry| {
                    expired(entry, now) || entry.next_attempt_at.is_some_and(|at| at <= now)
                })
                .collect();
            due.sort_by_key(|entry| (entry.next_attempt_at, entry.created_at));
            due.iter()
                .map(|entry| entry.correlation_id().to_owned())
                .collect()
        };
        if due.is_empty() {
            return Ok(0);
        }
        let me = federation.identity()?.companion_id().to_owned();
        for key in &due {
            self.attempt(federation, gate, &me, key).await?;
        }
        Ok(due.len())
    }

    /// Starts the sender loop: interrupted attempts are recovered, then
    /// the loop runs a pass whenever something is queued or due, until
    /// [`shutdown`](Self::shutdown). A pass that fails as a whole (the
    /// store cannot be written, the identity cannot be read) leaves the
    /// due entry due, so the next pass waits at least
    /// [`RETRY_BASE_SECS`] rather than running again at once.
    pub fn start(self: &Arc<Self>, federation: Arc<FederationState>, gate: Arc<FederationGate>) {
        let cancel = CancellationToken::new();
        let token = cancel.clone();
        let outbox = self.clone();
        let handle = tokio::spawn(async move {
            match outbox.recover_on_restart() {
                Ok(0) => {}
                Ok(count) => log::warn!(
                    "[federation] outbox: {count} delivery attempt(s) interrupted by a restart will be retried"
                ),
                Err(error) => log::warn!("[federation] outbox recovery failed: {error}"),
            }
            let mut last_pass_failed = false;
            loop {
                // A store that cannot be read is nothing due: polled again.
                let next_due = outbox.next_due().unwrap_or_default();
                let wait = pass_wait_secs(next_due, outbox.now(), last_pass_failed);
                tokio::select! {
                    _ = token.cancelled() => break,
                    _ = outbox.wake.notified() => {}
                    _ = tokio::time::sleep(Duration::from_secs(wait)) => {}
                }
                last_pass_failed = match outbox.run_due(&federation, &gate).await {
                    Ok(_) => false,
                    Err(error) => {
                        log::warn!("[federation] outbox delivery pass failed: {error}");
                        true
                    }
                };
            }
        });
        *self.running.lock().unwrap() = Some((cancel, handle));
    }

    /// Stops the sender loop and waits for its current pass to finish.
    pub async fn shutdown(&self) {
        let running = self.running.lock().unwrap().take();
        if let Some((cancel, handle)) = running {
            cancel.cancel();
            let _ = handle.await;
        }
    }

    /// One attempt at the entry keyed by `key`: an expired intent is
    /// settled as such without leaving; otherwise the attempt is written
    /// as in flight, the intent is sealed and posted to the peer's
    /// approved origins, and the answer decides what the entry becomes.
    async fn attempt(
        &self,
        federation: &FederationState,
        gate: &FederationGate,
        me: &str,
        key: &str,
    ) -> Result<(), FederationError> {
        let now = self.now();
        let Some(mut entry) = self.get(key)? else {
            return Ok(());
        };
        if entry.status.is_settled() {
            return Ok(());
        }
        if expired(&entry, now) {
            let reason = lapse_reason(&entry);
            let local = never_answered(&entry, reason);
            return self.settle(
                gate,
                me,
                entry,
                OutboxStatus::Expired,
                &local,
                Decision::deny(reason),
            );
        }
        // The peer as it stands now: the entry follows its pairing through
        // a key rotation, and a peer that is gone or revoked ends it.
        let peer = federation
            .overview()?
            .peers
            .into_iter()
            .find(|peer| peer.pairing_id == entry.pairing_id);
        let peer = match peer {
            Some(peer) if peer.state == PeerState::Paired => peer,
            Some(peer) => {
                let (code, reason) = match peer.state {
                    PeerState::Revoked => ("peer_revoked", DecisionReason::PeerRevoked),
                    _ => ("peer_not_paired", DecisionReason::PeerNotPaired),
                };
                let refused = AttemptOutcome::Refused {
                    status: None,
                    code: code.into(),
                };
                return self.give_up(gate, me, entry, now, refused, reason);
            }
            None => {
                let refused = AttemptOutcome::Refused {
                    status: None,
                    code: "unknown_peer".into(),
                };
                return self.give_up(gate, me, entry, now, refused, DecisionReason::PeerRevoked);
            }
        };
        entry.recipient = peer.companion_id.clone();

        // Written before the envelope leaves, with a provisional deadline,
        // so a process that dies here leaves a mark and a due time.
        let failures_before = entry.failures();
        entry.attempts.push(Attempt {
            at: now,
            outcome: AttemptOutcome::InFlight {},
        });
        entry.next_attempt_at = Some(now.saturating_add(backoff_secs(failures_before + 1)));
        entry.updated_at = now;
        self.save(entry.clone(), None)?;

        let envelope = federation.seal(&peer.companion_id, &entry.intent.encode())?;
        let mut outcome = Err(FederationError::Transport(
            "peer has no approved origin".into(),
        ));
        for origin in &peer.approved_origins {
            outcome = self
                .transport
                .post_transport(&format!("{origin}{INTENT_PATH}"), &envelope)
                .await;
            if !matches!(outcome, Err(FederationError::Transport(_))) {
                break;
            }
        }
        let answered = match outcome {
            Ok(answer) => match federation.open(&answer) {
                Ok(inbound) if inbound.peer.pairing_id == entry.pairing_id => {
                    match IntentResponse::decode(&inbound.body).and_then(|response| {
                        response.check_against(&entry.intent).map(|()| response)
                    }) {
                        Ok(response) => Ok(response),
                        Err(_) => Err(AttemptOutcome::Malformed {}),
                    }
                }
                Ok(_) | Err(_) => Err(AttemptOutcome::Malformed {}),
            },
            Err(FederationError::PeerRefused { status, error }) => Err(AttemptOutcome::Refused {
                status: Some(status),
                code: refusal_code(&error),
            }),
            Err(FederationError::Transport(_)) => Err(AttemptOutcome::Unreachable {}),
            Err(_) => Err(AttemptOutcome::Malformed {}),
        };
        let now = self.now();
        match answered {
            Ok(response) => self.answered(gate, me, entry, now, response),
            Err(AttemptOutcome::Refused {
                status: Some(status),
                code,
            }) if !transient_refusal(status, &code) => {
                let refused = AttemptOutcome::Refused {
                    status: Some(status),
                    code,
                };
                self.give_up(gate, me, entry, now, refused, DecisionReason::PeerRefused)
            }
            Err(failure) => self.failed_attempt(gate, me, entry, now, failure),
        }
    }

    /// The peer answered with a typed response, which the entry keeps as
    /// the peer's last word: `accepted` and `denied` settle the entry; a
    /// rate limit is a failed attempt that waits out the larger of the
    /// peer's window and the backoff and, past [`MAX_DELIVERY_ATTEMPTS`],
    /// fails the entry with the peer's rate limit as the reason; and
    /// `needs_owner` holds the entry until the peer's owner decides, with
    /// one receipt when it first waits.
    fn answered(
        &self,
        gate: &FederationGate,
        me: &str,
        mut entry: OutboxEntry,
        now: u64,
        response: IntentResponse,
    ) -> Result<(), FederationError> {
        let outcome = response.outcome();
        let reason = match &response {
            IntentResponse::Accepted { .. } => None,
            IntentResponse::Denied { reason, .. } | IntentResponse::NeedsOwner { reason, .. } => {
                Some(*reason)
            }
        };
        record_attempt(
            &mut entry,
            now,
            AttemptOutcome::Answered { outcome, reason },
        );
        entry.response = Some(response.clone());
        match &response {
            IntentResponse::Accepted { .. } => self.settle(
                gate,
                me,
                entry,
                OutboxStatus::Delivered,
                &response,
                Decision::allow(DecisionReason::Default),
            ),
            IntentResponse::Denied {
                reason: DecisionReason::RateLimited,
                retry_after_secs,
                ..
            } => {
                let failures = entry.failures();
                if failures >= MAX_DELIVERY_ATTEMPTS {
                    let recipient = &entry.recipient;
                    log::warn!(
                        "[federation] outbox: giving up on a request to companion {recipient} after {failures} attempts it refused under its rate limit"
                    );
                    let decision = Decision::deny(DecisionReason::RateLimited);
                    return self.settle(gate, me, entry, OutboxStatus::Failed, &response, decision);
                }
                let wait = backoff_secs(failures).max(retry_after_secs.unwrap_or(0));
                self.hold(entry, now, Some(now.saturating_add(wait)))
            }
            IntentResponse::Denied { reason, .. } => {
                let decision = Decision::deny(*reason);
                self.settle(gate, me, entry, OutboxStatus::Denied, &response, decision)
            }
            IntentResponse::NeedsOwner { reason, .. } => {
                let first = entry.status != OutboxStatus::WaitingOwner;
                entry.status = OutboxStatus::WaitingOwner;
                let next = Some(now.saturating_add(OWNER_RETRY_SECS));
                if first {
                    let decision = match reason {
                        DecisionReason::QuietHours => {
                            Decision::new(Verdict::Defer, DecisionReason::QuietHours)
                        }
                        reason => Decision::ask(*reason),
                    };
                    entry.next_attempt_at = next;
                    self.settle(
                        gate,
                        me,
                        entry,
                        OutboxStatus::WaitingOwner,
                        &response,
                        decision,
                    )
                } else {
                    self.hold(entry, now, next)
                }
            }
        }
    }

    /// A failed attempt: recorded, and either tried again after the
    /// backoff or, past [`MAX_DELIVERY_ATTEMPTS`], given up on.
    fn failed_attempt(
        &self,
        gate: &FederationGate,
        me: &str,
        mut entry: OutboxEntry,
        now: u64,
        failure: AttemptOutcome,
    ) -> Result<(), FederationError> {
        let kind = failure.name();
        record_attempt(&mut entry, now, failure);
        let failures = entry.failures();
        let recipient = entry.recipient.clone();
        if failures >= MAX_DELIVERY_ATTEMPTS {
            log::warn!(
                "[federation] outbox: giving up on a request to companion {recipient} after {failures} failed attempts (last: {kind})"
            );
            let local = never_answered(&entry, DecisionReason::Unreachable);
            return self.settle(
                gate,
                me,
                entry,
                OutboxStatus::Failed,
                &local,
                Decision::deny(DecisionReason::Unreachable),
            );
        }
        let wait = backoff_secs(failures);
        log::info!(
            "[federation] outbox: a request to companion {recipient} did not go through ({kind}, attempt {failures}); trying again in {wait}s"
        );
        let next = Some(now.saturating_add(wait));
        self.hold(entry, now, next)
    }

    /// A refusal that will not change, or a peer that is gone: the entry
    /// fails at once, recorded with `reason`.
    fn give_up(
        &self,
        gate: &FederationGate,
        me: &str,
        mut entry: OutboxEntry,
        now: u64,
        failure: AttemptOutcome,
        reason: DecisionReason,
    ) -> Result<(), FederationError> {
        record_attempt(&mut entry, now, failure);
        let local = never_answered(&entry, reason);
        self.settle(
            gate,
            me,
            entry,
            OutboxStatus::Failed,
            &local,
            Decision::deny(reason),
        )
    }

    /// Keeps an entry open: the last attempt as recorded, and when to try
    /// again.
    fn hold(
        &self,
        mut entry: OutboxEntry,
        now: u64,
        next: Option<u64>,
    ) -> Result<(), FederationError> {
        entry.next_attempt_at = next;
        entry.updated_at = now;
        self.save(entry, None)
    }

    /// Settles an entry as `status` on `response` (the peer's, which the
    /// caller has kept on the entry, or a local one built for a request
    /// the peer never answered, which is never kept as the peer's): the
    /// requesting-side audit line first, then the intent receipt, then
    /// the entry. A decision is not made without its receipt.
    fn settle(
        &self,
        gate: &FederationGate,
        me: &str,
        mut entry: OutboxEntry,
        status: OutboxStatus,
        response: &IntentResponse,
        decision: Decision,
    ) -> Result<(), FederationError> {
        let now = self.now();
        gate.record_requesting(
            &entry.pairing_id,
            me,
            &entry.recipient,
            entry.intent.class(),
            entry.intent.disclosure,
            &decision,
        )?;
        let basis = ReceiptBasis::Policy {
            reason: decision.reason,
            rule_id: None,
        };
        let receipt = IntentReceipt::new(
            AuditLog::new_id(),
            ReceiptSide::Requesting,
            entry.pairing_id.clone(),
            now,
            &entry.intent,
            response,
            basis,
        )
        .map_err(FederationError::Intent)?;
        entry.status = status;
        if status.is_settled() {
            entry.next_attempt_at = None;
        }
        entry.receipt_id = Some(receipt.id.clone());
        entry.updated_at = now;
        self.save(entry, Some(receipt))
    }

    fn broadcast(&self, entry: OutboxEntry) {
        if let Some((events, slug)) = &self.events {
            let _ = events.send(ServerEvent::OutboxUpdated {
                instance_slug: slug.clone(),
                entry,
            });
        }
    }

    /// Drops what retention says goes from memory and, when anything
    /// went, from the file.
    fn prune(&self, inner: &mut Inner) -> Result<(), FederationError> {
        let now = self.now();
        let mut entries = inner.entries.clone();
        let mut receipts = inner.receipts.clone();
        if !enforce_retention(&mut entries, &mut receipts, now) {
            return Ok(());
        }
        self.persist(&entries, &receipts)?;
        inner.entries = entries;
        inner.receipts = receipts;
        Ok(())
    }

    /// Reads the file once. A missing file is an empty outbox. A file that
    /// cannot be read, is not an outbox of this version, or has an entry
    /// of another version leaves the outbox marked unloadable: reported,
    /// never repaired, never overwritten.
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
            Ok(metadata) if metadata.len() > MAX_OUTBOX_FILE_BYTES => {
                return self.mark_unloadable(inner, "is larger than the outbox can be".to_owned());
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
        match serde_json::from_str::<OutboxFile>(&contents) {
            Ok(file)
                if file.version == OUTBOX_VERSION
                    && file
                        .entries
                        .iter()
                        .all(|entry| entry.version == OUTBOX_VERSION) =>
            {
                inner.entries = file.entries;
                inner.receipts = file.receipts;
            }
            _ => {
                let reason = match version {
                    Some(version) if version != OUTBOX_VERSION => {
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
            "[federation] outbox {} {reason}; nothing will be sent or queued until it is repaired or moved aside and the server restarted",
            self.path().display()
        );
        inner.unloadable = Some(reason);
    }

    fn persist(
        &self,
        entries: &[OutboxEntry],
        receipts: &[IntentReceipt],
    ) -> Result<(), FederationError> {
        let path = self.path();
        let file = OutboxFile {
            version: OUTBOX_VERSION,
            entries: entries.to_vec(),
            receipts: receipts.to_vec(),
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
                    "federation outbox {reason}; repair or move it aside and restart before anything is sent"
                ),
            }),
            None => Ok(()),
        }
    }
}

/// Whether `entry`'s intent can no longer be delivered: past its expiry
/// plus the skew allowance the receiving decoder gives.
fn expired(entry: &OutboxEntry, now: u64) -> bool {
    entry
        .intent
        .expires_at
        .saturating_add(MAX_INTENT_CLOCK_SKEW_SECS)
        < now
}

/// Replaces the in-flight mark of the current attempt with how it ended,
/// or records the attempt when nothing was written for it.
fn record_attempt(entry: &mut OutboxEntry, now: u64, outcome: AttemptOutcome) {
    match entry.attempts.last_mut() {
        Some(attempt) if attempt.outcome == AttemptOutcome::InFlight {} => {
            attempt.outcome = outcome;
        }
        _ => entry.attempts.push(Attempt { at: now, outcome }),
    }
}

/// The response a request the peer never answered is recorded against:
/// denied for `reason` (unreachable, refused, or gone), from the
/// companion that was asked. Built here, never sent, and never kept as
/// the peer's answer.
fn never_answered(entry: &OutboxEntry, reason: DecisionReason) -> IntentResponse {
    IntentResponse::Denied {
        version: INTENT_VERSION,
        correlation_id: entry.intent.correlation_id.clone(),
        responder: entry.recipient.clone(),
        reason,
        retry_after_secs: None,
    }
}

/// Why a request that lapsed unanswered ended, for its receipt: the
/// peer's own last word when it gave one (its owner never allowed what it
/// had to ask them about, or its rate limit never lifted), and
/// `unreachable` when nothing typed ever came back.
fn lapse_reason(entry: &OutboxEntry) -> DecisionReason {
    match &entry.response {
        Some(IntentResponse::NeedsOwner { reason, .. })
        | Some(IntentResponse::Denied { reason, .. }) => *reason,
        Some(IntentResponse::Accepted { .. }) | None => DecisionReason::Unreachable,
    }
}

/// The typed code an error status carried, when it carried one: the
/// peer's route answers a name in `[a-z0-9_]`, and a body that names
/// anything else (a proxy's own JSON, an empty field, no JSON at all,
/// which the transport already reports as `unknown`) is `unknown`.
fn refusal_code(error: &str) -> String {
    let code = crate::domain::federation_policy::sanitize_name(error);
    if code.is_empty() || code != error {
        "unknown".to_owned()
    } else {
        code
    }
}

/// Whether an error status is a failed attempt to try again rather than
/// a verdict on the request: any server error and `429` (from the peer
/// or from whatever stands in front of it), a status whose body named no
/// code (a proxy's or a tunnel's own page, so nothing the peer's route
/// said), and the peer's own transient codes: its rate limit, a nonce it
/// has seen (the envelope is sealed afresh next time), and its federation
/// state being unavailable. Everything else names a condition that will
/// not change by trying again.
fn transient_refusal(status: u16, code: &str) -> bool {
    status >= 500
        || status == 429
        || matches!(
            code,
            "unknown" | "rate_limited" | "replayed" | "federation_unavailable"
        )
}

/// How long the sender loop sleeps before its next pass: until the
/// earliest due entry, at most [`IDLE_POLL_SECS`] (and that long when
/// nothing is due or the store cannot be read), and at least
/// [`RETRY_BASE_SECS`] after a pass that failed as a whole, whose due
/// entry is still due and would otherwise be tried again at once.
fn pass_wait_secs(next_due: Option<u64>, now: u64, last_pass_failed: bool) -> u64 {
    let wait = match next_due {
        Some(at) => at.saturating_sub(now).min(IDLE_POLL_SECS),
        None => IDLE_POLL_SECS,
    };
    if last_pass_failed {
        wait.max(RETRY_BASE_SECS)
    } else {
        wait
    }
}

/// Drops what retention says goes: settled entries older than
/// [`OUTBOX_RETENTION_SECS`] and beyond the newest [`MAX_OUTBOX_ENTRIES`]
/// (open entries are kept whatever their age; they lapse with their
/// intent); receipts older than the same window, beyond the newest
/// [`MAX_OUTBOX_RECEIPTS_PER_PAIRING`] of their pairing, and beyond the
/// newest [`MAX_OUTBOX_RECEIPTS`] overall. Both lists are kept oldest
/// first. Returns whether anything went.
fn enforce_retention(
    entries: &mut Vec<OutboxEntry>,
    receipts: &mut Vec<IntentReceipt>,
    now: u64,
) -> bool {
    let before = entries.len() + receipts.len();
    let oldest_allowed = now.saturating_sub(OUTBOX_RETENTION_SECS);
    entries.retain(|entry| !entry.status.is_settled() || entry.updated_at > oldest_allowed);
    if entries.len() > MAX_OUTBOX_ENTRIES {
        let excess = entries.len() - MAX_OUTBOX_ENTRIES;
        entries.drain(..excess);
    }
    receipts.retain(|receipt| receipt.at > oldest_allowed);
    let mut kept_per_pairing: HashMap<&str, usize> = HashMap::new();
    let mut keep = vec![false; receipts.len()];
    for (index, receipt) in receipts.iter().enumerate().rev() {
        let kept = kept_per_pairing
            .entry(receipt.pairing_id.as_str())
            .or_insert(0);
        if *kept < MAX_OUTBOX_RECEIPTS_PER_PAIRING {
            *kept += 1;
            keep[index] = true;
        }
    }
    let mut index = 0;
    receipts.retain(|_| {
        let kept = keep[index];
        index += 1;
        kept
    });
    if receipts.len() > MAX_OUTBOX_RECEIPTS {
        let excess = receipts.len() - MAX_OUTBOX_RECEIPTS;
        receipts.drain(..excess);
    }
    entries.len() + receipts.len() != before
}

/// A fresh correlation id: [`CORRELATION_ID_BYTES`] random bytes as hex,
/// well inside the wire's bound and alphabet.
fn new_correlation_id() -> String {
    let mut bytes = [0u8; CORRELATION_ID_BYTES];
    getrandom::fill(&mut bytes).expect("operating system randomness is unavailable");
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A label this companion declares, bounded like the wire demands.
fn label(text: String, field: LabelField) -> Result<PeerLabel, FederationError> {
    PeerLabel::new(text.trim().to_owned())
        .map_err(|reason| FederationError::Intent(IntentError::InvalidLabel { field, reason }))
}

/// This owner's own words for the wire, bounded like any peer text; the
/// error names the length, never the text.
fn peer_text(text: String) -> Result<PeerText, FederationError> {
    PeerText::new(text).map_err(|error| FederationError::Malformed(error.to_string()))
}

/// The seconds to wait after the `failures`th failed attempt (counted from
/// one): [`RETRY_BASE_SECS`] doubled for every failure after the first,
/// never above [`RETRY_MAX_SECS`].
pub fn backoff_secs(failures: u32) -> u64 {
    let doublings = failures.saturating_sub(1).min(32);
    RETRY_BASE_SECS
        .checked_shl(doublings)
        .unwrap_or(RETRY_MAX_SECS)
        .min(RETRY_MAX_SECS)
}

/// The peer `hint` names: a companion id, or the first
/// [`MIN_PEER_PREFIX_CHARS`] or more characters of exactly one peer's id.
/// A hint that matches nothing is `UnknownPeer`; one that matches more
/// than one peer is refused as such.
pub fn resolve_peer(overview: &Overview, hint: &str) -> Result<PeerSummary, FederationError> {
    let hint = hint.trim();
    if let Some(peer) = overview.peers.iter().find(|peer| peer.companion_id == hint) {
        return Ok(peer.clone());
    }
    if hint.chars().count() < MIN_PEER_PREFIX_CHARS {
        return Err(FederationError::UnknownPeer);
    }
    let mut matches = overview
        .peers
        .iter()
        .filter(|peer| peer.companion_id.starts_with(hint));
    match (matches.next(), matches.next()) {
        (Some(peer), None) => Ok(peer.clone()),
        (Some(_), Some(_)) => Err(FederationError::Malformed(
            "that prefix names more than one companion; give the full id".into(),
        )),
        (None, _) => Err(FederationError::UnknownPeer),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::federation::{PairingRole, PeerState};
    use std::sync::atomic::{AtomicU64, Ordering};

    const T0: u64 = 1_800_000_000;
    const ME: &str = "yob7BCJNccQgNdznxaQZ_Rxn1k5ACU12hxmKeBKNhL0";
    const PEER: &str = "TFccHElqXR1lkUBqQoQPWYxPSm1wPjCl8WXbgm_cQ7E";
    const OTHER: &str = "TFccHElqXR1lkUBqQoQPWYxPSm1wPjCl8WXbgm_cQ7F";
    const STRANGER: &str = "SN7Fvp7FYlYfvTUUW4vPFzy1jh9gtfDVlA7I3_QSj4U";
    const PAIRING: &str = "9f1c0b7e2a6d4c31";

    /// A transport nothing here ever posts through.
    struct NoTransport;

    impl PeerTransport for NoTransport {
        fn post<'a>(
            &'a self,
            _url: &'a str,
            _envelope: &'a crate::domain::federation::SignedEnvelope,
        ) -> futures::future::BoxFuture<
            'a,
            Result<crate::domain::federation::SignedEnvelope, FederationError>,
        > {
            Box::pin(async { Err(FederationError::Transport("no transport".into())) })
        }

        fn post_transport<'a>(
            &'a self,
            _url: &'a str,
            _envelope: &'a crate::domain::federation::TransportEnvelope,
        ) -> futures::future::BoxFuture<
            'a,
            Result<crate::domain::federation::TransportEnvelope, FederationError>,
        > {
            Box::pin(async { Err(FederationError::Transport("no transport".into())) })
        }
    }

    fn open(root: &Path, now: &Arc<AtomicU64>) -> Outbox {
        let read = now.clone();
        Outbox::with_transport_and_clock(
            root,
            Arc::new(NoTransport),
            Arc::new(move || read.load(Ordering::SeqCst)),
        )
    }

    fn intent(correlation_id: &str, issued_at: u64) -> FederationIntent {
        FederationIntent {
            version: INTENT_VERSION,
            correlation_id: correlation_id.into(),
            sender: ME.into(),
            represented_owner: PeerLabel::new("Bob".into()).unwrap(),
            purpose: PeerLabel::new("say hello".into()).unwrap(),
            disclosure: DisclosureClass::None,
            issued_at,
            expires_at: issued_at + OUTBOX_INTENT_LIFETIME_SECS,
            intent: IntentPayload::Message {
                body: PeerText::new("see you on Friday at the lake".into()).unwrap(),
            },
        }
    }

    fn entry(correlation_id: &str, status: OutboxStatus, at: u64) -> OutboxEntry {
        OutboxEntry {
            version: OUTBOX_VERSION,
            recipient: PEER.into(),
            pairing_id: PAIRING.into(),
            intent: intent(correlation_id, at),
            status,
            attempts: Vec::new(),
            next_attempt_at: (!status.is_settled()).then_some(at),
            response: None,
            receipt_id: None,
            chat_id: "default".into(),
            created_at: at,
            updated_at: at,
            decision: None,
        }
    }

    fn accepted(correlation_id: &str) -> IntentResponse {
        IntentResponse::Accepted {
            version: INTENT_VERSION,
            correlation_id: correlation_id.into(),
            responder: PEER.into(),
            disclosure: DisclosureClass::None,
            answer: crate::domain::federation_intent::IntentAnswer::Delivered {},
        }
    }

    fn receipt(correlation_id: &str, pairing_id: &str, at: u64) -> IntentReceipt {
        IntentReceipt::new(
            AuditLog::new_id(),
            ReceiptSide::Requesting,
            pairing_id.into(),
            at,
            &intent(correlation_id, at),
            &accepted(correlation_id),
            ReceiptBasis::Policy {
                reason: DecisionReason::Default,
                rule_id: None,
            },
        )
        .unwrap()
    }

    fn peer(companion_id: &str, state: PeerState) -> PeerSummary {
        PeerSummary {
            companion_id: companion_id.into(),
            public_key: "k".into(),
            state,
            role: PairingRole::Issuer,
            pairing_id: PAIRING.into(),
            approved_origins: vec!["http://peer.test".into()],
            pending_origin: None,
            created_at: T0,
            updated_at: T0,
            last_seen_at: None,
            rotation_history: Vec::new(),
        }
    }

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn backoff_doubles_from_the_base_and_is_capped() {
        assert_eq!(backoff_secs(1), RETRY_BASE_SECS);
        assert_eq!(backoff_secs(2), 2 * RETRY_BASE_SECS);
        assert_eq!(backoff_secs(3), 4 * RETRY_BASE_SECS);
        assert_eq!(backoff_secs(7), 64 * RETRY_BASE_SECS);
        assert_eq!(backoff_secs(8), RETRY_MAX_SECS, "capped");
        assert_eq!(backoff_secs(40), RETRY_MAX_SECS, "no overflow");
        assert_eq!(backoff_secs(u32::MAX), RETRY_MAX_SECS);
        assert_eq!(backoff_secs(0), RETRY_BASE_SECS, "before any failure");
        // The whole retry ladder spans a night (a peer that is away until
        // the morning is still reached) and fits inside the intent's
        // lifetime with room for the peer's owner to answer.
        let ladder: u64 = (1..MAX_DELIVERY_ATTEMPTS).map(backoff_secs).sum();
        assert!(ladder >= 8 * 60 * 60, "{ladder}");
        assert!(ladder < OUTBOX_INTENT_LIFETIME_SECS / 2, "{ladder}");
    }

    #[test]
    fn the_loop_waits_for_the_next_due_entry_and_never_spins_after_a_failed_pass() {
        assert_eq!(pass_wait_secs(Some(T0 + 10), T0, false), 10);
        assert_eq!(pass_wait_secs(Some(T0), T0, false), 0, "due now");
        assert_eq!(pass_wait_secs(Some(T0 - 5), T0, false), 0, "overdue");
        assert_eq!(
            pass_wait_secs(Some(T0 + 10 * IDLE_POLL_SECS), T0, false),
            IDLE_POLL_SECS,
            "looks again at least once a poll"
        );
        assert_eq!(pass_wait_secs(None, T0, false), IDLE_POLL_SECS);
        // A pass that failed as a whole left its entry due: the next pass
        // waits the base backoff instead of running again at once.
        assert_eq!(pass_wait_secs(Some(T0), T0, true), RETRY_BASE_SECS);
        assert_eq!(pass_wait_secs(Some(T0 - 5), T0, true), RETRY_BASE_SECS);
        assert_eq!(pass_wait_secs(Some(T0 + 10), T0, true), RETRY_BASE_SECS);
        assert_eq!(pass_wait_secs(None, T0, true), IDLE_POLL_SECS);
    }

    #[test]
    fn a_status_is_a_verdict_only_when_the_peers_route_named_a_lasting_one() {
        // A typed code from the peer's route that will not change.
        for code in [
            "policy_denied",
            "sender_mismatch",
            "unknown_peer",
            "invalid_intent",
        ] {
            assert!(!transient_refusal(403, code), "{code}");
            assert!(!transient_refusal(400, code), "{code}");
        }
        // The peer's own transient codes, whatever the status.
        for code in ["rate_limited", "replayed", "federation_unavailable"] {
            assert!(transient_refusal(429, code), "{code}");
            assert!(transient_refusal(409, code), "{code}");
        }
        // Something in front of the peer: a server error or a rate limit
        // whatever the body, and a body that named no code at all.
        for status in [500, 502, 503, 504, 429] {
            assert!(transient_refusal(status, "unknown"), "{status}");
            assert!(transient_refusal(status, "policy_denied"), "{status}");
        }
        assert!(transient_refusal(404, "unknown"));
        assert!(transient_refusal(403, "unknown"));
        // Only a name the route could have answered is a code.
        assert_eq!(refusal_code("policy_denied"), "policy_denied");
        assert_eq!(refusal_code("unknown"), "unknown");
        assert_eq!(refusal_code("Bad Gateway"), "unknown");
        assert_eq!(refusal_code("Internal Server Error"), "unknown");
        assert_eq!(refusal_code(""), "unknown");
        assert_eq!(refusal_code("<html>"), "unknown");
    }

    #[test]
    fn a_rate_limited_answer_counts_as_a_failure_and_names_why_a_lapsed_entry_ended() {
        let mut waiting = entry("req-1", OutboxStatus::WaitingOwner, T0);
        for (outcome, reason) in [
            (IntentOutcome::NeedsOwner, Some(DecisionReason::Default)),
            (IntentOutcome::Denied, Some(DecisionReason::RateLimited)),
            (IntentOutcome::Denied, Some(DecisionReason::RateLimited)),
            (IntentOutcome::NeedsOwner, Some(DecisionReason::QuietHours)),
        ] {
            waiting.attempts.push(Attempt {
                at: T0,
                outcome: AttemptOutcome::Answered { outcome, reason },
            });
        }
        waiting.attempts.push(Attempt {
            at: T0,
            outcome: AttemptOutcome::Unreachable {},
        });
        waiting.attempts.push(Attempt {
            at: T0,
            outcome: AttemptOutcome::InFlight {},
        });
        assert_eq!(
            waiting.failures(),
            3,
            "two rate-limited answers and one unreachable attempt"
        );

        // Nothing typed ever came back: unreachable.
        assert_eq!(
            lapse_reason(&entry("req-2", OutboxStatus::Queued, T0)),
            DecisionReason::Unreachable
        );
        // The peer's owner never decided: the reason the peer gave for
        // asking them.
        waiting.response = Some(IntentResponse::NeedsOwner {
            version: INTENT_VERSION,
            correlation_id: "req-1".into(),
            responder: PEER.into(),
            reason: DecisionReason::QuietHours,
        });
        assert_eq!(lapse_reason(&waiting), DecisionReason::QuietHours);
        // The peer's rate limit never lifted.
        waiting.response = Some(IntentResponse::Denied {
            version: INTENT_VERSION,
            correlation_id: "req-1".into(),
            responder: PEER.into(),
            reason: DecisionReason::RateLimited,
            retry_after_secs: Some(30),
        });
        assert_eq!(lapse_reason(&waiting), DecisionReason::RateLimited);
    }

    #[test]
    fn nothing_is_written_until_something_is_queued_and_a_restart_reads_it_back() {
        let dir = tempfile::tempdir().unwrap();
        let now = Arc::new(AtomicU64::new(T0));
        let outbox = open(dir.path(), &now);
        assert!(outbox.view().unwrap().entries.is_empty());
        assert!(outbox.get("req-1").unwrap().is_none());
        assert_eq!(outbox.next_due().unwrap(), None);
        assert!(!outbox.path().exists(), "an empty outbox is not written");

        outbox
            .save(entry("req-1", OutboxStatus::Queued, T0), None)
            .unwrap();
        assert!(outbox.path().is_file());
        #[cfg(unix)]
        {
            assert_eq!(mode(&outbox.path()), 0o600);
            assert_eq!(mode(outbox.path().parent().unwrap()), 0o700);
        }
        assert_eq!(outbox.next_due().unwrap(), Some(T0));
        now.store(T0 + 5, Ordering::SeqCst);
        let mut delivered = entry("req-1", OutboxStatus::Delivered, T0);
        delivered.response = Some(accepted("req-1"));
        delivered.updated_at = T0 + 5;
        outbox
            .save(delivered.clone(), Some(receipt("req-1", PAIRING, T0 + 5)))
            .unwrap();

        // A restart reads the same entry and the same receipt back.
        let again = open(dir.path(), &now);
        let view = again.view().unwrap();
        assert_eq!(view.entries, vec![delivered.clone()]);
        assert_eq!(view.receipts.len(), 1);
        assert_eq!(view.receipts[0].side, ReceiptSide::Requesting);
        assert_eq!(view.receipts[0].requester, ME);
        assert_eq!(view.receipts[0].responder, PEER);
        assert_eq!(again.get("req-1").unwrap(), Some(delivered));
        assert_eq!(
            again.next_due().unwrap(),
            None,
            "settled entries are not due"
        );

        // Newest update first in the listing.
        again
            .save(entry("req-2", OutboxStatus::Queued, T0 + 5), None)
            .unwrap();
        now.store(T0 + 9, Ordering::SeqCst);
        let mut older = entry("req-1", OutboxStatus::Delivered, T0);
        older.updated_at = T0 + 9;
        again.save(older, None).unwrap();
        assert_eq!(
            again
                .view()
                .unwrap()
                .entries
                .iter()
                .map(|entry| entry.correlation_id().to_owned())
                .collect::<Vec<_>>(),
            ["req-1", "req-2"]
        );
    }

    #[test]
    fn interrupted_attempts_are_recovered_once_on_the_same_correlation_id() {
        let dir = tempfile::tempdir().unwrap();
        let now = Arc::new(AtomicU64::new(T0));
        let outbox = open(dir.path(), &now);
        let mut in_flight = entry("req-1", OutboxStatus::Queued, T0);
        in_flight.attempts.push(Attempt {
            at: T0,
            outcome: AttemptOutcome::Unreachable {},
        });
        in_flight.attempts.push(Attempt {
            at: T0 + 30,
            outcome: AttemptOutcome::InFlight {},
        });
        in_flight.next_attempt_at = Some(T0 + 90);
        outbox.save(in_flight, None).unwrap();
        let mut settled = entry("req-2", OutboxStatus::Delivered, T0);
        settled.attempts.push(Attempt {
            at: T0,
            outcome: AttemptOutcome::Answered {
                outcome: IntentOutcome::Accepted,
                reason: None,
            },
        });
        outbox.save(settled.clone(), None).unwrap();

        let fresh = open(dir.path(), &now);
        assert_eq!(fresh.recover_on_restart().unwrap(), 1);
        let recovered = fresh.get("req-1").unwrap().unwrap();
        assert_eq!(recovered.status, OutboxStatus::Queued);
        assert_eq!(
            recovered.correlation_id(),
            "req-1",
            "the same id is retried"
        );
        assert_eq!(
            recovered
                .attempts
                .iter()
                .map(|attempt| attempt.outcome.clone())
                .collect::<Vec<_>>(),
            [
                AttemptOutcome::Unreachable {},
                AttemptOutcome::Interrupted {}
            ]
        );
        assert_eq!(recovered.failures(), 2, "an interrupted attempt counts");
        assert_eq!(
            recovered.next_attempt_at,
            Some(T0 + 90),
            "the provisional deadline stands"
        );
        assert_eq!(fresh.get("req-2").unwrap().unwrap(), settled);
        assert_eq!(fresh.recover_on_restart().unwrap(), 0, "idempotent");
    }

    #[test]
    fn settled_entries_lapse_and_both_lists_are_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let now = Arc::new(AtomicU64::new(T0));
        let outbox = open(dir.path(), &now);
        outbox
            .save(entry("old", OutboxStatus::Delivered, T0), None)
            .unwrap();
        outbox
            .save(entry("open", OutboxStatus::Queued, T0), None)
            .unwrap();
        now.store(T0 + OUTBOX_RETENTION_SECS + 1, Ordering::SeqCst);
        let view = outbox.view().unwrap();
        assert_eq!(
            view.entries
                .iter()
                .map(|entry| entry.correlation_id().to_owned())
                .collect::<Vec<_>>(),
            ["open"],
            "a settled entry lapses, an open one stays"
        );

        // Entries: the newest MAX_OUTBOX_ENTRIES survive.
        let later = now.load(Ordering::SeqCst);
        for index in 0..(MAX_OUTBOX_ENTRIES + 5) {
            let at = later + index as u64;
            now.store(at, Ordering::SeqCst);
            outbox
                .save(entry(&format!("e{index}"), OutboxStatus::Denied, at), None)
                .unwrap();
        }
        let view = outbox.view().unwrap();
        assert_eq!(view.entries.len(), MAX_OUTBOX_ENTRIES);
        assert!(outbox.get("e0").unwrap().is_none(), "the oldest went");
        assert!(outbox.get("e5").unwrap().is_some());

        // Receipts: per pairing, a chatty peer only pushes out its own.
        let quiet = receipt("quiet", "quiet-pairing", now.load(Ordering::SeqCst));
        outbox
            .save(
                entry("quiet", OutboxStatus::Delivered, now.load(Ordering::SeqCst)),
                Some(quiet.clone()),
            )
            .unwrap();
        for index in 0..(MAX_OUTBOX_RECEIPTS_PER_PAIRING + 3) {
            let at = now.load(Ordering::SeqCst) + 1;
            now.store(at, Ordering::SeqCst);
            outbox
                .save(
                    entry(&format!("c{index}"), OutboxStatus::Delivered, at),
                    Some(receipt(&format!("c{index}"), PAIRING, at)),
                )
                .unwrap();
        }
        let receipts = outbox.view().unwrap().receipts;
        assert!(receipts.iter().any(|receipt| receipt.id == quiet.id));
        assert_eq!(
            receipts
                .iter()
                .filter(|receipt| receipt.pairing_id == PAIRING)
                .count(),
            MAX_OUTBOX_RECEIPTS_PER_PAIRING
        );
        assert_eq!(
            receipts[0].correlation_id,
            format!("c{}", MAX_OUTBOX_RECEIPTS_PER_PAIRING + 2),
            "newest first"
        );

        // Age: nothing older than the retention window.
        now.store(
            now.load(Ordering::SeqCst) + OUTBOX_RETENTION_SECS + 1,
            Ordering::SeqCst,
        );
        let at = now.load(Ordering::SeqCst);
        outbox
            .save(
                entry("fresh", OutboxStatus::Delivered, at),
                Some(receipt("fresh", PAIRING, at)),
            )
            .unwrap();
        let receipts = outbox.view().unwrap().receipts;
        assert_eq!(receipts.len(), 1, "{receipts:?}");
        assert_eq!(receipts[0].correlation_id, "fresh");
    }

    #[test]
    fn an_unloadable_store_fails_every_operation_closed_and_is_never_overwritten() {
        for (name, contents) in [
            ("junk", "not json".to_owned()),
            (
                "other version",
                r#"{"version":2,"entries":[],"receipts":[]}"#.to_owned(),
            ),
            ("wrong shape", r#"{"version":1,"entries":{}}"#.to_owned()),
            (
                "an entry with a field this build does not know",
                format!(
                    r#"{{"version":1,"entries":[{}],"receipts":[]}}"#,
                    serde_json::to_string(&entry("x", OutboxStatus::Queued, T0))
                        .unwrap()
                        .replacen("{", r#"{"peer_memory":"everything","#, 1)
                ),
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let now = Arc::new(AtomicU64::new(T0));
            let outbox = open(dir.path(), &now);
            std::fs::create_dir_all(outbox.path().parent().unwrap()).unwrap();
            std::fs::write(outbox.path(), &contents).unwrap();
            let refused = outbox.get("x");
            assert!(
                matches!(refused, Err(FederationError::Io { .. })),
                "{name}: {refused:?}"
            );
            assert!(outbox.view().is_err(), "{name}");
            assert!(outbox.next_due().is_err(), "{name}");
            assert!(outbox.recover_on_restart().is_err(), "{name}");
            let write = outbox.save(entry("y", OutboxStatus::Queued, T0), None);
            assert!(write.is_err(), "{name}");
            let error = write.unwrap_err().to_string();
            assert!(
                !error.contains("not json") && !error.contains("everything"),
                "{name}: {error}"
            );
            assert_eq!(
                std::fs::read_to_string(outbox.path()).unwrap(),
                contents,
                "{name}: the file survives byte for byte"
            );
        }
        // An oversize file is not ours either.
        let dir = tempfile::tempdir().unwrap();
        let now = Arc::new(AtomicU64::new(T0));
        let outbox = open(dir.path(), &now);
        std::fs::create_dir_all(outbox.path().parent().unwrap()).unwrap();
        let file = std::fs::File::create(outbox.path()).unwrap();
        file.set_len(MAX_OUTBOX_FILE_BYTES + 1).unwrap();
        assert!(outbox.view().is_err());
    }

    #[test]
    fn a_peer_is_named_by_its_id_or_a_unique_prefix_of_it() {
        let dir = tempfile::tempdir().unwrap();
        let me = identity::load_or_create(dir.path()).unwrap();
        let overview = Overview {
            companion_id: me.companion_id().to_owned(),
            identity: me.document().clone(),
            rotations: Vec::new(),
            invites: Vec::new(),
            peers: vec![
                peer(PEER, PeerState::Paired),
                peer(OTHER, PeerState::Revoked),
                peer(STRANGER, PeerState::Pending),
            ],
        };
        assert_eq!(resolve_peer(&overview, PEER).unwrap().companion_id, PEER);
        assert_eq!(
            resolve_peer(&overview, &STRANGER[..MIN_PEER_PREFIX_CHARS])
                .unwrap()
                .companion_id,
            STRANGER,
            "a unique prefix names the peer whatever its state"
        );
        assert_eq!(
            resolve_peer(&overview, &format!("  {}  ", &STRANGER[..12]))
                .unwrap()
                .companion_id,
            STRANGER,
            "surrounding whitespace is ignored"
        );
        // PEER and OTHER share every character but the last.
        let shared = &PEER[..PEER.len() - 1];
        assert!(
            matches!(
                resolve_peer(&overview, shared),
                Err(FederationError::Malformed(_))
            ),
            "an ambiguous prefix is refused, not guessed"
        );
        for hint in [
            "",
            "   ",
            &PEER[..MIN_PEER_PREFIX_CHARS - 1],
            "nobody-at-all-1234",
            &PEER[1..],
        ] {
            assert!(
                matches!(
                    resolve_peer(&overview, hint),
                    Err(FederationError::UnknownPeer)
                ),
                "{hint:?}"
            );
        }
    }

    #[test]
    fn a_peers_decision_is_noted_once_on_the_delivered_proposal_it_answers_and_on_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let now = Arc::new(AtomicU64::new(T0));
        let outbox = open(dir.path(), &now);
        let proposal = |correlation_id: &str, status: OutboxStatus| {
            let mut entry = entry(correlation_id, status, T0);
            entry.intent.intent = IntentPayload::Proposal {
                description: PeerText::new("lunch by the lake".into()).unwrap(),
                window: TimeWindow {
                    from: T0 + 3_600,
                    to: T0 + 7_200,
                },
            };
            entry
        };
        outbox
            .save(proposal("delivered", OutboxStatus::Delivered), None)
            .unwrap();
        outbox
            .save(proposal("waiting", OutboxStatus::WaitingOwner), None)
            .unwrap();
        outbox
            .save(entry("message", OutboxStatus::Delivered, T0), None)
            .unwrap();
        now.store(T0 + 60, Ordering::SeqCst);
        let noted = outbox
            .note_decision(PAIRING, "delivered", ProposalDecision::Accepted)
            .unwrap();
        assert_eq!(
            noted.decision,
            Some(PeerDecision {
                decision: ProposalDecision::Accepted,
                at: T0 + 60,
            })
        );
        assert_eq!(
            noted.status,
            OutboxStatus::Delivered,
            "the status is not the decision"
        );
        assert_eq!(noted.updated_at, T0 + 60);
        assert_eq!(
            outbox.get("delivered").unwrap().unwrap(),
            noted,
            "persisted"
        );
        // Told again: noted once; told the other way: refused, and the first
        // decision stands.
        now.store(T0 + 120, Ordering::SeqCst);
        let again = outbox
            .note_decision(PAIRING, "delivered", ProposalDecision::Accepted)
            .unwrap();
        assert_eq!(again, noted);
        assert!(matches!(
            outbox.note_decision(PAIRING, "delivered", ProposalDecision::Dismissed),
            Err(FederationError::UnknownRequest)
        ));
        assert_eq!(outbox.get("delivered").unwrap().unwrap(), noted);
        // Nothing else takes a decision: another pairing, a request not
        // delivered, a message, an unknown id.
        for (pairing, key) in [
            ("other-pairing", "delivered"),
            (PAIRING, "waiting"),
            (PAIRING, "message"),
            (PAIRING, "never-sent"),
        ] {
            assert!(
                matches!(
                    outbox.note_decision(pairing, key, ProposalDecision::Dismissed),
                    Err(FederationError::UnknownRequest)
                ),
                "{pairing} {key}"
            );
        }
        assert_eq!(outbox.get("waiting").unwrap().unwrap().decision, None);
        assert_eq!(outbox.get("message").unwrap().unwrap().decision, None);
        // An entry written before decisions existed reads back without one.
        let json = serde_json::to_string(&entry("old", OutboxStatus::Delivered, T0)).unwrap();
        assert!(!json.contains("\"decision\""), "{json}");
        let read: OutboxEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(read.decision, None);
    }
}
