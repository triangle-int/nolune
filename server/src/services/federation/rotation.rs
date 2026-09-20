//! Key rotation for companion federation (#108, PR 3).
//!
//! A rotation is the new self-signed identity together with two signatures
//! over the same canonical bytes (both ids, both keys, and the time): the
//! previous key's `endorsement`, which proves the owner of the old key chose
//! the new one, and the new key's `signature`, which proves the new key
//! exists. A peer that verifies both re-keys its record to the new identity
//! and keeps the rotation in the record's history; the previous key goes on
//! verifying for [`ROTATION_GRACE_SECS`] after that, so envelopes already in
//! flight still land, and retires after. The rotating server keeps its own
//! rotations in `federation/rotations.json` beside the keystore, so the
//! proof can be shown again to a peer that missed the notice.
//!
//! Nothing here logs.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{
    Canonical, ROTATION_SIGNING_DOMAIN, check_version, encode,
    identity::{self, SigningIdentity, VerifiedIdentity, decode_signature},
};
use crate::domain::federation::{
    FEDERATION_VERSION, FederationError, KeyRotation, PUBLIC_KEY_BYTES,
};

/// How long a retired key keeps verifying after a peer accepted the
/// rotation. Longer than the longest envelope lifetime plus the skew
/// allowance, so nothing sealed before the notice is lost.
pub const ROTATION_GRACE_SECS: u64 = 15 * 60;
/// This server's own rotations, under the keystore directory.
pub const ROTATIONS_FILE: &str = "rotations.json";
/// Transitions kept per peer record; the oldest is dropped past this.
pub const MAX_ROTATION_HISTORY: usize = 32;

const ROTATIONS_FORMAT_VERSION: u32 = 1;
/// Upper bound for the own history file; anything larger is not ours.
const MAX_ROTATIONS_FILE_BYTES: u64 = 4 * 1024 * 1024;

/// `workspace/federation/rotations.json`
pub fn rotations_path(workspace_root: &Path) -> PathBuf {
    identity::federation_dir(workspace_root).join(ROTATIONS_FILE)
}

/// Both keys sign these bytes.
pub(crate) fn rotation_signing_bytes(
    version: u32,
    previous_companion_id: &str,
    previous_public_key: &[u8; PUBLIC_KEY_BYTES],
    companion_id: &str,
    public_key: &[u8; PUBLIC_KEY_BYTES],
    rotated_at: u64,
) -> Vec<u8> {
    Canonical::new(ROTATION_SIGNING_DOMAIN)
        .u32(version)
        .str(previous_companion_id)
        .bytes(previous_public_key)
        .str(companion_id)
        .bytes(public_key)
        .u64(rotated_at)
        .finish()
}

/// Builds the rotation from `previous` to `next`, signed by both.
pub(crate) fn endorse(
    previous: &SigningIdentity,
    next: &SigningIdentity,
    rotated_at: u64,
) -> KeyRotation {
    let message = rotation_signing_bytes(
        FEDERATION_VERSION,
        previous.companion_id(),
        &previous.verified().public_key.to_bytes(),
        next.companion_id(),
        &next.verified().public_key.to_bytes(),
        rotated_at,
    );
    KeyRotation {
        version: FEDERATION_VERSION,
        previous: previous.document().clone(),
        identity: next.document().clone(),
        rotated_at,
        endorsement: encode(&previous.sign_raw(&message)),
        signature: encode(&next.sign_raw(&message)),
    }
}

/// Checks the version, both documents, that the keys differ, and both
/// signatures. Returns the verified previous and next identities.
pub fn verify_rotation(
    rotation: &KeyRotation,
) -> Result<(VerifiedIdentity, VerifiedIdentity), FederationError> {
    check_version(rotation.version)?;
    let previous = identity::verify_document(&rotation.previous)?;
    let next = identity::verify_document(&rotation.identity)?;
    if previous.public_key == next.public_key {
        return Err(FederationError::RotationMismatch);
    }
    let endorsement = decode_signature(&rotation.endorsement)?;
    let signature = decode_signature(&rotation.signature)?;
    let message = rotation_signing_bytes(
        rotation.version,
        &previous.companion_id,
        &previous.public_key.to_bytes(),
        &next.companion_id,
        &next.public_key.to_bytes(),
        rotation.rotated_at,
    );
    previous
        .public_key
        .verify_strict(&message, &endorsement)
        .map_err(|_| FederationError::SignatureMismatch)?;
    next.public_key
        .verify_strict(&message, &signature)
        .map_err(|_| FederationError::SignatureMismatch)?;
    Ok((previous, next))
}

