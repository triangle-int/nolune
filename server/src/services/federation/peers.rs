//! Peer store and invite book for companion federation (#108).
//!
//! Peers are persisted in `federation/peers.json` (mode `0600`, beside the
//! keystore and outside `instances/companion/`): each record holds the
//! peer's self-signed identity document, its handshake state, the origins the
//! owner approved, and the rotation history. Every document is re-verified on
//! load; a record whose document no longer verifies is dropped rather than
//! trusted.
//!
//! Invites live in memory only: an invite is a short-lived, one-time secret
//! that the issuing owner sees exactly once, and the store keeps just a
//! domain-separated SHA-256 of it, the way browser pairing codes are kept.
//! A restart forgets outstanding invites. Nothing here logs a secret.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use super::identity::VerifiedIdentity;
use crate::domain::federation::{FederationError, InviteSecret, InviteSummary, PeerRecord};

/// Peer records, under the keystore directory.
pub const PEERS_FILE: &str = "peers.json";
/// An invite must be redeemed within this window.
pub const INVITE_TTL_SECS: u64 = 10 * 60;
/// Outstanding invites kept at once; minting more evicts the oldest.
pub const MAX_PENDING_INVITES: usize = 8;
/// Failed redemptions are counted over this sliding window.
pub const FAILURE_WINDOW_SECS: u64 = 10 * 60;
/// Once this many redemptions fail inside the window, redeeming is refused
/// until the window drains.
pub const MAX_FAILURES_PER_WINDOW: u32 = 20;

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
    pub fn new(workspace_root: &Path) -> Self {
        Self::with_clock(workspace_root, system_clock())
    }

    pub(crate) fn with_clock(workspace_root: &Path, clock: Clock) -> Self {
        let _ = (workspace_root, clock);
        todo!("PR 2: peer store")
    }

    /// `workspace/federation/peers.json`
    pub fn path(&self) -> PathBuf {
        todo!("PR 2: peer store")
    }

    /// Mints an invite and returns its secret once.
    pub fn create_invite(&self) -> MintedInvite {
        todo!("PR 2: peer store")
    }

    /// Withdraws an outstanding invite. `false` when there is none by that id.
    pub fn cancel_invite(&self, id: &str) -> bool {
        let _ = id;
        todo!("PR 2: peer store")
    }

    /// Outstanding invites, oldest first, without their secrets.
    pub fn invites(&self) -> Vec<InviteSummary> {
        todo!("PR 2: peer store")
    }

    /// Consumes the invite whose hash matches `secret`. Wrong, expired,
    /// cancelled, and already redeemed secrets are all `InviteInvalid`; too
    /// many failures in the window are `RateLimited` before anything is
    /// compared.
    pub fn redeem_invite(&self, secret: &InviteSecret) -> Result<RedeemedInvite, FederationError> {
        let _ = secret;
        todo!("PR 2: peer store")
    }

    /// Inserts or replaces the record for `record.companion_id()` and persists.
    pub fn upsert(
        &self,
        record: PeerRecord,
        verified: VerifiedIdentity,
    ) -> Result<PeerRecord, FederationError> {
        let _ = (record, verified);
        todo!("PR 2: peer store")
    }

    pub fn get(&self, companion_id: &str) -> Option<Peer> {
        let _ = companion_id;
        todo!("PR 2: peer store")
    }

    /// Every record, oldest first.
    pub fn list(&self) -> Vec<PeerRecord> {
        todo!("PR 2: peer store")
    }

    /// Applies `change` to the record for `companion_id`, stamps `updated_at`,
    /// persists, and returns the new record; `Ok(None)` when unknown.
    pub fn update(
        &self,
        companion_id: &str,
        change: impl FnOnce(&mut PeerRecord),
    ) -> Result<Option<PeerRecord>, FederationError> {
        let _ = (companion_id, change);
        todo!("PR 2: peer store")
    }
}

#[derive(Default)]
struct Inner {
    loaded: bool,
    peers: Vec<Peer>,
    invites: Vec<Invite>,
    failures: Vec<u64>,
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
    fn wrong_expired_and_cancelled_invites_fail_closed_with_rate_limiting() {
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

        let live = store.create_invite();
        for _ in 0..MAX_FAILURES_PER_WINDOW {
            assert_eq!(
                store.redeem_invite(&wrong),
                Err(FederationError::InviteInvalid)
            );
        }
        assert_eq!(
            store.redeem_invite(&live.secret),
            Err(FederationError::RateLimited),
            "even the right secret is refused while the window is full"
        );
        now.fetch_add(FAILURE_WINDOW_SECS, Ordering::SeqCst);
        assert_eq!(
            store.redeem_invite(&live.secret),
            Err(FederationError::InviteInvalid),
            "the window drained but the invite expired meanwhile"
        );
        let fresh = store.create_invite();
        assert!(store.redeem_invite(&fresh.secret).is_ok());
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
            })
            .unwrap()
            .unwrap();
        assert_eq!(updated.state, PeerState::Paired);
        assert_eq!(updated.updated_at, T0 + 5);
        assert_eq!(updated.created_at, T0);
        assert_eq!(store.update("nobody", |_| {}).unwrap(), None);

        let reopened = PeerStore::new(tmp.path());
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

        let reopened = PeerStore::new(tmp.path());
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
            let reopened = PeerStore::new(tmp.path());
            assert!(reopened.list().is_empty(), "loaded peers from {junk}");
        }
    }
}
