//! Peer store and invite book for companion federation (#108).
//!
//! Peers are persisted in `federation/peers.json` (mode `0600`, beside the
//! keystore and outside `instances/companion/`): each record holds the
//! peer's self-signed identity document, its handshake state, the origins the
//! owner approved, and the rotation history. Every document is re-verified on
//! load; a record whose document no longer verifies is dropped rather than
//! trusted. A file this build cannot load (another version, junk, the wrong
//! shape) is a trust store this build must not touch: reads see no peers and
//! every write fails closed, so the file is never replaced by an empty one.
//!
//! Invites live in memory only: an invite is a short-lived, one-time secret
//! that the issuing owner sees exactly once, and the store keeps just a
//! domain-separated SHA-256 of it, the way browser pairing codes are kept.
//! The secret is 32 random bytes, so there is no failure counter: one would
//! only let a stranger at the public route lock the owner out. A restart
//! forgets outstanding invites. Nothing here logs a secret.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    INVITE_HASH_DOMAIN, encode,
    identity::{self, VerifiedIdentity},
};
use crate::domain::federation::{
    FederationError, InviteSecret, InviteSummary, PeerRecord, PeerState,
};

/// Peer records, under the keystore directory.
pub const PEERS_FILE: &str = "peers.json";
/// An invite must be redeemed within this window.
pub const INVITE_TTL_SECS: u64 = 10 * 60;
/// Outstanding invites kept at once; minting more evicts the oldest.
pub const MAX_PENDING_INVITES: usize = 8;

const STORE_VERSION: u32 = 1;
const INVITE_SECRET_BYTES: usize = 32;
const INVITE_ID_BYTES: usize = 8;
/// Upper bound for the store file; anything larger is not ours.
const MAX_STORE_FILE_BYTES: u64 = 4 * 1024 * 1024;

/// Unix seconds. Tests substitute a fixed clock.
pub(crate) type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

pub(crate) fn system_clock() -> Clock {
    Arc::new(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0)
    })
}

/// A freshly minted invite. The secret is returned here once and only its
/// hash stays behind.
#[derive(Clone)]
pub struct MintedInvite {
    pub id: String,
    pub secret: InviteSecret,
    pub created_at: u64,
    pub expires_at: u64,
}

impl std::fmt::Debug for MintedInvite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MintedInvite")
            .field("id", &self.id)
            .field("created_at", &self.created_at)
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

/// An invite that was just redeemed and is now gone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedeemedInvite {
    pub id: String,
    pub created_at: u64,
}

/// A stored peer whose document was verified when it was stored or loaded.
#[derive(Debug, Clone)]
pub struct Peer {
    pub record: PeerRecord,
    pub verified: VerifiedIdentity,
}

pub struct PeerStore {
    root: PathBuf,
    inner: Mutex<Inner>,
    clock: Clock,
}

impl PeerStore {
    /// A store over `workspace_root/federation/peers.json`. Nothing is read
    /// until the first access.
    pub(crate) fn with_clock(workspace_root: &Path, clock: Clock) -> Self {
        Self {
            root: workspace_root.to_path_buf(),
            inner: Mutex::new(Inner::default()),
            clock,
        }
    }

    /// `workspace/federation/peers.json`
    pub fn path(&self) -> PathBuf {
        identity::federation_dir(&self.root).join(PEERS_FILE)
    }

    /// Mints an invite and returns its secret once.
    pub fn create_invite(&self) -> MintedInvite {
        let now = (self.clock)();
        let mut inner = self.inner.lock().unwrap();
        inner.invites.retain(|invite| invite.expires_at > now);
        while inner.invites.len() >= MAX_PENDING_INVITES {
            inner.invites.remove(0);
        }
        let (secret, secret_hash) = loop {
            let candidate = InviteSecret::new(encode(&random_bytes(INVITE_SECRET_BYTES)));
            let hash = hash_invite(&candidate);
            if !inner
                .invites
                .iter()
                .any(|invite| invite.secret_hash == hash)
            {
                break (candidate, hash);
            }
        };
        let id = loop {
            let candidate = hex(&random_bytes(INVITE_ID_BYTES));
            if !inner.invites.iter().any(|invite| invite.id == candidate) {
                break candidate;
            }
        };
        let expires_at = now + INVITE_TTL_SECS;
        inner.invites.push(Invite {
            id: id.clone(),
            secret_hash,
            created_at: now,
            expires_at,
        });
        drop(inner);
        log::info!("[federation] invite {id} created, valid for {INVITE_TTL_SECS}s");
        MintedInvite {
            id,
            secret,
            created_at: now,
            expires_at,
        }
    }