/// This server's own rotations, oldest first; empty when none happened.
pub fn load_rotations(workspace_root: &Path) -> Result<Vec<KeyRotation>, FederationError> {
    let path = rotations_path(workspace_root);
    let len = match std::fs::metadata(&path) {
        Ok(metadata) => metadata.len(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io_error(&path, error)),
    };
    if len > MAX_ROTATIONS_FILE_BYTES {
        return Err(FederationError::Malformed(format!(
            "{} is larger than a rotation history can be",
            path.display()
        )));
    }
    let text = std::fs::read_to_string(&path).map_err(|error| io_error(&path, error))?;
    let file: RotationsFile = serde_json::from_str(&text)
        .map_err(|error| FederationError::Malformed(format!("rotation history: {error}")))?;
    if file.version != ROTATIONS_FORMAT_VERSION {
        return Err(FederationError::Malformed(format!(
            "rotation history has unsupported version {}",
            file.version
        )));
    }
    // Every entry is re-verified: a history that no longer proves its
    // transitions is reported, not trusted.
    for rotation in &file.rotations {
        verify_rotation(rotation)?;
    }
    Ok(file.rotations)
}

/// Appends `rotation` to the own history, owner-readable only.
pub(crate) fn append_rotation(
    workspace_root: &Path,
    rotation: &KeyRotation,
) -> Result<(), FederationError> {
    let mut rotations = load_rotations(workspace_root)?;
    rotations.push(rotation.clone());
    let path = rotations_path(workspace_root);
    let mut json = serde_json::to_string_pretty(&RotationsFile {
        version: ROTATIONS_FORMAT_VERSION,
        rotations,
    })
    .expect("rotations serialize");
    json.push('\n');
    identity::replace_private(&path, json.as_bytes()).map_err(|error| io_error(&path, error))
}

