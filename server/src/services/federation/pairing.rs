//! Owner-confirmed pairing between two companions (#108).
//!
//! The handshake, with A issuing and B accepting:
//!
//! 1. A's owner mints an invite (`create_invite`): a one-time secret shown
//!    once, plus A's identity document and base URL to hand over out of band.
//! 2. B's owner types those into B (`accept_invite`). B pins A's document,
//!    signs a `pair_request` carrying the secret and B's own document, and
//!    posts it to `{origin}/federation/v1/pair`. A verifies B's document and
//!    signature, redeems the secret, records B as `pending`, and answers with
//!    a signed `pair_response`; B verifies it against the pinned document and
//!    records A as `pending`. B's owner has now confirmed.
//! 3. A's owner confirms (`confirm_peer`): B becomes `paired` on A, and A
//!    posts a signed `pair_confirm` to B's approved origin; B verifies it
//!    against the stored key and marks A `paired`.
//! 4. Either owner revokes (`revoke_peer`): the record becomes `revoked`
//!    locally and a signed `pair_revoke` tells the peer, which does the same.
//!
//! Every inbound step is verified by signature only. The peer-side routes
//! never consult the API token, browser sessions, the remote address, the
//! `Host` header, or anything about the deployment; two profiles on one
//! machine pair exactly like two machines. Secrets travel only inside signed
//! POST bodies, never in URLs. Nothing here logs a secret.
//!
//! Every state transition is a compare-and-set inside the peer store's lock
//! (`PeerStore::update`): the state a step requires is checked against the
//! record as it is at that moment, never against an earlier snapshot, so a
//! confirmation racing a revoke can never leave a revoked peer paired.
//!
//! After pairing, companions talk through the transport envelope
//! (`seal`/`open`, `services::federation::envelope`): addressed, single-use
//! by nonce, time-bounded, and verified with the key the peer store holds
//! for the sender, which may be a key the peer rotated away from inside its
//! grace window. `rotate_identity` replaces this server's key, withdraws
//! its invites, revokes its pending pairings (started under the old key,
//! they could not finish under the new one), and tells every paired peer
//! with a notice signed by the old key; `receive_rotation` verifies such a
//! notice and re-keys the peer's record with an audited transition. The
//! issuer-side steps choose their signing key and change the peer store
//! under the identity lock the rotation holds (`with_identity`), so no
//! pending record is ever confirmed under an identity other than the one
//! it was started with.
//!
//! Every time something signed by a peer verifies here (a pairing step, a
//! notice or its acknowledgement, a transport envelope) the peer's record
//! is stamped as seen (`PeerStore::touch`), which is what the owner list
//! reports as `last_seen_at`; nothing that failed verification and nothing
//! the owner does locally counts as a sighting.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use futures::future::BoxFuture;
use serde::Serialize;
use zeroize::Zeroizing;

use super::{
    check_version, decode,
    envelope::{self, ReplayGuard},
    identity::{self, SigningIdentity},
    peers::{Clock, Peer, PeerStore, SenderKey},
    rotation,
};
use crate::domain::federation::{
    AcceptInvite, FEDERATION_VERSION, FederationError, IdentityDocument, InviteSummary,
    IssuedInvite, KeyRotation, PairingMessage, PairingRole, PeerRecord, PeerState, PeerSummary,
    SignedEnvelope, TransportEnvelope, TransportMessage,
};

/// Peer-side route for pair requests, relative to the peer's base URL.
pub const PAIR_PATH: &str = "/federation/v1/pair";
/// Peer-side route for confirmation notices.
pub const CONFIRM_PATH: &str = "/federation/v1/pair/confirm";
/// Peer-side route for revocation notices.
pub const REVOKE_PATH: &str = "/federation/v1/pair/revoke";
/// Peer-side route for a transport ping between paired companions.
pub const PING_PATH: &str = "/federation/v1/ping";
/// Peer-side route for key rotation notices.
pub const ROTATE_PATH: &str = "/federation/v1/rotate";
/// Largest envelope accepted on the wire, in either direction.
pub const MAX_ENVELOPE_BYTES: usize = 64 * 1024;
/// A peer that does not answer within this is unreachable.
const TRANSPORT_TIMEOUT: Duration = Duration::from_secs(15);
/// Longest base URL accepted for a peer.
const MAX_ORIGIN_LEN: usize = 255;
/// Longest pairing id accepted from a peer (ours are 16 hex characters).
const MAX_PAIRING_ID_LEN: usize = 64;

/// Sends one signed envelope to a peer URL and returns the signed answer.
/// Production uses HTTPS through [`HttpTransport`]; tests route into another
/// in-process server. Pairing steps use [`SignedEnvelope`]; everything after
/// pairing uses the addressed [`TransportEnvelope`].
pub trait PeerTransport: Send + Sync {
    fn post<'a>(
        &'a self,
        url: &'a str,
        envelope: &'a SignedEnvelope,
    ) -> BoxFuture<'a, Result<SignedEnvelope, FederationError>>;

    fn post_transport<'a>(
        &'a self,
        url: &'a str,
        envelope: &'a TransportEnvelope,
    ) -> BoxFuture<'a, Result<TransportEnvelope, FederationError>>;
}

/// The real transport: a JSON POST with a timeout and a size cap.
pub struct HttpTransport {
    client: reqwest::Client,
}

impl HttpTransport {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }

    /// POSTs `envelope` as JSON and returns the answer's text once it is a
    /// success under the size cap.
    async fn post_json(
        &self,
        url: &str,
        envelope: &impl Serialize,
    ) -> Result<String, FederationError> {
        let mut response = self
            .client
            .post(url)
            .timeout(TRANSPORT_TIMEOUT)
            .json(envelope)
            .send()
            .await
            .map_err(|error| FederationError::Transport(error.to_string()))?;
        let status = response.status();
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| FederationError::Transport(error.to_string()))?
        {
            if bytes.len() + chunk.len() > MAX_ENVELOPE_BYTES {
                return Err(FederationError::Malformed(
                    "peer answer exceeds the envelope size cap".into(),
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        if !status.is_success() {
            let error = serde_json::from_slice::<serde_json::Value>(&bytes)
                .ok()
                .and_then(|value| value["error"].as_str().map(str::to_owned))
                .unwrap_or_else(|| "unknown".to_owned());
            return Err(FederationError::PeerRefused {
                status: status.as_u16(),
                error,
            });
        }
        String::from_utf8(bytes)
            .map_err(|_| FederationError::Malformed("peer answer is not UTF-8".into()))
    }
}

impl PeerTransport for HttpTransport {
    fn post<'a>(
        &'a self,
        url: &'a str,
        envelope: &'a SignedEnvelope,
    ) -> BoxFuture<'a, Result<SignedEnvelope, FederationError>> {
        Box::pin(async move { identity::parse_envelope(&self.post_json(url, envelope).await?) })
    }

    fn post_transport<'a>(
        &'a self,
        url: &'a str,
        envelope: &'a TransportEnvelope,
    ) -> BoxFuture<'a, Result<TransportEnvelope, FederationError>> {
        Box::pin(async move { envelope::parse(&self.post_json(url, envelope).await?) })
    }
}

/// What the owner list reports: this companion's identity, its own key
/// rotations, its outstanding invites, and its peers.
#[derive(Debug, Clone, Serialize)]
pub struct Overview {
    pub companion_id: String,
    pub identity: IdentityDocument,
    pub rotations: Vec<KeyRotation>,
    pub invites: Vec<InviteSummary>,
    pub peers: Vec<PeerSummary>,
}

/// A transport envelope that verified: the paired peer it came from, which
/// of its keys signed it, and the body.
#[derive(Debug, Clone)]
pub struct Inbound {
    pub peer: PeerRecord,
    pub key: SenderKey,
    pub body: Vec<u8>,
}

/// What the owner gets back from a rotation: the new identity, the proof,
/// and which paired peers acknowledged the notice.
#[derive(Debug, Clone, Serialize)]
pub struct RotationReport {
    pub identity: IdentityDocument,
    pub rotation: KeyRotation,
    pub notified: Vec<String>,
    pub unreachable: Vec<String>,
}

/// This server's federation side: its signing identity (created on first
/// use), its peers, the nonces it accepted, and the transport to reach
/// peers.
pub struct FederationState {
    root: PathBuf,
    identity: Mutex<Option<Arc<SigningIdentity>>>,
    peers: PeerStore,
    replay: Mutex<ReplayGuard>,
    transport: Arc<dyn PeerTransport>,
    clock: Clock,
}

impl FederationState {
    /// Federation over `workspace_root`, reaching peers through `client`.
    pub fn new(workspace_root: &Path, client: reqwest::Client) -> Self {
        Self::with_transport(workspace_root, Arc::new(HttpTransport::new(client)))
    }

    pub(crate) fn with_transport(workspace_root: &Path, transport: Arc<dyn PeerTransport>) -> Self {
        Self::with_transport_and_clock(workspace_root, transport, super::peers::system_clock())
    }

    pub(crate) fn with_transport_and_clock(
        workspace_root: &Path,
        transport: Arc<dyn PeerTransport>,
        clock: Clock,
    ) -> Self {
        Self {
            root: workspace_root.to_path_buf(),
            identity: Mutex::new(None),
            peers: PeerStore::with_clock(workspace_root, clock.clone()),
            replay: Mutex::new(ReplayGuard::default()),
            transport,
            clock,
        }
    }

    /// This companion's signing identity, created under the workspace root
    /// on first use. Fails closed on an unusable keystore.
    pub fn identity(&self) -> Result<Arc<SigningIdentity>, FederationError> {
        self.with_identity(|identity| Ok(identity.clone()))
    }

    /// Runs `step` with the current identity while the identity lock is
    /// held, so a rotation cannot land between choosing the signing key and
    /// the peer-store change `step` makes with it. The issuer-side pairing
    /// steps use this: a record they create or confirm is always tied to
    /// the identity that signs the answer, and `rotate_identity` revokes
    /// pending records under the same lock. Lock order is identity, then
    /// the peer store; nothing takes them the other way round.
    fn with_identity<T>(
        &self,
        step: impl FnOnce(&Arc<SigningIdentity>) -> Result<T, FederationError>,
    ) -> Result<T, FederationError> {
        let mut slot = self.identity.lock().unwrap();
        let identity = match slot.as_ref() {
            Some(identity) => identity.clone(),
            None => {
                let identity = Arc::new(identity::load_or_create(&self.root)?);
                *slot = Some(identity.clone());
                identity
            }
        };
        step(&identity)
    }

    /// Mints an invite for the owner to hand to another owner. `origin` is
    /// this server's own base URL, which the accepter will post to.
    pub fn create_invite(&self, origin: &str) -> Result<IssuedInvite, FederationError> {
        let origin = normalize_origin(origin)?;
        let identity = self.identity()?;
        let minted = self.peers.create_invite();
        Ok(IssuedInvite {
            id: minted.id,
            secret: minted.secret,
            created_at: minted.created_at,
            expires_at: minted.expires_at,
            expires_in_secs: minted.expires_at.saturating_sub(minted.created_at),
            origin,
            issuer: identity.document().clone(),
        })
    }

    pub fn cancel_invite(&self, id: &str) -> bool {
        self.peers.cancel_invite(id)
    }

    pub fn overview(&self) -> Result<Overview, FederationError> {
        let identity = self.identity()?;
        Ok(Overview {
            companion_id: identity.companion_id().to_owned(),
            identity: identity.document().clone(),
            rotations: rotation::load_rotations(&self.root)?,
            invites: self.peers.invites(),
            peers: self.peers.list().iter().map(PeerRecord::summary).collect(),
        })
    }

    /// Seals `body` for the peer `recipient` as this companion.
    pub fn seal(&self, recipient: &str, body: &[u8]) -> Result<TransportEnvelope, FederationError> {
        envelope::seal(&*self.identity()?, recipient, body, (self.clock)())
    }

