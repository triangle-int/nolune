//! Wire shapes for companion federation (#108).
//!
//! A federation peer is a companion signing identity, never a machine, a
//! profile name, a port, or a hostname. The [`IdentityDocument`] is the only
//! thing a peer ever binds trust to: its `companion_id` is derived from the
//! Ed25519 public key it carries and the document is self-signed over the
//! canonical bytes described in `services::federation`.
//!
//! Pairing (#108, PR 2) adds the owner-facing invite shapes, the peer record
//! a server persists per paired companion, and the [`PairingMessage`] bodies
//! that travel inside a [`SignedEnvelope`] between two servers. The one
//! secret in this module, the invite secret, lives in [`InviteSecret`], which
//! never formats itself.
//!
//! The transport (#108, PR 3) adds the [`TransportEnvelope`] that paired
//! companions exchange after pairing: addressed to one recipient, single-use
//! by nonce, bounded by `issued_at`/`expires_at`, and signed over the body's
//! digest. [`KeyRotation`] is the new identity endorsed by the old key, and
//! [`KeyTransition`] is the audited record a peer keeps of accepting one.
//!
//! Everything here is a shape. Signing, verification, and the keystore live in
//! `services::federation::identity`; the peer store and the handshake live in
//! `services::federation::{peers, pairing}`; the transport verifier and the
//! rotation proof live in `services::federation::{envelope, rotation}`; the
//! on-disk location is documented in `docs/companion-storage.md`.

use std::{fmt, path::PathBuf};

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

/// Version of the identity document and signed envelope this server writes.
pub const FEDERATION_VERSION: u32 = 1;

/// Oldest version this server still accepts. Anything below is a downgrade
/// and fails closed regardless of its signature.
pub const MIN_FEDERATION_VERSION: u32 = 1;

/// Raw Ed25519 public key length.
pub const PUBLIC_KEY_BYTES: usize = 32;

/// Raw Ed25519 signature length.
pub const SIGNATURE_BYTES: usize = 64;

/// A `companion_id` is a SHA-256 digest of the public key, so this many bytes
/// before base64url encoding.
pub const COMPANION_ID_BYTES: usize = 32;

/// Raw nonce length in a transport envelope: 128 random bits per message.
pub const NONCE_BYTES: usize = 16;

/// The body hash in a transport envelope is a SHA-256 digest.
pub const BODY_HASH_BYTES: usize = 32;

/// Public, self-signed identity of one companion. Peers persist this document
/// (or the key and id inside it) and nothing else about the companion's
/// deployment.
///
/// Binary fields are base64url without padding. Unknown fields are rejected so
/// that a future version cannot be reinterpreted as this one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityDocument {
    pub version: u32,
    /// Derived from `public_key`; see `services::federation::companion_id_for`.
    pub companion_id: String,
    /// Raw Ed25519 public key.
    pub public_key: String,
    /// Unix seconds. Informational only; peers never trust it as an ordering.
    pub created_at: u64,
    /// Ed25519 signature by `public_key` over the canonical identity bytes.
    pub signature: String,
}

/// The smallest signed message shape: a body signed by a known companion.
/// The pairing handshake uses it; everything after pairing travels in a
/// [`TransportEnvelope`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedEnvelope {
    pub version: u32,
    /// `companion_id` of the signer.
    pub sender: String,
    /// Opaque body bytes, base64url without padding.
    pub body: String,
    /// Ed25519 signature by the sender over the canonical envelope bytes,
    /// which commit to the SHA-256 of the body rather than the body itself.
    pub signature: String,
}

/// The signed transport envelope paired companions exchange. It is addressed
/// (`recipient`), single-use (`nonce`, remembered by the recipient until the
/// envelope could no longer be accepted), bounded in time (`issued_at` and
/// `expires_at`, judged with a clock-skew allowance), and signed over the
/// canonical bytes of every field but the body, which is committed to by
/// `body_hash`. Nothing in it names a host, port, or profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransportEnvelope {
    pub version: u32,
    /// `companion_id` of the signer.
    pub sender: String,
    /// `companion_id` the envelope is for; anyone else refuses it.
    pub recipient: String,
    /// [`NONCE_BYTES`] random bytes, base64url without padding.
    pub nonce: String,
    /// Unix seconds by the sender's clock.
    pub issued_at: u64,
    /// Unix seconds by the sender's clock; at most a few minutes after
    /// `issued_at`.
    pub expires_at: u64,
    /// base64url SHA-256 of the decoded body.
    pub body_hash: String,
    /// Opaque body bytes, base64url without padding.
    pub body: String,
    /// Ed25519 signature by the sender over the canonical transport bytes.
    pub signature: String,
}