    /// Withdraws an outstanding invite. `false` when there is none by that id.
    pub fn cancel_invite(&self, id: &str) -> bool {
        let removed = {
            let mut inner = self.inner.lock().unwrap();
            let before = inner.invites.len();
            inner.invites.retain(|invite| invite.id != id);
            before != inner.invites.len()
        };
        if removed {
            log::info!("[federation] invite {id} cancelled");
        }
        removed
    }

    /// Outstanding invites, oldest first, without their secrets.
    pub fn invites(&self) -> Vec<InviteSummary> {
        let now = (self.clock)();
        let mut inner = self.inner.lock().unwrap();
        inner.invites.retain(|invite| invite.expires_at > now);
        inner
            .invites
            .iter()
            .map(|invite| InviteSummary {
                id: invite.id.clone(),
                state: PeerState::Invited,
                created_at: invite.created_at,
                expires_at: invite.expires_at,
            })
            .collect()
    }

    /// Consumes the invite whose hash matches `secret`. Wrong, expired,
    /// cancelled, and already redeemed secrets are all `InviteInvalid`, and
    /// a wrong secret changes nothing: the next presentation is judged on
    /// its own.
    pub fn redeem_invite(&self, secret: &InviteSecret) -> Result<RedeemedInvite, FederationError> {
        let now = (self.clock)();
        let mut inner = self.inner.lock().unwrap();
        inner.invites.retain(|invite| invite.expires_at > now);
        let presented = hash_invite(secret);
        let Some(index) = inner
            .invites
            .iter()
            .position(|invite| constant_time_eq(&invite.secret_hash, &presented))
        else {
            log::warn!("[federation] invite redemption failed: no outstanding invite matches");
            return Err(FederationError::InviteInvalid);
        };
        // One-time: the invite is gone before anything is recorded about
        // the companion that redeemed it.
        let invite = inner.invites.remove(index);
        drop(inner);
        log::info!(
            "[federation] invite {} redeemed {}s after it was created",
            invite.id,
            now.saturating_sub(invite.created_at)
        );
        Ok(RedeemedInvite {
            id: invite.id,
            created_at: invite.created_at,
        })
    }

    /// Inserts or replaces the record for `record.companion_id()` and
    /// persists. Fails closed, changing nothing, over a store file this
    /// build could not load.
    pub fn upsert(
        &self,
        record: PeerRecord,
        verified: VerifiedIdentity,
    ) -> Result<PeerRecord, FederationError> {
        if verified.companion_id != record.identity.companion_id
            || verified.public_key.as_bytes() != identity_key_bytes(&record)?.as_slice()
        {
            return Err(FederationError::CompanionIdMismatch);
        }
        let mut inner = self.inner.lock().unwrap();
        self.ensure_loaded(&mut inner);
        self.refuse_if_unloadable(&inner)?;
        let peer = Peer {
            record: record.clone(),
            verified,
        };
        match inner
            .peers
            .iter()
            .position(|existing| existing.record.companion_id() == record.companion_id())
        {
            Some(index) => inner.peers[index] = peer,
            None => inner.peers.push(peer),
        }
        self.persist(&inner)?;
        Ok(record)
    }

    pub fn get(&self, companion_id: &str) -> Option<Peer> {
        let mut inner = self.inner.lock().unwrap();
        self.ensure_loaded(&mut inner);
        inner
            .peers
            .iter()
            .find(|peer| peer.record.companion_id() == companion_id)
            .cloned()
    }

    /// Every record, oldest first.
    pub fn list(&self) -> Vec<PeerRecord> {
        let mut inner = self.inner.lock().unwrap();
        self.ensure_loaded(&mut inner);
        inner.peers.iter().map(|peer| peer.record.clone()).collect()
    }

