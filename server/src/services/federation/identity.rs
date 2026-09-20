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
//! Rotation ([`rotate_at`]) is the one time the files change: the new key
//! and document replace the old ones through temporary files and renames,
//! after the rotation proof was appended to `federation/rotations.json`.
//! The two renames are not one step: a process that dies between them
//! leaves the new key beside the old document, and [`load`] completes that
//! rotation from the recorded proof (which names both documents) instead of
//! refusing a keystore nothing else could repair. Every other mismatch
//! between the key and the document fails closed.
//!
//! Every function here is pure over a workspace root. Nothing here logs.

use std::{
    fmt, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use ed25519_dalek::{Signature, Signer as _, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize as _, Zeroizing};

use super::{
    Canonical, ENVELOPE_SIGNING_DOMAIN, IDENTITY_SIGNING_DOMAIN, check_version, companion_id_for,
    decode, decode_exact, encode,
};
use crate::domain::federation::{
    COMPANION_ID_BYTES, FEDERATION_VERSION, FederationError, IdentityDocument, PUBLIC_KEY_BYTES,
    SIGNATURE_BYTES, SignedEnvelope,
};

/// Directory under the workspace root. Deliberately not under `instances/`.
pub const FEDERATION_DIR: &str = "federation";
/// Public, self-signed identity document.
pub const IDENTITY_FILE: &str = "identity.json";
/// Private Ed25519 seed. Owner-only permissions are enforced on load.
pub const SIGNING_KEY_FILE: &str = "signing_key.json";

const SIGNING_KEY_FORMAT_VERSION: u32 = 1;
const SIGNING_KEY_ALGORITHM: &str = "ed25519";
/// Upper bound for either keystore file; anything larger is not ours.
const MAX_KEYSTORE_FILE_BYTES: u64 = 16 * 1024;
const SEED_BYTES: usize = 32;

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
        sign_envelope_with(&self.key, FEDERATION_VERSION, self.companion_id(), body)
    }

    /// Raw signature over already canonical bytes, for the transport
    /// envelope and the rotation proof. Callers pass domain-tagged bytes.
    pub(super) fn sign_raw(&self, message: &[u8]) -> [u8; SIGNATURE_BYTES] {
        self.key.sign(message).to_bytes()
    }
}

impl fmt::Debug for SigningIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SigningIdentity")
            .field("companion_id", &self.verified.companion_id)
            .field("public_key", &self.document.public_key)
            .field("created_at", &self.document.created_at)
            .finish_non_exhaustive()
    }
}

impl fmt::Display for SigningIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "companion {} (ed25519 {})",
            self.verified.companion_id, self.document.public_key
        )
    }
}

/// Shape of `signing_key.json`. Never derives `Debug`, so it cannot be
/// formatted by accident, and wipes the seed when dropped.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredSigningKey {
    version: u32,
    algorithm: String,
    /// base64url of the 32-byte seed.
    secret_key: String,
}

impl Drop for StoredSigningKey {
    fn drop(&mut self) {
        self.secret_key.zeroize();
    }
}

/// Loads the identity, generating and persisting a new one when neither file
/// exists yet. A half-present or inconsistent keystore fails closed.
pub fn load_or_create(workspace_root: &Path) -> Result<SigningIdentity, FederationError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    load_or_create_at(workspace_root, now)
}