/// Bodies that travel in a [`TransportEnvelope`] between paired companions.
/// Kind-tagged like [`PairingMessage`]; message semantics beyond a ping and
/// the rotation notice arrive with #110.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum TransportMessage {
    /// Proves the transport works end to end; answered with a `pong`.
    #[serde(rename = "ping")]
    Ping { version: u32 },
    #[serde(rename = "pong")]
    Pong { version: u32 },
    /// The sender rotated its key; sent inside an envelope signed by the
    /// key being retired.
    #[serde(rename = "key_rotation")]
    KeyRotation {
        version: u32,
        rotation: Box<KeyRotation>,
    },
    /// The recipient recorded the rotation and now knows the sender as
    /// `companion_id`.
    #[serde(rename = "rotation_ack")]
    RotationAck {
        version: u32,
        companion_id: String,
        state: PeerState,
    },
}

/// A key rotation: the new self-signed identity, endorsed by the previous
/// key and acknowledged by the new one over the same canonical bytes, so a
/// reader with the previous document can check that its owner chose the new
/// key and that the new key exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyRotation {
    pub version: u32,
    pub previous: IdentityDocument,
    pub identity: IdentityDocument,
    /// Unix seconds by the rotating companion's clock.
    pub rotated_at: u64,
    /// Signature by `previous`'s key over the canonical rotation bytes.
    pub endorsement: String,
    /// Signature by `identity`'s key over the same bytes.
    pub signature: String,
}

/// Where a peer stands in the pairing handshake. Only a `paired` peer is
/// trusted; a `revoked` record is kept so a stale confirmation can never
/// revive it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeerState {
    /// An invite the owner minted that no companion has accepted yet. Lives
    /// in memory only: there is no key to bind to.
    Invited,
    /// Keys are exchanged; one owner still has to confirm (see [`PairingRole`]).
    Pending,
    /// Both owners confirmed. The only state in which messages verify.
    Paired,
    /// An owner withdrew trust. Terminal until a new invite pairs the
    /// companion again.
    Revoked,
}

impl fmt::Display for PeerState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Invited => "invited",
            Self::Pending => "pending",
            Self::Paired => "paired",
            Self::Revoked => "revoked",
        })
    }
}

/// Which side of the handshake this server took for a peer. A pending peer
/// waits for the issuer's owner: on the issuer that is us, on the accepter it
/// is them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairingRole {
    /// This server minted the invite.
    Issuer,
    /// This server redeemed the invite.
    Accepter,
}

/// One audited key transition of a peer: the rotation exactly as the peer
/// sent it (both documents and both signatures, so it can be re-verified
/// later) and when this server accepted it. The previous key keeps
/// verifying for a transition window after `accepted_at`, then retires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyTransition {
    pub rotation: KeyRotation,
    /// Unix seconds by this server's clock.
    pub accepted_at: u64,
}

impl KeyTransition {
    pub fn previous_companion_id(&self) -> &str {
        &self.rotation.previous.companion_id
    }
}

/// What a server persists about one peer: the peer's self-signed identity
/// (key and id), the handshake state, the origins its owner approved, and
/// the rotation history. Nothing about the peer's deployment (host, port,
/// profile, display name) is stored, because nothing about it is trusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeerRecord {
    pub identity: IdentityDocument,
    pub state: PeerState,
    pub role: PairingRole,
    /// Id of the invite that started this pairing. Every notice about the
    /// pairing names it, so a notice from an earlier pairing is stale.
    pub pairing_id: String,
    /// Base URLs this server may send to for this peer. Filled by the owner:
    /// the accepter approves the origin it typed, the issuer approves the
    /// requested origin when confirming.
    pub approved_origins: Vec<String>,
    /// The origin the peer reported while pairing, until the owner approves it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_origin: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub rotation_history: Vec<KeyTransition>,
}

