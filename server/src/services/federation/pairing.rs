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
    identity::{self, SigningIdentity},
    peers::{Clock, Peer, PeerStore},
};
use crate::domain::federation::{
    AcceptInvite, FEDERATION_VERSION, FederationError, IdentityDocument, InviteSummary,
    IssuedInvite, PairingMessage, PairingRole, PeerRecord, PeerState, PeerSummary, SignedEnvelope,
};

/// Peer-side route for pair requests, relative to the peer's base URL.
pub const PAIR_PATH: &str = "/federation/v1/pair";
/// Peer-side route for confirmation notices.
pub const CONFIRM_PATH: &str = "/federation/v1/pair/confirm";
/// Peer-side route for revocation notices.
pub const REVOKE_PATH: &str = "/federation/v1/pair/revoke";
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
/// in-process server.
pub trait PeerTransport: Send + Sync {
    fn post<'a>(
        &'a self,
        url: &'a str,
        envelope: &'a SignedEnvelope,
    ) -> BoxFuture<'a, Result<SignedEnvelope, FederationError>>;
}

/// The real transport: a JSON POST with a timeout and a size cap.
pub struct HttpTransport {
    client: reqwest::Client,
}

impl HttpTransport {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }
}

impl PeerTransport for HttpTransport {
    fn post<'a>(
        &'a self,
        url: &'a str,
        envelope: &'a SignedEnvelope,
    ) -> BoxFuture<'a, Result<SignedEnvelope, FederationError>> {
        Box::pin(async move {
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
            let text = std::str::from_utf8(&bytes)
                .map_err(|_| FederationError::Malformed("peer answer is not UTF-8".into()))?;
            identity::parse_envelope(text)
        })
    }
}

/// What the owner list reports: this companion's identity, its outstanding
/// invites, and its peers.
#[derive(Debug, Clone, Serialize)]
pub struct Overview {
    pub companion_id: String,
    pub identity: IdentityDocument,
    pub invites: Vec<InviteSummary>,
    pub peers: Vec<PeerSummary>,
}