/// [`load_or_create`] with an explicit creation time (unix seconds).
pub(crate) fn load_or_create_at(
    workspace_root: &Path,
    now: u64,
) -> Result<SigningIdentity, FederationError> {
    if let Some(existing) = load(workspace_root)? {
        return Ok(existing);
    }

    let mut seed = Zeroizing::new([0u8; SEED_BYTES]);
    getrandom::fill(seed.as_mut()).map_err(|_| FederationError::RandomnessUnavailable)?;
    let identity = from_seed(&seed, now);

    let dir = federation_dir(workspace_root);
    create_private_dir(&dir).map_err(|error| io_error(&dir, error))?;

    let key_path = signing_key_path(workspace_root);
    let stored = StoredSigningKey {
        version: SIGNING_KEY_FORMAT_VERSION,
        algorithm: SIGNING_KEY_ALGORITHM.to_owned(),
        secret_key: encode(seed.as_ref()),
    };
    let mut key_json = serde_json::to_string_pretty(&stored).expect("signing key serializes");
    key_json.push('\n');
    let key_json = Zeroizing::new(key_json);
    match write_new_private(&key_path, key_json.as_bytes()) {
        Ok(()) => {}
        // Another process created the identity between our load and our write:
        // theirs wins and ours is dropped.
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            return load(workspace_root)?.ok_or_else(|| {
                FederationError::IdentityDocumentMissing(identity_path(workspace_root))
            });
        }
        Err(error) => return Err(io_error(&key_path, error)),
    }

    let doc_path = identity_path(workspace_root);
    let mut doc_json =
        serde_json::to_string_pretty(identity.document()).expect("identity document serializes");
    doc_json.push('\n');
    write_new_private(&doc_path, doc_json.as_bytes())
        .map_err(|error| io_error(&doc_path, error))?;

    Ok(identity)
}

/// Rotates the identity on disk from `previous` to a fresh key: the rotation
/// proof (signed by both keys) is appended to `federation/rotations.json`
/// first, then the key file and the document are replaced. Fails closed
/// with `KeyMismatch` when the identity on disk is not `previous` any more,
/// so two rotations cannot race past each other.
pub(crate) fn rotate_at(
    workspace_root: &Path,
    previous: &SigningIdentity,
    now: u64,
) -> Result<(SigningIdentity, crate::domain::federation::KeyRotation), FederationError> {
    let on_disk = load(workspace_root)?
        .ok_or_else(|| FederationError::SigningKeyMissing(signing_key_path(workspace_root)))?;
    if on_disk.verified.public_key != previous.verified.public_key {
        return Err(FederationError::KeyMismatch);
    }

    let mut seed = Zeroizing::new([0u8; SEED_BYTES]);
    getrandom::fill(seed.as_mut()).map_err(|_| FederationError::RandomnessUnavailable)?;
    let next = from_seed(&seed, now);
    let rotation = super::rotation::endorse(previous, &next, now);

    // The proof first: if the process dies before the key files change, the
    // old key stays active and the entry only records an attempt; the other
    // order could leave a live key that no peer can be told about.
    super::rotation::append_rotation(workspace_root, &rotation)?;

    let key_path = signing_key_path(workspace_root);
    let stored = StoredSigningKey {
        version: SIGNING_KEY_FORMAT_VERSION,
        algorithm: SIGNING_KEY_ALGORITHM.to_owned(),
        secret_key: encode(seed.as_ref()),
    };
    let mut key_json = serde_json::to_string_pretty(&stored).expect("signing key serializes");
    key_json.push('\n');
    let key_json = Zeroizing::new(key_json);
    replace_private(&key_path, key_json.as_bytes()).map_err(|error| io_error(&key_path, error))?;

    // The key is live from here on. Dying before the next rename leaves the
    // old document beside it, which `load` completes from the proof.
    write_document(workspace_root, next.document())?;

    Ok((next, rotation))
}

/// Replaces `identity.json` with `document`, owner-readable only.
fn write_document(
    workspace_root: &Path,
    document: &IdentityDocument,
) -> Result<(), FederationError> {
    let doc_path = identity_path(workspace_root);
    let mut doc_json =
        serde_json::to_string_pretty(document).expect("identity document serializes");
    doc_json.push('\n');
    replace_private(&doc_path, doc_json.as_bytes()).map_err(|error| io_error(&doc_path, error))
}

