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

use super::{
    identity::{self, SigningIdentity},
    peers::{Clock, PeerStore},
};
use crate::domain::federation::{
    AcceptInvite, FederationError, IdentityDocument, InviteSummary, IssuedInvite, PairingMessage,
    PeerRecord, PeerSummary, SignedEnvelope,
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
        let _ = (&self.client, url, envelope, TRANSPORT_TIMEOUT);
        todo!("PR 2: transport")
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
        let _ = (workspace_root, transport, clock);
        todo!("PR 2: pairing")
    }

    /// This companion's signing identity, created under the workspace root
    /// on first use. Fails closed on an unusable keystore.
    pub fn identity(&self) -> Result<Arc<SigningIdentity>, FederationError> {
        todo!("PR 2: pairing")
    }

    /// Mints an invite for the owner to hand to another owner. `origin` is
    /// this server's own base URL, which the accepter will post to.
    pub fn create_invite(&self, origin: &str) -> Result<IssuedInvite, FederationError> {
        let _ = origin;
        todo!("PR 2: pairing")
    }

    pub fn cancel_invite(&self, id: &str) -> bool {
        let _ = id;
        todo!("PR 2: pairing")
    }

    pub fn overview(&self) -> Result<Overview, FederationError> {
        todo!("PR 2: pairing")
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
        let _ = (accept, own_origin);
        todo!("PR 2: pairing")
    }

    /// The issuer's side of step 2: verifies a `pair_request`, redeems the
    /// invite, records the accepter as `pending`, and answers.
    pub fn receive_pair_request(
        &self,
        envelope: &SignedEnvelope,
    ) -> Result<SignedEnvelope, FederationError> {
        let _ = envelope;
        todo!("PR 2: pairing")
    }

    /// The issuing owner's confirmation. Returns the record and whether the
    /// peer acknowledged the notice; an unreachable peer does not undo the
    /// local confirmation (confirming again resends).
    pub async fn confirm_peer(
        &self,
        companion_id: &str,
    ) -> Result<(PeerRecord, bool), FederationError> {
        let _ = companion_id;
        todo!("PR 2: pairing")
    }

    /// The accepter's side of step 3.
    pub fn receive_confirm(
        &self,
        envelope: &SignedEnvelope,
    ) -> Result<SignedEnvelope, FederationError> {
        let _ = envelope;
        todo!("PR 2: pairing")
    }

    /// Withdraws trust locally and tells the peer. Returns the record and
    /// whether the peer acknowledged; revoking an already revoked peer is a
    /// no-op.
    pub async fn revoke_peer(
        &self,
        companion_id: &str,
    ) -> Result<(PeerRecord, bool), FederationError> {
        let _ = companion_id;
        todo!("PR 2: pairing")
    }

    /// The peer's side of step 4.
    pub fn receive_revoke(
        &self,
        envelope: &SignedEnvelope,
    ) -> Result<SignedEnvelope, FederationError> {
        let _ = envelope;
        todo!("PR 2: pairing")
    }

    /// Verifies an envelope from a paired peer and returns the peer and the
    /// body. Unknown, pending, and revoked senders fail closed.
    pub fn verify_from_peer(
        &self,
        envelope: &SignedEnvelope,
    ) -> Result<(PeerRecord, Vec<u8>), FederationError> {
        let _ = envelope;
        todo!("PR 2: pairing")
    }
}

/// Signs a pairing message as `signer`.
pub(crate) fn sign_message(signer: &SigningIdentity, message: &PairingMessage) -> SignedEnvelope {
    let _ = (signer, message);
    todo!("PR 2: pairing")
}

/// Accepts `http(s)://host[:port][/path]` and returns it without a trailing
/// slash, with the scheme and host lower-cased. Userinfo, query strings, and
/// fragments are refused: a base URL never carries anything secret.
pub fn normalize_origin(input: &str) -> Result<String, FederationError> {
    let _ = (input, MAX_ORIGIN_LEN, PAIR_PATH, CONFIRM_PATH, REVOKE_PATH);
    let _ = identity::load;
    todo!("PR 2: pairing")
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
                &PairingMessage::PairConfirm {
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
                &PairingMessage::PairRevoke {
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
            PairingMessage::PairRequest {
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
            &PairingMessage::PairAck {
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
                &PairingMessage::PairConfirm {
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
            &PairingMessage::PairConfirm {
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