    /// Seals `body` as an explicit identity: the retiring key signs the
    /// rotation notices after the keystore already moved on.
    fn seal_as(
        &self,
        signer: &SigningIdentity,
        recipient: &str,
        body: &[u8],
    ) -> Result<TransportEnvelope, FederationError> {
        envelope::seal(signer, recipient, body, (self.clock)())
    }

    /// Verifies a transport envelope from a paired peer: version, recipient,
    /// sender (current key, or a rotated-away key inside its grace window),
    /// signature, state, body hash, times, and finally the nonce, which is
    /// consumed only once everything else passed. Unknown, pending, revoked,
    /// and retired senders fail closed with distinct errors.
    pub fn open(&self, envelope: &TransportEnvelope) -> Result<Inbound, FederationError> {
        check_version(envelope.version)?;
        let me = self.identity()?;
        if envelope.recipient != me.companion_id() {
            return Err(FederationError::RecipientMismatch);
        }
        let sender = self.peers.resolve_sender(&envelope.sender)?;
        let now = (self.clock)();
        let verified = envelope::verify(envelope, &sender.key, me.companion_id(), now)?;
        match sender.peer.record.state {
            PeerState::Paired => {}
            PeerState::Revoked => return Err(FederationError::PeerRevoked),
            state => return Err(FederationError::PeerNotPaired { state }),
        }
        // Last, so that nothing that failed above consumed the nonce and a
        // stranger cannot fill the set with envelopes that never verified.
        self.replay.lock().unwrap().reserve(
            &envelope.sender,
            verified.nonce,
            verified.retire_at,
            now,
        )?;
        self.seen(sender.peer.record.companion_id());
        Ok(Inbound {
            peer: sender.peer.record,
            key: sender.kind,
            body: verified.body,
        })
    }

    /// Stamps a peer as seen after something it signed verified. A store
    /// that cannot take the stamp does not undo the verification: the
    /// sighting is informational, the trust decision was already made.
    fn seen(&self, companion_id: &str) {
        if let Err(error) = self.peers.touch(companion_id) {
            log::warn!("[federation] companion {companion_id} sighting not recorded: {error}");
        }
    }

    /// A ping from a paired peer, answered with a sealed pong.
    pub fn receive_ping(
        &self,
        envelope: &TransportEnvelope,
    ) -> Result<TransportEnvelope, FederationError> {
        let inbound = self.open(envelope)?;
        let TransportMessage::Ping { version } = parse_transport_message(&inbound.body)? else {
            return Err(FederationError::Malformed("expected a ping".into()));
        };
        check_version(version)?;
        self.seal(
            inbound.peer.companion_id(),
            &serde_json::to_vec(&TransportMessage::Pong {
                version: FEDERATION_VERSION,
            })
            .expect("transport messages serialize"),
        )
    }

    /// Replaces this companion's key: the rotation proof is persisted and
    /// the keystore rewritten before anything is sent, outstanding invites
    /// (which carried the old document) are withdrawn, pending pairings
    /// (started under the old document, on either role) are revoked, and
    /// every paired peer gets a notice signed by the old key. Peers that do
    /// not acknowledge are reported; the local rotation stands regardless.
    pub async fn rotate_identity(&self) -> Result<RotationReport, FederationError> {
        // Under the identity lock from the check to the swap, so two
        // rotations cannot both endorse from the same key, and through the
        // withdrawal of everything the old identity left half done, so no
        // issuer-side step can confirm a pending record under the new key
        // (`with_identity`).
        let (previous, next, rotation, revoked) = {
            let mut slot = self.identity.lock().unwrap();
            let previous = match slot.as_ref() {
                Some(identity) => identity.clone(),
                None => Arc::new(identity::load_or_create(&self.root)?),
            };
            let (next, rotation) = identity::rotate_at(&self.root, &previous, (self.clock)())?;
            let next = Arc::new(next);
            *slot = Some(next.clone());
            self.peers.cancel_all_invites();
            // The rotation already stands; a store this build cannot load
            // refuses every pairing write anyway, so nothing pending in it
            // can be confirmed either.
            let revoked = self.peers.revoke_pending().unwrap_or_else(|error| {
                log::warn!(
                    "[federation] pending pairings could not be revoked by the rotation: {error}"
                );
                0
            });
            (previous, next, rotation, revoked)
        };
        if revoked > 0 {
            log::info!(
                "[federation] {revoked} pending pairing(s) revoked by the rotation: they were started under the retired identity and must be paired again"
            );
        }
        log::info!(
            "[federation] identity rotated: companion {} is now companion {}",
            previous.companion_id(),
            next.companion_id()
        );
        let notice = serde_json::to_vec(&TransportMessage::KeyRotation {
            version: FEDERATION_VERSION,
            rotation: Box::new(rotation.clone()),
        })
        .expect("transport messages serialize");
        let mut notified = Vec::new();
        let mut unreachable = Vec::new();
        for record in self.peers.list() {
            if record.state != PeerState::Paired {
                continue;
            }
            let acknowledged = self
                .notify_rotation(&previous, &next, &record, &notice)
                .await;
            if acknowledged {
                notified.push(record.identity.companion_id);
            } else {
                unreachable.push(record.identity.companion_id);
            }
        }
        Ok(RotationReport {
            identity: next.document().clone(),
            rotation,
            notified,
            unreachable,
        })
    }

    /// Posts the rotation notice, signed by the retiring key, to the peer's
    /// approved origins until one answers with an ack for the new identity.
    async fn notify_rotation(
        &self,
        previous: &SigningIdentity,
        next: &SigningIdentity,
        peer: &PeerRecord,
        notice: &[u8],
    ) -> bool {
        for origin in &peer.approved_origins {
            let url = format!("{origin}{ROTATE_PATH}");
            let outcome = match self.seal_as(previous, peer.companion_id(), notice) {
                Ok(outgoing) => match self.transport.post_transport(&url, &outgoing).await {
                    Ok(answer) => self.check_rotation_ack(next, peer, &answer),
                    Err(error) => Err(error),
                },
                Err(error) => Err(error),
            };
            match outcome {
                Ok(()) => return true,
                Err(error) => log::warn!(
                    "[federation] companion {} at {origin} did not acknowledge the rotation: {error}",
                    peer.companion_id()
                ),
            }
        }
        false
    }

    /// The ack is a transport envelope for the new identity; `open` verifies
    /// it against the peer's current key and consumes its nonce.
    fn check_rotation_ack(
        &self,
        next: &SigningIdentity,
        peer: &PeerRecord,
        answer: &TransportEnvelope,
    ) -> Result<(), FederationError> {
        let inbound = self.open(answer)?;
        if inbound.peer.companion_id() != peer.companion_id() {
            return Err(FederationError::SenderMismatch);
        }
        let TransportMessage::RotationAck {
            version,
            companion_id,
            state,
        } = parse_transport_message(&inbound.body)?
        else {
            return Err(FederationError::Malformed("expected a rotation ack".into()));
        };
        check_version(version)?;
        if companion_id != next.companion_id() {
            return Err(FederationError::RotationMismatch);
        }
        if state != PeerState::Paired {
            return Err(FederationError::PeerNotPaired { state });
        }
        Ok(())
    }

    /// The peer's side of a rotation: the notice must be signed by the key
    /// being retired, endorse a new identity that verifies, and fit the
    /// record on file. Answered with a sealed ack addressed to the new
    /// identity. A notice resent for a rotation already applied is
    /// acknowledged again.
    pub fn receive_rotation(
        &self,
        envelope: &TransportEnvelope,
    ) -> Result<TransportEnvelope, FederationError> {
        let inbound = self.open(envelope)?;
        let TransportMessage::KeyRotation { version, rotation } =
            parse_transport_message(&inbound.body)?
        else {
            return Err(FederationError::Malformed("expected a key rotation".into()));
        };
        check_version(version)?;
        // The notice must come from the key it retires.
        if rotation.previous.companion_id != envelope.sender {
            return Err(FederationError::RotationMismatch);
        }
        let (_, next) = rotation::verify_rotation(&rotation)?;
        let peer = self
            .peers
            .rotate(&rotation, next)?
            .ok_or(FederationError::UnknownPeer)?;
        // Signed by the peer's current key: the record was not re-keyed
        // yet, so this notice applied it. A resend inside the grace window
        // arrives under a previous key and changed nothing.
        if inbound.key == SenderKey::Current {
            log::info!(
                "[federation] companion {} rotated its key and is now companion {}",
                rotation.previous.companion_id,
                peer.record.companion_id()
            );
        }
        self.seal(
            peer.record.companion_id(),
            &serde_json::to_vec(&TransportMessage::RotationAck {
                version: FEDERATION_VERSION,
                companion_id: peer.record.identity.companion_id.clone(),
                state: peer.record.state,
            })
            .expect("transport messages serialize"),
        )
    }

    /// The accepting owner's step: redeem `accept` against the issuer at
    /// `accept.origin`, reporting `own_origin` as where this server can be
    /// reached. On success the issuer is a `pending` peer awaiting its
    /// owner's confirmation.
    pub async fn accept_invite(
        &self,
        accept: AcceptInvite,
        own_origin: &str,
    ) -> Result<PeerRecord, FederationError> {
        let AcceptInvite {
            origin,
            secret,
            issuer: issuer_document,
        } = accept;
        let origin = normalize_origin(&origin)?;
        let own_origin = normalize_origin(own_origin)?;
        // Pin the issuer before anything goes over the wire: whatever answers
        // at `origin` must sign with this key.
        let issuer = identity::verify_document(&issuer_document)?;
        let me = self.identity()?;
        if issuer.public_key == me.verified().public_key {
            return Err(FederationError::IssuerMismatch);
        }
        let outgoing = sign_message(
            &me,
            &PairingMessage::Request {
                version: FEDERATION_VERSION,
                secret,
                issuer: issuer.companion_id.clone(),
                accepter: me.document().clone(),
                origin: own_origin,
            },
        );
        let answer = self
            .transport
            .post(&format!("{origin}{PAIR_PATH}"), &outgoing)
            .await?;
        let body = identity::verify_envelope(&answer, &issuer)?;
        let PairingMessage::Response {
            version,
            pairing_id,
            issuer: answering_document,
            accepter,
            state,
        } = parse_message(&body)?
        else {
            return Err(FederationError::Malformed(
                "expected a pair response".into(),
            ));
        };
        check_version(version)?;
        if answering_document != issuer_document {
            return Err(FederationError::IssuerMismatch);
        }
        if accepter != me.companion_id() {
            return Err(FederationError::RecipientMismatch);
        }
        if state != PeerState::Pending {
            return Err(FederationError::Malformed(
                "pair response reports an unexpected state".into(),
            ));
        }
        check_pairing_id(&pairing_id)?;
        let record = self.fresh_record(
            issuer_document,
            PairingRole::Accepter,
            pairing_id,
            vec![origin],
            None,
        );
        let record = self.peers.upsert(record, issuer)?;
        log::info!(
            "[federation] pairing {}: companion {} accepted, waiting for its owner",
            record.pairing_id,
            record.companion_id()
        );
        Ok(record)
    }

