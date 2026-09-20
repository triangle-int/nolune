//! The signed transport envelope between paired companions (#108, PR 3).
//!
//! An envelope is addressed to one recipient, carries a random nonce, an
//! issue and an expiry time, and the SHA-256 of its body, and is signed by
//! the sender over the canonical bytes of all of that. [`verify`] checks the
//! signature, the recipient, the body hash, and the times against a clock
//! with a skew allowance; [`ReplayGuard`] remembers accepted nonces until
//! the envelope could no longer be accepted, in a bounded set that fails
//! closed when full. Sender lookup (which key, whether the peer is paired)
//! is the peer store's job, so this module knows nothing about peers,
//! origins, or the deployment.
//!
//! Nothing here logs.

use std::collections::HashMap;

use ed25519_dalek::VerifyingKey;
use sha2::{Digest, Sha256};

use super::identity::SigningIdentity;
use crate::domain::federation::{FederationError, NONCE_BYTES, TransportEnvelope};

/// How far a sender's clock may run ahead of or behind ours.
pub const MAX_CLOCK_SKEW_SECS: u64 = 120;
/// Longest lifetime an envelope may claim; a longer one is refused outright.
pub const MAX_ENVELOPE_LIFETIME_SECS: u64 = 300;
/// Lifetime of the envelopes this server seals.
pub const ENVELOPE_LIFETIME_SECS: u64 = 60;
/// Nonces remembered at once; mirrors the resource capability replay set.
pub const MAX_REPLAY_ENTRIES: usize = 65_536;

/// What [`verify`] hands back: the body, the nonce to reserve, and the moment
/// after which the envelope could not be accepted anyway (so the reservation
/// can lapse).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedTransport {
    pub body: Vec<u8>,
    pub nonce: [u8; NONCE_BYTES],
    pub retire_at: u64,
}

/// Seals `body` for `recipient` as `signer` with a fresh nonce, issued now
/// and expiring after [`ENVELOPE_LIFETIME_SECS`].
pub fn seal(
    signer: &SigningIdentity,
    recipient: &str,
    body: &[u8],
    now: u64,
) -> Result<TransportEnvelope, FederationError> {
    let _ = (signer, recipient, body, now);
    todo!("PR 3: seal a transport envelope")
}

/// [`seal`] with every variable input fixed, for fixtures and tests.
pub(crate) fn seal_with(
    signer: &SigningIdentity,
    recipient: &str,
    body: &[u8],
    nonce: [u8; NONCE_BYTES],
    issued_at: u64,
    expires_at: u64,
) -> TransportEnvelope {
    let _ = (signer, recipient, body, nonce, issued_at, expires_at);
    todo!("PR 3: seal a transport envelope deterministically")
}

/// Parses a transport envelope, refusing unsupported versions before the
/// strict shape check so a downgrade is reported as such.
pub fn parse(json: &str) -> Result<TransportEnvelope, FederationError> {
    let _ = json;
    todo!("PR 3: parse a transport envelope")
}

/// Verifies `envelope` as sent by the holder of `sender_key` to `recipient`
/// at time `now`: version, encodings, recipient, signature, body hash, then
/// the times. The nonce is not consumed here; see [`ReplayGuard`].
pub fn verify(
    envelope: &TransportEnvelope,
    sender_key: &VerifyingKey,
    recipient: &str,
    now: u64,
) -> Result<VerifiedTransport, FederationError> {
    let _ = (envelope, sender_key, recipient, now);
    todo!("PR 3: verify a transport envelope")
}

/// SHA-256 of a body, as the envelope commits to it.
pub(crate) fn body_hash(body: &[u8]) -> [u8; 32] {
    Sha256::digest(body).into()
}

/// The nonces accepted so far, per sender, each kept until the envelope it
/// came in could no longer be accepted. Bounded: a full set refuses new
/// envelopes rather than forgetting old nonces.
pub struct ReplayGuard {
    entries: HashMap<(String, [u8; NONCE_BYTES]), u64>,
    capacity: usize,
}