impl PeerRecord {
    pub fn companion_id(&self) -> &str {
        &self.identity.companion_id
    }

    /// The owner-facing view: the same fields with the key and id lifted out
    /// of the document.
    pub fn summary(&self) -> PeerSummary {
        PeerSummary {
            companion_id: self.identity.companion_id.clone(),
            public_key: self.identity.public_key.clone(),
            state: self.state,
            role: self.role,
            pairing_id: self.pairing_id.clone(),
            approved_origins: self.approved_origins.clone(),
            pending_origin: self.pending_origin.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
            rotation_history: self.rotation_history.clone(),
        }
    }
}

/// A [`PeerRecord`] as the owner routes report it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PeerSummary {
    pub companion_id: String,
    pub public_key: String,
    pub state: PeerState,
    pub role: PairingRole,
    pub pairing_id: String,
    pub approved_origins: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_origin: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    pub rotation_history: Vec<KeyTransition>,
}

/// The one-time invite secret. It is shown to the issuing owner once, typed
/// into the accepting server once, and travels once inside a signed pair
/// request. It serializes (that is how it travels) but never formats: `Debug`
/// prints a placeholder and there is no `Display`.
#[derive(Clone)]
pub struct InviteSecret(Zeroizing<String>);

impl InviteSecret {
    pub fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }

    /// The secret itself, for hashing or serializing. Never for logging.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl Serialize for InviteSecret {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for InviteSecret {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Self::new)
    }
}

impl fmt::Debug for InviteSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("InviteSecret(<redacted>)")
    }
}

/// A freshly minted invite, returned to the issuing owner exactly once.
/// `origin` and `issuer` are what the accepting owner needs alongside the
/// secret; the accepting server pins `issuer` before it contacts `origin`.
#[derive(Clone, Serialize)]
pub struct IssuedInvite {
    pub id: String,
    pub secret: InviteSecret,
    pub created_at: u64,
    pub expires_at: u64,
    pub expires_in_secs: u64,
    pub origin: String,
    pub issuer: IdentityDocument,
}

impl fmt::Debug for IssuedInvite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IssuedInvite")
            .field("id", &self.id)
            .field("created_at", &self.created_at)
            .field("expires_at", &self.expires_at)
            .field("expires_in_secs", &self.expires_in_secs)
            .field("origin", &self.origin)
            .field("issuer", &self.issuer.companion_id)
            .finish_non_exhaustive()
    }
}

impl fmt::Display for IssuedInvite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invite {} from {} at {} (expires in {}s)",
            self.id, self.issuer.companion_id, self.origin, self.expires_in_secs
        )
    }
}

/// An outstanding invite as the owner list shows it: no secret, only the
/// handle needed to cancel it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InviteSummary {
    pub id: String,
    pub state: PeerState,
    pub created_at: u64,
    pub expires_at: u64,
}

/// What the accepting owner hands their server: where the issuer is, its
/// identity document (pinned before any contact), and the secret.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptInvite {
    pub origin: String,
    pub secret: InviteSecret,
    pub issuer: IdentityDocument,
}

impl fmt::Debug for AcceptInvite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AcceptInvite")
            .field("origin", &self.origin)
            .field("issuer", &self.issuer.companion_id)
            .finish_non_exhaustive()
    }
}

impl fmt::Display for AcceptInvite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invite from {} at {}",
            self.issuer.companion_id, self.origin
        )
    }
}