/// `Ok(None)` when no identity was created yet; `Err` when the files exist but
/// cannot be trusted: missing halves, wrong permissions, a document that does
/// not verify, or a document not signed by the stored key. The one mismatch
/// that is repaired rather than refused is a rotation interrupted between
/// its two renames (see [`complete_interrupted_rotation`]).
pub fn load(workspace_root: &Path) -> Result<Option<SigningIdentity>, FederationError> {
    let key_path = signing_key_path(workspace_root);
    let doc_path = identity_path(workspace_root);
    match (key_path.exists(), doc_path.exists()) {
        (false, false) => return Ok(None),
        (false, true) => return Err(FederationError::SigningKeyMissing(key_path)),
        (true, false) => return Err(FederationError::IdentityDocumentMissing(doc_path)),
        (true, true) => {}
    }

    let key = read_signing_key(&key_path)?;
    let doc_json = read_keystore_file(&doc_path)?;
    let doc_json = std::str::from_utf8(&doc_json)
        .map_err(|_| FederationError::Malformed("identity document is not UTF-8".into()))?;
    let document = parse_document(doc_json)?;
    let verified = verify_document(&document)?;
    if verified.public_key == key.verifying_key() {
        return Ok(Some(SigningIdentity {
            key,
            document,
            verified,
        }));
    }
    let (document, verified) = complete_interrupted_rotation(workspace_root, &key, &document)?
        .ok_or(FederationError::KeyMismatch)?;
    Ok(Some(SigningIdentity {
        key,
        document,
        verified,
    }))
}

/// [`rotate_at`] appends the proof, replaces the key file, then replaces the
/// document. A process that dies between the last two leaves the new key
/// beside the old document: the key on disk is the latest recorded
/// rotation's new key and the document is that rotation's previous one. In
/// exactly that state the document is rewritten from the proof (both
/// documents in it were re-verified when the history was read) and the
/// rotated identity is returned. Anything else, including a history that
/// cannot be read, is `None` and left untouched: it is a mismatch, not an
/// interrupted rotation.
fn complete_interrupted_rotation(
    workspace_root: &Path,
    key: &SigningKey,
    document: &IdentityDocument,
) -> Result<Option<(IdentityDocument, VerifiedIdentity)>, FederationError> {
    let Ok(rotations) = super::rotation::load_rotations(workspace_root) else {
        return Ok(None);
    };
    let Some(latest) = rotations.last() else {
        return Ok(None);
    };
    if latest.previous != *document {
        return Ok(None);
    }
    let verified = verify_document(&latest.identity)?;
    if verified.public_key != key.verifying_key() {
        return Ok(None);
    }
    write_document(workspace_root, &latest.identity)?;
    Ok(Some((latest.identity.clone(), verified)))
}

/// Parses an identity document, refusing unsupported versions before the
/// strict shape check so a downgrade is reported as such.
pub fn parse_document(json: &str) -> Result<IdentityDocument, FederationError> {
    check_declared_version(json, "identity document")?;
    serde_json::from_str(json)
        .map_err(|error| FederationError::Malformed(format!("identity document: {error}")))
}

/// Checks version, encodings, id derivation, and the self-signature.
pub fn verify_document(document: &IdentityDocument) -> Result<VerifiedIdentity, FederationError> {
    check_version(document.version)?;
    let public_key = decode_public_key(&document.public_key)?;
    let signature = decode_signature(&document.signature)?;
    if decode_exact(&document.companion_id, COMPANION_ID_BYTES).is_none() {
        return Err(FederationError::Malformed(
            "companion id is not a base64url digest".into(),
        ));
    }
    if document.companion_id != companion_id_for(&public_key.to_bytes()) {
        return Err(FederationError::CompanionIdMismatch);
    }
    let message = document_signing_bytes(
        document.version,
        &document.companion_id,
        &public_key.to_bytes(),
        document.created_at,
    );
    public_key
        .verify_strict(&message, &signature)
        .map_err(|_| FederationError::SignatureMismatch)?;
    Ok(VerifiedIdentity {
        version: document.version,
        companion_id: document.companion_id.clone(),
        public_key,
        created_at: document.created_at,
    })
}

/// Parses a signed envelope with the same version-first policy as documents.
pub fn parse_envelope(json: &str) -> Result<SignedEnvelope, FederationError> {
    check_declared_version(json, "envelope")?;
    serde_json::from_str(json)
        .map_err(|error| FederationError::Malformed(format!("envelope: {error}")))
}

/// Verifies `envelope` against a sender whose identity was already verified
/// and returns the body bytes. The sender id in the envelope must match.
pub fn verify_envelope(
    envelope: &SignedEnvelope,
    sender: &VerifiedIdentity,
) -> Result<Vec<u8>, FederationError> {
    check_version(envelope.version)?;
    if envelope.sender != sender.companion_id {
        return Err(FederationError::SenderMismatch);
    }
    let signature = decode_signature(&envelope.signature)?;
    let body = decode_body(&envelope.body)?;
    let message = envelope_signing_bytes(envelope.version, &envelope.sender, &body);
    sender
        .public_key
        .verify_strict(&message, &signature)
        .map_err(|_| FederationError::SignatureMismatch)?;
    Ok(body)
}

