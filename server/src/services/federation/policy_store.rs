//! The owner's federation policy on disk (#109): `federation/policy.json`
//! beside the peer store, mode `0600`, written through a temporary file and
//! a rename.
//!
//! A missing file is the default document (nothing granted, the default
//! rate limit, no quiet hours) and is not written until the owner changes
//! something. A file this build cannot load (another version, junk, the
//! wrong shape) is never repaired and never overwritten: every read and
//! write fails closed, so nothing is judged against a policy that could
//! not be read. Every change goes through [`PolicyStore::update`] under the
//! store lock, so a revocation is visible to the very next evaluation.

use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::Deserialize;

use super::identity;
use crate::domain::federation::FederationError;
use crate::domain::federation_policy::{POLICY_VERSION, PolicyDocument};

/// The policy document, under the keystore directory.
pub const POLICY_FILE: &str = "policy.json";

/// Upper bound for the policy file; anything larger is not ours.
const MAX_POLICY_FILE_BYTES: u64 = 1024 * 1024;

pub struct PolicyStore {
    root: PathBuf,
    inner: Mutex<Inner>,
}

impl PolicyStore {
    /// A store over `workspace_root/federation/policy.json`. Nothing is read
    /// until the first access.
    pub fn new(workspace_root: &Path) -> Self {
        Self {
            root: workspace_root.to_path_buf(),
            inner: Mutex::new(Inner::default()),
        }
    }

    /// `workspace/federation/policy.json`
    pub fn path(&self) -> PathBuf {
        identity::federation_dir(&self.root).join(POLICY_FILE)
    }

    /// The document as it is now. Fails closed over a file this build could
    /// not load.
    pub fn document(&self) -> Result<PolicyDocument, FederationError> {
        todo!("read the policy document")
    }

    /// Applies `change` under the store lock and persists; a change that
    /// leaves the document as it was is not written. Fails closed, changing
    /// nothing, over a file this build could not load or when `change`
    /// refuses.
    pub fn update(
        &self,
        change: impl FnOnce(&mut PolicyDocument) -> Result<(), FederationError>,
    ) -> Result<PolicyDocument, FederationError> {
        todo!("change and persist the policy document")
    }

    fn ensure_loaded(&self, inner: &mut Inner) {
        todo!("load the policy file once")
    }

    fn refuse_if_unloadable(&self, inner: &Inner) -> Result<(), FederationError> {
        match &inner.unloadable {
            Some(reason) => Err(FederationError::Io {
                path: self.path(),
                message: format!(
                    "federation policy {reason}; repair or move it aside and restart before federation is used"
                ),
            }),
            None => Ok(()),
        }
    }
}

#[derive(Default)]
struct Inner {
    loaded: bool,
    /// Why the file could not be loaded, when it exists but could not.
    unloadable: Option<String>,
    document: PolicyDocument,
}

/// Just the version, read leniently so an unsupported file is reported as
/// such rather than as the wrong shape.
#[derive(Deserialize)]
struct FileVersion {
    version: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::federation_policy::{
        Access, DisclosureClass, IntentClass, PeerPolicy, PolicyRule, QuietHoursPolicy,
    };

    const T0: u64 = 1_800_000_000;

    fn store(dir: &Path) -> PolicyStore {
        PolicyStore::new(dir)
    }