    /// The issuer's side of step 2: verifies a `pair_request`, redeems the
    /// invite, records the accepter as `pending`, and answers.
    pub fn receive_pair_request(
        &self,
        envelope: &SignedEnvelope,
    ) -> Result<SignedEnvelope, FederationError> {
        check_version(envelope.version)?;
        // The sender is unknown by construction, so its document travels in
        // the body: decode, verify the document, then verify the envelope
        // against it before anything else is believed.
        let body = decode(&envelope.body)
            .map(Zeroizing::new)
            .ok_or_else(|| FederationError::Malformed("body is not canonical base64url".into()))?;
        let PairingMessage::Request {
            version,
            secret,
            issuer,
            accepter,
            origin,
        } = parse_message(&body)?
        else {
            return Err(FederationError::Malformed("expected a pair request".into()));
        };
        check_version(version)?;
        let accepter_identity = identity::verify_document(&accepter)?;
        identity::verify_envelope(envelope, &accepter_identity)?;
        // Under the identity lock: the record is created under the identity
        // that answers, and a rotation landing meanwhile either finds it
        // (and revokes it) or has already withdrawn the invite.
        let (me, record, redeemed) = self.with_identity(|me| {
            if issuer != me.companion_id()
                || accepter_identity.public_key == me.verified().public_key
            {
                return Err(FederationError::IssuerMismatch);
            }
            let origin = normalize_origin(&origin)?;
            let redeemed = self.peers.redeem_invite(&secret)?;
            // A companion that was pending, paired, or revoked before starts
            // over: its owner redeemed a fresh invite, and ours confirms again.
            let record = self.fresh_record(
                accepter,
                PairingRole::Issuer,
                redeemed.id.clone(),
                Vec::new(),
                Some(origin),
            );
            let record = self.peers.upsert(record, accepter_identity)?;
            Ok((me.clone(), record, redeemed))
        })?;
        log::info!(
            "[federation] pairing {}: companion {} redeemed the invite, waiting for the owner",
            record.pairing_id,
            record.companion_id()
        );
        Ok(sign_message(
            &me,
            &PairingMessage::Response {
                version: FEDERATION_VERSION,
                pairing_id: redeemed.id,
                issuer: me.document().clone(),
                accepter: record.identity.companion_id.clone(),
                state: PeerState::Pending,
            },
        ))
    }

    /// The issuing owner's confirmation. Returns the record and whether the
    /// peer acknowledged the notice; an unreachable peer does not undo the
    /// local confirmation (confirming again resends).
    ///
    /// The state check and the transition run under the store lock, so a
    /// revoke that lands meanwhile wins: the confirmation then fails with
    /// `PeerRevoked` and nothing is sent. They also run under the identity
    /// lock, with the key that signs the notice chosen there, so a rotation
    /// that lands meanwhile wins the same way: it revokes the pending record
    /// first, or the record is paired and told under the key the peer knows.
    pub async fn confirm_peer(
        &self,
        companion_id: &str,
    ) -> Result<(PeerRecord, bool), FederationError> {
        let mut paired_now = false;
        let mut notify = true;
        let (me, peer) = self.with_identity(|me| {
            let peer = self
                .peers
                .update(companion_id, |record| match (record.role, record.state) {
                    (_, PeerState::Revoked) => Err(FederationError::PeerRevoked),
                    (PairingRole::Issuer, PeerState::Pending) => {
                        record.state = PeerState::Paired;
                        record.approved_origins =
                            record.pending_origin.take().into_iter().collect();
                        paired_now = true;
                        Ok(())
                    }
                    (PairingRole::Issuer, PeerState::Paired) => Ok(()),
                    (PairingRole::Accepter, PeerState::Paired) => {
                        notify = false;
                        Ok(())
                    }
                    (_, state) => Err(FederationError::PeerNotPaired { state }),
                })?
                .ok_or(FederationError::UnknownPeer)?;
            Ok((me.clone(), peer))
        })?;
        if paired_now {
            log::info!(
                "[federation] pairing {}: companion {} paired by the owner",
                peer.record.pairing_id,
                peer.record.companion_id()
            );
        }
        if !notify {
            return Ok((peer.record, false));
        }
        let notice = PairingMessage::Confirm {
            version: FEDERATION_VERSION,
            pairing_id: peer.record.pairing_id.clone(),
            issuer: me.companion_id().to_owned(),
            accepter: peer.record.identity.companion_id.clone(),
        };
        let notified = self
            .notify(&me, &peer, &notice, CONFIRM_PATH, PeerState::Paired)
            .await;
        Ok((peer.record, notified))
    }

    /// The accepter's side of step 3.
    pub fn receive_confirm(
        &self,
        envelope: &SignedEnvelope,
    ) -> Result<SignedEnvelope, FederationError> {
        let (peer, body) = self.verify_known_sender(envelope)?;
        let PairingMessage::Confirm {
            version,
            pairing_id,
            issuer,
            accepter,
        } = parse_message(&body)?
        else {
            return Err(FederationError::Malformed(
                "expected a pair confirmation".into(),
            ));
        };
        check_version(version)?;
        let me = self.identity()?;
        if issuer != peer.record.companion_id() {
            return Err(FederationError::SenderMismatch);
        }
        if accepter != me.companion_id() {
            return Err(FederationError::RecipientMismatch);
        }
        // Everything about the record's state is checked under the store
        // lock, against the record as it is now: a revoke that landed since
        // the signature was verified is never overwritten, and a replayed
        // notice cannot revive a revoked peer.
        let mut paired_now = false;
        let peer = self
            .peers
            .update(peer.record.companion_id(), |record| {
                if record.state == PeerState::Revoked {
                    return Err(FederationError::PeerRevoked);
                }
                if pairing_id != record.pairing_id {
                    return Err(FederationError::PairingMismatch);
                }
                if record.role != PairingRole::Accepter {
                    return Err(FederationError::Malformed(
                        "only the issuing side confirms a pairing".into(),
                    ));
                }
                match record.state {
                    PeerState::Pending => {
                        record.state = PeerState::Paired;
                        paired_now = true;
                        Ok(())
                    }
                    // Already paired: the notice was resent or replayed,
                    // and nothing changes.
                    PeerState::Paired => Ok(()),
                    state => Err(FederationError::PeerNotPaired { state }),
                }
            })?
            .ok_or(FederationError::UnknownPeer)?;
        if paired_now {
            log::info!(
                "[federation] pairing {}: companion {} confirmed the pairing",
                peer.record.pairing_id,
                peer.record.companion_id()
            );
        }
        Ok(self.ack(&me, &peer, PeerState::Paired))
    }

    /// Withdraws trust locally and tells the peer. Returns the record and
    /// whether the peer acknowledged; revoking an already revoked peer is a
    /// no-op. A revoke wins against any confirmation in flight.
    pub async fn revoke_peer(
        &self,
        companion_id: &str,
    ) -> Result<(PeerRecord, bool), FederationError> {
        let mut already_revoked = false;
        let peer = self
            .peers
            .update(companion_id, |record| {
                if record.state == PeerState::Revoked {
                    already_revoked = true;
                } else {
                    record.state = PeerState::Revoked;
                    record.pending_origin = None;
                }
                Ok(())
            })?
            .ok_or(FederationError::UnknownPeer)?;
        if already_revoked {
            return Ok((peer.record, false));
        }
        log::info!(
            "[federation] pairing {}: companion {} revoked by the owner",
            peer.record.pairing_id,
            peer.record.companion_id()
        );
        let me = self.identity()?;
        let notice = PairingMessage::Revoke {
            version: FEDERATION_VERSION,
            pairing_id: peer.record.pairing_id.clone(),
            sender: me.companion_id().to_owned(),
            peer: peer.record.identity.companion_id.clone(),
        };
        let notified = self
            .notify(&me, &peer, &notice, REVOKE_PATH, PeerState::Revoked)
            .await;
        Ok((peer.record, notified))
    }

    /// The peer's side of step 4.
    pub fn receive_revoke(
        &self,
        envelope: &SignedEnvelope,
    ) -> Result<SignedEnvelope, FederationError> {
        let (peer, body) = self.verify_known_sender(envelope)?;
        let PairingMessage::Revoke {
            version,
            pairing_id,
            sender,
            peer: recipient,
        } = parse_message(&body)?
        else {
            return Err(FederationError::Malformed(
                "expected a pair revocation".into(),
            ));
        };
        check_version(version)?;
        let me = self.identity()?;
        if sender != peer.record.companion_id() {
            return Err(FederationError::SenderMismatch);
        }
        if recipient != me.companion_id() {
            return Err(FederationError::RecipientMismatch);
        }
        // Checked and applied under the store lock, like a confirmation.
        let mut revoked_now = false;
        let peer = self
            .peers
            .update(peer.record.companion_id(), |record| {
                if pairing_id != record.pairing_id {
                    return Err(FederationError::PairingMismatch);
                }
                if record.state != PeerState::Revoked {
                    record.state = PeerState::Revoked;
                    record.pending_origin = None;
                    revoked_now = true;
                }
                Ok(())
            })?
            .ok_or(FederationError::UnknownPeer)?;
        if revoked_now {
            log::info!(
                "[federation] pairing {}: companion {} revoked the pairing",
                peer.record.pairing_id,
                peer.record.companion_id()
            );
        }
        Ok(self.ack(&me, &peer, PeerState::Revoked))
    }

    /// Verifies an envelope from a paired peer and returns the peer and the
    /// body. Unknown, pending, and revoked senders fail closed.
    // The signed transport (#108, PR 3) is the first production caller.
    #[allow(dead_code)]
    pub fn verify_from_peer(
        &self,
        envelope: &SignedEnvelope,
    ) -> Result<(PeerRecord, Vec<u8>), FederationError> {
        let (peer, body) = self.verify_known_sender(envelope)?;
        match peer.record.state {
            PeerState::Paired => Ok((peer.record, body)),
            PeerState::Revoked => Err(FederationError::PeerRevoked),
            state => Err(FederationError::PeerNotPaired { state }),
        }
    }

    /// Looks the sender up in the peer store and verifies the signature with
    /// the stored key, whatever the peer's state.
    fn verify_known_sender(
        &self,
        envelope: &SignedEnvelope,
    ) -> Result<(Peer, Vec<u8>), FederationError> {
        let peer = self
            .peers
            .get(&envelope.sender)
            .ok_or(FederationError::UnknownPeer)?;
        let body = identity::verify_envelope(envelope, &peer.verified)?;
        self.seen(peer.record.companion_id());
        Ok((peer, body))
    }

    /// A record for a (re)started pairing. An earlier record for the same
    /// companion keeps its creation time and rotation history.
    fn fresh_record(
        &self,
        document: IdentityDocument,
        role: PairingRole,
        pairing_id: String,
        approved_origins: Vec<String>,
        pending_origin: Option<String>,
    ) -> PeerRecord {
        let now = (self.clock)();
        let previous = self.peers.get(&document.companion_id);
        PeerRecord {
            identity: document,
            state: PeerState::Pending,
            role,
            pairing_id,
            approved_origins,
            pending_origin,
            created_at: previous
                .as_ref()
                .map_or(now, |previous| previous.record.created_at),
            updated_at: now,
            // A record is created only after something the peer signed
            // verified (its request, or its answer to ours).
            last_seen_at: Some(now),
            rotation_history: previous
                .map(|previous| previous.record.rotation_history)
                .unwrap_or_default(),
        }
    }

    /// Posts `notice`, signed as `me`, to the peer's approved origins until
    /// one acknowledges it with a signed ack for this pairing. `me` is the
    /// identity the caller made its change under, not whatever is current
    /// by now. Failures are reported, never fatal: the local state already
    /// changed.
    async fn notify(
        &self,
        me: &SigningIdentity,
        peer: &Peer,
        notice: &PairingMessage,
        path: &str,
        expected: PeerState,
    ) -> bool {
        let outgoing = sign_message(me, notice);
        for origin in &peer.record.approved_origins {
            let url = format!("{origin}{path}");
            let outcome = match self.transport.post(&url, &outgoing).await {
                Ok(answer) => self.check_ack(peer, &answer, expected),
                Err(error) => Err(error),
            };
            match outcome {
                Ok(()) => return true,
                Err(error) => log::warn!(
                    "[federation] pairing {}: companion {} at {origin} did not acknowledge: {error}",
                    peer.record.pairing_id,
                    peer.record.companion_id()
                ),
            }
        }
        false
    }

    fn check_ack(
        &self,
        peer: &Peer,
        answer: &SignedEnvelope,
        expected: PeerState,
    ) -> Result<(), FederationError> {
        let body = identity::verify_envelope(answer, &peer.verified)?;
        self.seen(peer.record.companion_id());
        let PairingMessage::Ack {
            version,
            pairing_id,
            sender,
            state,
        } = parse_message(&body)?
        else {
            return Err(FederationError::Malformed("expected a pair ack".into()));
        };
        check_version(version)?;
        if sender != peer.record.companion_id() {
            return Err(FederationError::SenderMismatch);
        }
        if pairing_id != peer.record.pairing_id {
            return Err(FederationError::PairingMismatch);
        }
        if state != expected {
            return Err(FederationError::PeerNotPaired { state });
        }
        Ok(())
    }