/// Builds an identity from a raw seed with a freshly signed document.
pub(super) fn from_seed(seed: &[u8; SEED_BYTES], created_at: u64) -> SigningIdentity {
    let key = SigningKey::from_bytes(seed);
    let companion_id = companion_id_for(&key.verifying_key().to_bytes());
    let document = sign_document(&key, FEDERATION_VERSION, &companion_id, created_at);
    let verified = VerifiedIdentity {
        version: document.version,
        companion_id,
        public_key: key.verifying_key(),
        created_at,
    };
    SigningIdentity {
        key,
        document,
        verified,
    }
}

/// `companion_id` is passed explicitly so tests can produce a document that
/// lies about it; production callers derive it from `key`.
fn sign_document(
    key: &SigningKey,
    version: u32,
    companion_id: &str,
    created_at: u64,
) -> IdentityDocument {
    let public_key = key.verifying_key().to_bytes();
    let message = document_signing_bytes(version, companion_id, &public_key, created_at);
    IdentityDocument {
        version,
        companion_id: companion_id.to_owned(),
        public_key: encode(&public_key),
        created_at,
        signature: encode(&key.sign(&message).to_bytes()),
    }
}

fn sign_envelope_with(key: &SigningKey, version: u32, sender: &str, body: &[u8]) -> SignedEnvelope {
    let message = envelope_signing_bytes(version, sender, body);
    SignedEnvelope {
        version,
        sender: sender.to_owned(),
        body: encode(body),
        signature: encode(&key.sign(&message).to_bytes()),
    }
}

fn document_signing_bytes(
    version: u32,
    companion_id: &str,
    public_key: &[u8; PUBLIC_KEY_BYTES],
    created_at: u64,
) -> Vec<u8> {
    Canonical::new(IDENTITY_SIGNING_DOMAIN)
        .u32(version)
        .str(companion_id)
        .bytes(public_key)
        .u64(created_at)
        .finish()
}

/// Commits to the body's digest so large bodies are hashed once and the
/// transport envelope can later carry the digest without the body.
fn envelope_signing_bytes(version: u32, sender: &str, body: &[u8]) -> Vec<u8> {
    Canonical::new(ENVELOPE_SIGNING_DOMAIN)
        .u32(version)
        .str(sender)
        .bytes(&Sha256::digest(body))
        .finish()
}

/// Reads only the `version` field so an unsupported version is reported
/// before the strict shape check can mask it as a generic parse error.
fn check_declared_version(json: &str, what: &str) -> Result<(), FederationError> {
    #[derive(Deserialize)]
    struct Versioned {
        version: u32,
    }
    let versioned: Versioned = serde_json::from_str(json)
        .map_err(|error| FederationError::Malformed(format!("{what}: {error}")))?;
    check_version(versioned.version)
}

pub(super) fn decode_public_key(encoded: &str) -> Result<VerifyingKey, FederationError> {
    let bytes = decode_exact(encoded, PUBLIC_KEY_BYTES)
        .ok_or_else(|| FederationError::Malformed("public key is not 32 base64url bytes".into()))?;
    let array: [u8; PUBLIC_KEY_BYTES] = bytes.try_into().expect("length checked");
    let key = VerifyingKey::from_bytes(&array).map_err(|_| FederationError::InvalidPublicKey)?;
    // A small-order point is a valid encoding that no seed produces, and
    // under lax verification it admits a signature valid for every message.
    // `verify_strict` refuses such keys too; refusing them here keeps them
    // out of every later check and reports them as a bad key, not a bad
    // signature.
    if key.is_weak() {
        return Err(FederationError::InvalidPublicKey);
    }
    Ok(key)
}