impl Default for ReplayGuard {
    fn default() -> Self {
        Self::with_capacity(MAX_REPLAY_ENTRIES)
    }
}

impl ReplayGuard {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            capacity,
        }
    }

    /// Reserves `nonce` for `sender` until `retire_at`. A nonce seen before
    /// from the same sender is [`FederationError::Replayed`]; a full set is
    /// [`FederationError::ReplayCapacity`].
    pub fn reserve(
        &mut self,
        sender: &str,
        nonce: [u8; NONCE_BYTES],
        retire_at: u64,
        now: u64,
    ) -> Result<(), FederationError> {
        let _ = (sender, nonce, retire_at, now);
        todo!("PR 3: reserve a nonce")
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::federation::{
            BODY_HASH_BYTES, FEDERATION_VERSION, IdentityDocument, SIGNATURE_BYTES,
            TransportMessage,
        },
        services::federation::{
            decode_exact, encode,
            identity::{from_seed, verify_document},
        },
    };

    const IDENTITY_FIXTURE: &str =
        include_str!("../../../tests/fixtures/federation/identity_v1.json");
    const PEER_IDENTITY_FIXTURE: &str =
        include_str!("../../../tests/fixtures/federation/peer_identity_v1.json");
    const TRANSPORT_FIXTURE: &str =
        include_str!("../../../tests/fixtures/federation/transport_v1.json");
    /// Match the labels in `tests/fixtures/federation/generate.py`.
    const FIXTURE_SEED_LABEL: &[u8] = b"nolune/federation/fixture-seed/v1";
    const PEER_SEED_LABEL: &[u8] = b"nolune/federation/fixture-seed/v1/peer";
    const FIXTURE_NONCE_LABEL: &[u8] = b"nolune/federation/fixture-nonce/v1";
    const FIXTURE_CREATED_AT: u64 = 1_789_862_400;
    const FIXTURE_ISSUED_AT: u64 = FIXTURE_CREATED_AT;
    const FIXTURE_EXPIRES_AT: u64 = FIXTURE_CREATED_AT + 60;
    const FIXTURE_BODY: &[u8] = br#"{"kind":"ping","version":1}"#;

    fn seed(label: &[u8]) -> [u8; 32] {
        Sha256::digest(label).into()
    }

    fn sender() -> SigningIdentity {
        from_seed(&seed(FIXTURE_SEED_LABEL), FIXTURE_CREATED_AT)
    }

    fn peer() -> SigningIdentity {
        from_seed(&seed(PEER_SEED_LABEL), FIXTURE_CREATED_AT)
    }

    fn fixture_nonce() -> [u8; NONCE_BYTES] {
        Sha256::digest(FIXTURE_NONCE_LABEL)[..NONCE_BYTES]
            .try_into()
            .unwrap()
    }

    fn fixture() -> TransportEnvelope {
        parse(TRANSPORT_FIXTURE).unwrap()
    }

    fn sender_key() -> VerifyingKey {
        sender().verified().public_key
    }

    fn peer_id() -> String {
        peer().companion_id().to_owned()
    }

    fn flip_last_byte(encoded: &str) -> String {
        let mut bytes = decode_exact(encoded, SIGNATURE_BYTES).unwrap();
        *bytes.last_mut().unwrap() ^= 0x01;
        encode(&bytes)
    }

    #[test]
    fn fixture_envelope_verifies_for_its_recipient_at_issue_time() {
        let sender_document: IdentityDocument = serde_json::from_str(IDENTITY_FIXTURE).unwrap();
        let peer_document: IdentityDocument = serde_json::from_str(PEER_IDENTITY_FIXTURE).unwrap();
        let sender = verify_document(&sender_document).unwrap();
        let recipient = verify_document(&peer_document).unwrap();
        let envelope = fixture();
        assert_eq!(envelope.sender, sender.companion_id);
        assert_eq!(envelope.recipient, recipient.companion_id);

        let verified = verify(
            &envelope,
            &sender.public_key,
            &recipient.companion_id,
            FIXTURE_ISSUED_AT,
        )
        .unwrap();
        assert_eq!(verified.body, FIXTURE_BODY);
        assert_eq!(verified.nonce, fixture_nonce());
        assert_eq!(
            verified.retire_at,
            FIXTURE_EXPIRES_AT + MAX_CLOCK_SKEW_SECS,
            "a reservation must outlive every clock at which the envelope is still accepted"
        );
        assert_eq!(
            serde_json::from_slice::<TransportMessage>(&verified.body).unwrap(),
            TransportMessage::Ping {
                version: FEDERATION_VERSION
            }
        );
        // Anywhere in the lifetime, on either side of the skew allowance.
        for now in [
            FIXTURE_ISSUED_AT.saturating_sub(MAX_CLOCK_SKEW_SECS),
            FIXTURE_ISSUED_AT + 30,
            FIXTURE_EXPIRES_AT,
            FIXTURE_EXPIRES_AT + MAX_CLOCK_SKEW_SECS - 1,
        ] {
            assert!(
                verify(&envelope, &sender.public_key, &recipient.companion_id, now).is_ok(),
                "at {now}"
            );
        }
    }

    #[test]
    fn fixture_is_reproduced_byte_for_byte_from_the_fixed_inputs() {
        // The fixture was signed by OpenSSL; Ed25519 is deterministic, so
        // the same seed, nonce, times, and canonical bytes give the same file.
        let sealed = seal_with(
            &sender(),
            &peer_id(),
            FIXTURE_BODY,
            fixture_nonce(),
            FIXTURE_ISSUED_AT,
            FIXTURE_EXPIRES_AT,
        );
        assert_eq!(sealed, fixture());
        assert_eq!(
            serde_json::to_string_pretty(&sealed).unwrap() + "\n",
            TRANSPORT_FIXTURE
        );
        let peer_document: IdentityDocument = serde_json::from_str(PEER_IDENTITY_FIXTURE).unwrap();
        assert_eq!(peer().document(), &peer_document);
        assert_eq!(
            serde_json::to_string_pretty(peer().document()).unwrap() + "\n",
            PEER_IDENTITY_FIXTURE
        );
    }

    #[test]
    fn sealed_envelopes_carry_fresh_nonces_and_bounded_lifetimes() {
        let me = sender();
        let now = FIXTURE_ISSUED_AT;
        let first = seal(&me, &peer_id(), FIXTURE_BODY, now).unwrap();
        let second = seal(&me, &peer_id(), FIXTURE_BODY, now).unwrap();
        assert_eq!(first.version, FEDERATION_VERSION);
        assert_eq!(first.sender, me.companion_id());
        assert_eq!(first.recipient, peer_id());
        assert_eq!(first.issued_at, now);
        assert_eq!(first.expires_at, now + ENVELOPE_LIFETIME_SECS);
        assert!(ENVELOPE_LIFETIME_SECS <= MAX_ENVELOPE_LIFETIME_SECS);
        assert_eq!(
            decode_exact(&first.nonce, NONCE_BYTES).unwrap().len(),
            NONCE_BYTES
        );
        assert_ne!(first.nonce, second.nonce, "nonces are random per envelope");
        assert_ne!(first.signature, second.signature);
        assert_eq!(
            decode_exact(&first.body_hash, BODY_HASH_BYTES).unwrap(),
            body_hash(FIXTURE_BODY)
        );
        assert_eq!(first.body, encode(FIXTURE_BODY));
        for envelope in [&first, &second] {
            let verified = verify(envelope, &me.verified().public_key, &peer_id(), now).unwrap();
            assert_eq!(verified.body, FIXTURE_BODY);
        }
        // Parsing a sealed envelope gives it back unchanged.
        assert_eq!(
            parse(&serde_json::to_string(&first).unwrap()).unwrap(),
            first
        );
    }

    #[test]
    fn tampered_bodies_and_hashes_fail_with_distinct_errors() {
        let envelope = fixture();
        let now = FIXTURE_ISSUED_AT;

        // The body was altered after signing: the hash catches it.
        let mut tampered = envelope.clone();
        tampered.body = encode(br#"{"kind":"pong","version":1}"#);
        assert_eq!(
            verify(&tampered, &sender_key(), &peer_id(), now),
            Err(FederationError::BodyHashMismatch)
        );
        // Same length, one bit.
        let mut flipped_body = envelope.clone();
        let mut body = decode_exact(&envelope.body, FIXTURE_BODY.len()).unwrap();
        body[3] ^= 0x20;
        flipped_body.body = encode(&body);
        assert_eq!(
            verify(&flipped_body, &sender_key(), &peer_id(), now),
            Err(FederationError::BodyHashMismatch)
        );
        // The hash was fixed up to match the new body: the signature catches it.
        let mut rehashed = tampered.clone();
        rehashed.body_hash = encode(&body_hash(br#"{"kind":"pong","version":1}"#));
        assert_eq!(
            verify(&rehashed, &sender_key(), &peer_id(), now),
            Err(FederationError::SignatureMismatch)
        );
        // A hash that is not a digest.
        let mut short_hash = envelope.clone();
        short_hash.body_hash = encode(&[1; 31]);
        assert!(matches!(
            verify(&short_hash, &sender_key(), &peer_id(), now),
            Err(FederationError::Malformed(_))
        ));
        // A body that is not canonical base64url.
        let mut padded = envelope.clone();
        padded.body.push('=');
        assert!(matches!(
            verify(&padded, &sender_key(), &peer_id(), now),
            Err(FederationError::Malformed(_))
        ));
        // Unchanged: the original still verifies after all that.
        assert!(verify(&envelope, &sender_key(), &peer_id(), now).is_ok());
    }

    #[test]
    fn altered_signatures_and_headers_fail_closed() {
        let envelope = fixture();
        let now = FIXTURE_ISSUED_AT;

        let mut flipped = envelope.clone();
        flipped.signature = flip_last_byte(&envelope.signature);
        assert_eq!(
            verify(&flipped, &sender_key(), &peer_id(), now),
            Err(FederationError::SignatureMismatch)
        );
        // Signed by someone else entirely.
        let foreign = seal_with(
            &peer(),
            &peer_id(),
            FIXTURE_BODY,
            fixture_nonce(),
            FIXTURE_ISSUED_AT,
            FIXTURE_EXPIRES_AT,
        );
        let mut relabelled = foreign.clone();
        relabelled.sender = envelope.sender.clone();
        assert_eq!(
            verify(&relabelled, &sender_key(), &peer_id(), now),
            Err(FederationError::SignatureMismatch)
        );
        // Every signed header, changed after signing.
        let mut other_recipient = envelope.clone();
        other_recipient.recipient = envelope.sender.clone();
        assert_eq!(
            verify(&other_recipient, &sender_key(), &envelope.sender, now),
            Err(FederationError::SignatureMismatch)
        );
        let mut later = envelope.clone();
        later.expires_at += 1;
        assert_eq!(
            verify(&later, &sender_key(), &peer_id(), now),
            Err(FederationError::SignatureMismatch)
        );
        let mut earlier = envelope.clone();
        earlier.issued_at -= 1;
        assert_eq!(
            verify(&earlier, &sender_key(), &peer_id(), now),
            Err(FederationError::SignatureMismatch)
        );
        let mut renonced = envelope.clone();
        renonced.nonce = encode(&[7; NONCE_BYTES]);
        assert_eq!(
            verify(&renonced, &sender_key(), &peer_id(), now),
            Err(FederationError::SignatureMismatch)
        );
        for broken in [
            envelope.signature[..80].to_owned(),
            format!("{}=", envelope.signature),
            String::new(),
        ] {
            let mut malformed = envelope.clone();
            malformed.signature = broken;
            assert!(matches!(
                verify(&malformed, &sender_key(), &peer_id(), now),
                Err(FederationError::Malformed(_))
            ));
        }
        let mut short_nonce = envelope.clone();
        short_nonce.nonce = encode(&[7; NONCE_BYTES - 1]);
        assert!(matches!(
            verify(&short_nonce, &sender_key(), &peer_id(), now),
            Err(FederationError::Malformed(_))
        ));
    }

    #[test]
    fn envelopes_for_someone_else_are_refused_before_the_signature() {
        let envelope = fixture();
        assert_eq!(
            verify(
                &envelope,
                &sender_key(),
                &envelope.sender,
                FIXTURE_ISSUED_AT
            ),
            Err(FederationError::RecipientMismatch)
        );
        // Even with a broken signature: the recipient check comes first, so
        // a misdirected envelope reveals nothing about its signer.
        let mut flipped = envelope.clone();
        flipped.signature = flip_last_byte(&envelope.signature);
        assert_eq!(
            verify(&flipped, &sender_key(), "nobody", FIXTURE_ISSUED_AT),
            Err(FederationError::RecipientMismatch)
        );
    }

    #[test]
    fn expiry_and_skew_are_judged_against_the_clock() {
        let envelope = fixture();
        let key = sender_key();
        let recipient = peer_id();

        // Expired: past the expiry plus the allowance.
        let too_late = FIXTURE_EXPIRES_AT + MAX_CLOCK_SKEW_SECS;
        assert_eq!(
            verify(&envelope, &key, &recipient, too_late),
            Err(FederationError::Expired {
                expires_at: FIXTURE_EXPIRES_AT,
                now: too_late,
            })
        );
        assert!(verify(&envelope, &key, &recipient, too_late - 1).is_ok());
        assert_eq!(
            verify(&envelope, &key, &recipient, FIXTURE_EXPIRES_AT + 86_400),
            Err(FederationError::Expired {
                expires_at: FIXTURE_EXPIRES_AT,
                now: FIXTURE_EXPIRES_AT + 86_400,
            })
        );

        // Issued too far in the future: beyond the allowance.
        let too_early = FIXTURE_ISSUED_AT - MAX_CLOCK_SKEW_SECS - 1;
        assert_eq!(
            verify(&envelope, &key, &recipient, too_early),
            Err(FederationError::IssuedInFuture {
                issued_at: FIXTURE_ISSUED_AT,
                now: too_early,
            })
        );
        assert!(verify(&envelope, &key, &recipient, too_early + 1).is_ok());

        // A lifetime that is not positive or is too long is refused
        // whatever the clock says; both are signed, so they are sealed here.
        let me = sender();
        let inverted = seal_with(
            &me,
            &recipient,
            FIXTURE_BODY,
            fixture_nonce(),
            FIXTURE_ISSUED_AT,
            FIXTURE_ISSUED_AT,
        );
        assert_eq!(
            verify(&inverted, &key, &recipient, FIXTURE_ISSUED_AT),
            Err(FederationError::InvalidLifetime {
                issued_at: FIXTURE_ISSUED_AT,
                expires_at: FIXTURE_ISSUED_AT,
            })
        );
        let too_long = seal_with(
            &me,
            &recipient,
            FIXTURE_BODY,
            fixture_nonce(),
            FIXTURE_ISSUED_AT,
            FIXTURE_ISSUED_AT + MAX_ENVELOPE_LIFETIME_SECS + 1,
        );
        assert_eq!(
            verify(&too_long, &key, &recipient, FIXTURE_ISSUED_AT),
            Err(FederationError::InvalidLifetime {
                issued_at: FIXTURE_ISSUED_AT,
                expires_at: FIXTURE_ISSUED_AT + MAX_ENVELOPE_LIFETIME_SECS + 1,
            })
        );
        let longest = seal_with(
            &me,
            &recipient,
            FIXTURE_BODY,
            fixture_nonce(),
            FIXTURE_ISSUED_AT,
            FIXTURE_ISSUED_AT + MAX_ENVELOPE_LIFETIME_SECS,
        );
        assert!(verify(&longest, &key, &recipient, FIXTURE_ISSUED_AT).is_ok());
    }

    #[test]
    fn downgraded_and_unknown_versions_are_refused_first() {
        let envelope = fixture();
        let now = FIXTURE_ISSUED_AT;
        let me = sender();

        // Properly signed at another version: still refused.
        let mut downgraded = envelope.clone();
        downgraded.version = 0;
        assert_eq!(
            verify(&downgraded, &sender_key(), &peer_id(), now),
            Err(FederationError::VersionTooOld { found: 0, min: 1 })
        );
        let mut unknown = envelope.clone();
        unknown.version = FEDERATION_VERSION + 1;
        assert_eq!(
            verify(&unknown, &sender_key(), &peer_id(), now),
            Err(FederationError::VersionUnsupported {
                found: FEDERATION_VERSION + 1
            })
        );
        // Also on the raw wire, before the shape is even checked.
        let json = serde_json::to_string(&seal(&me, &peer_id(), FIXTURE_BODY, now).unwrap())
            .unwrap()
            .replacen("\"version\":1", "\"version\":0", 1);
        assert_eq!(
            parse(&json),
            Err(FederationError::VersionTooOld { found: 0, min: 1 })
        );
        assert_eq!(
            parse(r#"{"version":99,"port":26559}"#),
            Err(FederationError::VersionUnsupported { found: 99 })
        );
        assert!(matches!(
            parse(r#"{"version":1,"port":26559}"#),
            Err(FederationError::Malformed(_))
        ));
        assert!(matches!(
            parse("not json"),
            Err(FederationError::Malformed(_))
        ));
    }

    #[test]
    fn replay_guard_accepts_each_nonce_once_per_sender() {
        let mut guard = ReplayGuard::default();
        let nonce = fixture_nonce();
        let now = FIXTURE_ISSUED_AT;
        let retire_at = FIXTURE_EXPIRES_AT + MAX_CLOCK_SKEW_SECS;
        assert!(guard.is_empty());
        guard.reserve("a", nonce, retire_at, now).unwrap();
        assert_eq!(guard.len(), 1);
        assert_eq!(
            guard.reserve("a", nonce, retire_at, now),
            Err(FederationError::Replayed)
        );
        assert_eq!(
            guard.reserve("a", nonce, retire_at, retire_at - 1),
            Err(FederationError::Replayed),
            "the reservation holds as long as the envelope could be accepted"
        );
        // Another sender with the same nonce bytes is a different envelope.
        guard.reserve("b", nonce, retire_at, now).unwrap();
        let mut other = nonce;
        other[0] ^= 1;
        guard.reserve("a", other, retire_at, now).unwrap();
        assert_eq!(guard.len(), 3);
        // Lapsed reservations are forgotten; the envelope itself is expired
        // by then, so the nonce cannot come back.
        guard.reserve("c", nonce, retire_at, retire_at).unwrap();
        assert_eq!(guard.len(), 1);
    }

    #[test]
    fn replay_guard_fails_closed_when_full() {
        let mut guard = ReplayGuard::with_capacity(2);
        let now = FIXTURE_ISSUED_AT;
        guard.reserve("a", [1; NONCE_BYTES], now + 10, now).unwrap();
        guard.reserve("a", [2; NONCE_BYTES], now + 20, now).unwrap();
        assert_eq!(
            guard.reserve("a", [3; NONCE_BYTES], now + 30, now),
            Err(FederationError::ReplayCapacity)
        );
        // A replay is still reported as such, not as capacity.
        assert_eq!(
            guard.reserve("a", [1; NONCE_BYTES], now + 10, now),
            Err(FederationError::Replayed)
        );
        // Room again once the oldest lapses.
        guard
            .reserve("a", [3; NONCE_BYTES], now + 30, now + 10)
            .unwrap();
        assert_eq!(guard.len(), 2);
        assert_eq!(ReplayGuard::default().capacity, MAX_REPLAY_ENTRIES);
        assert_eq!(MAX_REPLAY_ENTRIES, 65_536);
    }
}
