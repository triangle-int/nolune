//! Wire shapes for companion federation (#108).
//!
//! A federation peer is a companion signing identity, never a machine, a
//! profile name, a port, or a hostname. The [`IdentityDocument`] is the only
//! thing a peer ever binds trust to: its `companion_id` is derived from the
//! Ed25519 public key it carries and the document is self-signed over the
//! canonical bytes described in `services::federation`.
//!
//! Everything here is a shape. Signing, verification, and the keystore live in
//! `services::federation::identity`; the on-disk location is documented in
//! `docs/companion-storage.md`.

// Foundation for federation pairing and transport (#108, later PRs); nothing
// reaches these types from a route yet.
#![allow(dead_code)]

use std::{fmt, path::PathBuf};

use serde::{Deserialize, Serialize};

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
    /// padding, wrong lengths. Never carries the offending contents.
    Malformed(String),
    /// The public key bytes are not a valid Ed25519 point.
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
}