pub(super) fn decode_signature(encoded: &str) -> Result<Signature, FederationError> {
    let bytes = decode_exact(encoded, SIGNATURE_BYTES)
        .ok_or_else(|| FederationError::Malformed("signature is not 64 base64url bytes".into()))?;
    let array: [u8; SIGNATURE_BYTES] = bytes.try_into().expect("length checked");
    Ok(Signature::from_bytes(&array))
}

fn decode_body(encoded: &str) -> Result<Vec<u8>, FederationError> {
    decode(encoded)
        .ok_or_else(|| FederationError::Malformed("body is not canonical base64url".into()))
}

fn read_signing_key(path: &Path) -> Result<SigningKey, FederationError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path)
            .map_err(|error| io_error(path, error))?
            .permissions()
            .mode()
            & 0o777;
        if mode & 0o077 != 0 {
            return Err(FederationError::InsecureKeyPermissions {
                path: path.to_path_buf(),
                mode,
            });
        }
    }
    let raw = read_keystore_file(path)?;
    // serde's message quotes the primitive it rejected, which for this file
    // could be the seed; the shape is all a caller needs to know.
    let stored: StoredSigningKey = serde_json::from_slice(&raw).map_err(|_| {
        FederationError::Malformed("signing key file does not have the expected shape".into())
    })?;
    if stored.version != SIGNING_KEY_FORMAT_VERSION || stored.algorithm != SIGNING_KEY_ALGORITHM {
        return Err(FederationError::Malformed(
            "signing key file has an unsupported version or algorithm".into(),
        ));
    }
    let decoded = decode_exact(&stored.secret_key, SEED_BYTES)
        .map(Zeroizing::new)
        .ok_or_else(|| FederationError::Malformed("signing key seed has the wrong shape".into()))?;
    let mut seed = Zeroizing::new([0u8; SEED_BYTES]);
    seed.copy_from_slice(&decoded);
    Ok(SigningKey::from_bytes(&seed))
}

/// Reads a keystore file into a buffer that is wiped on drop.
fn read_keystore_file(path: &Path) -> Result<Zeroizing<Vec<u8>>, FederationError> {
    let len = std::fs::metadata(path)
        .map_err(|error| io_error(path, error))?
        .len();
    if len > MAX_KEYSTORE_FILE_BYTES {
        return Err(FederationError::Malformed(format!(
            "{} is larger than a keystore file can be",
            path.display()
        )));
    }
    std::fs::read(path)
        .map(Zeroizing::new)
        .map_err(|error| io_error(path, error))
}

pub(super) fn create_private_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Creates `path` owner-readable only and never replaces an existing file.
fn write_new_private(path: &Path, contents: &[u8]) -> io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut handle = options.open(path)?;
    use std::io::Write as _;
    handle.write_all(contents)?;
    handle.sync_all()
}