/// Bodies of the pairing handshake. Each travels as the body of a
/// [`SignedEnvelope`] signed by the companion named as its sender, and each
/// names the pairing it belongs to, so a body of one kind can never be read
/// as another and a notice about an earlier pairing is stale.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum PairingMessage {
    /// Accepter to issuer: redeem the invite. Carries the accepter's own
    /// identity because the issuer has never seen it.
    #[serde(rename = "pair_request")]
    Request {
        version: u32,
        secret: InviteSecret,
        /// `companion_id` the accepter expects to be talking to.
        issuer: String,
        accepter: IdentityDocument,
        /// Base URL the accepter can be reached at, for the issuing owner to
        /// approve.
        origin: String,
    },
    /// Issuer to accepter, in the same exchange: the invite was redeemed and
    /// the pairing now waits for the issuing owner.
    #[serde(rename = "pair_response")]
    Response {
        version: u32,
        pairing_id: String,
        issuer: IdentityDocument,
        accepter: String,
        state: PeerState,
    },
    /// Issuer to accepter, after the issuing owner confirmed.
    #[serde(rename = "pair_confirm")]
    Confirm {
        version: u32,
        pairing_id: String,
        issuer: String,
        accepter: String,
    },
    /// Either side: trust withdrawn.
    #[serde(rename = "pair_revoke")]
    Revoke {
        version: u32,
        pairing_id: String,
        sender: String,
        peer: String,
    },
    /// Answer to a confirm or revoke notice.
    #[serde(rename = "pair_ack")]
    Ack {
        version: u32,
        pairing_id: String,
        sender: String,
        state: PeerState,
    },
}

/// Every way an identity document, signed envelope, or keystore can be
/// refused. Each variant is distinct so callers and tests can tell tampering,
/// downgrades, and deployment mistakes apart without parsing messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FederationError {
    /// The version is below [`MIN_FEDERATION_VERSION`].
    VersionTooOld {
        found: u32,
        min: u32,
    },
    /// The version is newer than anything this server understands.
    VersionUnsupported {
        found: u32,
    },
    /// The shape or an encoding is wrong: unknown fields, missing fields,
    /// padding, wrong lengths. For the public shapes the message may quote
    /// the parser's reason, including a rejected value, so a peer's broken
    /// document can be debugged; for the signing key file it never carries
    /// anything from the file.
    Malformed(String),
    /// The public key bytes are not a valid Ed25519 point, or are a
    /// small-order point that no seed produces and that would verify forged
    /// signatures under lax verification.
    InvalidPublicKey,
    /// The signature does not verify over the canonical bytes: the content or
    /// the signature was altered.
    SignatureMismatch,
    /// The document's `companion_id` is not derived from its `public_key`.
    CompanionIdMismatch,
    /// The envelope names a sender other than the identity it was checked
    /// against.
    SenderMismatch,
    /// The stored identity document was not signed by the stored signing key.
    KeyMismatch,
    /// An identity document exists without its signing key.
    SigningKeyMissing(PathBuf),
    /// A signing key exists without its identity document.
    IdentityDocumentMissing(PathBuf),
    /// The signing key file is readable by other users.
    InsecureKeyPermissions {
        path: PathBuf,
        mode: u32,
    },
    /// Operating system randomness was unavailable while generating a key.
    RandomnessUnavailable,
    Io {
        path: PathBuf,
        message: String,
    },
    /// The invite secret is wrong, expired, cancelled, or already redeemed.
    /// One variant on purpose: a caller learns nothing about which.
    InviteInvalid,
    /// The pair request names an issuer other than this companion.
    IssuerMismatch,
    /// A notice names a recipient other than this companion.
    RecipientMismatch,
    /// A notice names a pairing other than the one on record for its sender.
    PairingMismatch,
    /// The sender is not a peer of this companion.
    UnknownPeer,
    /// The peer was revoked; nothing from it verifies until it pairs again.
    PeerRevoked,
    /// The peer exists but is not paired yet, or the handshake step does not
    /// fit its state.
    PeerNotPaired {
        state: PeerState,
    },
    /// A base URL for a peer is not `http(s)://host[:port][/path]`.
    InvalidOrigin(String),
    /// The peer could not be reached or answered something other than an
    /// envelope.
    Transport(String),
    /// The peer answered with an error status.
    PeerRefused {
        status: u16,
        error: String,
    },
    /// The transport envelope's `expires_at` lies further in the past than
    /// the clock-skew allowance.
    Expired {
        expires_at: u64,
        now: u64,
    },
    /// The transport envelope's `issued_at` lies further in the future than
    /// the clock-skew allowance.
    IssuedInFuture {
        issued_at: u64,
        now: u64,
    },
    /// `expires_at` is not after `issued_at`, or the lifetime exceeds the
    /// maximum an envelope may claim.
    InvalidLifetime {
        issued_at: u64,
        expires_at: u64,
    },
    /// The body does not hash to the signed `body_hash`: the body was
    /// altered after signing.
    BodyHashMismatch,
    /// The nonce was already accepted from this sender: a replay.
    Replayed,
    /// The replay set is full; nothing verifies until entries expire.
    ReplayCapacity,
    /// The sender is a previous key of a peer whose transition window has
    /// closed; only the rotated key verifies now.
    KeyRetired,
    /// A rotation notice does not fit the peer on record: the previous
    /// identity is not the sender's, the new key is the old one, or the new
    /// identity already belongs to someone else.
    RotationMismatch,
    /// The owner's federation policy did not allow the intent (#109); the
    /// decision says whether it was denied, needs the owner, or waits.
    PolicyRefused(crate::domain::federation_policy::Decision),
}

