//! The companion's federation signing identity (#108).
//!
//! One Ed25519 key per server profile, generated on first use and kept in
//! `federation/signing_key.json` (mode 0600) under the workspace root, beside a
//! self-signed [`IdentityDocument`] in `federation/identity.json`. The
//! directory sits outside `instances/companion/`, so the companion export
//! archive never contains the key; moving the companion to another host means
//! copying `federation/` too, and nothing about the old host, port, profile
//! name, or path is part of the identity.
//!
//! Every function here is pure over a workspace root. Wiring into startup and
//! routes arrives with the peer store. Nothing here logs.

use std::{
    fmt,
    path::{Path, PathBuf},
};

use ed25519_dalek::{SigningKey, VerifyingKey};

use crate::domain::federation::{FederationError, IdentityDocument, SignedEnvelope};

/// Directory under the workspace root. Deliberately not under `instances/`.
pub const FEDERATION_DIR: &str = "federation";
/// Public, self-signed identity document.
pub const IDENTITY_FILE: &str = "identity.json";
/// Private Ed25519 seed. Owner-only permissions are enforced on load.
pub const SIGNING_KEY_FILE: &str = "signing_key.json";

/// `workspace/federation`
pub fn federation_dir(workspace_root: &Path) -> PathBuf {
    workspace_root.join(FEDERATION_DIR)
}

/// `workspace/federation/identity.json`
pub fn identity_path(workspace_root: &Path) -> PathBuf {
    federation_dir(workspace_root).join(IDENTITY_FILE)
}

/// `workspace/federation/signing_key.json`
pub fn signing_key_path(workspace_root: &Path) -> PathBuf {
    federation_dir(workspace_root).join(SIGNING_KEY_FILE)
}

/// An identity document whose self-signature and id derivation were checked.
/// Only [`verify_document`] produces one, so an envelope verifier can require
/// it instead of a bare key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedIdentity {
    pub version: u32,
    pub companion_id: String,
    pub public_key: VerifyingKey,
    pub created_at: u64,
}

/// This server's own identity: the signing key plus its verified document.
/// `Debug` and `Display` show the public half only.
pub struct SigningIdentity {
    key: SigningKey,
    document: IdentityDocument,
    verified: VerifiedIdentity,
}

impl SigningIdentity {
    pub fn companion_id(&self) -> &str {
        &self.verified.companion_id
    }

    pub fn document(&self) -> &IdentityDocument {
        &self.document
    }

    pub fn verified(&self) -> &VerifiedIdentity {
        &self.verified
    }

    /// Signs `body` as this companion at the current wire version.
    pub fn sign_envelope(&self, body: &[u8]) -> SignedEnvelope {
        todo!("#108: envelope signing")
    }
}

impl fmt::Debug for SigningIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!("#108: redacted debug")
    }
}

impl fmt::Display for SigningIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!("#108: redacted display")
    }
}

/// Loads the identity, generating and persisting a new one when neither file
/// exists yet. A half-present or inconsistent keystore fails closed.
pub fn load_or_create(workspace_root: &Path) -> Result<SigningIdentity, FederationError> {
    todo!("#108: keystore")
}

/// [`load_or_create`] with an explicit creation time (unix seconds).
pub(crate) fn load_or_create_at(
    workspace_root: &Path,
    now: u64,
) -> Result<SigningIdentity, FederationError> {
    todo!("#108: keystore")
}

/// `Ok(None)` when no identity was created yet; `Err` when the files exist but
/// cannot be trusted: missing halves, wrong permissions, a document that does
/// not verify, or a document not signed by the stored key.
pub fn load(workspace_root: &Path) -> Result<Option<SigningIdentity>, FederationError> {
    todo!("#108: keystore")
}

/// Parses an identity document, refusing unsupported versions before the
/// strict shape check so a downgrade is reported as such.
pub fn parse_document(json: &str) -> Result<IdentityDocument, FederationError> {
    todo!("#108: parsing")
}

/// Checks version, encodings, id derivation, and the self-signature.
pub fn verify_document(document: &IdentityDocument) -> Result<VerifiedIdentity, FederationError> {
    todo!("#108: verification")
}

/// Parses a signed envelope with the same version-first policy as documents.
pub fn parse_envelope(json: &str) -> Result<SignedEnvelope, FederationError> {
    todo!("#108: parsing")
}

/// Verifies `envelope` against a sender whose identity was already verified
/// and returns the body bytes. The sender id in the envelope must match.
pub fn verify_envelope(
    envelope: &SignedEnvelope,
    sender: &VerifiedIdentity,
) -> Result<Vec<u8>, FederationError> {
    todo!("#108: verification")
}