    /// Applies `change` to the record for `companion_id` under the store
    /// lock and returns the peer; `Ok(None)` when unknown.
    ///
    /// The closure sees the record as it is now, not as a caller last saw
    /// it, so a state check inside it is a compare-and-set: a transition
    /// that no longer fits returns an error, nothing is persisted, and the
    /// error is passed through. A change that leaves the record as it was
    /// is not stamped or written; otherwise `updated_at` is stamped and the
    /// file is rewritten. Fails closed over a store file this build could
    /// not load.
    pub fn update(
        &self,
        companion_id: &str,
        change: impl FnOnce(&mut PeerRecord) -> Result<(), FederationError>,
    ) -> Result<Option<Peer>, FederationError> {
        let now = (self.clock)();
        let mut inner = self.inner.lock().unwrap();
        self.ensure_loaded(&mut inner);
        self.refuse_if_unloadable(&inner)?;
        let Some(index) = inner
            .peers
            .iter()
            .position(|peer| peer.record.companion_id() == companion_id)
        else {
            return Ok(None);
        };
        let mut record = inner.peers[index].record.clone();
        change(&mut record)?;
        if record == inner.peers[index].record {
            return Ok(Some(inner.peers[index].clone()));
        }
        record.updated_at = now;
        inner.peers[index].record = record;
        self.persist(&inner)?;
        Ok(Some(inner.peers[index].clone()))
    }

    /// Reads the store file once. Records whose document does not verify
    /// are dropped. A file that cannot be read or is not a store of this
    /// version leaves the store empty and marked unloadable: it is reported,
    /// never repaired, and never overwritten.
    fn ensure_loaded(&self, inner: &mut Inner) {
        if inner.loaded {
            return;
        }
        inner.loaded = true;
        let path = self.path();
        let text = match std::fs::metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => {
                return self.mark_unloadable(inner, format!("cannot be read ({error})"));
            }
            Ok(metadata) if metadata.len() > MAX_STORE_FILE_BYTES => {
                return self
                    .mark_unloadable(inner, "is larger than a peer store can be".to_owned());
            }
            Ok(_) => match std::fs::read_to_string(&path) {
                Ok(text) => text,
                Err(error) => {
                    return self.mark_unloadable(inner, format!("cannot be read ({error})"));
                }
            },
        };
        let version = serde_json::from_str::<StoreVersion>(&text)
            .ok()
            .map(|file| file.version);
        let file = match serde_json::from_str::<StoreFile>(&text) {
            Ok(file) if file.version == STORE_VERSION => file,
            _ => {
                let reason = match version {
                    Some(version) if version != STORE_VERSION => {
                        format!("has unsupported version {version}")
                    }
                    _ => "does not have the expected shape".to_owned(),
                };
                return self.mark_unloadable(inner, reason);
            }
        };
        for record in file.peers {
            match identity::verify_document(&record.identity) {
                Ok(verified) => inner.peers.push(Peer { record, verified }),
                Err(error) => log::warn!(
                    "[federation] dropping peer {} whose identity document does not verify: {error}",
                    record.identity.companion_id
                ),
            }
        }
    }

    fn mark_unloadable(&self, inner: &mut Inner, reason: String) {
        log::warn!(
            "[federation] peer store {} {reason}; no peers are trusted and nothing will be written until it is repaired or moved aside and the server restarted",
            self.path().display()
        );
        inner.unloadable = Some(reason);
    }

    fn refuse_if_unloadable(&self, inner: &Inner) -> Result<(), FederationError> {
        match &inner.unloadable {
            Some(reason) => Err(FederationError::Io {
                path: self.path(),
                message: format!(
                    "peer store {reason}; repair or move it aside and restart before pairing"
                ),
            }),
            None => Ok(()),
        }
    }

    fn persist(&self, inner: &Inner) -> Result<(), FederationError> {
        let file = StoreFile {
            version: STORE_VERSION,
            peers: inner.peers.iter().map(|peer| peer.record.clone()).collect(),
        };
        let path = self.path();
        write_private(&path, &file).map_err(|error| FederationError::Io {
            path,
            message: error.to_string(),
        })
    }
}

fn identity_key_bytes(record: &PeerRecord) -> Result<Vec<u8>, FederationError> {
    super::decode_exact(
        &record.identity.public_key,
        crate::domain::federation::PUBLIC_KEY_BYTES,
    )
    .ok_or_else(|| FederationError::Malformed("public key is not 32 base64url bytes".into()))
}

