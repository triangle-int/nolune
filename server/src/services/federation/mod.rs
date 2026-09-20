//! Companion federation primitives (#108): canonical encoding, companion id
//! derivation, and the Ed25519 identity keystore in [`identity`].
//!
//! Canonical bytes are what gets signed. Every signed shape starts with a
//! NUL-terminated domain tag, then its fields in a fixed order: integers as
//! big-endian `u32`/`u64`, byte strings as a big-endian `u32` length followed
//! by the bytes. There is no self-describing structure, so two encoders
//! agree byte for byte or a signature simply does not verify.
//! `server/tests/fixtures/federation/generate.py` is the second encoder.
//!
//! Nothing in this module logs. Secrets never leave the process except
//! through the keystore's private files.

// Foundation for federation pairing and transport (#108, later PRs); routes
// and startup wiring arrive with the peer store.
#![allow(dead_code)]

pub mod identity;

use crate::domain::federation::{FederationError, PUBLIC_KEY_BYTES};

/// Domain tag for `companion_id = sha256(tag || public_key)`.
pub(crate) const COMPANION_ID_DOMAIN: &[u8] = b"nolune/federation/companion-id/v1\0";
/// Domain tag for the self-signature in an identity document.
pub(crate) const IDENTITY_SIGNING_DOMAIN: &[u8] = b"nolune/federation/identity/v1\0";
/// Domain tag for a signed envelope.
pub(crate) const ENVELOPE_SIGNING_DOMAIN: &[u8] = b"nolune/federation/envelope/v1\0";

/// Builder for canonical signing bytes; see the module docs for the layout.
pub(crate) struct Canonical {
    bytes: Vec<u8>,
}

impl Canonical {
    pub(crate) fn new(domain: &[u8]) -> Self {
        todo!("#108: canonical encoding")
    }

    pub(crate) fn u32(self, value: u32) -> Self {
        todo!("#108: canonical encoding")
    }

    pub(crate) fn u64(self, value: u64) -> Self {
        todo!("#108: canonical encoding")
    }

    pub(crate) fn bytes(self, value: &[u8]) -> Self {
        todo!("#108: canonical encoding")
    }

    pub(crate) fn str(self, value: &str) -> Self {
        self.bytes(value.as_bytes())
    }

    pub(crate) fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

/// base64url without padding, the encoding of every binary wire field.
pub(crate) fn encode(bytes: &[u8]) -> String {
    todo!("#108: canonical base64url")
}

/// Decodes a base64url field that must be exactly `len` bytes and must be the
/// canonical encoding of those bytes (no padding, no trailing bits).
pub(crate) fn decode_exact(value: &str, len: usize) -> Option<Vec<u8>> {
    todo!("#108: canonical base64url")
}

/// Stable companion id: base64url of `sha256(COMPANION_ID_DOMAIN || public_key)`.
pub fn companion_id_for(public_key: &[u8; PUBLIC_KEY_BYTES]) -> String {
    todo!("#108: companion id derivation")
}

/// Rejects versions this server does not speak, downgrades first.
pub(crate) fn check_version(found: u32) -> Result<(), FederationError> {
    todo!("#108: version gate")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::federation::{FEDERATION_VERSION, MIN_FEDERATION_VERSION};

    #[test]
    fn canonical_bytes_are_length_prefixed_big_endian_after_the_domain() {
        let bytes = Canonical::new(b"tag\0")
            .u32(1)
            .str("ab")
            .bytes(&[0xff])
            .u64(2)
            .finish();
        assert_eq!(
            bytes,
            [
                b"tag\0".as_slice(),
                &[0, 0, 0, 1],
                &[0, 0, 0, 2, b'a', b'b'],
                &[0, 0, 0, 1, 0xff],
                &[0, 0, 0, 0, 0, 0, 0, 2],
            ]
            .concat()
        );
        // An empty field still carries its length, so "" then "x" differs from "x" then "".
        assert_ne!(
            Canonical::new(b"t").str("").str("x").finish(),
            Canonical::new(b"t").str("x").str("").finish()
        );
    }

    #[test]
    fn base64url_fields_must_be_canonical_and_exact() {
        let raw = [0u8, 1, 2, 250, 251, 252, 253, 254, 255];
        let encoded = encode(&raw);
        assert!(!encoded.contains('='));
        assert!(!encoded.contains('+') && !encoded.contains('/'));
        assert_eq!(decode_exact(&encoded, raw.len()).unwrap(), raw);
        assert!(decode_exact(&encoded, raw.len() + 1).is_none(), "length");
        assert!(
            decode_exact(&format!("{encoded}="), raw.len()).is_none(),
            "padding"
        );
        assert!(decode_exact("AB+/", 3).is_none(), "standard alphabet");
        // "AR" and "AQ" both decode to [1] with lenient decoders; only "AQ" is canonical.
        assert_eq!(decode_exact("AQ", 1).unwrap(), [1]);
        assert!(decode_exact("AR", 1).is_none(), "trailing bits");
        assert!(decode_exact("", 0).is_some());
        assert!(decode_exact("", 1).is_none());
    }

    #[test]
    fn companion_id_is_domain_separated_and_url_safe() {
        let key = [3u8; PUBLIC_KEY_BYTES];
        let id = companion_id_for(&key);
        assert_eq!(id.len(), 43, "43 base64url chars for 32 digest bytes");
        assert!(
            id.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        );
        assert_ne!(id, companion_id_for(&[4u8; PUBLIC_KEY_BYTES]));
        let plain = {
            use sha2::{Digest, Sha256};
            encode(&Sha256::digest(key))
        };
        assert_ne!(id, plain, "the id must not be a bare hash of the key");
    }

    #[test]
    fn version_gate_rejects_downgrades_and_unknown_versions() {
        assert_eq!(check_version(FEDERATION_VERSION), Ok(()));
        assert_eq!(
            check_version(MIN_FEDERATION_VERSION - 1),
            Err(FederationError::VersionTooOld {
                found: MIN_FEDERATION_VERSION - 1,
                min: MIN_FEDERATION_VERSION,
            })
        );
        assert_eq!(
            check_version(FEDERATION_VERSION + 1),
            Err(FederationError::VersionUnsupported {
                found: FEDERATION_VERSION + 1,
            })
        );
    }
}