impl fmt::Display for FederationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::VersionTooOld { found, min } => write!(
                f,
                "federation version {found} is older than the minimum accepted version {min}"
            ),
            Self::VersionUnsupported { found } => {
                write!(f, "federation version {found} is not supported")
            }
            Self::Malformed(message) => write!(f, "malformed federation data: {message}"),
            Self::InvalidPublicKey => f.write_str("federation public key is not a valid key"),
            Self::SignatureMismatch => f.write_str("federation signature does not verify"),
            Self::CompanionIdMismatch => {
                f.write_str("federation companion id is not derived from the public key")
            }
            Self::SenderMismatch => {
                f.write_str("federation envelope sender does not match the verified identity")
            }
            Self::KeyMismatch => {
                f.write_str("federation identity document was not signed by the stored key")
            }
            Self::SigningKeyMissing(path) => {
                write!(f, "federation signing key is missing at {}", path.display())
            }
            Self::IdentityDocumentMissing(path) => write!(
                f,
                "federation identity document is missing at {}",
                path.display()
            ),
            Self::InsecureKeyPermissions { path, mode } => write!(
                f,
                "federation signing key {} has mode {mode:o}; it must be readable by the owner only",
                path.display()
            ),
            Self::RandomnessUnavailable => {
                f.write_str("operating system randomness is unavailable")
            }
            Self::Io { path, message } => write!(f, "{}: {message}", path.display()),
            Self::InviteInvalid => f.write_str("federation invite is invalid or expired"),
            Self::IssuerMismatch => {
                f.write_str("federation pair request is addressed to another companion")
            }
            Self::RecipientMismatch => {
                f.write_str("federation notice is addressed to another companion")
            }
            Self::PairingMismatch => {
                f.write_str("federation notice names a pairing other than the one on record")
            }
            Self::UnknownPeer => f.write_str("federation sender is not a peer of this companion"),
            Self::PeerRevoked => f.write_str("federation peer is revoked"),
            Self::PeerNotPaired { state } => {
                write!(f, "federation peer is {state}, not paired")
            }
            Self::InvalidOrigin(origin) => {
                write!(f, "federation origin {origin:?} is not a base URL")
            }
            Self::Transport(message) => write!(f, "federation peer unreachable: {message}"),
            Self::PeerRefused { status, error } => {
                write!(f, "federation peer refused with {status} {error}")
            }
            Self::Expired { expires_at, now } => write!(
                f,
                "federation envelope expired at {expires_at}, before {now} less the skew allowance"
            ),
            Self::IssuedInFuture { issued_at, now } => write!(
                f,
                "federation envelope is issued at {issued_at}, beyond {now} plus the skew allowance"
            ),
            Self::InvalidLifetime {
                issued_at,
                expires_at,
            } => write!(
                f,
                "federation envelope lifetime from {issued_at} to {expires_at} is not allowed"
            ),
            Self::BodyHashMismatch => {
                f.write_str("federation envelope body does not match its signed hash")
            }
            Self::Replayed => f.write_str("federation envelope nonce was already accepted"),
            Self::ReplayCapacity => f.write_str("federation replay set is full"),
            Self::KeyRetired => f.write_str(
                "federation sender key was rotated away and its transition window closed",
            ),
            Self::RotationMismatch => {
                f.write_str("federation rotation does not fit the peer on record")
            }
            Self::PolicyRefused(decision) => write!(f, "federation policy: {decision}"),
        }
    }
}