/// Writes the store file owner-only through a temporary file in the same
/// directory, so a reader sees the old file or the new one.
fn write_private(path: &Path, file: &StoreFile) -> std::io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other("peer store path has no directory"))?;
    identity::create_private_dir(dir)?;
    let mut json = serde_json::to_string_pretty(file)?;
    json.push('\n');
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tmp.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    use std::io::Write as _;
    tmp.write_all(json.as_bytes())?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn random_bytes(len: usize) -> Vec<u8> {
    let mut buf = vec![0u8; len];
    getrandom::fill(&mut buf).expect("operating system randomness is unavailable");
    buf
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hash_invite(secret: &InviteSecret) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(INVITE_HASH_DOMAIN);
    hasher.update(secret.expose().as_bytes());
    hasher.finalize().into()
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[derive(Default)]
struct Inner {
    loaded: bool,
    /// Why the store file could not be loaded, when it exists but could not.
    unloadable: Option<String>,
    peers: Vec<Peer>,
    invites: Vec<Invite>,
}

struct Invite {
    id: String,
    secret_hash: [u8; 32],
    created_at: u64,
    expires_at: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoreFile {
    version: u32,
    peers: Vec<PeerRecord>,
}

/// Just the version, read leniently so an unsupported store is reported as
/// such rather than as the wrong shape.
#[derive(Deserialize)]
struct StoreVersion {
    version: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::federation::{PairingRole, PeerState},
        services::federation::identity::{self, SigningIdentity},
    };
    use std::sync::atomic::{AtomicU64, Ordering};

    const T0: u64 = 1_800_000_000;

    fn clock() -> (Arc<AtomicU64>, Clock) {
        let now = Arc::new(AtomicU64::new(T0));
        let read = now.clone();
        (now, Arc::new(move || read.load(Ordering::SeqCst)))
    }

    fn store() -> (tempfile::TempDir, Arc<AtomicU64>, PeerStore) {
        let tmp = tempfile::tempdir().unwrap();
        let (now, clock) = clock();
        let store = PeerStore::with_clock(tmp.path(), clock);
        (tmp, now, store)
    }

    fn identity(label: &str) -> SigningIdentity {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join(label);
        identity::load_or_create_at(&root, T0).unwrap()
    }

    fn record(identity: &SigningIdentity, state: PeerState) -> PeerRecord {
        PeerRecord {
            identity: identity.document().clone(),
            state,
            role: PairingRole::Issuer,
            pairing_id: "0011223344556677".into(),
            approved_origins: vec!["https://peer.example".into()],
            pending_origin: None,
            created_at: T0,
            updated_at: T0,
            rotation_history: Vec::new(),
        }
    }

    #[test]
    fn invites_are_one_time_and_only_their_hash_stays_behind() {
        let (_tmp, _now, store) = store();
        let minted = store.create_invite();
        assert_eq!(minted.id.len(), 16, "eight random bytes as hex");
        assert_eq!(
            minted.secret.expose().len(),
            43,
            "32 random bytes as base64url"
        );
        assert_eq!(minted.created_at, T0);
        assert_eq!(minted.expires_at, T0 + INVITE_TTL_SECS);
        assert!(!format!("{minted:?}").contains(minted.secret.expose()));

        let listed = store.invites();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, minted.id);
        assert_eq!(listed[0].state, PeerState::Invited);
        assert_eq!(listed[0].expires_at, minted.expires_at);
        assert!(
            !serde_json::to_string(&listed)
                .unwrap()
                .contains(minted.secret.expose()),
            "the list never shows the secret again"
        );

        let redeemed = store.redeem_invite(&minted.secret).unwrap();
        assert_eq!(
            redeemed,
            RedeemedInvite {
                id: minted.id.clone(),
                created_at: T0,
            }
        );
        assert!(store.invites().is_empty(), "redeeming consumes the invite");
        assert_eq!(
            store.redeem_invite(&minted.secret),
            Err(FederationError::InviteInvalid),
            "a replayed secret fails"
        );
        assert!(!store.path().exists(), "invites are never written to disk");
    }

    #[test]
    fn wrong_expired_and_cancelled_invites_fail_closed() {
        let (_tmp, now, store) = store();
        let wrong = InviteSecret::new("not-the-secret".into());
        assert_eq!(
            store.redeem_invite(&wrong),
            Err(FederationError::InviteInvalid),
            "no invite outstanding"
        );

        let cancelled = store.create_invite();
        assert!(store.cancel_invite(&cancelled.id));
        assert!(!store.cancel_invite(&cancelled.id));
        assert_eq!(
            store.redeem_invite(&cancelled.secret),
            Err(FederationError::InviteInvalid)
        );

        let expired = store.create_invite();
        now.store(T0 + INVITE_TTL_SECS, Ordering::SeqCst);
        assert_eq!(
            store.redeem_invite(&expired.secret),
            Err(FederationError::InviteInvalid)
        );
        assert!(store.invites().is_empty(), "expired invites are dropped");
    }

    /// The secret is 32 random bytes, so guessing is hopeless and a failure
    /// counter would only let a stranger at the public route lock the owner
    /// out of a legitimate redemption. Any number of wrong secrets, from
    /// anyone, leaves the right one redeemable.
    #[test]
    fn strangers_cannot_exhaust_invite_redemption_with_wrong_secrets() {
        let (_tmp, _now, store) = store();
        let live = store.create_invite();
        let wrong = InviteSecret::new("not-the-secret".into());
        for _ in 0..1_000 {
            assert_eq!(
                store.redeem_invite(&wrong),
                Err(FederationError::InviteInvalid)
            );
        }
        assert_eq!(store.invites().len(), 1, "the invite is untouched");
        assert!(
            store.redeem_invite(&live.secret).is_ok(),
            "the right secret is never refused because strangers guessed wrong"
        );
    }

    #[test]
    fn minting_beyond_the_cap_evicts_the_oldest_invite() {
        let (_tmp, _now, store) = store();
        let first = store.create_invite();
        for _ in 0..MAX_PENDING_INVITES {
            store.create_invite();
        }
        assert_eq!(store.invites().len(), MAX_PENDING_INVITES);
        assert_eq!(
            store.redeem_invite(&first.secret),
            Err(FederationError::InviteInvalid)
        );
    }

    #[test]
    fn peers_persist_owner_only_and_reload_verified() {
        let (tmp, now, store) = store();
        let peer = identity("peer");
        assert!(store.get(peer.companion_id()).is_none());
        assert!(store.list().is_empty());

        let inserted = store
            .upsert(record(&peer, PeerState::Pending), peer.verified().clone())
            .unwrap();
        assert_eq!(inserted.state, PeerState::Pending);
        let path = store.path();
        assert_eq!(path, tmp.path().join("federation").join(PEERS_FILE));
        assert!(path.is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                std::fs::metadata(path.parent().unwrap())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
        let file: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(file["version"], 1);
        assert_eq!(
            file["peers"][0]["identity"]["companion_id"],
            peer.companion_id()
        );
        assert_eq!(file["peers"][0]["state"], "pending");
        let text = std::fs::read_to_string(&path).unwrap();
        for absent in ["secret", "hostname", "profile", "\"port\"", "label"] {
            assert!(!text.contains(absent), "peers.json carries {absent:?}");
        }

        now.store(T0 + 5, Ordering::SeqCst);
        let updated = store
            .update(peer.companion_id(), |record| {
                record.state = PeerState::Paired;
                record
                    .approved_origins
                    .push("https://second.example".into());
                Ok(())
            })
            .unwrap()
            .unwrap()
            .record;
        assert_eq!(updated.state, PeerState::Paired);
        assert_eq!(updated.updated_at, T0 + 5);
        assert_eq!(updated.created_at, T0);
        assert!(store.update("nobody", |_| Ok(())).unwrap().is_none());

        let reopened = PeerStore::with_clock(tmp.path(), system_clock());
        let loaded = reopened.get(peer.companion_id()).expect("record reloads");
        assert_eq!(loaded.record, updated);
        assert_eq!(loaded.verified.public_key, peer.verified().public_key);
        assert_eq!(reopened.list(), vec![updated.clone()]);

        // Replacing keeps one record per companion.
        let replaced = reopened
            .upsert(record(&peer, PeerState::Revoked), peer.verified().clone())
            .unwrap();
        assert_eq!(replaced.state, PeerState::Revoked);
        assert_eq!(reopened.list().len(), 1);
    }

    #[test]
    fn records_that_no_longer_verify_are_dropped_on_load() {
        let (tmp, _now, store) = store();
        let good = identity("good");
        let bad = identity("bad");
        store
            .upsert(record(&good, PeerState::Paired), good.verified().clone())
            .unwrap();
        store
            .upsert(record(&bad, PeerState::Paired), bad.verified().clone())
            .unwrap();
        let path = store.path();
        let mut file: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        // Swap the second record's key for the first one's: the id and the
        // self-signature no longer belong to it.
        file["peers"][1]["identity"]["public_key"] =
            file["peers"][0]["identity"]["public_key"].clone();
        std::fs::write(&path, serde_json::to_string(&file).unwrap()).unwrap();

        let reopened = PeerStore::with_clock(tmp.path(), system_clock());
        assert!(reopened.get(good.companion_id()).is_some());
        assert!(
            reopened.get(bad.companion_id()).is_none(),
            "a record whose document does not verify is not a peer"
        );
        assert_eq!(reopened.list().len(), 1);

        for junk in [
            r#"{"version":2,"peers":[]}"#,
            "not json",
            r#"{"version":1,"peers":[{"state":"paired"}]}"#,
        ] {
            std::fs::write(&path, junk).unwrap();
            let reopened = PeerStore::with_clock(tmp.path(), system_clock());
            assert!(reopened.list().is_empty(), "loaded peers from {junk}");
        }
    }

    /// A file this build cannot load (a newer version, junk, the wrong
    /// shape) is a trust store this build must not touch: reads start
    /// empty, every write fails closed, and the file stays byte for byte.
    #[test]
    fn an_unloadable_store_file_is_never_overwritten() {
        let (tmp, _now, store) = store();
        let peer = identity("peer");
        let path = store.path();
        for unloadable in [
            r#"{"version":2,"peers":[{"future":"record"}],"extra":"data"}"#,
            r#"{"version":2,"peers":[]}"#,
            "not json",
            r#"{"version":1,"peers":[{"state":"paired"}]}"#,
        ] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, unloadable).unwrap();
            let reopened = PeerStore::with_clock(tmp.path(), system_clock());
            assert!(reopened.list().is_empty(), "{unloadable}");
            assert!(reopened.get(peer.companion_id()).is_none());

            let error = reopened
                .upsert(record(&peer, PeerState::Pending), peer.verified().clone())
                .unwrap_err();
            assert!(
                matches!(&error, FederationError::Io { path: at, .. } if *at == path),
                "{unloadable}: {error:?}"
            );
            let error = reopened
                .update(peer.companion_id(), |record| {
                    record.state = PeerState::Paired;
                    Ok(())
                })
                .unwrap_err();
            assert!(
                matches!(error, FederationError::Io { .. }),
                "{unloadable}: {error:?}"
            );
            assert!(
                reopened.list().is_empty() && reopened.get(peer.companion_id()).is_none(),
                "{unloadable}: a refused write left something behind"
            );
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                unloadable,
                "the unloadable file was rewritten"
            );
        }

        // Invites do not depend on the store file and still work.
        let reopened = PeerStore::with_clock(tmp.path(), system_clock());
        let minted = reopened.create_invite();
        assert!(reopened.redeem_invite(&minted.secret).is_ok());
    }

    /// The closure runs under the store lock and may refuse the change; a
    /// refusal persists nothing, and an unchanged record is not rewritten.
    #[test]
    fn update_applies_the_change_under_the_lock_or_not_at_all() {
        let (_tmp, now, store) = store();
        let peer = identity("peer");
        store
            .upsert(record(&peer, PeerState::Pending), peer.verified().clone())
            .unwrap();
        // The store writes through a temp file and a rename, so a rewrite
        // is a new inode even when the bytes are the same.
        #[cfg(unix)]
        let written = {
            use std::os::unix::fs::MetadataExt;
            std::fs::metadata(store.path()).unwrap().ino()
        };

        now.store(T0 + 7, Ordering::SeqCst);
        let refused = store
            .update(peer.companion_id(), |record| {
                assert_eq!(record.state, PeerState::Pending);
                record.state = PeerState::Paired;
                Err(FederationError::PeerRevoked)
            })
            .unwrap_err();
        assert_eq!(refused, FederationError::PeerRevoked);
        let kept = store.get(peer.companion_id()).unwrap().record;
        assert_eq!(kept.state, PeerState::Pending, "a refused change is undone");
        assert_eq!(kept.updated_at, T0, "a refused change is not stamped");

        let same = store
            .update(peer.companion_id(), |_| Ok(()))
            .unwrap()
            .unwrap();
        assert_eq!(
            same.record.updated_at, T0,
            "an unchanged record is not stamped"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(
                std::fs::metadata(store.path()).unwrap().ino(),
                written,
                "an unchanged record is not rewritten"
            );
        }
        assert_eq!(
            store
                .update("nobody", |_| Ok(()))
                .unwrap()
                .map(|p| p.record),
            None
        );

        let changed = store
            .update(peer.companion_id(), |record| {
                record.state = PeerState::Paired;
                Ok(())
            })
            .unwrap()
            .unwrap();
        assert_eq!(changed.record.state, PeerState::Paired);
        assert_eq!(changed.record.updated_at, T0 + 7);
        assert_eq!(changed.verified.public_key, peer.verified().public_key);
        let reopened = PeerStore::with_clock(_tmp.path(), system_clock());
        assert_eq!(reopened.list()[0], changed.record);
    }
}