fn io_error(path: &Path, error: std::io::Error) -> FederationError {
    FederationError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RotationsFile {
    version: u32,
    rotations: Vec<KeyRotation>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::federation::{FEDERATION_VERSION, IdentityDocument, SIGNATURE_BYTES},
        services::federation::{decode_exact, encode, identity::from_seed},
    };
    use sha2::{Digest, Sha256};

    const IDENTITY_FIXTURE: &str =
        include_str!("../../../tests/fixtures/federation/identity_v1.json");
    const ROTATED_IDENTITY_FIXTURE: &str =
        include_str!("../../../tests/fixtures/federation/rotated_identity_v1.json");
    const ROTATION_FIXTURE: &str =
        include_str!("../../../tests/fixtures/federation/rotation_v1.json");
    /// Match the labels in `tests/fixtures/federation/generate.py`.
    const FIXTURE_SEED_LABEL: &[u8] = b"nolune/federation/fixture-seed/v1";
    const ROTATED_SEED_LABEL: &[u8] = b"nolune/federation/fixture-seed/v1/rotated";
    const PEER_SEED_LABEL: &[u8] = b"nolune/federation/fixture-seed/v1/peer";
    const FIXTURE_CREATED_AT: u64 = 1_789_862_400;
    const FIXTURE_ROTATED_AT: u64 = FIXTURE_CREATED_AT + 86_400;

    fn seed(label: &[u8]) -> [u8; 32] {
        Sha256::digest(label).into()
    }

    fn previous() -> SigningIdentity {
        from_seed(&seed(FIXTURE_SEED_LABEL), FIXTURE_CREATED_AT)
    }

    fn rotated() -> SigningIdentity {
        from_seed(&seed(ROTATED_SEED_LABEL), FIXTURE_ROTATED_AT)
    }

    fn fixture() -> KeyRotation {
        serde_json::from_str(ROTATION_FIXTURE).unwrap()
    }

    fn flip_last_byte(encoded: &str) -> String {
        let mut bytes = decode_exact(encoded, SIGNATURE_BYTES).unwrap();
        *bytes.last_mut().unwrap() ^= 0x01;
        encode(&bytes)
    }

    #[test]
    fn fixture_rotation_verifies_and_links_the_two_fixture_identities() {
        let rotation = fixture();
        let previous_document: IdentityDocument = serde_json::from_str(IDENTITY_FIXTURE).unwrap();
        let next_document: IdentityDocument =
            serde_json::from_str(ROTATED_IDENTITY_FIXTURE).unwrap();
        assert_eq!(rotation.previous, previous_document);
        assert_eq!(rotation.identity, next_document);
        let (from, to) = verify_rotation(&rotation).unwrap();
        assert_eq!(from.companion_id, previous_document.companion_id);
        assert_eq!(to.companion_id, next_document.companion_id);
        assert_ne!(from.public_key, to.public_key);
    }

    #[test]
    fn fixture_is_reproduced_byte_for_byte_from_the_fixed_seeds() {
        let rotation = endorse(&previous(), &rotated(), FIXTURE_ROTATED_AT);
        assert_eq!(rotation, fixture());
        assert_eq!(
            serde_json::to_string_pretty(&rotation).unwrap() + "\n",
            ROTATION_FIXTURE
        );
        assert_eq!(
            serde_json::to_string_pretty(rotated().document()).unwrap() + "\n",
            ROTATED_IDENTITY_FIXTURE
        );
    }

    #[test]
    fn tampered_rotations_fail_closed() {
        let rotation = fixture();
        let stranger = from_seed(&seed(PEER_SEED_LABEL), FIXTURE_CREATED_AT);

        let mut endorsement = rotation.clone();
        endorsement.endorsement = flip_last_byte(&rotation.endorsement);
        assert_eq!(
            verify_rotation(&endorsement),
            Err(FederationError::SignatureMismatch)
        );
        let mut possession = rotation.clone();
        possession.signature = flip_last_byte(&rotation.signature);
        assert_eq!(
            verify_rotation(&possession),
            Err(FederationError::SignatureMismatch)
        );
        let mut swapped = rotation.clone();
        std::mem::swap(&mut swapped.endorsement, &mut swapped.signature);
        assert_eq!(
            verify_rotation(&swapped),
            Err(FederationError::SignatureMismatch)
        );
        let mut retimed = rotation.clone();
        retimed.rotated_at += 1;
        assert_eq!(
            verify_rotation(&retimed),
            Err(FederationError::SignatureMismatch)
        );

        // Endorsed by a stranger rather than the previous key: the previous
        // document is genuine, the endorsement is not its.
        let by_stranger = endorse(&stranger, &rotated(), FIXTURE_ROTATED_AT);
        let mut forged = rotation.clone();
        forged.endorsement = by_stranger.endorsement;
        assert_eq!(
            verify_rotation(&forged),
            Err(FederationError::SignatureMismatch)
        );
        // A previous document that does not verify on its own.
        let mut tampered_previous = rotation.clone();
        tampered_previous.previous.created_at += 1;
        assert_eq!(
            verify_rotation(&tampered_previous),
            Err(FederationError::SignatureMismatch)
        );
        // A new document that does not verify on its own.
        let mut tampered_next = rotation.clone();
        tampered_next.identity.companion_id = stranger.companion_id().to_owned();
        assert_eq!(
            verify_rotation(&tampered_next),
            Err(FederationError::CompanionIdMismatch)
        );
        // Rotating to the same key is not a rotation.
        let same = endorse(&previous(), &previous(), FIXTURE_ROTATED_AT);
        assert_eq!(
            verify_rotation(&same),
            Err(FederationError::RotationMismatch)
        );
        // Signed at another version.
        for (version, expected) in [
            (0, FederationError::VersionTooOld { found: 0, min: 1 }),
            (
                FEDERATION_VERSION + 1,
                FederationError::VersionUnsupported {
                    found: FEDERATION_VERSION + 1,
                },
            ),
        ] {
            let mut other = rotation.clone();
            other.version = version;
            assert_eq!(verify_rotation(&other), Err(expected));
        }
        // Malformed signatures.
        let mut short = rotation.clone();
        short.endorsement.truncate(80);
        assert!(matches!(
            verify_rotation(&short),
            Err(FederationError::Malformed(_))
        ));
        assert!(
            verify_rotation(&rotation).is_ok(),
            "the original still verifies"
        );
    }

    #[test]
    fn keystore_rotation_replaces_the_key_and_records_the_transition() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let first = identity::load_or_create_at(root, FIXTURE_CREATED_AT).unwrap();
        assert_eq!(load_rotations(root).unwrap(), Vec::new());
        assert!(!rotations_path(root).exists());

        let (second, rotation) = identity::rotate_at(root, &first, FIXTURE_ROTATED_AT).unwrap();
        assert_ne!(second.companion_id(), first.companion_id());
        assert_eq!(&rotation.previous, first.document());
        assert_eq!(&rotation.identity, second.document());
        assert_eq!(rotation.rotated_at, FIXTURE_ROTATED_AT);
        assert_eq!(second.document().created_at, FIXTURE_ROTATED_AT);
        assert!(verify_rotation(&rotation).is_ok());

        // The keystore now loads the new identity, and only that.
        let reloaded = identity::load(root).unwrap().unwrap();
        assert_eq!(reloaded.companion_id(), second.companion_id());
        assert_eq!(reloaded.document(), second.document());
        let again = identity::load_or_create_at(root, FIXTURE_ROTATED_AT + 1).unwrap();
        assert_eq!(again.companion_id(), second.companion_id());

        // The own history has the transition, owner-readable only.
        assert_eq!(load_rotations(root).unwrap(), vec![rotation.clone()]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for file in ["rotations.json", "signing_key.json", "identity.json"] {
                let mode = std::fs::metadata(root.join("federation").join(file))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777;
                assert_eq!(mode, 0o600, "{file}");
            }
        }
        let text = std::fs::read_to_string(rotations_path(root)).unwrap();
        assert!(text.contains(first.companion_id()));
        assert!(text.contains(second.companion_id()));
        assert!(
            !text.contains("secret_key"),
            "the rotation history holds public documents only"
        );

        // A stale handle cannot rotate again: the on-disk identity moved on.
        assert_eq!(
            identity::rotate_at(root, &first, FIXTURE_ROTATED_AT + 2).unwrap_err(),
            FederationError::KeyMismatch
        );
        assert_eq!(load_rotations(root).unwrap().len(), 1);

        // A second rotation chains from the second identity.
        let (third, next) = identity::rotate_at(root, &second, FIXTURE_ROTATED_AT + 3).unwrap();
        assert_eq!(&next.previous, second.document());
        assert_eq!(&next.identity, third.document());
        assert_eq!(load_rotations(root).unwrap(), vec![rotation, next]);
        assert_eq!(
            identity::load(root).unwrap().unwrap().companion_id(),
            third.companion_id()
        );
    }

    #[test]
    fn own_rotation_history_fails_closed_on_junk() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("federation")).unwrap();
        std::fs::write(rotations_path(root), "{\"version\":2,\"rotations\":[]}").unwrap();
        assert!(matches!(
            load_rotations(root),
            Err(FederationError::Malformed(_))
        ));
        std::fs::write(rotations_path(root), "junk").unwrap();
        assert!(matches!(
            load_rotations(root),
            Err(FederationError::Malformed(_))
        ));
        // An entry that no longer verifies is reported, not silently dropped.
        let mut broken = fixture();
        broken.endorsement = flip_last_byte(&broken.endorsement);
        std::fs::write(
            rotations_path(root),
            serde_json::to_string(&RotationsFile {
                version: ROTATIONS_FORMAT_VERSION,
                rotations: vec![broken],
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            load_rotations(root),
            Err(FederationError::SignatureMismatch)
        );
    }
}