    fn rule() -> PolicyRule {
        PolicyRule {
            intent: IntentClass::Message,
            disclosure: DisclosureClass::None,
            access: Access::Allow,
            granted_at: T0,
            expires_at: Some(T0 + 3600),
        }
    }

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn a_missing_file_is_the_default_document_and_is_not_written_until_changed() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        assert_eq!(store.document().unwrap(), PolicyDocument::default());
        assert!(!store.path().exists(), "reading must not create the file");
        assert!(
            !dir.path().join("federation").exists(),
            "reading must not create the directory either"
        );
        // An update that changes nothing writes nothing.
        store.update(|_| Ok(())).unwrap();
        assert!(!store.path().exists());
    }

    #[test]
    fn updates_persist_owner_only_beside_the_peer_store_and_reload() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        let written = store
            .update(|document| {
                document.quiet_hours = Some(QuietHoursPolicy {
                    start_hour: 22,
                    end_hour: 7,
                    timezone: Some("Europe/Berlin".into()),
                });
                document
                    .peers
                    .entry("peer-a".into())
                    .or_default()
                    .rules
                    .push(rule());
                Ok(())
            })
            .unwrap();
        assert_eq!(written.peer("peer-a").rules, vec![rule()]);
        let path = store.path();
        assert_eq!(
            path,
            dir.path().join("federation").join("policy.json"),
            "the policy lives beside peers.json"
        );
        assert!(path.is_file());
        #[cfg(unix)]
        {
            assert_eq!(mode(&path), 0o600);
            assert_eq!(mode(path.parent().unwrap()), 0o700);
        }
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.ends_with('\n'));
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(json["version"], POLICY_VERSION);
        assert_eq!(json["quiet_hours"]["timezone"], "Europe/Berlin");
        assert_eq!(json["peers"]["peer-a"]["rules"][0]["intent"], "message");
        assert_eq!(json["peers"]["peer-a"]["rules"][0]["expires_at"], T0 + 3600);
        assert!(
            json["peers"]["peer-a"].get("rate_limit").is_none(),
            "an absent override is not written"
        );

        // A fresh store over the same root sees the same document.
        let again = PolicyStore::new(dir.path());
        assert_eq!(again.document().unwrap(), written);

        // Revoking the rule is visible at once and persisted.
        let revoked = again
            .update(|document| {
                document.peers.get_mut("peer-a").unwrap().rules.clear();
                Ok(())
            })
            .unwrap();
        assert!(revoked.peer("peer-a").rules.is_empty());
        assert_eq!(
            PolicyStore::new(dir.path())
                .document()
                .unwrap()
                .peer("peer-a"),
            PeerPolicy::default()
        );
    }

    #[test]
    fn a_refused_change_persists_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        store
            .update(|document| {
                document.peers.entry("peer-a".into()).or_default();
                Ok(())
            })
            .unwrap();
        let before = std::fs::read_to_string(store.path()).unwrap();
        let error = store
            .update(|document| {
                document.peers.clear();
                Err(FederationError::Malformed("no".into()))
            })
            .unwrap_err();
        assert_eq!(error, FederationError::Malformed("no".into()));
        assert_eq!(std::fs::read_to_string(store.path()).unwrap(), before);
        assert!(store.document().unwrap().peers.contains_key("peer-a"));
    }

    #[test]
    fn an_unloadable_file_fails_closed_and_is_never_overwritten() {
        for contents in [
            r#"{"version": 2, "peers": {}, "future": true}"#,
            r#"{"version": 2}"#,
            "not json at all",
            r#"{"version": 1, "peers": {"p": {"rules": [{"intent": "shell"}]}}}"#,
            r#"{"version": 1, "peers": {"p": {"tools": ["*"]}}}"#,
        ] {
            let dir = tempfile::tempdir().unwrap();
            let store = store(dir.path());
            std::fs::create_dir_all(store.path().parent().unwrap()).unwrap();
            std::fs::write(store.path(), contents).unwrap();
            let read = store.document().unwrap_err();
            assert!(
                matches!(&read, FederationError::Io { path, .. } if *path == store.path()),
                "{contents}: {read:?}"
            );
            let message = read.to_string();
            assert!(
                !message.contains("shell") && !message.contains("tools"),
                "the error must not quote the file: {message}"
            );
            let write = store
                .update(|document| {
                    document.peers.entry("peer-a".into()).or_default();
                    Ok(())
                })
                .unwrap_err();
            assert!(matches!(write, FederationError::Io { .. }), "{contents}");
            assert_eq!(
                std::fs::read_to_string(store.path()).unwrap(),
                contents,
                "the file must survive byte for byte"
            );
            if contents.contains("\"version\": 2") {
                assert!(
                    message.contains("version 2"),
                    "a newer file is reported as such: {message}"
                );
            }
        }
    }

    #[test]
    fn an_oversize_file_is_unloadable() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        std::fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        let mut big = String::from(r#"{"version": 1, "peers": {}, "pad": ""#);
        big.push_str(&"x".repeat(MAX_POLICY_FILE_BYTES as usize + 1));
        big.push_str("\"}");
        std::fs::write(store.path(), &big).unwrap();
        assert!(matches!(store.document(), Err(FederationError::Io { .. })));
        assert_eq!(std::fs::read_to_string(store.path()).unwrap(), big);
    }
}