impl std::error::Error for FederationError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_document_round_trips_and_rejects_unknown_fields() {
        let document = IdentityDocument {
            version: FEDERATION_VERSION,
            companion_id: "id".into(),
            public_key: "pk".into(),
            created_at: 7,
            signature: "sig".into(),
        };
        let json = serde_json::to_string(&document).unwrap();
        assert_eq!(
            serde_json::from_str::<IdentityDocument>(&json).unwrap(),
            document
        );
        assert!(
            serde_json::from_str::<IdentityDocument>(
                r#"{"version":1,"companion_id":"id","public_key":"pk","created_at":7,"signature":"sig","hostname":"mac-mini"}"#
            )
            .is_err(),
            "deployment metadata must never ride along in the identity document"
        );
        assert!(
            serde_json::from_str::<SignedEnvelope>(
                r#"{"version":1,"sender":"id","body":"","signature":"sig","profile":"molinka"}"#
            )
            .is_err()
        );
    }

    #[test]
    fn errors_are_distinct_and_describe_themselves_without_contents() {
        let errors = [
            FederationError::VersionTooOld { found: 0, min: 1 },
            FederationError::VersionUnsupported { found: 2 },
            FederationError::Malformed("signature length".into()),
            FederationError::InvalidPublicKey,
            FederationError::SignatureMismatch,
            FederationError::CompanionIdMismatch,
            FederationError::SenderMismatch,
            FederationError::KeyMismatch,
            FederationError::SigningKeyMissing("k".into()),
            FederationError::IdentityDocumentMissing("d".into()),
            FederationError::InsecureKeyPermissions {
                path: "k".into(),
                mode: 0o644,
            },
            FederationError::RandomnessUnavailable,
            FederationError::Io {
                path: "k".into(),
                message: "denied".into(),
            },
            FederationError::InviteInvalid,
            FederationError::IssuerMismatch,
            FederationError::RecipientMismatch,
            FederationError::PairingMismatch,
            FederationError::UnknownPeer,
            FederationError::PeerRevoked,
            FederationError::PeerNotPaired {
                state: PeerState::Pending,
            },
            FederationError::InvalidOrigin("ftp://x".into()),
            FederationError::Transport("timed out".into()),
            FederationError::PeerRefused {
                status: 401,
                error: "invalid_invite".into(),
            },
            FederationError::Expired {
                expires_at: 10,
                now: 200,
            },
            FederationError::IssuedInFuture {
                issued_at: 400,
                now: 200,
            },
            FederationError::InvalidLifetime {
                issued_at: 10,
                expires_at: 5,
            },
            FederationError::BodyHashMismatch,
            FederationError::Replayed,
            FederationError::ReplayCapacity,
            FederationError::KeyRetired,
            FederationError::RotationMismatch,
            FederationError::PolicyRefused(crate::domain::federation_policy::Decision::ask(
                crate::domain::federation_policy::DecisionReason::Default,
            )),
        ];
        let rendered: Vec<String> = errors.iter().map(ToString::to_string).collect();
        for (index, text) in rendered.iter().enumerate() {
            assert!(!text.is_empty());
            assert!(
                rendered.iter().filter(|other| *other == text).count() == 1,
                "error {index} shares its message with another variant: {text}"
            );
        }
        assert!(rendered[0].contains("older than"));
        assert!(rendered[10].contains("644"));
        assert_eq!(
            rendered.last().unwrap(),
            "federation policy: ask (default)",
            "a policy refusal says the verdict and the reason, nothing about the intent's payload"
        );
    }

    const SECRET: &str = "issue-108-invite-secret-that-must-never-print";

    fn document() -> IdentityDocument {
        IdentityDocument {
            version: FEDERATION_VERSION,
            companion_id: "cid".into(),
            public_key: "pk".into(),
            created_at: 7,
            signature: "sig".into(),
        }
    }

    fn issued() -> IssuedInvite {
        IssuedInvite {
            id: "0011223344556677".into(),
            secret: InviteSecret::new(SECRET.into()),
            created_at: 100,
            expires_at: 700,
            expires_in_secs: 600,
            origin: "https://a.example".into(),
            issuer: document(),
        }
    }

    #[test]
    fn invite_types_never_format_the_secret() {
        let issued = issued();
        let accept = AcceptInvite {
            origin: "https://a.example".into(),
            secret: InviteSecret::new(SECRET.into()),
            issuer: document(),
        };
        let request = PairingMessage::Request {
            version: FEDERATION_VERSION,
            secret: InviteSecret::new(SECRET.into()),
            issuer: "cid".into(),
            accepter: document(),
            origin: "https://b.example".into(),
        };
        let rendered = [
            format!("{issued:?}"),
            format!("{issued}"),
            format!("{issued:#?}"),
            format!("{accept:?}"),
            format!("{accept}"),
            format!("{accept:#?}"),
            format!("{request:?}"),
            format!("{request:#?}"),
            format!("{:?}", issued.secret),
            format!("{:?}", FederationError::InviteInvalid),
        ];
        for (index, text) in rendered.iter().enumerate() {
            assert!(!text.is_empty(), "rendering {index} is empty");
            assert!(
                !text.contains(SECRET) && !text.contains(&SECRET[..12]),
                "rendering {index} leaks the secret: {text}"
            );
        }
        assert!(rendered[0].contains("0011223344556677"), "{}", rendered[0]);
        assert!(rendered[1].contains("0011223344556677"), "{}", rendered[1]);
        assert!(rendered[4].contains("https://a.example"), "{}", rendered[4]);
        assert!(rendered[8].contains("redacted"), "{}", rendered[8]);
        // The secret still travels: the owner response and the wire body
        // carry it, which is the only way it ever leaves the process.
        assert!(serde_json::to_string(&issued).unwrap().contains(SECRET));
        assert!(serde_json::to_string(&request).unwrap().contains(SECRET));
        assert_eq!(issued.secret.expose(), SECRET);
    }

    #[test]
    fn pairing_messages_are_kind_tagged_and_reject_unknown_fields() {
        let confirm = PairingMessage::Confirm {
            version: FEDERATION_VERSION,
            pairing_id: "p".into(),
            issuer: "a".into(),
            accepter: "b".into(),
        };
        let json = serde_json::to_value(&confirm).unwrap();
        assert_eq!(json["kind"], "pair_confirm");
        let parsed: PairingMessage = serde_json::from_value(json).unwrap();
        assert!(matches!(parsed, PairingMessage::Confirm { .. }));

        for bad in [
            // extra deployment metadata
            r#"{"kind":"pair_confirm","version":1,"pairing_id":"p","issuer":"a","accepter":"b","hostname":"m"}"#,
            // wrong kind for the fields
            r#"{"kind":"pair_ack","version":1,"pairing_id":"p","issuer":"a","accepter":"b"}"#,
            // unknown kind
            r#"{"kind":"pair_hello","version":1}"#,
            // missing kind
            r#"{"version":1,"pairing_id":"p","sender":"a","state":"paired"}"#,
        ] {
            assert!(
                serde_json::from_str::<PairingMessage>(bad).is_err(),
                "accepted {bad}"
            );
        }
        assert!(
            serde_json::from_str::<AcceptInvite>(
                r#"{"origin":"https://a","secret":"s","issuer":{"version":1,"companion_id":"c","public_key":"k","created_at":1,"signature":"s"},"profile":"molinka"}"#
            )
            .is_err()
        );
    }

    #[test]
    fn transport_envelope_and_messages_are_strict_shapes() {
        let envelope = TransportEnvelope {
            version: FEDERATION_VERSION,
            sender: "a".into(),
            recipient: "b".into(),
            nonce: "n".into(),
            issued_at: 1,
            expires_at: 61,
            body_hash: "h".into(),
            body: "".into(),
            signature: "s".into(),
        };
        let json = serde_json::to_value(&envelope).unwrap();
        let mut keys: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "body",
                "body_hash",
                "expires_at",
                "issued_at",
                "nonce",
                "recipient",
                "sender",
                "signature",
                "version",
            ]
        );
        assert_eq!(
            serde_json::from_value::<TransportEnvelope>(json.clone()).unwrap(),
            envelope
        );
        for extra in ["hostname", "port", "profile", "origin"] {
            let mut with_extra = json.clone();
            with_extra[extra] = serde_json::json!("x");
            assert!(
                serde_json::from_value::<TransportEnvelope>(with_extra).is_err(),
                "deployment metadata {extra:?} rode along in a transport envelope"
            );
        }
        for missing in ["nonce", "recipient", "expires_at", "body_hash"] {
            let mut without = json.clone();
            without.as_object_mut().unwrap().remove(missing);
            assert!(
                serde_json::from_value::<TransportEnvelope>(without).is_err(),
                "an envelope without {missing:?} was accepted"
            );
        }

        let ping = TransportMessage::Ping {
            version: FEDERATION_VERSION,
        };
        let json = serde_json::to_value(&ping).unwrap();
        assert_eq!(json, serde_json::json!({ "kind": "ping", "version": 1 }));
        assert_eq!(
            serde_json::from_value::<TransportMessage>(json).unwrap(),
            ping
        );
        let ack = TransportMessage::RotationAck {
            version: FEDERATION_VERSION,
            companion_id: "c".into(),
            state: PeerState::Paired,
        };
        assert_eq!(serde_json::to_value(&ack).unwrap()["kind"], "rotation_ack");
        for bad in [
            r#"{"kind":"ping","version":1,"hostname":"m"}"#,
            r#"{"kind":"pong"}"#,
            r#"{"kind":"hello","version":1}"#,
            r#"{"version":1}"#,
            r#"{"kind":"key_rotation","version":1}"#,
        ] {
            assert!(
                serde_json::from_str::<TransportMessage>(bad).is_err(),
                "accepted {bad}"
            );
        }
    }

    #[test]
    fn key_transitions_carry_the_whole_rotation() {
        let rotation = KeyRotation {
            version: FEDERATION_VERSION,
            previous: document(),
            identity: IdentityDocument {
                companion_id: "next".into(),
                ..document()
            },
            rotated_at: 50,
            endorsement: "e".into(),
            signature: "s".into(),
        };
        let transition = KeyTransition {
            rotation: rotation.clone(),
            accepted_at: 60,
        };
        assert_eq!(transition.previous_companion_id(), "cid");
        assert_eq!(transition.rotation.identity.companion_id, "next");
        let json = serde_json::to_value(&transition).unwrap();
        let mut keys: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["accepted_at", "rotation"]);
        let mut rotation_keys: Vec<&str> = json["rotation"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        rotation_keys.sort_unstable();
        assert_eq!(
            rotation_keys,
            [
                "endorsement",
                "identity",
                "previous",
                "rotated_at",
                "signature",
                "version",
            ]
        );
        assert_eq!(
            serde_json::from_value::<KeyTransition>(json.clone()).unwrap(),
            transition
        );
        let mut extra = json;
        extra["rotation"]["origin"] = serde_json::json!("https://b.example");
        assert!(serde_json::from_value::<KeyTransition>(extra).is_err());
    }

    #[test]
    fn peer_state_and_record_shapes_are_stable() {
        for (state, text) in [
            (PeerState::Invited, "invited"),
            (PeerState::Pending, "pending"),
            (PeerState::Paired, "paired"),
            (PeerState::Revoked, "revoked"),
        ] {
            assert_eq!(state.to_string(), text);
            assert_eq!(serde_json::to_value(state).unwrap(), text);
        }
        let record = PeerRecord {
            identity: document(),
            state: PeerState::Paired,
            role: PairingRole::Issuer,
            pairing_id: "p".into(),
            approved_origins: vec!["https://b.example".into()],
            pending_origin: None,
            created_at: 1,
            updated_at: 2,
            rotation_history: Vec::new(),
        };
        let json = serde_json::to_value(&record).unwrap();
        let mut keys: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "approved_origins",
                "created_at",
                "identity",
                "pairing_id",
                "role",
                "rotation_history",
                "state",
                "updated_at",
            ],
            "the record persists trust state only"
        );
        assert_eq!(
            serde_json::from_value::<PeerRecord>(json.clone()).unwrap(),
            record
        );
        let mut extra = json;
        extra["display_name"] = serde_json::json!("Molinka");
        assert!(serde_json::from_value::<PeerRecord>(extra).is_err());
        let summary = record.summary();
        assert_eq!(summary.companion_id, "cid");
        assert_eq!(summary.public_key, "pk");
        assert_eq!(summary.state, PeerState::Paired);
    }
}