/// This server's federation side: its signing identity (created on first
/// use), its peers, and the transport to reach them.
pub struct FederationState {
    root: PathBuf,
    identity: Mutex<Option<Arc<SigningIdentity>>>,
    peers: PeerStore,
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
            transport,
            clock,
        }
    }

    /// This companion's signing identity, created under the workspace root
    /// on first use. Fails closed on an unusable keystore.
    pub fn identity(&self) -> Result<Arc<SigningIdentity>, FederationError> {
        let mut slot = self.identity.lock().unwrap();
        if let Some(identity) = slot.as_ref() {
            return Ok(identity.clone());
        }
        let identity = Arc::new(identity::load_or_create(&self.root)?);
        *slot = Some(identity.clone());
        Ok(identity)
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
            invites: self.peers.invites(),
            peers: self.peers.list().iter().map(PeerRecord::summary).collect(),
        })
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
        let me = self.identity()?;
        if issuer != me.companion_id() || accepter_identity.public_key == me.verified().public_key {
            return Err(FederationError::IssuerMismatch);
        }
        let origin = normalize_origin(&origin)?;
        let redeemed = self.peers.redeem_invite(&secret)?;
        // A companion that was pending, paired, or revoked before starts over:
        // its owner redeemed a fresh invite, and ours confirms again.
        let record = self.fresh_record(
            accepter,
            PairingRole::Issuer,
            redeemed.id.clone(),
            Vec::new(),
            Some(origin),
        );
        let record = self.peers.upsert(record, accepter_identity)?;
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
    pub async fn confirm_peer(
        &self,
        companion_id: &str,
    ) -> Result<(PeerRecord, bool), FederationError> {
        let peer = self
            .peers
            .get(companion_id)
            .ok_or(FederationError::UnknownPeer)?;
        let record = match (peer.record.role, peer.record.state) {
            (_, PeerState::Revoked) => return Err(FederationError::PeerRevoked),
            (PairingRole::Issuer, PeerState::Pending) => {
                let record = self
                    .peers
                    .update(companion_id, |record| {
                        record.state = PeerState::Paired;
                        record.approved_origins =
                            record.pending_origin.take().into_iter().collect();
                    })?
                    .ok_or(FederationError::UnknownPeer)?;
                log::info!(
                    "[federation] pairing {}: companion {} paired by the owner",
                    record.pairing_id,
                    record.companion_id()
                );
                record
            }
            (PairingRole::Issuer, PeerState::Paired) => peer.record.clone(),
            (PairingRole::Accepter, PeerState::Paired) => return Ok((peer.record, false)),
            (_, state) => return Err(FederationError::PeerNotPaired { state }),
        };
        let me = self.identity()?;
        let notice = PairingMessage::Confirm {
            version: FEDERATION_VERSION,
            pairing_id: record.pairing_id.clone(),
            issuer: me.companion_id().to_owned(),
            accepter: record.identity.companion_id.clone(),
        };
        let peer = Peer {
            record: record.clone(),
            verified: peer.verified,
        };
        let notified = self
            .notify(&peer, &notice, CONFIRM_PATH, PeerState::Paired)
            .await;
        Ok((record, notified))
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
        if peer.record.state == PeerState::Revoked {
            return Err(FederationError::PeerRevoked);
        }
        if pairing_id != peer.record.pairing_id {
            return Err(FederationError::PairingMismatch);
        }
        if peer.record.role != PairingRole::Accepter {
            return Err(FederationError::Malformed(
                "only the issuing side confirms a pairing".into(),
            ));
        }
        // Already paired: the notice was resent or replayed, and nothing changes.
        if peer.record.state == PeerState::Pending {
            self.peers.update(peer.record.companion_id(), |record| {
                record.state = PeerState::Paired;
            })?;
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
    /// no-op.
    pub async fn revoke_peer(
        &self,
        companion_id: &str,
    ) -> Result<(PeerRecord, bool), FederationError> {
        let peer = self
            .peers
            .get(companion_id)
            .ok_or(FederationError::UnknownPeer)?;
        if peer.record.state == PeerState::Revoked {
            return Ok((peer.record, false));
        }
        let record = self
            .peers
            .update(companion_id, |record| {
                record.state = PeerState::Revoked;
                record.pending_origin = None;
            })?
            .ok_or(FederationError::UnknownPeer)?;
        log::info!(
            "[federation] pairing {}: companion {} revoked by the owner",
            record.pairing_id,
            record.companion_id()
        );
        let me = self.identity()?;
        let notice = PairingMessage::Revoke {
            version: FEDERATION_VERSION,
            pairing_id: record.pairing_id.clone(),
            sender: me.companion_id().to_owned(),
            peer: record.identity.companion_id.clone(),
        };
        let peer = Peer {
            record: record.clone(),
            verified: peer.verified,
        };
        let notified = self
            .notify(&peer, &notice, REVOKE_PATH, PeerState::Revoked)
            .await;
        Ok((record, notified))
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
        if pairing_id != peer.record.pairing_id {
            return Err(FederationError::PairingMismatch);
        }
        if peer.record.state != PeerState::Revoked {
            self.peers.update(peer.record.companion_id(), |record| {
                record.state = PeerState::Revoked;
                record.pending_origin = None;
            })?;
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
            rotation_history: previous
                .map(|previous| previous.record.rotation_history)
                .unwrap_or_default(),
        }
    }

    /// Posts `notice` to the peer's approved origins until one acknowledges
    /// it with a signed ack for this pairing. Failures are reported, never
    /// fatal: the local state already changed.
    async fn notify(
        &self,
        peer: &Peer,
        notice: &PairingMessage,
        path: &str,
        expected: PeerState,
    ) -> bool {
        let Ok(me) = self.identity() else {
            return false;
        };
        let outgoing = sign_message(&me, notice);
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
}