/// Replaces `path` owner-readable only, through a temporary file in the same
/// directory and a rename, so a reader sees the old file or the new one.
pub(super) fn replace_private(path: &Path, contents: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::other("keystore path has no directory"))?;
    create_private_dir(dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tmp.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    use std::io::Write as _;
    tmp.write_all(contents)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn io_error(path: &Path, error: io::Error) -> FederationError {
    FederationError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
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
    use curve25519_dalek::{edwards::EdwardsPoint, scalar::Scalar, traits::Identity as _};
    use ed25519_dalek::Verifier as _;
    use sha2::{Digest, Sha256, Sha512};

    const IDENTITY_FIXTURE: &str =
        include_str!("../../../tests/fixtures/federation/identity_v1.json");
    const ENVELOPE_FIXTURE: &str =
        include_str!("../../../tests/fixtures/federation/envelope_v1.json");
    /// Matches `FIXTURE_SEED_LABEL` in `tests/fixtures/federation/generate.py`.
    const FIXTURE_SEED_LABEL: &[u8] = b"nolune/federation/fixture-seed/v1";
    const FIXTURE_CREATED_AT: u64 = 1_789_862_400;
    const FIXTURE_BODY: &[u8] = br#"{"kind":"ping"}"#;

    /// The eight small-order points of the Ed25519 group in compressed form,
    /// the identity first. `VerifyingKey::from_bytes` accepts every one of
    /// them; only `is_weak` tells them apart from a real key.
    const SMALL_ORDER_POINTS: [&str; 8] = [
        "0100000000000000000000000000000000000000000000000000000000000000",
        "ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "0000000000000000000000000000000000000000000000000000000000000080",
        "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac037a",
        "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac03fa",
        "26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc05",
        "26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc85",
    ];

    /// The group order `L`, little-endian (RFC 8032 section 5.1).
    const GROUP_ORDER: [u8; 32] = [
        0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde,
        0x14, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x10,
    ];

    fn fixture_seed() -> [u8; 32] {
        Sha256::digest(FIXTURE_SEED_LABEL).into()
    }

    fn point_from_hex(hex: &str) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        for (byte, pair) in bytes.iter_mut().zip(hex.as_bytes().chunks(2)) {
            *byte = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
        }
        bytes
    }

    /// The lax verification equation is `R == [S]B - [k]A`. With `R` fixed to
    /// the identity point it becomes `[S]B == [k]A`, which the key's owner can
    /// satisfy for any message by setting `S = k * a`. `verify_strict` refuses
    /// a small-order `R` regardless, so this is the one signature that tells a
    /// strict verifier from a lax one when the key itself is sound.
    fn small_order_r_signature(key: &SigningKey, message: &[u8]) -> [u8; 64] {
        let r = EdwardsPoint::identity().compress();
        let challenge = Sha512::new()
            .chain_update(r.as_bytes())
            .chain_update(key.verifying_key().as_bytes())
            .chain_update(message)
            .finalize();
        let k = Scalar::from_bytes_mod_order_wide(&challenge.into());
        let mut signature = [0u8; 64];
        signature[..32].copy_from_slice(r.as_bytes());
        signature[32..].copy_from_slice(&(k * key.to_scalar()).to_bytes());
        signature
    }

    /// `S + L` encodes the same scalar to a lax decoder and must be refused.
    fn add_group_order_to_scalar(signature: &str) -> String {
        let mut bytes = decode_exact(signature, 64).unwrap();
        let mut carry = 0u16;
        for (byte, order) in bytes[32..].iter_mut().zip(GROUP_ORDER) {
            let sum = u16::from(*byte) + u16::from(order) + carry;
            *byte = (sum & 0xff) as u8;
            carry = sum >> 8;
        }
        assert_eq!(carry, 0, "S < L, so S + L still fits in 32 bytes");
        encode(&bytes)
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

        // Roughly half of all 32-byte strings decode to no curve point.
        let not_a_point = (0u8..=255)
            .map(|byte| [byte; 32])
            .find(|bytes| VerifyingKey::from_bytes(bytes).is_err())
            .unwrap();
        let mut bad_key = document.clone();
        bad_key.public_key = encode(&not_a_point);
        bad_key.companion_id = companion_id_for(&not_a_point);
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
            format!("+{}", &document.signature[1..]),
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
    fn small_order_public_keys_are_refused_before_any_signature_check() {
        // With A = identity the lax equation R == [S]B - [k]A no longer
        // depends on the message, so R = [S]B verifies everything; S = 0 and
        // R = identity is the smallest such pair. The decoder must refuse the
        // key before a signature is even looked at.
        let mut forged_bytes = [0u8; 64];
        forged_bytes[0] = 1;
        let forged = encode(&forged_bytes);

        let document = parse_document(IDENTITY_FIXTURE).unwrap();
        for hex in SMALL_ORDER_POINTS {
            let point = point_from_hex(hex);
            let key = VerifyingKey::from_bytes(&point).expect("small-order points decode");
            assert!(key.is_weak(), "{hex}");

            let mut weak = document.clone();
            weak.public_key = encode(&point);
            weak.companion_id = companion_id_for(&point);
            weak.signature = forged.clone();
            assert_eq!(
                verify_document(&weak),
                Err(FederationError::InvalidPublicKey),
                "{hex}"
            );
        }

        // Prove the forgery is real, then check that the envelope verifier
        // refuses it even when handed a weak identity that skipped decoding.
        let identity_point = point_from_hex(SMALL_ORDER_POINTS[0]);
        let weak_sender = VerifiedIdentity {
            version: FEDERATION_VERSION,
            companion_id: companion_id_for(&identity_point),
            public_key: VerifyingKey::from_bytes(&identity_point).unwrap(),
            created_at: 0,
        };
        let forged_signature = Signature::from_bytes(&forged_bytes);
        for body in [b"ping".as_slice(), b"transfer everything".as_slice()] {
            let message =
                envelope_signing_bytes(FEDERATION_VERSION, &weak_sender.companion_id, body);
            assert!(
                weak_sender
                    .public_key
                    .verify(&message, &forged_signature)
                    .is_ok(),
                "lax verification accepts the universal forgery"
            );
            let envelope = SignedEnvelope {
                version: FEDERATION_VERSION,
                sender: weak_sender.companion_id.clone(),
                body: encode(body),
                signature: forged.clone(),
            };
            assert_eq!(
                verify_envelope(&envelope, &weak_sender),
                Err(FederationError::SignatureMismatch)
            );
        }
    }

    #[test]
    fn small_order_r_signatures_are_refused_even_from_a_genuine_key() {
        let identity = fixture_identity();
        let public_key = identity.verified().public_key;

        let document = identity.document().clone();
        let message = document_signing_bytes(
            document.version,
            &document.companion_id,
            &public_key.to_bytes(),
            document.created_at,
        );
        let forged = small_order_r_signature(&identity.key, &message);
        assert!(
            public_key
                .verify(&message, &Signature::from_bytes(&forged))
                .is_ok(),
            "lax verification accepts a small-order R"
        );
        let mut lax_only = document.clone();
        lax_only.signature = encode(&forged);
        assert_eq!(
            verify_document(&lax_only),
            Err(FederationError::SignatureMismatch)
        );

        let mut envelope = identity.sign_envelope(FIXTURE_BODY);
        let message = envelope_signing_bytes(envelope.version, &envelope.sender, FIXTURE_BODY);
        let forged = small_order_r_signature(&identity.key, &message);
        assert!(
            public_key
                .verify(&message, &Signature::from_bytes(&forged))
                .is_ok()
        );
        envelope.signature = encode(&forged);
        assert_eq!(
            verify_envelope(&envelope, identity.verified()),
            Err(FederationError::SignatureMismatch)
        );
    }

    #[test]
    fn malleated_signature_scalars_are_refused() {
        let document = parse_document(IDENTITY_FIXTURE).unwrap();
        let identity = verify_document(&document).unwrap();

        let mut malleated = document.clone();
        malleated.signature = add_group_order_to_scalar(&document.signature);
        assert_ne!(malleated.signature, document.signature);
        assert_eq!(
            verify_document(&malleated),
            Err(FederationError::SignatureMismatch)
        );

        let mut envelope = parse_envelope(ENVELOPE_FIXTURE).unwrap();
        envelope.signature = add_group_order_to_scalar(&envelope.signature);
        assert_eq!(
            verify_envelope(&envelope, &identity),
            Err(FederationError::SignatureMismatch)
        );
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
        let key_text = std::fs::read_to_string(&key_path).unwrap();
        let key_file: serde_json::Value = serde_json::from_str(&key_text).unwrap();
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
            key_text,
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

        // A corrupt key file is reported by shape only. serde's type-mismatch
        // message quotes the primitive it rejected, which for this file could
        // be the seed, so none of the file's values may reach the error.
        for corrupt in [
            r#"{"version":"SEEDSEEDSEEDSEEDSEEDSEEDSEEDSEEDSEEDSEEDSEED","algorithm":"ed25519","secret_key":"x"}"#,
            r#"{"version":1,"algorithm":"ed25519","secret_key":424242424242}"#,
            r#"{"version":1,"algorithm":"LEAKALGO","secret_key":"NOTASEED","note":"LEAKNOTE"}"#,
            r#"["SEEDSEEDSEEDSEEDSEEDSEEDSEEDSEEDSEEDSEEDSEED"]"#,
        ] {
            std::fs::write(&key_path, corrupt).unwrap();
            let error = load(root).unwrap_err();
            assert!(matches!(error, FederationError::Malformed(_)), "{error}");
            let rendered = format!("{error} / {error:?}");
            for value in ["SEEDSEED", "424242", "LEAKALGO", "NOTASEED", "LEAKNOTE"] {
                assert!(!rendered.contains(value), "{rendered}");
            }
        }
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
