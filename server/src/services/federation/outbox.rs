//! Outbound structured intents (#110, PR 3): the persisted outbox that
//! carries what this companion asks a paired peer for, the sender loop
//! that delivers it with bounded retries, and the receipts of what was
//! asked and what the peer disclosed.
//!
//! The chat tools in `services::tools::federation` build a typed
//! [`Outgoing`] request (a message, an availability query, a reminder) and
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
//! did not verify or decode, or that refused with a transient code is
//! tried again after [`backoff_secs`] (doubling from [`RETRY_BASE_SECS`],
//! capped at [`RETRY_MAX_SECS`]); after [`MAX_DELIVERY_ATTEMPTS`] such
//! failures the entry is visibly `failed`. An intent that expires before it
//! was delivered is `expired`. Every change to an entry is broadcast as
//! `ServerEvent::OutboxUpdated`, so the activity page shows where each
//! request stands.
//!
//! Delivery is idempotent end to end: the correlation id never changes
//! across retries or a restart, and the receiving side answers a request it
//! already settled with the same response again ([`super::inbound`]).
//! An attempt is written as in flight before the envelope leaves, so a
//! process that dies mid-delivery leaves a mark; [`Outbox::recover_on_restart`]
//! turns it into `interrupted` and the entry is retried on the same id,
//! never queued a second time.
//!
//! Every settled outcome, and the first `needs_owner`, writes a
//! requesting-side audit receipt through the gate and an [`IntentReceipt`]
//! naming what was requested and what the peer disclosed, read off the
//! typed response only. `federation/outbox.json` (`0600`, written through
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
    identity,
    inbound::INTENT_PATH,
    pairing::{FederationState, HttpTransport, Overview, PeerTransport},
    peers::{Clock, system_clock},
};
use crate::domain::{
    events::ServerEvent,
    federation::{FederationError, PeerState, PeerSummary},
    federation_intent::{
        FederationIntent, INTENT_VERSION, IntentError, IntentOutcome, IntentPayload, IntentReceipt,
        IntentResponse, LabelField, MAX_INTENT_CLOCK_SKEW_SECS, PeerLabel, ReceiptBasis,
        TimeWindow,
    },
    federation_policy::{
        Decision, DecisionReason, DisclosureClass, IntentClass, IntentRequest, PeerText,
        ReceiptSide, Verdict,
    },
};

/// The store, under the keystore directory.
pub const OUTBOX_FILE: &str = "outbox.json";
/// Version of the store file and of every entry in it.
pub const OUTBOX_VERSION: u32 = 1;
/// How long a queued intent stands: it may wait out an outage, and the
/// peer's owner may take a while, but not forever. Below the decoder's
/// week-long maximum.
pub const OUTBOX_INTENT_LIFETIME_SECS: u64 = 24 * 60 * 60;
/// Failed attempts (unreachable, undecodable, transiently refused) before
/// an entry is given up on and shown as failed.
pub const MAX_DELIVERY_ATTEMPTS: u32 = 8;
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
    /// The peer's route refused with a typed code.
    Refused { code: String },
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
}

impl OutboxEntry {
    pub fn correlation_id(&self) -> &str {
        &self.intent.correlation_id
    }

    /// Attempts that count towards giving up: everything but an answer.
    pub fn failures(&self) -> u32 {
        self.attempts
            .iter()
            .filter(|attempt| {
                !matches!(
                    attempt.outcome,
                    AttemptOutcome::Answered { .. } | AttemptOutcome::InFlight {}
                )
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
        let _ = (federation, gate, outgoing);
        todo!("PR 3 of #110: queue a typed intent behind the own policy gate")
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
        let _ = (entry, receipt);
        todo!("PR 3 of #110: persist and broadcast one entry")
    }

    /// Every entry and every kept receipt, newest first.
    pub fn view(&self) -> Result<OutboxView, FederationError> {
        todo!("PR 3 of #110: the owner listing")
    }

    /// The entry keyed by `correlation_id`, if any.
    pub fn get(&self, correlation_id: &str) -> Result<Option<OutboxEntry>, FederationError> {
        let _ = correlation_id;
        todo!("PR 3 of #110: one entry")
    }

    /// When the earliest unsettled entry is due, if any.
    pub fn next_due(&self) -> Result<Option<u64>, FederationError> {
        todo!("PR 3 of #110: the next deadline")
    }

    /// Attempts left in flight by a previous process can never finish:
    /// they are marked interrupted, and the entry is tried again on the
    /// same correlation id when it is due. Returns how many were marked.
    pub fn recover_on_restart(&self) -> Result<usize, FederationError> {
        todo!("PR 3 of #110: recover interrupted attempts")
    }

    /// One delivery pass: every unsettled entry that is due is attempted
    /// once, and every one whose intent expired is settled as such.
    /// Returns how many entries were touched.
    pub async fn run_due(
        &self,
        federation: &FederationState,
        gate: &FederationGate,
    ) -> Result<usize, FederationError> {
        let _ = (federation, gate);
        todo!("PR 3 of #110: one delivery pass")
    }

    /// Starts the sender loop: interrupted attempts are recovered, then
    /// the loop runs a pass whenever something is queued or due, until
    /// [`shutdown`](Self::shutdown).
    pub fn start(self: &Arc<Self>, federation: Arc<FederationState>, gate: Arc<FederationGate>) {
        let _ = (federation, gate);
        todo!("PR 3 of #110: the sender loop")
    }

    /// Stops the sender loop and waits for its current pass to finish.
    pub async fn shutdown(&self) {
        todo!("PR 3 of #110: stop the sender loop")
    }
}

/// The seconds to wait after the `failures`th failed attempt (counted from
/// one): [`RETRY_BASE_SECS`] doubled for every failure after the first,
/// never above [`RETRY_MAX_SECS`].
pub fn backoff_secs(failures: u32) -> u64 {
    let _ = failures;
    todo!("PR 3 of #110: bounded exponential backoff")
}

/// The peer `hint` names: a companion id, or the first
/// [`MIN_PEER_PREFIX_CHARS`] or more characters of exactly one peer's id.
/// A hint that matches nothing is `UnknownPeer`; one that matches more
/// than one peer is refused as such.
pub fn resolve_peer(overview: &Overview, hint: &str) -> Result<PeerSummary, FederationError> {
    let _ = (overview, hint);
    todo!("PR 3 of #110: resolve a peer by id or prefix")
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
        // The whole retry ladder fits inside the intent's lifetime with
        // room for the peer's owner to answer.
        let ladder: u64 = (1..MAX_DELIVERY_ATTEMPTS).map(backoff_secs).sum();
        assert!(ladder < OUTBOX_INTENT_LIFETIME_SECS / 2, "{ladder}");
        assert!(
            OUTBOX_INTENT_LIFETIME_SECS
                <= crate::domain::federation_intent::MAX_INTENT_LIFETIME_SECS
        );
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
            resolve_peer(&overview, &format!("  {}  ", &PEER[..12]))
                .unwrap()
                .companion_id,
            PEER,
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
}