/// Builds an identity from a raw seed. `companion_id` is passed explicitly so
/// tests can produce a document that lies about it.
fn from_seed(seed: &[u8; 32], created_at: u64) -> SigningIdentity {
    todo!("#108: keystore")
}

fn sign_document(
    key: &SigningKey,
    version: u32,
    companion_id: &str,
    created_at: u64,
) -> IdentityDocument {
    todo!("#108: signing")
}

fn sign_envelope_with(key: &SigningKey, version: u32, sender: &str, body: &[u8]) -> SignedEnvelope {
    todo!("#108: signing")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::federation::{FEDERATION_VERSION, MIN_FEDERATION_VERSION},
        services::{
            companion::companion_dir,
            federation::{companion_id_for, decode_exact, encode},
        },
    };
    use sha2::{Digest, Sha256};

    const IDENTITY_FIXTURE: &str =
        include_str!("../../../tests/fixtures/federation/identity_v1.json");
    const ENVELOPE_FIXTURE: &str =
        include_str!("../../../tests/fixtures/federation/envelope_v1.json");
    /// Matches `FIXTURE_SEED_LABEL` in `tests/fixtures/federation/generate.py`.
    const FIXTURE_SEED_LABEL: &[u8] = b"nolune/federation/fixture-seed/v1";
    const FIXTURE_CREATED_AT: u64 = 1_789_862_400;
    const FIXTURE_BODY: &[u8] = br#"{"kind":"ping"}"#;

    fn fixture_seed() -> [u8; 32] {
        Sha256::digest(FIXTURE_SEED_LABEL).into()
    }

    fn fixture_identity() -> SigningIdentity {
        from_seed(&fixture_seed(), FIXTURE_CREATED_AT)
    }

    fn other_identity() -> SigningIdentity {
        from_seed(&[7u8; 32], 1)
    }

    fn flip_last_signature_byte(signature: &str) -> String {
        let mut bytes = decode_exact(signature, 64).unwrap();
        bytes[63] ^= 0x01;
        encode(&bytes)
    }

    #[test]
    fn fixture_identity_document_verifies() {
        let document = parse_document(IDENTITY_FIXTURE).unwrap();
        let verified = verify_document(&document).unwrap();
        assert_eq!(verified.version, FEDERATION_VERSION);
        assert_eq!(verified.companion_id, document.companion_id);
        assert_eq!(verified.created_at, FIXTURE_CREATED_AT);
        assert_eq!(
            verified.companion_id,
            companion_id_for(&verified.public_key.to_bytes())
        );
    }

    #[test]
    fn fixture_envelope_verifies_against_the_fixture_identity() {
        let identity = verify_document(&parse_document(IDENTITY_FIXTURE).unwrap()).unwrap();
        let envelope = parse_envelope(ENVELOPE_FIXTURE).unwrap();
        assert_eq!(verify_envelope(&envelope, &identity).unwrap(), FIXTURE_BODY);
    }

    #[test]
    fn fixtures_are_reproduced_byte_for_byte_from_the_fixed_seed() {
        // The fixtures were signed by OpenSSL; Ed25519 is deterministic, so
        // the same seed and canonical bytes must give the same files.
        let identity = fixture_identity();
        let expected: IdentityDocument = serde_json::from_str(IDENTITY_FIXTURE).unwrap();
        assert_eq!(identity.document(), &expected);
        assert_eq!(
            serde_json::to_string_pretty(identity.document()).unwrap() + "\n",
            IDENTITY_FIXTURE
        );

        let envelope = identity.sign_envelope(FIXTURE_BODY);
        assert_eq!(
            serde_json::to_string_pretty(&envelope).unwrap() + "\n",
            ENVELOPE_FIXTURE
        );
    }

    #[test]
    fn tampered_document_fields_fail_closed() {
        let document = parse_document(IDENTITY_FIXTURE).unwrap();

        let mut created = document.clone();
        created.created_at += 1;
        assert_eq!(
            verify_document(&created),
            Err(FederationError::SignatureMismatch)
        );

        let mut swapped_key = document.clone();
        swapped_key.public_key = other_identity().document().public_key.clone();
        assert_eq!(
            verify_document(&swapped_key),
            Err(FederationError::CompanionIdMismatch)
        );

        let mut swapped_id = document.clone();
        swapped_id.companion_id = other_identity().companion_id().to_owned();
        assert_eq!(
            verify_document(&swapped_id),
            Err(FederationError::CompanionIdMismatch)
        );

        let mut bad_key = document.clone();
        bad_key.public_key = encode(&[0xff; 32]);
        bad_key.companion_id = companion_id_for(&[0xff; 32]);
        assert_eq!(
            verify_document(&bad_key),
            Err(FederationError::InvalidPublicKey)
        );

        let mut short_key = document.clone();
        short_key.public_key = encode(&[1; 31]);
        assert!(matches!(
            verify_document(&short_key),
            Err(FederationError::Malformed(_))
        ));
    }

    #[test]
    fn altered_signatures_fail_closed() {
        let document = parse_document(IDENTITY_FIXTURE).unwrap();

        let mut flipped = document.clone();
        flipped.signature = flip_last_signature_byte(&document.signature);
        assert_eq!(
            verify_document(&flipped),
            Err(FederationError::SignatureMismatch)
        );

        let mut foreign = document.clone();
        foreign.signature = other_identity().document().signature.clone();
        assert_eq!(
            verify_document(&foreign),
            Err(FederationError::SignatureMismatch)
        );

        for broken in [
            document.signature[..80].to_owned(),
            format!("{}=", document.signature),
            document.signature.replace('-', "+"),
            String::new(),
        ] {
            let mut malformed = document.clone();
            malformed.signature = broken;
            assert!(
                matches!(
                    verify_document(&malformed),
                    Err(FederationError::Malformed(_))
                ),
                "{:?}",
                malformed.signature
            );
        }
    }

    #[test]
    fn lower_and_unknown_versions_fail_closed_even_when_signed() {
        let identity = fixture_identity();
        let id = identity.companion_id().to_owned();

        let downgraded = sign_document(&identity.key, MIN_FEDERATION_VERSION - 1, &id, 5);
        assert_eq!(
            verify_document(&downgraded),
            Err(FederationError::VersionTooOld {
                found: MIN_FEDERATION_VERSION - 1,
                min: MIN_FEDERATION_VERSION,
            })
        );
        let future = sign_document(&identity.key, FEDERATION_VERSION + 1, &id, 5);
        assert_eq!(
            verify_document(&future),
            Err(FederationError::VersionUnsupported {
                found: FEDERATION_VERSION + 1,
            })
        );

        // The version gate runs before the shape check, so a downgrade is
        // reported as a downgrade even when the rest of the document is junk.
        assert_eq!(
            parse_document(r#"{"version":0,"hostname":"mini"}"#),
            Err(FederationError::VersionTooOld { found: 0, min: 1 })
        );
        assert_eq!(
            parse_document(r#"{"version":99}"#),
            Err(FederationError::VersionUnsupported { found: 99 })
        );
        assert!(matches!(
            parse_document(r#"{"companion_id":"x"}"#),
            Err(FederationError::Malformed(_))
        ));
        let mut with_extra: serde_json::Value = serde_json::from_str(IDENTITY_FIXTURE).unwrap();
        with_extra["port"] = 8080.into();
        assert!(matches!(
            parse_document(&with_extra.to_string()),
            Err(FederationError::Malformed(_))
        ));
    }

    #[test]
    fn mismatched_companion_id_fails_closed_even_when_signed() {
        let identity = fixture_identity();
        let lying = sign_document(
            &identity.key,
            FEDERATION_VERSION,
            other_identity().companion_id(),
            FIXTURE_CREATED_AT,
        );
        assert_eq!(
            verify_document(&lying),
            Err(FederationError::CompanionIdMismatch)
        );

        let mut not_an_id = lying;
        not_an_id.companion_id = "molinka".into();
        assert!(matches!(
            verify_document(&not_an_id),
            Err(FederationError::Malformed(_))
        ));
    }

    #[test]
    fn envelopes_fail_closed_on_wrong_sender_tampering_and_versions() {
        let identity = fixture_identity();
        let verified = identity.verified().clone();
        let envelope = identity.sign_envelope(FIXTURE_BODY);
        assert_eq!(verify_envelope(&envelope, &verified).unwrap(), FIXTURE_BODY);

        let stranger = other_identity();
        assert_eq!(
            verify_envelope(&envelope, stranger.verified()),
            Err(FederationError::SenderMismatch)
        );
        let mut renamed = envelope.clone();
        renamed.sender = stranger.companion_id().to_owned();
        assert_eq!(
            verify_envelope(&renamed, stranger.verified()),
            Err(FederationError::SignatureMismatch),
            "a stranger's id on a genuine envelope is not the stranger's signature"
        );

        let mut tampered = envelope.clone();
        tampered.body = encode(br#"{"kind":"pong"}"#);
        assert_eq!(
            verify_envelope(&tampered, &verified),
            Err(FederationError::SignatureMismatch)
        );

        let mut flipped = envelope.clone();
        flipped.signature = flip_last_signature_byte(&envelope.signature);
        assert_eq!(
            verify_envelope(&flipped, &verified),
            Err(FederationError::SignatureMismatch)
        );

        let mut padded = envelope.clone();
        padded.body = format!("{}=", envelope.body);
        assert!(matches!(
            verify_envelope(&padded, &verified),
            Err(FederationError::Malformed(_))
        ));

        let downgraded = sign_envelope_with(
            &identity.key,
            MIN_FEDERATION_VERSION - 1,
            identity.companion_id(),
            FIXTURE_BODY,
        );
        assert_eq!(
            verify_envelope(&downgraded, &verified),
            Err(FederationError::VersionTooOld {
                found: MIN_FEDERATION_VERSION - 1,
                min: MIN_FEDERATION_VERSION,
            })
        );
        assert_eq!(
            parse_envelope(r#"{"version":7,"sender":"x","body":"","signature":""}"#),
            Err(FederationError::VersionUnsupported { found: 7 })
        );
        assert!(matches!(
            parse_envelope(r#"{"version":1,"sender":"x","body":"","signature":"","nonce":"n"}"#),
            Err(FederationError::Malformed(_))
        ));
    }

    #[test]
    fn debug_and_display_never_print_the_secret() {
        let seed = fixture_seed();
        let identity = fixture_identity();
        let secret_forms = [
            encode(&seed),
            seed.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            format!("{seed:?}"),
            format!("{:?}", seed.as_slice()),
        ];
        for rendered in [format!("{identity:?}"), identity.to_string()] {
            assert!(rendered.contains(identity.companion_id()), "{rendered}");
            for secret in &secret_forms {
                assert!(!rendered.contains(secret.as_str()), "{rendered}");
            }
            assert!(!rendered.to_lowercase().contains("secret"), "{rendered}");
        }
        let verified = format!("{:?}", identity.verified());
        for secret in &secret_forms {
            assert!(!verified.contains(secret.as_str()));
        }
    }

    #[test]
    fn keystore_generates_once_and_reloads_the_same_identity() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert!(load(root).unwrap().is_none());
        assert!(!federation_dir(root).exists(), "load never creates files");

        let first = load_or_create_at(root, 1_700_000_000).unwrap();
        let verified = verify_document(first.document()).unwrap();
        assert_eq!(verified.companion_id, first.companion_id());
        assert_eq!(first.document().created_at, 1_700_000_000);
        assert_ne!(
            first.companion_id(),
            fixture_identity().companion_id(),
            "fresh keys are random"
        );

        let key_path = signing_key_path(root);
        let doc_path = identity_path(root);
        let key_file: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&key_path).unwrap()).unwrap();
        assert_eq!(key_file["version"], 1);
        assert_eq!(key_file["algorithm"], "ed25519");
        let secret = key_file["secret_key"].as_str().unwrap().to_owned();
        assert_eq!(decode_exact(&secret, 32).unwrap().len(), 32);
        let doc_text = std::fs::read_to_string(&doc_path).unwrap();
        assert!(!doc_text.contains(&secret), "the document is public");
        assert_eq!(
            parse_document(&doc_text).unwrap(),
            *first.document(),
            "the stored document is the pretty JSON of the identity"
        );
        assert!(!format!("{first:?}").contains(&secret));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&key_path), 0o600);
            assert_eq!(mode(&doc_path), 0o600);
            assert_eq!(mode(&federation_dir(root)), 0o700);
        }

        let again = load_or_create_at(root, 1_800_000_000).unwrap();
        assert_eq!(again.companion_id(), first.companion_id());
        assert_eq!(again.document(), first.document(), "never regenerated");
        let loaded = load(root).unwrap().expect("identity exists");
        assert_eq!(loaded.document(), first.document());
        assert_eq!(
            verify_envelope(&loaded.sign_envelope(b"hi"), first.verified()).unwrap(),
            b"hi"
        );
        assert_eq!(
            std::fs::read_to_string(&key_path).unwrap(),
            serde_json::to_string_pretty(&key_file).unwrap() + "\n",
            "the key file is never rewritten"
        );
    }

    #[test]
    fn keystore_fails_closed_on_missing_halves_tampering_and_foreign_keys() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let identity = load_or_create_at(root, 10).unwrap();
        let key_path = signing_key_path(root);
        let doc_path = identity_path(root);
        let key_text = std::fs::read(&key_path).unwrap();
        let doc_text = std::fs::read_to_string(&doc_path).unwrap();

        std::fs::remove_file(&doc_path).unwrap();
        assert_eq!(
            load(root).unwrap_err(),
            FederationError::IdentityDocumentMissing(doc_path.clone())
        );
        assert!(matches!(
            load_or_create_at(root, 11),
            Err(FederationError::IdentityDocumentMissing(_))
        ));
        assert_eq!(std::fs::read(&key_path).unwrap(), key_text, "key untouched");
        std::fs::write(&doc_path, &doc_text).unwrap();

        std::fs::remove_file(&key_path).unwrap();
        assert_eq!(
            load(root).unwrap_err(),
            FederationError::SigningKeyMissing(key_path.clone())
        );
        assert!(matches!(
            load_or_create_at(root, 11),
            Err(FederationError::SigningKeyMissing(_))
        ));
        assert!(
            !key_path.exists(),
            "a missing key is never silently replaced"
        );
        std::fs::write(&key_path, &key_text).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        assert_eq!(
            load(root).unwrap().unwrap().document(),
            identity.document(),
            "restored"
        );

        let mut tampered: serde_json::Value = serde_json::from_str(&doc_text).unwrap();
        tampered["created_at"] = 99.into();
        std::fs::write(&doc_path, tampered.to_string()).unwrap();
        assert_eq!(load(root).unwrap_err(), FederationError::SignatureMismatch);
        tampered["version"] = 0.into();
        std::fs::write(&doc_path, tampered.to_string()).unwrap();
        assert_eq!(
            load(root).unwrap_err(),
            FederationError::VersionTooOld { found: 0, min: 1 }
        );
        std::fs::write(&doc_path, "{not json").unwrap();
        assert!(matches!(load(root), Err(FederationError::Malformed(_))));
        std::fs::write(&doc_path, &doc_text).unwrap();

        let other = tempfile::tempdir().unwrap();
        load_or_create_at(other.path(), 10).unwrap();
        std::fs::copy(signing_key_path(other.path()), &key_path).unwrap();
        assert_eq!(load(root).unwrap_err(), FederationError::KeyMismatch);
        std::fs::write(
            &key_path,
            r#"{"version":1,"algorithm":"rsa","secret_key":"AA"}"#,
        )
        .unwrap();
        assert!(matches!(load(root), Err(FederationError::Malformed(_))));
        std::fs::write(&key_path, &key_text).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert_eq!(
                load(root).unwrap_err(),
                FederationError::InsecureKeyPermissions {
                    path: key_path.clone(),
                    mode: 0o644,
                }
            );
            std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        assert!(load(root).unwrap().is_some());
    }

    #[test]
    fn moving_the_federation_directory_preserves_the_identity() {
        let old_host = tempfile::tempdir().unwrap();
        let new_host = tempfile::tempdir().unwrap();
        let original = load_or_create_at(old_host.path(), 42).unwrap();

        std::fs::create_dir_all(federation_dir(new_host.path())).unwrap();
        for file in [SIGNING_KEY_FILE, IDENTITY_FILE] {
            std::fs::copy(
                federation_dir(old_host.path()).join(file),
                federation_dir(new_host.path()).join(file),
            )
            .unwrap();
        }
        let moved = load(new_host.path()).unwrap().expect("identity travelled");
        assert_eq!(moved.document(), original.document());
        assert_eq!(
            verify_envelope(&moved.sign_envelope(b"same key"), original.verified()).unwrap(),
            b"same key"
        );
        // Neither document mentions where it lives.
        let text = serde_json::to_string(original.document()).unwrap();
        for deployment in [
            old_host.path().to_string_lossy().into_owned(),
            new_host.path().to_string_lossy().into_owned(),
        ] {
            assert!(!text.contains(&deployment));
        }
    }

    #[test]
    fn signing_key_lives_outside_the_companion_export_root() {
        let root = Path::new("/srv/nolune");
        let key = signing_key_path(root);
        assert!(!key.starts_with(companion_dir(root)));
        assert!(!key.starts_with(root.join("instances")));
        assert_eq!(key, root.join("federation").join("signing_key.json"));
        assert_eq!(
            identity_path(root),
            root.join("federation").join("identity.json")
        );
    }
}