    fn ack(&self, me: &SigningIdentity, peer: &Peer, state: PeerState) -> SignedEnvelope {
        sign_message(
            me,
            &PairingMessage::Ack {
                version: FEDERATION_VERSION,
                pairing_id: peer.record.pairing_id.clone(),
                sender: me.companion_id().to_owned(),
                state,
            },
        )
    }
}

/// Signs a pairing message as `signer`.
pub(crate) fn sign_message(signer: &SigningIdentity, message: &PairingMessage) -> SignedEnvelope {
    let body = Zeroizing::new(serde_json::to_vec(message).expect("pairing messages serialize"));
    signer.sign_envelope(&body)
}

/// Parses a verified transport body; no secret travels in one, but the
/// detail is discarded all the same so no body is ever quoted back.
fn parse_transport_message(body: &[u8]) -> Result<TransportMessage, FederationError> {
    serde_json::from_slice(body).map_err(|_| {
        FederationError::Malformed("transport message does not have the expected shape".into())
    })
}

/// Parses a verified body. serde's detail is discarded on purpose: it could
/// quote a field of the body, and one of them is the invite secret.
fn parse_message(body: &[u8]) -> Result<PairingMessage, FederationError> {
    serde_json::from_slice(body).map_err(|_| {
        FederationError::Malformed("pairing message does not have the expected shape".into())
    })
}

fn check_pairing_id(pairing_id: &str) -> Result<(), FederationError> {
    if pairing_id.is_empty()
        || pairing_id.len() > MAX_PAIRING_ID_LEN
        || !pairing_id.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        return Err(FederationError::Malformed(
            "pairing id is not a short alphanumeric token".into(),
        ));
    }
    Ok(())
}

