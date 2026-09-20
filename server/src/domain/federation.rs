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
//! Everything here is a shape. Signing, verification, and the keystore live in
//! `services::federation::identity`; the peer store and the handshake live in
//! `services::federation::{peers, pairing}`; the on-disk location is
//! documented in `docs/companion-storage.md`.

// Foundation for federation pairing and transport (#108, later PRs); nothing
// reaches these types from a route yet.
#![allow(dead_code)]

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
/// Transport envelopes (nonce, expiry, recipient) extend this in later work.
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

/// One audited key transition of a peer. Nothing writes these yet: key
/// rotation (#108, PR 3) fills the history; the field exists so the store
/// format does not change when it does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyTransition {
    pub previous_public_key: String,
    pub public_key: String,
    pub rotated_at: u64,
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
        todo!("PR 2: redact")
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
        todo!("PR 2: redact")
    }
}

impl fmt::Display for IssuedInvite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!("PR 2: redact")
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
        todo!("PR 2: redact")
    }
}

impl fmt::Display for AcceptInvite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!("PR 2: redact")
    }
}

/// Bodies of the pairing handshake. Each travels as the body of a
/// [`SignedEnvelope`] signed by the companion named as its sender, and each
/// names the pairing it belongs to, so a body of one kind can never be read
/// as another and a notice about an earlier pairing is stale.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PairingMessage {
    /// Accepter to issuer: redeem the invite. Carries the accepter's own
    /// identity because the issuer has never seen it.
    PairRequest {
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
    PairResponse {
        version: u32,
        pairing_id: String,
        issuer: IdentityDocument,
        accepter: String,
        state: PeerState,
    },
    /// Issuer to accepter, after the issuing owner confirmed.
    PairConfirm {
        version: u32,
        pairing_id: String,
        issuer: String,
        accepter: String,
    },
    /// Either side: trust withdrawn.
    PairRevoke {
        version: u32,
        pairing_id: String,
        sender: String,
        peer: String,
    },
    /// Answer to a confirm or revoke notice.
    PairAck {
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
    /// Too many invite redemptions failed recently.
    RateLimited,
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
            Self::RateLimited => f.write_str("too many failed federation pairing attempts"),
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
            FederationError::RateLimited,
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
        let request = PairingMessage::PairRequest {
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
        let confirm = PairingMessage::PairConfirm {
            version: FEDERATION_VERSION,
            pairing_id: "p".into(),
            issuer: "a".into(),
            accepter: "b".into(),
        };
        let json = serde_json::to_value(&confirm).unwrap();
        assert_eq!(json["kind"], "pair_confirm");
        let parsed: PairingMessage = serde_json::from_value(json).unwrap();
        assert!(matches!(parsed, PairingMessage::PairConfirm { .. }));

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