/// Accepts `http(s)://host[:port][/path]` and returns it without a trailing
/// slash, with the scheme and host lower-cased. Userinfo, query strings, and
/// fragments are refused: a base URL never carries anything secret.
pub fn normalize_origin(input: &str) -> Result<String, FederationError> {
    let trimmed = input.trim();
    let refuse = || FederationError::InvalidOrigin(trimmed.chars().take(MAX_ORIGIN_LEN).collect());
    if trimmed.is_empty() || trimmed.len() > MAX_ORIGIN_LEN {
        return Err(refuse());
    }
    let url = reqwest::Url::parse(trimmed).map_err(|_| refuse())?;
    let host = url
        .host_str()
        .filter(|host| !host.is_empty())
        .ok_or_else(refuse)?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(refuse());
    }
    let path = url.path().trim_end_matches('/');
    if path.split('/').any(|segment| segment == "federation") {
        return Err(refuse());
    }
    let port = url
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    Ok(format!("{}://{host}{port}{path}", url.scheme()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::federation::{
        FEDERATION_VERSION, InviteSecret, PairingRole, PeerState, SignedEnvelope,
    };
    use std::{
        collections::HashMap,
        sync::atomic::{AtomicU64, Ordering},
    };

    const T0: u64 = 1_800_000_000;
    const ORIGIN_A: &str = "https://a.example";
    const ORIGIN_B: &str = "http://localhost:26560";

    /// Routes envelopes straight into another in-process `FederationState`,
    /// by base URL, exactly as the HTTP routes would.
    #[derive(Default)]
    struct Direct {
        servers: Mutex<HashMap<String, Arc<FederationState>>>,
    }

    impl Direct {
        fn connect(&self, origin: &str, server: Arc<FederationState>) {
            self.servers
                .lock()
                .unwrap()
                .insert(origin.to_owned(), server);
        }
    }

    impl PeerTransport for Direct {
        fn post<'a>(
            &'a self,
            url: &'a str,
            envelope: &'a SignedEnvelope,
        ) -> BoxFuture<'a, Result<SignedEnvelope, FederationError>> {
            Box::pin(async move {
                let (origin, path) = url
                    .find("/federation/")
                    .map(|at| (&url[..at], &url[at..]))
                    .expect("peer URLs are base URL plus a federation path");
                let server = self
                    .servers
                    .lock()
                    .unwrap()
                    .get(origin)
                    .cloned()
                    .ok_or_else(|| FederationError::Transport(format!("no route to {origin}")))?;
                match path {
                    PAIR_PATH => server.receive_pair_request(envelope),
                    CONFIRM_PATH => server.receive_confirm(envelope),
                    REVOKE_PATH => server.receive_revoke(envelope),
                    other => Err(FederationError::Transport(format!("no route {other}"))),
                }
            })
        }

        fn post_transport<'a>(
            &'a self,
            url: &'a str,
            envelope: &'a TransportEnvelope,
        ) -> BoxFuture<'a, Result<TransportEnvelope, FederationError>> {
            Box::pin(async move {
                let (origin, path) = url
                    .find("/federation/")
                    .map(|at| (&url[..at], &url[at..]))
                    .expect("peer URLs are base URL plus a federation path");
                let server = self
                    .servers
                    .lock()
                    .unwrap()
                    .get(origin)
                    .cloned()
                    .ok_or_else(|| FederationError::Transport(format!("no route to {origin}")))?;
                match path {
                    PING_PATH => server.receive_ping(envelope),
                    ROTATE_PATH => server.receive_rotation(envelope),
                    other => Err(FederationError::Transport(format!("no route {other}"))),
                }
            })
        }
    }

    struct Network {
        direct: Arc<Direct>,
        now: Arc<AtomicU64>,
        _dirs: Vec<tempfile::TempDir>,
    }

    impl Network {
        fn new() -> Self {
            Self {
                direct: Arc::new(Direct::default()),
                now: Arc::new(AtomicU64::new(T0)),
                _dirs: Vec::new(),
            }
        }

        fn server(&mut self, origin: &str) -> Arc<FederationState> {
            let dir = tempfile::tempdir().unwrap();
            let read = self.now.clone();
            let server = Arc::new(FederationState::with_transport_and_clock(
                dir.path(),
                self.direct.clone(),
                Arc::new(move || read.load(Ordering::SeqCst)),
            ));
            self.direct.connect(origin, server.clone());
            self._dirs.push(dir);
            server
        }
    }

    fn accept_for(invite: &IssuedInvite) -> AcceptInvite {
        AcceptInvite {
            origin: invite.origin.clone(),
            secret: invite.secret.clone(),
            issuer: invite.issuer.clone(),
        }
    }

    async fn paired(network: &mut Network) -> (Arc<FederationState>, Arc<FederationState>) {
        let a = network.server(ORIGIN_A);
        let b = network.server(ORIGIN_B);
        let invite = a.create_invite(ORIGIN_A).unwrap();
        b.accept_invite(accept_for(&invite), ORIGIN_B)
            .await
            .unwrap();
        let (_, notified) = a
            .confirm_peer(b.identity().unwrap().companion_id())
            .await
            .unwrap();
        assert!(notified);
        (a, b)
    }

    fn ping(from: &FederationState) -> SignedEnvelope {
        from.identity()
            .unwrap()
            .sign_envelope(br#"{"kind":"ping"}"#)
    }

    #[test]
    fn origins_are_base_urls_without_secrets_or_credentials() {
        for (input, expected) in [
            ("https://A.Example/", "https://a.example"),
            ("HTTPS://a.example", "https://a.example"),
            ("http://localhost:26560", "http://localhost:26560"),
            ("http://127.0.0.1:26560/", "http://127.0.0.1:26560"),
            ("https://a.example/nolune/", "https://a.example/nolune"),
            ("https://a.example:8443/x/y", "https://a.example:8443/x/y"),
            ("  https://a.example  ", "https://a.example"),
        ] {
            assert_eq!(normalize_origin(input).as_deref(), Ok(expected), "{input}");
        }
        for rejected in [
            "",
            "a.example",
            "ftp://a.example",
            "file:///tmp/x",
            "https://",
            "https://user:pw@a.example",
            "https://a.example/?secret=x",
            "https://a.example/#frag",
            "https://a.example/x?y=1",
            "https://a.example/federation/v1/pair",
            "https://a.exa mple",
            "http://[::1",
        ] {
            assert!(
                matches!(
                    normalize_origin(rejected),
                    Err(FederationError::InvalidOrigin(_))
                ),
                "accepted {rejected:?}"
            );
        }
        let long = format!("https://{}.example", "a".repeat(MAX_ORIGIN_LEN));
        assert!(matches!(
            normalize_origin(&long),
            Err(FederationError::InvalidOrigin(_))
        ));
    }

    #[test]
    fn identity_is_created_under_the_workspace_root_on_first_use() {
        let tmp = tempfile::tempdir().unwrap();
        let state = FederationState::with_transport(tmp.path(), Arc::new(Direct::default()));
        assert!(
            !tmp.path().join("federation").exists(),
            "nothing before first use"
        );
        let first = state.identity().unwrap();
        assert!(tmp.path().join("federation/identity.json").is_file());
        assert!(tmp.path().join("federation/signing_key.json").is_file());
        let again = state.identity().unwrap();
        assert_eq!(first.companion_id(), again.companion_id());
        assert_eq!(
            identity::load(tmp.path()).unwrap().unwrap().companion_id(),
            first.companion_id(),
            "the keystore is the one identity.rs manages"
        );
        let overview = state.overview().unwrap();
        assert_eq!(overview.companion_id, first.companion_id());
        assert_eq!(&overview.identity, first.document());
        assert!(overview.invites.is_empty() && overview.peers.is_empty());
    }

    #[tokio::test]
    async fn two_companions_pair_through_both_owners_and_bind_keys() {
        let mut network = Network::new();
        let a = network.server(ORIGIN_A);
        let b = network.server(ORIGIN_B);
        let a_id = a.identity().unwrap().companion_id().to_owned();
        let b_id = b.identity().unwrap().companion_id().to_owned();

        let invite = a.create_invite("https://A.example/").unwrap();
        assert_eq!(
            invite.origin, ORIGIN_A,
            "the invite carries the normalized origin"
        );
        assert_eq!(&invite.issuer, a.identity().unwrap().document());
        assert_eq!(invite.expires_in_secs, super::super::peers::INVITE_TTL_SECS);
        let overview = a.overview().unwrap();
        assert_eq!(overview.invites.len(), 1);
        assert_eq!(overview.invites[0].id, invite.id);
        assert_eq!(overview.invites[0].state, PeerState::Invited);

        let on_b = b
            .accept_invite(accept_for(&invite), "http://LOCALHOST:26560/")
            .await
            .unwrap();
        assert_eq!(on_b.companion_id(), a_id);
        assert_eq!(on_b.identity, invite.issuer);
        assert_eq!(on_b.state, PeerState::Pending);
        assert_eq!(on_b.role, PairingRole::Accepter);
        assert_eq!(on_b.pairing_id, invite.id);
        assert_eq!(on_b.approved_origins, vec![ORIGIN_A.to_owned()]);
        assert_eq!(on_b.pending_origin, None);

        let overview = a.overview().unwrap();
        assert!(overview.invites.is_empty(), "the invite was consumed");
        assert_eq!(overview.peers.len(), 1);
        let on_a = &overview.peers[0];
        assert_eq!(on_a.companion_id, b_id);
        assert_eq!(on_a.public_key, b.identity().unwrap().document().public_key);
        assert_eq!(on_a.state, PeerState::Pending);
        assert_eq!(on_a.role, PairingRole::Issuer);
        assert_eq!(on_a.pairing_id, invite.id);
        assert!(
            on_a.approved_origins.is_empty(),
            "nothing approved before the owner confirms"
        );
        assert_eq!(on_a.pending_origin.as_deref(), Some(ORIGIN_B));

        // Nothing verifies before the issuing owner confirms.
        assert_eq!(
            a.verify_from_peer(&ping(&b)).unwrap_err(),
            FederationError::PeerNotPaired {
                state: PeerState::Pending
            }
        );
        assert_eq!(
            b.verify_from_peer(&ping(&a)).unwrap_err(),
            FederationError::PeerNotPaired {
                state: PeerState::Pending
            }
        );
        assert_eq!(
            b.confirm_peer(&a_id).await.unwrap_err(),
            FederationError::PeerNotPaired {
                state: PeerState::Pending
            },
            "only the issuer confirms"
        );

        let (confirmed, notified) = a.confirm_peer(&b_id).await.unwrap();
        assert!(notified);
        assert_eq!(confirmed.state, PeerState::Paired);
        assert_eq!(confirmed.approved_origins, vec![ORIGIN_B.to_owned()]);
        assert_eq!(confirmed.pending_origin, None);
        let on_b = b.overview().unwrap().peers;
        assert_eq!(on_b.len(), 1);
        assert_eq!(on_b[0].state, PeerState::Paired);
        assert_eq!(on_b[0].companion_id, a_id);

        // Both sides bind the same ids to the same keys.
        let a_view = a.overview().unwrap();
        assert_eq!(
            a_view.peers[0].public_key,
            b.identity().unwrap().document().public_key
        );
        assert_eq!(
            on_b[0].public_key,
            a.identity().unwrap().document().public_key
        );

        let (peer, body) = a.verify_from_peer(&ping(&b)).unwrap();
        assert_eq!(peer.companion_id(), b_id);
        assert_eq!(body, br#"{"kind":"ping"}"#);
        let (peer, _) = b.verify_from_peer(&ping(&a)).unwrap();
        assert_eq!(peer.companion_id(), a_id);

        // Replaying the invite fails on the wire and leaves both stores alone.
        assert_eq!(
            b.accept_invite(accept_for(&invite), ORIGIN_B)
                .await
                .unwrap_err(),
            FederationError::InviteInvalid
        );
        let c = network.server("https://c.example");
        assert_eq!(
            c.accept_invite(accept_for(&invite), "https://c.example")
                .await
                .unwrap_err(),
            FederationError::InviteInvalid
        );
        assert_eq!(a.overview().unwrap().peers.len(), 1);
        assert!(c.overview().unwrap().peers.is_empty());

        // Confirming again is idempotent and resends the notice.
        let (again, notified) = a.confirm_peer(&b_id).await.unwrap();
        assert!(notified);
        assert_eq!(again.state, PeerState::Paired);

        // Unknown senders and tampered envelopes fail closed.
        assert_eq!(
            a.verify_from_peer(&ping(&c)).unwrap_err(),
            FederationError::UnknownPeer
        );
        let mut forged = ping(&b);
        forged.body = super::super::encode(br#"{"kind":"pong"}"#);
        assert_eq!(
            a.verify_from_peer(&forged).unwrap_err(),
            FederationError::SignatureMismatch
        );
        let mut renamed = ping(&c);
        renamed.sender = b_id.clone();
        assert_eq!(
            a.verify_from_peer(&renamed).unwrap_err(),
            FederationError::SignatureMismatch
        );
    }

    #[tokio::test]
    async fn revoking_on_either_side_stops_verification_on_both() {
        for revoker_is_issuer in [true, false] {
            let mut network = Network::new();
            let (a, b) = paired(&mut network).await;
            let a_id = a.identity().unwrap().companion_id().to_owned();
            let b_id = b.identity().unwrap().companion_id().to_owned();
            assert!(a.verify_from_peer(&ping(&b)).is_ok());
            assert!(b.verify_from_peer(&ping(&a)).is_ok());

            let (revoked, notified) = if revoker_is_issuer {
                a.revoke_peer(&b_id).await.unwrap()
            } else {
                b.revoke_peer(&a_id).await.unwrap()
            };
            assert!(notified, "issuer revoked: {revoker_is_issuer}");
            assert_eq!(revoked.state, PeerState::Revoked);
            assert_eq!(
                a.verify_from_peer(&ping(&b)).unwrap_err(),
                FederationError::PeerRevoked
            );
            assert_eq!(
                b.verify_from_peer(&ping(&a)).unwrap_err(),
                FederationError::PeerRevoked
            );
            assert_eq!(a.overview().unwrap().peers[0].state, PeerState::Revoked);
            assert_eq!(b.overview().unwrap().peers[0].state, PeerState::Revoked);

            // Revoking again is a no-op that sends nothing.
            let (again, notified) = a.revoke_peer(&b_id).await.unwrap();
            assert!(!notified);
            assert_eq!(again.state, PeerState::Revoked);
            assert_eq!(
                a.revoke_peer("nobody").await.unwrap_err(),
                FederationError::UnknownPeer
            );

            // A stale confirmation cannot revive a revoked peer.
            let stale = sign_message(
                &a.identity().unwrap(),
                &PairingMessage::Confirm {
                    version: FEDERATION_VERSION,
                    pairing_id: revoked.pairing_id.clone(),
                    issuer: a_id.clone(),
                    accepter: b_id.clone(),
                },
            );
            assert_eq!(
                b.receive_confirm(&stale).unwrap_err(),
                FederationError::PeerRevoked
            );
            assert_eq!(b.overview().unwrap().peers[0].state, PeerState::Revoked);

            // A new invite pairs them again, under a new pairing id.
            let invite = a.create_invite(ORIGIN_A).unwrap();
            b.accept_invite(accept_for(&invite), ORIGIN_B)
                .await
                .unwrap();
            let (paired, notified) = a.confirm_peer(&b_id).await.unwrap();
            assert!(notified);
            assert_eq!(paired.state, PeerState::Paired);
            assert_ne!(paired.pairing_id, revoked.pairing_id);
            assert_eq!(
                paired.created_at, revoked.created_at,
                "the record keeps its history"
            );
            assert!(a.verify_from_peer(&ping(&b)).is_ok());
            assert!(b.verify_from_peer(&ping(&a)).is_ok());
            assert_eq!(a.overview().unwrap().peers.len(), 1);
            assert_eq!(b.overview().unwrap().peers.len(), 1);

            // The old revocation notice names the old pairing and is stale.
            let old_revoke = sign_message(
                &a.identity().unwrap(),
                &PairingMessage::Revoke {
                    version: FEDERATION_VERSION,
                    pairing_id: revoked.pairing_id.clone(),
                    sender: a_id.clone(),
                    peer: b_id.clone(),
                },
            );
            assert_eq!(
                b.receive_revoke(&old_revoke).unwrap_err(),
                FederationError::PairingMismatch
            );
            assert!(b.verify_from_peer(&ping(&a)).is_ok());
        }
    }

    #[tokio::test]
    async fn pair_requests_fail_closed_on_spoofing_and_downgrade() {
        let mut network = Network::new();
        let a = network.server(ORIGIN_A);
        let b = network.server(ORIGIN_B);
        let c = network.server("https://c.example");
        let a_id = a.identity().unwrap().companion_id().to_owned();
        let b_identity = b.identity().unwrap();
        let c_identity = c.identity().unwrap();
        let invite = a.create_invite(ORIGIN_A).unwrap();

        let request = |secret: &str, issuer: &str, accepter: &IdentityDocument, origin: &str| {
            PairingMessage::Request {
                version: FEDERATION_VERSION,
                secret: InviteSecret::new(secret.into()),
                issuer: issuer.into(),
                accepter: accepter.clone(),
                origin: origin.into(),
            }
        };
        let genuine = request(
            invite.secret.expose(),
            &a_id,
            b_identity.document(),
            ORIGIN_B,
        );

        // Signed by C but claiming to be B.
        let spoofed = sign_message(&c_identity, &genuine);
        assert_eq!(
            a.receive_pair_request(&spoofed).unwrap_err(),
            FederationError::SenderMismatch
        );
        let mut relabelled = spoofed.clone();
        relabelled.sender = b_identity.companion_id().to_owned();
        assert_eq!(
            a.receive_pair_request(&relabelled).unwrap_err(),
            FederationError::SignatureMismatch
        );

        // Addressed to another issuer.
        let elsewhere = sign_message(
            &b_identity,
            &request(
                invite.secret.expose(),
                c_identity.companion_id(),
                b_identity.document(),
                ORIGIN_B,
            ),
        );
        assert_eq!(
            a.receive_pair_request(&elsewhere).unwrap_err(),
            FederationError::IssuerMismatch
        );

        // A document that does not verify, and an origin that is not one.
        let mut tampered = b_identity.document().clone();
        tampered.created_at += 1;
        let tampered = sign_message(
            &b_identity,
            &request(invite.secret.expose(), &a_id, &tampered, ORIGIN_B),
        );
        assert_eq!(
            a.receive_pair_request(&tampered).unwrap_err(),
            FederationError::SignatureMismatch
        );
        let bad_origin = sign_message(
            &b_identity,
            &request(
                invite.secret.expose(),
                &a_id,
                b_identity.document(),
                "https://b.example/?token=x",
            ),
        );
        assert!(matches!(
            a.receive_pair_request(&bad_origin).unwrap_err(),
            FederationError::InvalidOrigin(_)
        ));

        // Wrong kind of body, and a downgraded envelope.
        let wrong_kind = sign_message(
            &b_identity,
            &PairingMessage::Ack {
                version: FEDERATION_VERSION,
                pairing_id: invite.id.clone(),
                sender: b_identity.companion_id().to_owned(),
                state: PeerState::Paired,
            },
        );
        assert!(matches!(
            a.receive_pair_request(&wrong_kind).unwrap_err(),
            FederationError::Malformed(_)
        ));
        let mut downgraded = sign_message(&b_identity, &genuine);
        downgraded.version = 0;
        assert_eq!(
            a.receive_pair_request(&downgraded).unwrap_err(),
            FederationError::VersionTooOld { found: 0, min: 1 }
        );

        // Wrong secret: nothing is recorded, the invite survives.
        let wrong = sign_message(
            &b_identity,
            &request("not-it", &a_id, b_identity.document(), ORIGIN_B),
        );
        assert_eq!(
            a.receive_pair_request(&wrong).unwrap_err(),
            FederationError::InviteInvalid
        );
        assert!(a.overview().unwrap().peers.is_empty());
        assert_eq!(a.overview().unwrap().invites.len(), 1);

        // The genuine request works exactly once.
        let response = a
            .receive_pair_request(&sign_message(&b_identity, &genuine))
            .unwrap();
        assert_eq!(response.sender, a_id);
        assert_eq!(
            a.receive_pair_request(&sign_message(&b_identity, &genuine))
                .unwrap_err(),
            FederationError::InviteInvalid
        );

        // An expired invite is refused before anything else about the peer.
        let invite = a.create_invite(ORIGIN_A).unwrap();
        network
            .now
            .fetch_add(super::super::peers::INVITE_TTL_SECS, Ordering::SeqCst);
        assert_eq!(
            c.accept_invite(accept_for(&invite), "https://c.example")
                .await
                .unwrap_err(),
            FederationError::InviteInvalid
        );
        assert!(c.overview().unwrap().peers.is_empty());
    }

    #[tokio::test]
    async fn accepters_pin_the_issuer_and_refuse_impostors() {
        let mut network = Network::new();
        let a = network.server(ORIGIN_A);
        let b = network.server(ORIGIN_B);
        let impostor = network.server("https://impostor.example");
        let invite = a.create_invite(ORIGIN_A).unwrap();

        // The owner was told A's document but a different server answers at
        // the origin: whatever it says is signed by the wrong key.
        let mut misdirected = accept_for(&invite);
        misdirected.origin = "https://impostor.example".into();
        let error = b.accept_invite(misdirected, ORIGIN_B).await.unwrap_err();
        assert!(
            matches!(
                error,
                FederationError::IssuerMismatch | FederationError::InviteInvalid
            ),
            "{error}"
        );
        assert!(b.overview().unwrap().peers.is_empty());
        assert!(impostor.overview().unwrap().peers.is_empty());

        // A document the owner mistyped never gets as far as the wire.
        let mut broken = accept_for(&invite);
        broken.issuer.created_at += 1;
        assert_eq!(
            b.accept_invite(broken, ORIGIN_B).await.unwrap_err(),
            FederationError::SignatureMismatch
        );
        assert!(matches!(
            b.accept_invite(
                AcceptInvite {
                    origin: "a.example".into(),
                    ..accept_for(&invite)
                },
                ORIGIN_B
            )
            .await
            .unwrap_err(),
            FederationError::InvalidOrigin(_)
        ));
        assert!(matches!(
            b.accept_invite(accept_for(&invite), "not a url")
                .await
                .unwrap_err(),
            FederationError::InvalidOrigin(_)
        ));
        assert_eq!(
            a.overview().unwrap().invites.len(),
            1,
            "the invite is untouched"
        );

        // Notices are bound to the pairing and the recipient.
        b.accept_invite(accept_for(&invite), ORIGIN_B)
            .await
            .unwrap();
        let a_identity = a.identity().unwrap();
        let b_id = b.identity().unwrap().companion_id().to_owned();
        let confirm = |pairing_id: &str, accepter: &str| {
            sign_message(
                &a_identity,
                &PairingMessage::Confirm {
                    version: FEDERATION_VERSION,
                    pairing_id: pairing_id.into(),
                    issuer: a_identity.companion_id().to_owned(),
                    accepter: accepter.into(),
                },
            )
        };
        assert_eq!(
            b.receive_confirm(&confirm("other-pairing", &b_id))
                .unwrap_err(),
            FederationError::PairingMismatch
        );
        assert_eq!(
            b.receive_confirm(&confirm(&invite.id, "someone-else"))
                .unwrap_err(),
            FederationError::RecipientMismatch
        );
        let from_stranger = sign_message(
            &impostor.identity().unwrap(),
            &PairingMessage::Confirm {
                version: FEDERATION_VERSION,
                pairing_id: invite.id.clone(),
                issuer: impostor.identity().unwrap().companion_id().to_owned(),
                accepter: b_id.clone(),
            },
        );
        assert_eq!(
            b.receive_confirm(&from_stranger).unwrap_err(),
            FederationError::UnknownPeer
        );
        assert_eq!(b.overview().unwrap().peers[0].state, PeerState::Pending);
        let ack = b.receive_confirm(&confirm(&invite.id, &b_id)).unwrap();
        assert_eq!(ack.sender, b_id);
        assert_eq!(b.overview().unwrap().peers[0].state, PeerState::Paired);
        assert!(b.verify_from_peer(&ping(&a)).is_ok());
    }

    /// Runs `first` and `second` on two threads released by one barrier, so
    /// the store sees them in whichever order the scheduler picks.
    fn race<A, B>(first: impl FnOnce() -> A + Send, second: impl FnOnce() -> B + Send) -> (A, B)
    where
        A: Send,
        B: Send,
    {
        let barrier = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            let one = scope.spawn(|| {
                barrier.wait();
                first()
            });
            let two = scope.spawn(|| {
                barrier.wait();
                second()
            });
            (one.join().unwrap(), two.join().unwrap())
        })
    }

    /// A confirmation that races the owner's revoke never revives the peer:
    /// the state check and the transition happen under one lock, on both
    /// the accepter (a replayed `pair_confirm` against `revoke_peer`) and
    /// the issuer (the owner's `confirm_peer` against a `pair_revoke`).
    #[test]
    fn a_confirm_racing_a_revoke_never_leaves_the_peer_paired() {
        use futures::executor::block_on;
        const ROUNDS: usize = 24;

        for round in 0..ROUNDS {
            let mut network = Network::new();
            let a = network.server(ORIGIN_A);
            let b = network.server(ORIGIN_B);
            let a_id = a.identity().unwrap().companion_id().to_owned();
            let b_id = b.identity().unwrap().companion_id().to_owned();
            let invite = a.create_invite(ORIGIN_A).unwrap();
            block_on(b.accept_invite(accept_for(&invite), ORIGIN_B)).unwrap();
            // Exactly the notice A sends on confirmation, held by whoever
            // captured it: without a nonce it stays valid for replay.
            let confirm = sign_message(
                &a.identity().unwrap(),
                &PairingMessage::Confirm {
                    version: FEDERATION_VERSION,
                    pairing_id: invite.id.clone(),
                    issuer: a_id.clone(),
                    accepter: b_id.clone(),
                },
            );

            // Accepter side: the replayed confirm against B's owner revoking.
            let (confirmed, revoked) = race(
                || b.receive_confirm(&confirm),
                || block_on(b.revoke_peer(&a_id)),
            );
            let (record, _) = revoked.unwrap();
            assert_eq!(record.state, PeerState::Revoked);
            // Either order is legal; only the outcome is not.
            if let Err(error) = confirmed {
                assert_eq!(error, FederationError::PeerRevoked, "round {round}");
            }
            let on_b = b.overview().unwrap().peers;
            assert_eq!(
                on_b[0].state,
                PeerState::Revoked,
                "round {round}: a confirm racing the revoke revived the peer on B"
            );
            assert_eq!(
                b.verify_from_peer(&ping(&a)).unwrap_err(),
                FederationError::PeerRevoked,
                "round {round}"
            );
            assert_eq!(
                b.receive_confirm(&confirm).unwrap_err(),
                FederationError::PeerRevoked,
                "round {round}: a later replay must fail closed too"
            );
            assert_eq!(a.overview().unwrap().peers[0].state, PeerState::Revoked);

            // Issuer side: pair again, then A's owner confirms while B's
            // owner revokes; both stores must end revoked whichever wins.
            let invite = a.create_invite(ORIGIN_A).unwrap();
            block_on(b.accept_invite(accept_for(&invite), ORIGIN_B)).unwrap();
            let (confirmed, revoked) = race(
                || block_on(a.confirm_peer(&b_id)),
                || block_on(b.revoke_peer(&a_id)),
            );
            let (record, _) = revoked.unwrap();
            assert_eq!(record.state, PeerState::Revoked);
            match confirmed {
                Ok((record, _)) => assert_eq!(record.state, PeerState::Paired),
                Err(error) => assert_eq!(error, FederationError::PeerRevoked, "round {round}"),
            }
            assert_eq!(
                a.overview().unwrap().peers[0].state,
                PeerState::Revoked,
                "round {round}: the owner's confirm overwrote the revoke on A"
            );
            assert_eq!(
                b.overview().unwrap().peers[0].state,
                PeerState::Revoked,
                "round {round}: the confirm notice revived the peer on B"
            );
            assert_eq!(
                a.verify_from_peer(&ping(&b)).unwrap_err(),
                FederationError::PeerRevoked
            );
            assert_eq!(
                b.verify_from_peer(&ping(&a)).unwrap_err(),
                FederationError::PeerRevoked
            );
        }
    }

    const PING: &[u8] = br#"{"kind":"ping","version":1}"#;

    /// `last_seen_at` (#108, PR 4) is when something signed by the peer
    /// last verified here: set by the pairing steps that create a record,
    /// moved by every verified notice and transport envelope, and never by
    /// anything that failed verification or by the owner's own actions.
    #[tokio::test]
    async fn verified_messages_stamp_the_peer_as_seen() {
        let mut network = Network::new();
        let a = network.server(ORIGIN_A);
        let b = network.server(ORIGIN_B);
        let c = network.server("https://c.example");
        let a_id = a.identity().unwrap().companion_id().to_owned();
        let b_id = b.identity().unwrap().companion_id().to_owned();
        let seen = |server: &FederationState, id: &str| {
            server.peers.get(id).map(|peer| peer.record.last_seen_at)
        };

        // Accepting: B verified A's signed response, A verified B's request.
        let invite = a.create_invite(ORIGIN_A).unwrap();
        network.now.store(T0 + 10, Ordering::SeqCst);
        b.accept_invite(accept_for(&invite), ORIGIN_B)
            .await
            .unwrap();
        assert_eq!(seen(&a, &b_id), Some(Some(T0 + 10)));
        assert_eq!(seen(&b, &a_id), Some(Some(T0 + 10)));

        // Confirming: B verified A's confirm notice; A verified B's ack.
        network.now.store(T0 + 20, Ordering::SeqCst);
        a.confirm_peer(&b_id).await.unwrap();
        assert_eq!(seen(&b, &a_id), Some(Some(T0 + 20)));
        assert_eq!(seen(&a, &b_id), Some(Some(T0 + 20)));
        let a_updated = a.peers.get(&b_id).unwrap().record.updated_at;

        // A transport envelope that verifies moves it; one that does not
        // (a replay, a stranger, a body tampered after signing) does not.
        network.now.store(T0 + 40, Ordering::SeqCst);
        let envelope = transport_ping(&b, &a);
        a.open(&envelope).unwrap();
        assert_eq!(seen(&a, &b_id), Some(Some(T0 + 40)));
        assert_eq!(
            a.peers.get(&b_id).unwrap().record.updated_at,
            a_updated,
            "a sighting is not a trust change"
        );
        network.now.store(T0 + 50, Ordering::SeqCst);
        assert_eq!(a.open(&envelope).unwrap_err(), FederationError::Replayed);
        assert_eq!(
            a.open(&transport_ping(&c, &a)).unwrap_err(),
            FederationError::UnknownPeer
        );
        let mut tampered = transport_ping(&b, &a);
        tampered.body = super::super::encode(br#"{"kind":"ping","version":9}"#);
        assert_eq!(
            a.open(&tampered).unwrap_err(),
            FederationError::BodyHashMismatch
        );
        assert_eq!(seen(&a, &b_id), Some(Some(T0 + 40)));
        assert_eq!(seen(&b, &a_id), Some(Some(T0 + 20)), "B heard nothing new");

        // The owner revoking is not a sighting of the peer; the peer's
        // acknowledgement of the notice is.
        network.now.store(T0 + 60, Ordering::SeqCst);
        b.revoke_peer(&a_id).await.unwrap();
        assert_eq!(seen(&b, &a_id), Some(Some(T0 + 60)), "A acknowledged");
        assert_eq!(
            seen(&a, &b_id),
            Some(Some(T0 + 60)),
            "A verified the revoke"
        );

        // The owner list carries it for the CLI and the Companions section.
        let listed = a.overview().unwrap();
        assert_eq!(listed.peers[0].last_seen_at, Some(T0 + 60));
        assert_eq!(
            serde_json::to_value(&listed).unwrap()["peers"][0]["last_seen_at"],
            T0 + 60
        );
    }

    fn transport_ping(from: &FederationState, to: &FederationState) -> TransportEnvelope {
        from.seal(to.identity().unwrap().companion_id(), PING)
            .unwrap()
    }

    #[tokio::test]
    async fn paired_companions_exchange_transport_envelopes_once_each() {
        use super::super::envelope::{ENVELOPE_LIFETIME_SECS, MAX_CLOCK_SKEW_SECS};
        use crate::domain::federation::TransportMessage;
        let mut network = Network::new();
        let (a, b) = paired(&mut network).await;
        let c = network.server("https://c.example");
        let a_id = a.identity().unwrap().companion_id().to_owned();
        let b_id = b.identity().unwrap().companion_id().to_owned();

        // B pings A: verifies once, replays never.
        let envelope = transport_ping(&b, &a);
        assert_eq!(envelope.sender, b_id);
        assert_eq!(envelope.recipient, a_id);
        assert_eq!(envelope.issued_at, T0);
        assert_eq!(envelope.expires_at, T0 + ENVELOPE_LIFETIME_SECS);
        let inbound = a.open(&envelope).unwrap();
        assert_eq!(inbound.peer.companion_id(), b_id);
        assert_eq!(inbound.key, SenderKey::Current);
        assert_eq!(inbound.body, PING);
        assert_eq!(a.open(&envelope).unwrap_err(), FederationError::Replayed);
        // A fresh envelope with the same body is a different message.
        let again = transport_ping(&b, &a);
        assert_ne!(again.nonce, envelope.nonce);
        assert!(a.open(&again).is_ok());

        // The ping route answers with a pong that B can open, once.
        let pong = a.receive_ping(&transport_ping(&b, &a)).unwrap();
        assert_eq!(pong.sender, a_id);
        assert_eq!(pong.recipient, b_id);
        let opened = b.open(&pong).unwrap();
        assert_eq!(
            serde_json::from_slice::<TransportMessage>(&opened.body).unwrap(),
            TransportMessage::Pong {
                version: FEDERATION_VERSION
            }
        );
        assert_eq!(b.open(&pong).unwrap_err(), FederationError::Replayed);
        // A body that is not a ping is refused by the ping route.
        let not_a_ping = b.seal(&a_id, br#"{"kind":"pong","version":1}"#).unwrap();
        assert!(matches!(
            a.receive_ping(&not_a_ping).unwrap_err(),
            FederationError::Malformed(_)
        ));

        // Strangers, misdirected envelopes, tampering, and downgrades.
        assert_eq!(
            a.open(&transport_ping(&c, &a)).unwrap_err(),
            FederationError::UnknownPeer
        );
        assert_eq!(
            a.open(&transport_ping(&b, &c)).unwrap_err(),
            FederationError::RecipientMismatch
        );
        let mut tampered = transport_ping(&b, &a);
        tampered.body = super::super::encode(br#"{"kind":"pong","version":1}"#);
        assert_eq!(
            a.open(&tampered).unwrap_err(),
            FederationError::BodyHashMismatch
        );
        let mut relabelled = transport_ping(&c, &a);
        relabelled.sender = b_id.clone();
        assert_eq!(
            a.open(&relabelled).unwrap_err(),
            FederationError::SignatureMismatch
        );
        let mut downgraded = transport_ping(&b, &a);
        downgraded.version = 0;
        assert_eq!(
            a.open(&downgraded).unwrap_err(),
            FederationError::VersionTooOld { found: 0, min: 1 }
        );
        let mut unknown = transport_ping(&b, &a);
        unknown.version = FEDERATION_VERSION + 1;
        assert_eq!(
            a.open(&unknown).unwrap_err(),
            FederationError::VersionUnsupported {
                found: FEDERATION_VERSION + 1
            }
        );

        // Expiry is judged by the recipient's clock with a skew allowance,
        // and nothing that failed consumed its nonce.
        let late = transport_ping(&b, &a);
        network.now.store(
            T0 + ENVELOPE_LIFETIME_SECS + MAX_CLOCK_SKEW_SECS,
            Ordering::SeqCst,
        );
        assert_eq!(
            a.open(&late).unwrap_err(),
            FederationError::Expired {
                expires_at: T0 + ENVELOPE_LIFETIME_SECS,
                now: T0 + ENVELOPE_LIFETIME_SECS + MAX_CLOCK_SKEW_SECS,
            }
        );
        network.now.store(T0, Ordering::SeqCst);
        assert!(
            a.open(&late).is_ok(),
            "the nonce was not consumed by the failed attempt"
        );
        let early = transport_ping(&b, &a);
        network
            .now
            .store(T0 - MAX_CLOCK_SKEW_SECS - 1, Ordering::SeqCst);
        assert_eq!(
            a.open(&early).unwrap_err(),
            FederationError::IssuedInFuture {
                issued_at: T0,
                now: T0 - MAX_CLOCK_SKEW_SECS - 1,
            }
        );
        network.now.store(T0, Ordering::SeqCst);

        // Pending and revoked peers fail closed after the signature check.
        let d = network.server("https://d.example");
        let invite = a.create_invite(ORIGIN_A).unwrap();
        d.accept_invite(accept_for(&invite), "https://d.example")
            .await
            .unwrap();
        assert_eq!(
            a.open(&transport_ping(&d, &a)).unwrap_err(),
            FederationError::PeerNotPaired {
                state: PeerState::Pending
            }
        );
        b.revoke_peer(&a_id).await.unwrap();
        assert_eq!(
            a.open(&transport_ping(&b, &a)).unwrap_err(),
            FederationError::PeerRevoked
        );
        assert_eq!(
            b.open(&transport_ping(&a, &b)).unwrap_err(),
            FederationError::PeerRevoked
        );
    }

    #[tokio::test]
    async fn rotation_moves_trust_to_the_new_key_with_an_audited_transition() {
        use super::super::rotation::{ROTATION_GRACE_SECS, verify_rotation};
        let mut network = Network::new();
        let (a, b) = paired(&mut network).await;
        let a_id = a.identity().unwrap().companion_id().to_owned();
        let old_b = b.identity().unwrap();
        let old_b_id = old_b.companion_id().to_owned();
        let outstanding = b.create_invite(ORIGIN_B).unwrap();

        network.now.store(T0 + 100, Ordering::SeqCst);
        let report = b.rotate_identity().await.unwrap();
        let new_b = b.identity().unwrap();
        let new_b_id = new_b.companion_id().to_owned();
        assert_ne!(new_b_id, old_b_id);
        assert_eq!(&report.identity, new_b.document());
        assert_eq!(&report.rotation.previous, old_b.document());
        assert_eq!(&report.rotation.identity, new_b.document());
        assert_eq!(report.rotation.rotated_at, T0 + 100);
        assert!(verify_rotation(&report.rotation).is_ok());
        assert_eq!(report.notified, vec![a_id.clone()]);
        assert!(report.unreachable.is_empty());

        // B's own history and keystore moved on; the outstanding invite
        // carried the old document and is gone.
        let overview = b.overview().unwrap();
        assert_eq!(overview.companion_id, new_b_id);
        assert_eq!(overview.rotations, vec![report.rotation.clone()]);
        assert!(overview.invites.is_empty());
        assert!(
            !b.cancel_invite(&outstanding.id),
            "the invite was withdrawn by the rotation"
        );

        // A re-keyed its record with the transition on file.
        let on_a = a.overview().unwrap().peers;
        assert_eq!(on_a.len(), 1);
        assert_eq!(on_a[0].companion_id, new_b_id);
        assert_eq!(on_a[0].public_key, new_b.document().public_key);
        assert_eq!(on_a[0].state, PeerState::Paired);
        assert_eq!(on_a[0].rotation_history.len(), 1);
        assert_eq!(on_a[0].rotation_history[0].rotation, report.rotation);
        assert_eq!(on_a[0].rotation_history[0].accepted_at, T0 + 100);
        assert_eq!(
            on_a[0].approved_origins,
            vec![ORIGIN_B.to_owned()],
            "the approved origin survives the rotation"
        );

        // Both directions work under the new identity.
        let inbound = a.open(&transport_ping(&b, &a)).unwrap();
        assert_eq!(inbound.peer.companion_id(), new_b_id);
        assert_eq!(inbound.key, SenderKey::Current);
        assert!(b.open(&transport_ping(&a, &b)).is_ok());
        // A still addressing B by its old id is refused by B.
        assert_eq!(
            b.open(&a.seal(&old_b_id, PING).unwrap()).unwrap_err(),
            FederationError::RecipientMismatch
        );
        // On B, the old id no longer belongs to anyone.
        assert_eq!(
            b.open(&a.seal(&new_b_id, PING).unwrap())
                .map(|inbound| inbound.key)
                .unwrap(),
            SenderKey::Current
        );

        // The old key still verifies on A inside the grace window, then retires.
        let old_signed = envelope::seal(&old_b, &a_id, PING, T0 + 100).unwrap();
        let inbound = a.open(&old_signed).unwrap();
        assert_eq!(inbound.key, SenderKey::Previous);
        assert_eq!(inbound.peer.companion_id(), new_b_id);
        network
            .now
            .store(T0 + 100 + ROTATION_GRACE_SECS, Ordering::SeqCst);
        let retired = envelope::seal(&old_b, &a_id, PING, T0 + 100 + ROTATION_GRACE_SECS).unwrap();
        assert_eq!(a.open(&retired).unwrap_err(), FederationError::KeyRetired);
        assert!(a.open(&transport_ping(&b, &a)).is_ok());

        // A restart of either side sees the same state.
        let a_again = FederationState::with_transport_and_clock(
            &a.root,
            network.direct.clone(),
            Arc::new(|| T0 + 100 + ROTATION_GRACE_SECS),
        );
        assert_eq!(a_again.overview().unwrap().peers[0].companion_id, new_b_id);
        assert!(a_again.open(&transport_ping(&b, &a)).is_ok());
        let b_again = FederationState::with_transport_and_clock(
            &b.root,
            network.direct.clone(),
            Arc::new(|| T0 + 100 + ROTATION_GRACE_SECS),
        );
        assert_eq!(b_again.identity().unwrap().companion_id(), new_b_id);
        assert_eq!(b_again.overview().unwrap().rotations.len(), 1);

        // Rotating again chains from the new key and A follows.
        network
            .now
            .store(T0 + 200 + ROTATION_GRACE_SECS, Ordering::SeqCst);
        let second = b.rotate_identity().await.unwrap();
        assert_eq!(&second.rotation.previous, new_b.document());
        assert_eq!(second.notified, vec![a_id.clone()]);
        let on_a = a.overview().unwrap().peers;
        assert_eq!(on_a[0].companion_id, b.identity().unwrap().companion_id());
        assert_eq!(on_a[0].rotation_history.len(), 2);
        assert!(a.open(&transport_ping(&b, &a)).is_ok());
    }

    #[tokio::test]
    async fn rotation_notices_are_bound_to_the_retiring_key_and_fail_closed() {
        use super::super::rotation::endorse;
        use crate::domain::federation::TransportMessage;
        let mut network = Network::new();
        let (a, b) = paired(&mut network).await;
        let a_id = a.identity().unwrap().companion_id().to_owned();
        let b_identity = b.identity().unwrap();
        let b_id = b_identity.companion_id().to_owned();
        // C is paired with A too, so its envelopes open.
        let c = network.server("https://c.example");
        let invite = a.create_invite(ORIGIN_A).unwrap();
        c.accept_invite(accept_for(&invite), "https://c.example")
            .await
            .unwrap();
        a.confirm_peer(c.identity().unwrap().companion_id())
            .await
            .unwrap();
        let c_identity = c.identity().unwrap();
        let fresh = |label: &str| {
            let tmp = tempfile::tempdir().unwrap();
            identity::load_or_create_at(&tmp.path().join(label), T0).unwrap()
        };
        let notice = |from: &FederationState, rotation: &KeyRotation| {
            let body = serde_json::to_vec(&TransportMessage::KeyRotation {
                version: FEDERATION_VERSION,
                rotation: Box::new(rotation.clone()),
            })
            .unwrap();
            from.seal(&a_id, &body).unwrap()
        };

        // C claims B rotated: the previous identity is not C's.
        let next = fresh("next");
        let b_to_next = endorse(&b_identity, &next, T0 + 1);
        assert_eq!(
            a.receive_rotation(&notice(&c, &b_to_next)).unwrap_err(),
            FederationError::RotationMismatch
        );
        // B sends a rotation whose endorsement does not verify.
        let mut forged = b_to_next.clone();
        forged.endorsement = endorse(&c_identity, &next, T0 + 1).endorsement;
        assert_eq!(
            a.receive_rotation(&notice(&b, &forged)).unwrap_err(),
            FederationError::SignatureMismatch
        );
        // A rotation to the same key.
        let same = endorse(&b_identity, &b_identity, T0 + 1);
        assert_eq!(
            a.receive_rotation(&notice(&b, &same)).unwrap_err(),
            FederationError::RotationMismatch
        );
        // A rotation to C's identity, which is already a peer.
        let to_c = endorse(&b_identity, &c_identity, T0 + 1);
        assert_eq!(
            a.receive_rotation(&notice(&b, &to_c)).unwrap_err(),
            FederationError::RotationMismatch
        );
        // Nothing changed on A.
        let on_a = a.overview().unwrap().peers;
        assert_eq!(on_a.len(), 2);
        assert!(on_a.iter().all(|peer| peer.rotation_history.is_empty()));
        assert!(on_a.iter().any(|peer| peer.companion_id == b_id));

        // The genuine notice: applied once, replay refused, resend acknowledged.
        let genuine = notice(&b, &b_to_next);
        let ack = a.receive_rotation(&genuine).unwrap();
        assert_eq!(ack.recipient, next.companion_id());
        assert_eq!(
            a.receive_rotation(&genuine).unwrap_err(),
            FederationError::Replayed
        );
        let on_a = a.overview().unwrap().peers;
        let rekeyed = on_a
            .iter()
            .find(|peer| peer.companion_id == next.companion_id())
            .expect("B's record moved to the new id");
        assert_eq!(rekeyed.rotation_history.len(), 1);
        // The ack is addressed to the new identity and names it.
        let opened = envelope::verify(
            &ack,
            &a.identity().unwrap().verified().public_key,
            next.companion_id(),
            T0,
        )
        .unwrap();
        assert_eq!(
            serde_json::from_slice::<TransportMessage>(&opened.body).unwrap(),
            TransportMessage::RotationAck {
                version: FEDERATION_VERSION,
                companion_id: next.companion_id().to_owned(),
                state: PeerState::Paired,
            }
        );
        // Resent (new nonce, old key inside its window): acknowledged, not
        // applied twice.
        let resent = notice(&b, &b_to_next);
        let ack = a.receive_rotation(&resent).unwrap();
        assert_eq!(ack.recipient, next.companion_id());
        assert_eq!(
            a.overview()
                .unwrap()
                .peers
                .iter()
                .find(|peer| peer.companion_id == next.companion_id())
                .unwrap()
                .rotation_history
                .len(),
            1
        );
        // But the retired key cannot rotate the record somewhere else.
        let elsewhere = fresh("elsewhere");
        let b_to_elsewhere = endorse(&b_identity, &elsewhere, T0 + 2);
        assert_eq!(
            a.receive_rotation(&notice(&b, &b_to_elsewhere))
                .unwrap_err(),
            FederationError::RotationMismatch
        );
    }

    #[tokio::test]
    async fn rotation_reports_peers_that_did_not_acknowledge() {
        let mut network = Network::new();
        let (a, b) = paired(&mut network).await;
        let a_id = a.identity().unwrap().companion_id().to_owned();
        // D pairs with B but is then unreachable: its origin is dropped
        // from the network before B rotates.
        let d = network.server("https://d.example");
        let d_id = d.identity().unwrap().companion_id().to_owned();
        let invite = b.create_invite(ORIGIN_B).unwrap();
        d.accept_invite(accept_for(&invite), "https://d.example")
            .await
            .unwrap();
        b.confirm_peer(&d_id).await.unwrap();
        network
            .direct
            .servers
            .lock()
            .unwrap()
            .remove("https://d.example");

        let report = b.rotate_identity().await.unwrap();
        assert_eq!(report.notified, vec![a_id]);
        assert_eq!(report.unreachable, vec![d_id.clone()]);
        // The local rotation stands: D will need the proof from B's history.
        assert_eq!(b.overview().unwrap().rotations.len(), 1);
        assert_eq!(
            b.identity().unwrap().companion_id(),
            report.identity.companion_id
        );
        // D still knows the old identity and refuses the new one.
        assert_eq!(
            d.open(&transport_ping(&b, &d)).unwrap_err(),
            FederationError::UnknownPeer
        );
    }

    /// A pending pairing was started under the identity being retired: the
    /// peer holds the old document, so this side would sign the
    /// confirmation with a key the peer does not know, and the peer would
    /// address its confirmation to an id this side no longer has. Neither
    /// owner may finish it: the rotation revokes every pending record, on
    /// both roles, and both owners pair again with a new invite.
    #[tokio::test]
    async fn rotation_revokes_pending_pairings_so_none_can_finish_one_sided() {
        let mut network = Network::new();
        let (a, b) = paired(&mut network).await;
        let a_id = a.identity().unwrap().companion_id().to_owned();
        let old_b_id = b.identity().unwrap().companion_id().to_owned();
        // C redeemed B's invite: pending on B (issuer) and on C (accepter).
        let c = network.server("https://c.example");
        let c_id = c.identity().unwrap().companion_id().to_owned();
        let invite = b.create_invite(ORIGIN_B).unwrap();
        c.accept_invite(accept_for(&invite), "https://c.example")
            .await
            .unwrap();
        // B redeemed D's invite: pending on D (issuer) and on B (accepter).
        let d = network.server("https://d.example");
        let d_id = d.identity().unwrap().companion_id().to_owned();
        let invite = d.create_invite("https://d.example").unwrap();
        b.accept_invite(accept_for(&invite), ORIGIN_B)
            .await
            .unwrap();
        let on_b = b.overview().unwrap().peers;
        assert_eq!(on_b.len(), 3);
        assert_eq!(
            on_b.iter()
                .filter(|peer| peer.state == PeerState::Pending)
                .count(),
            2
        );

        network.now.store(T0 + 100, Ordering::SeqCst);
        let report = b.rotate_identity().await.unwrap();
        let new_b_id = b.identity().unwrap().companion_id().to_owned();
        assert_eq!(report.notified, vec![a_id.clone()], "paired peers only");
        assert!(report.unreachable.is_empty());

        // On B both pending records are revoked, with their role and
        // history kept; A is paired as before.
        let on_b = b.overview().unwrap().peers;
        let find = |id: &str| on_b.iter().find(|peer| peer.companion_id == id).unwrap();
        assert_eq!(find(&a_id).state, PeerState::Paired);
        assert_eq!(find(&c_id).state, PeerState::Revoked);
        assert_eq!(find(&c_id).role, PairingRole::Issuer);
        assert_eq!(find(&c_id).pending_origin, None);
        assert_eq!(find(&c_id).updated_at, T0 + 100);
        assert_eq!(find(&d_id).state, PeerState::Revoked);
        assert_eq!(find(&d_id).role, PairingRole::Accepter);

        // B's owner cannot confirm C any more, and C never hears a
        // confirmation: its record for the old B stays pending.
        assert_eq!(
            b.confirm_peer(&c_id).await.unwrap_err(),
            FederationError::PeerRevoked
        );
        let on_c = c.overview().unwrap().peers;
        assert_eq!(on_c.len(), 1);
        assert_eq!(on_c[0].companion_id, old_b_id);
        assert_eq!(on_c[0].state, PeerState::Pending);
        assert_eq!(
            b.open(&c.seal(&old_b_id, PING).unwrap()).unwrap_err(),
            FederationError::RecipientMismatch
        );

        // D's owner confirming lands on B as a refusal, so D learns the
        // pairing did not complete; B trusts nothing from D.
        let (on_d, notified) = d.confirm_peer(&old_b_id).await.unwrap();
        assert_eq!(
            on_d.state,
            PeerState::Paired,
            "D's local confirmation stands"
        );
        assert!(!notified, "B refused the confirmation");
        assert_eq!(
            b.peers.get(&d_id).unwrap().record.state,
            PeerState::Revoked,
            "the refused confirmation changed nothing on B"
        );
        assert_eq!(
            b.open(&d.seal(&old_b_id, PING).unwrap()).unwrap_err(),
            FederationError::RecipientMismatch
        );
        assert_eq!(
            b.verify_from_peer(&ping(&d)).unwrap_err(),
            FederationError::PeerRevoked
        );

        // Both pair again with a new invite under the new identity: the
        // revoked records are replaced and keep their creation time.
        let invite = b.create_invite(ORIGIN_B).unwrap();
        assert_eq!(&invite.issuer, b.identity().unwrap().document());
        c.accept_invite(accept_for(&invite), "https://c.example")
            .await
            .unwrap();
        let (record, notified) = b.confirm_peer(&c_id).await.unwrap();
        assert!(notified);
        assert_eq!(record.state, PeerState::Paired);
        assert_eq!(record.created_at, T0, "the earlier record's creation time");
        let on_c = c.overview().unwrap().peers;
        let on_c_new = on_c
            .iter()
            .find(|peer| peer.companion_id == new_b_id)
            .expect("C paired with the rotated B");
        assert_eq!(on_c_new.state, PeerState::Paired);
        assert!(c.open(&transport_ping(&b, &c)).is_ok());
        assert!(b.open(&transport_ping(&c, &b)).is_ok());

        let invite = d.create_invite("https://d.example").unwrap();
        b.accept_invite(accept_for(&invite), ORIGIN_B)
            .await
            .unwrap();
        let (_, notified) = d.confirm_peer(&new_b_id).await.unwrap();
        assert!(notified);
        assert!(d.open(&transport_ping(&b, &d)).is_ok());
        assert!(b.open(&transport_ping(&d, &b)).is_ok());

        // A restart of B sees the same.
        let b_again = FederationState::with_transport_and_clock(
            &b.root,
            network.direct.clone(),
            Arc::new(|| T0 + 100),
        );
        let on_b = b_again.overview().unwrap().peers;
        assert_eq!(on_b.len(), 3);
        assert!(on_b.iter().all(|peer| peer.state == PeerState::Paired));
    }

    /// The owner's confirmation and the owner's rotation race: whichever
    /// wins, no pending record is confirmed under the retired identity,
    /// because the confirmation picks its signing key and flips the record
    /// under the identity lock that the rotation revokes pending records
    /// under. The peer either heard the confirmation from the old key (and
    /// the rotation notice after it, or is reported unreachable when the
    /// notice overtook the confirmation), or nothing at all.
    #[test]
    fn a_confirm_racing_a_rotation_never_pairs_under_the_retired_identity() {
        use futures::executor::block_on;
        const ROUNDS: usize = 16;

        for round in 0..ROUNDS {
            let mut network = Network::new();
            let b = network.server(ORIGIN_B);
            let c = network.server("https://c.example");
            let old_b_id = b.identity().unwrap().companion_id().to_owned();
            let c_id = c.identity().unwrap().companion_id().to_owned();
            let invite = b.create_invite(ORIGIN_B).unwrap();
            block_on(c.accept_invite(accept_for(&invite), "https://c.example")).unwrap();

            let (confirmed, rotated) = race(
                || block_on(b.confirm_peer(&c_id)),
                || block_on(b.rotate_identity()),
            );
            let report = rotated.unwrap();
            let new_b_id = report.identity.companion_id.clone();
            let on_b = b.overview().unwrap().peers;
            assert_eq!(on_b.len(), 1, "round {round}");
            let on_c = c.overview().unwrap().peers;
            match confirmed {
                // The rotation won: the record was revoked before the owner
                // could confirm it, and C heard nothing.
                Err(error) => {
                    assert_eq!(error, FederationError::PeerRevoked, "round {round}");
                    assert_eq!(on_b[0].state, PeerState::Revoked, "round {round}");
                    assert_eq!(on_c.len(), 1);
                    assert_eq!(on_c[0].companion_id, old_b_id, "round {round}");
                    assert_eq!(on_c[0].state, PeerState::Pending, "round {round}");
                    assert!(report.notified.is_empty() && report.unreachable.is_empty());
                }
                // The confirmation won under the old key: C is paired on B
                // and was told with the old key. The rotation notice then
                // either re-keyed C or overtook the confirmation, in which
                // case C is reported unreachable and keeps the old id until
                // it hears the proof again.
                Ok((record, notified)) => {
                    assert_eq!(record.state, PeerState::Paired, "round {round}");
                    assert!(
                        notified,
                        "round {round}: C refused the old key's confirmation"
                    );
                    assert_eq!(on_b[0].state, PeerState::Paired, "round {round}");
                    assert_eq!(on_c.len(), 1);
                    assert_eq!(on_c[0].state, PeerState::Paired, "round {round}");
                    if report.notified == [c_id.clone()] {
                        assert_eq!(on_c[0].companion_id, new_b_id, "round {round}");
                        assert!(c.open(&transport_ping(&b, &c)).is_ok(), "round {round}");
                    } else {
                        assert_eq!(report.unreachable, vec![c_id.clone()], "round {round}");
                        assert_eq!(on_c[0].companion_id, old_b_id, "round {round}");
                    }
                }
            }
            assert_ne!(on_b[0].state, PeerState::Pending, "round {round}");
        }
    }
}
