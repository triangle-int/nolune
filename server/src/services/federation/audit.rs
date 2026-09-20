//! The federation audit log (#109): `federation/audit.jsonl` beside the
//! peer store, one [`AuditReceipt`] per line, mode `0600`, appended to and
//! never edited.
//!
//! A receipt says who asked whom for which intent at which disclosure class
//! and what was decided. It never carries the payload, because nothing here
//! takes an envelope or peer text, so the log can be listed and kept without
//! re-reading what a peer sent. Retention is bounded like proactive run
//! records: the newest [`MAX_AUDIT_RECEIPTS`] overall, the newest
//! [`MAX_RECEIPTS_PER_PEER`] per peer (so one chatty peer cannot push the
//! others out), and nothing older than [`AUDIT_RETENTION_DAYS`]; past a
//! bound the file is compacted through a temporary file and a rename.
//!
//! A file this build cannot load is never repaired and never overwritten:
//! every read and write fails closed, and because a decision is not made
//! without its receipt, nothing is admitted until the owner looks.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

use super::{identity, peers::Clock};
use crate::domain::federation::FederationError;
use crate::domain::federation_policy::{AuditReceipt, RECEIPT_VERSION as RECEIPT_FORMAT_VERSION};

/// The audit log, under the keystore directory.
pub const AUDIT_FILE: &str = "audit.jsonl";
/// Receipts kept overall, newest first.
pub const MAX_AUDIT_RECEIPTS: usize = 1000;
/// Receipts kept per peer (as requester or responder), newest first.
pub const MAX_RECEIPTS_PER_PEER: usize = 200;
/// Receipts older than this are dropped.
pub const AUDIT_RETENTION_DAYS: u64 = 30;

/// Upper bound for the log file; anything larger is not ours.
const MAX_AUDIT_FILE_BYTES: u64 = 8 * 1024 * 1024;
const RECEIPT_ID_BYTES: usize = 8;

pub struct AuditLog {
    root: PathBuf,
    inner: Mutex<Inner>,
    clock: Clock,
}

impl AuditLog {
    /// A log over `workspace_root/federation/audit.jsonl`. Nothing is read
    /// until the first access.
    pub(crate) fn with_clock(workspace_root: &Path, clock: Clock) -> Self {
        Self {
            root: workspace_root.to_path_buf(),
            inner: Mutex::new(Inner::default()),
            clock,
        }
    }

    /// `workspace/federation/audit.jsonl`
    pub fn path(&self) -> PathBuf {
        identity::federation_dir(&self.root).join(AUDIT_FILE)
    }

    /// A fresh receipt id: 16 hex characters.
    pub fn new_id() -> String {
        let mut buf = [0u8; RECEIPT_ID_BYTES];
        getrandom::fill(&mut buf).expect("operating system randomness is unavailable");
        buf.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Appends `receipt` and enforces retention. Fails closed over a log
    /// this build could not load or write.
    pub fn record(&self, receipt: AuditReceipt) -> Result<(), FederationError> {
        let now = (self.clock)();
        let mut inner = self.inner.lock().unwrap();
        self.ensure_loaded(&mut inner);
        self.refuse_if_unloadable(&inner)?;
        let mut receipts = inner.receipts.clone();
        receipts.push(receipt.clone());
        if enforce_retention(&mut receipts, now) {
            // Something was dropped: rewrite the whole file.
            self.rewrite(&receipts)?;
        } else {
            self.append(&receipt)?;
        }
        inner.receipts = receipts;
        Ok(())
    }

    /// Every kept receipt, newest first.
    pub fn list(&self) -> Result<Vec<AuditReceipt>, FederationError> {
        let mut inner = self.inner.lock().unwrap();
        self.ensure_loaded(&mut inner);
        self.refuse_if_unloadable(&inner)?;
        let mut receipts = inner.receipts.clone();
        receipts.reverse();
        Ok(receipts)
    }

    /// Reads the log once and applies retention to what it holds. A file
    /// that cannot be read, is too large, or has a line that is not a
    /// receipt of this version leaves the log marked unloadable: reported,
    /// never repaired, never overwritten.
    fn ensure_loaded(&self, inner: &mut Inner) {
        if inner.loaded {
            return;
        }
        inner.loaded = true;
        let path = self.path();
        let contents = match std::fs::metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => {
                return self.mark_unloadable(inner, format!("cannot be read ({error})"));
            }
            Ok(metadata) if metadata.len() > MAX_AUDIT_FILE_BYTES => {
                return self
                    .mark_unloadable(inner, "is larger than an audit log can be".to_owned());
            }
            Ok(_) => match std::fs::read_to_string(&path) {
                Ok(contents) => contents,
                Err(error) => {
                    return self.mark_unloadable(inner, format!("cannot be read ({error})"));
                }
            },
        };
        let mut receipts = Vec::new();
        for (index, line) in contents.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<AuditReceipt>(line) {
                Ok(receipt) if receipt.version == RECEIPT_FORMAT_VERSION => receipts.push(receipt),
                Ok(receipt) => {
                    return self.mark_unloadable(
                        inner,
                        format!(
                            "has a receipt of unsupported version {} on line {}",
                            receipt.version,
                            index + 1
                        ),
                    );
                }
                Err(_) => {
                    return self.mark_unloadable(
                        inner,
                        format!("does not have the expected shape on line {}", index + 1),
                    );
                }
            }
        }
        inner.receipts = receipts;
        if enforce_retention(&mut inner.receipts, (self.clock)()) {
            // Old entries are dropped from memory now and from the file on
            // the next write; a read alone never rewrites the file.
        }
    }

    fn mark_unloadable(&self, inner: &mut Inner, reason: String) {
        log::warn!(
            "[federation] audit log {} {reason}; no intent will be admitted and nothing will be written until it is repaired or moved aside and the server restarted",
            self.path().display()
        );
        inner.unloadable = Some(reason);
    }

    /// Appends one line, creating the file owner-only if needed.
    fn append(&self, receipt: &AuditReceipt) -> Result<(), FederationError> {
        let path = self.path();
        let io = |error: std::io::Error| FederationError::Io {
            path: path.clone(),
            message: error.to_string(),
        };
        let dir = path
            .parent()
            .ok_or_else(|| std::io::Error::other("audit log path has no directory"))
            .map_err(io)?;
        identity::create_private_dir(dir).map_err(io)?;
        let mut line = serde_json::to_string(receipt)
            .map_err(|error| std::io::Error::other(error.to_string()))
            .map_err(io)?;
        line.push('\n');
        let mut options = std::fs::OpenOptions::new();
        options.append(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path).map_err(io)?;
        use std::io::Write as _;
        file.write_all(line.as_bytes()).map_err(io)?;
        file.sync_all().map_err(io)
    }

    /// Replaces the file with `receipts`, oldest first, through a temporary
    /// file and a rename.
    fn rewrite(&self, receipts: &[AuditReceipt]) -> Result<(), FederationError> {
        let path = self.path();
        let io = |error: std::io::Error| FederationError::Io {
            path: path.clone(),
            message: error.to_string(),
        };
        let mut contents = String::new();
        for receipt in receipts {
            contents.push_str(
                &serde_json::to_string(receipt)
                    .map_err(|error| std::io::Error::other(error.to_string()))
                    .map_err(io)?,
            );
            contents.push('\n');
        }
        identity::replace_private(&path, contents.as_bytes()).map_err(io)
    }

    fn refuse_if_unloadable(&self, inner: &Inner) -> Result<(), FederationError> {
        match &inner.unloadable {
            Some(reason) => Err(FederationError::Io {
                path: self.path(),
                message: format!(
                    "federation audit log {reason}; repair or move it aside and restart before federation is used"
                ),
            }),
            None => Ok(()),
        }
    }
}

/// Drops receipts beyond the bounds; returns whether anything was dropped.
/// `receipts` is oldest first on the way in and out.
fn enforce_retention(receipts: &mut Vec<AuditReceipt>, now: u64) -> bool {
    let before = receipts.len();
    let oldest_allowed = now.saturating_sub(AUDIT_RETENTION_DAYS * 86_400);
    receipts.retain(|receipt| receipt.at > oldest_allowed);
    // Per peer: a receipt belongs to the companion on the other side, so a
    // flood from one peer only ever pushes out that peer's own history.
    let mut kept_per_peer: HashMap<String, usize> = HashMap::new();
    let mut keep = vec![false; receipts.len()];
    for (index, receipt) in receipts.iter().enumerate().rev() {
        let peer = receipt.peer().to_owned();
        let kept = kept_per_peer.entry(peer).or_insert(0);
        if *kept < MAX_RECEIPTS_PER_PEER {
            *kept += 1;
            keep[index] = true;
        }
    }
    let mut index = 0;
    receipts.retain(|_| {
        let kept = keep[index];
        index += 1;
        kept
    });
    if receipts.len() > MAX_AUDIT_RECEIPTS {
        let excess = receipts.len() - MAX_AUDIT_RECEIPTS;
        receipts.drain(..excess);
    }
    receipts.len() != before
}

#[derive(Default)]
struct Inner {
    loaded: bool,
    /// Why the file could not be loaded, when it exists but could not.
    unloadable: Option<String>,
    /// Oldest first.
    receipts: Vec<AuditReceipt>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::federation_policy::{
        Decision, DecisionReason, RECEIPT_VERSION, ReceiptSide,
    };
    use crate::services::federation::peers::system_clock;
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };

    const T0: u64 = 1_800_000_000;
    const DAY: u64 = 86_400;

    fn clock() -> (Arc<AtomicU64>, Clock) {
        let now = Arc::new(AtomicU64::new(T0));
        let read = now.clone();
        (now, Arc::new(move || read.load(Ordering::SeqCst)))
    }

    fn receipt(requester: &str, at: u64) -> AuditReceipt {
        AuditReceipt {
            version: RECEIPT_VERSION,
            id: AuditLog::new_id(),
            side: ReceiptSide::Answering,
            requester: requester.into(),
            responder: "me".into(),
            intent: "message".into(),
            disclosure: "none".into(),
            detail: None,
            decision: Decision::ask(DecisionReason::Default),
            at,
            summary: format!("{requester} asked me for message (none): ask (default)"),
        }
    }

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn receipt_ids_are_sixteen_hex_characters_and_distinct() {
        let a = AuditLog::new_id();
        let b = AuditLog::new_id();
        assert_eq!(a.len(), 16);
        assert!(a.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn receipts_append_as_owner_only_json_lines_and_list_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let (now, clock) = clock();
        let log = AuditLog::with_clock(dir.path(), clock);
        assert_eq!(log.list().unwrap(), Vec::new());
        assert!(!log.path().exists(), "listing must not create the file");

        let first = receipt("peer-a", T0);
        log.record(first.clone()).unwrap();
        now.store(T0 + 5, Ordering::SeqCst);
        let second = receipt("peer-b", T0 + 5);
        log.record(second.clone()).unwrap();

        let path = log.path();
        assert_eq!(path, dir.path().join("federation").join("audit.jsonl"));
        #[cfg(unix)]
        {
            assert_eq!(mode(&path), 0o600);
            assert_eq!(mode(path.parent().unwrap()), 0o700);
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "one line per receipt: {text}");
        assert!(text.ends_with('\n'));
        assert_eq!(
            serde_json::from_str::<AuditReceipt>(lines[0]).unwrap(),
            first,
            "oldest first on disk"
        );
        assert_eq!(
            serde_json::from_str::<AuditReceipt>(lines[1]).unwrap(),
            second
        );
        assert!(
            !lines[0].contains('\n') && !text.contains("\n\n"),
            "each receipt is exactly one line"
        );

        assert_eq!(log.list().unwrap(), vec![second.clone(), first.clone()]);
        // A fresh log over the same root reads the same receipts.
        let again = AuditLog::with_clock(dir.path(), system_clock());
        assert_eq!(again.list().unwrap(), vec![second, first]);
    }

    #[test]
    fn retention_keeps_the_newest_per_peer_and_overall_and_drops_old_ones() {
        // Per peer.
        let mut receipts: Vec<AuditReceipt> = [receipt("quiet", T0)]
            .into_iter()
            .chain((1..MAX_RECEIPTS_PER_PEER as u64 + 6).map(|i| receipt("chatty", T0 + i)))
            .collect();
        assert!(enforce_retention(&mut receipts, T0 + 1000));
        assert_eq!(
            receipts.iter().filter(|r| r.requester == "chatty").count(),
            MAX_RECEIPTS_PER_PEER
        );
        assert_eq!(
            receipts.iter().filter(|r| r.requester == "quiet").count(),
            1,
            "a quiet peer's receipt survives a chatty one's flood"
        );
        assert_eq!(
            receipts
                .iter()
                .filter(|r| r.requester == "chatty")
                .map(|r| r.at)
                .min(),
            Some(T0 + 6),
            "the oldest of the chatty peer's receipts went first"
        );
        assert!(
            receipts.windows(2).all(|pair| pair[0].at <= pair[1].at),
            "order is kept"
        );
        assert!(
            !enforce_retention(&mut receipts, T0 + 1000),
            "already within bounds"
        );

        // Overall, across many peers.
        let mut receipts: Vec<AuditReceipt> = (0..MAX_AUDIT_RECEIPTS as u64 + 10)
            .map(|i| receipt(&format!("peer-{}", i % 50), T0 + i))
            .collect();
        assert!(enforce_retention(&mut receipts, T0 + 5000));
        assert_eq!(receipts.len(), MAX_AUDIT_RECEIPTS);
        assert_eq!(receipts[0].at, T0 + 10, "the oldest went first");

        // Age. A receipt counts as a peer's whether it asked or answered.
        let mut receipts = vec![
            receipt("peer-a", T0),
            receipt("peer-a", T0 + DAY),
            receipt("peer-a", T0 + 2 * DAY),
        ];
        assert!(enforce_retention(
            &mut receipts,
            T0 + AUDIT_RETENTION_DAYS * DAY + DAY
        ));
        assert_eq!(
            receipts.iter().map(|r| r.at).collect::<Vec<_>>(),
            [T0 + 2 * DAY],
            "only the receipt younger than the retention window stays"
        );
        let mut requester_side = receipt("peer-a", T0);
        requester_side.side = ReceiptSide::Requesting;
        requester_side.requester = "me".into();
        requester_side.responder = "peer-a".into();
        let mut mixed: Vec<AuditReceipt> = (0..MAX_RECEIPTS_PER_PEER as u64)
            .map(|i| receipt("peer-a", T0 + 1 + i))
            .collect();
        mixed.insert(0, requester_side);
        assert!(enforce_retention(&mut mixed, T0 + 1000));
        assert_eq!(mixed.len(), MAX_RECEIPTS_PER_PEER);
        assert!(
            mixed.iter().all(|r| r.side == ReceiptSide::Answering),
            "the requesting-side receipt about peer-a was the oldest of that peer's"
        );
    }

    #[test]
    fn the_file_is_compacted_when_retention_drops_receipts() {
        let dir = tempfile::tempdir().unwrap();
        let (now, clock) = clock();
        let log = AuditLog::with_clock(dir.path(), clock);
        for i in 0..MAX_RECEIPTS_PER_PEER as u64 + 3 {
            now.store(T0 + i, Ordering::SeqCst);
            log.record(receipt("chatty", T0 + i)).unwrap();
        }
        let text = std::fs::read_to_string(log.path()).unwrap();
        assert_eq!(text.lines().count(), MAX_RECEIPTS_PER_PEER);
        assert_eq!(log.list().unwrap().len(), MAX_RECEIPTS_PER_PEER);
        assert_eq!(
            log.list().unwrap()[0].at,
            T0 + MAX_RECEIPTS_PER_PEER as u64 + 2
        );
        #[cfg(unix)]
        assert_eq!(mode(&log.path()), 0o600, "compaction keeps the mode");
        // Old receipts are dropped when a new one arrives past the window,
        // and on load.
        now.store(T0 + (AUDIT_RETENTION_DAYS + 1) * 86_400, Ordering::SeqCst);
        log.record(receipt("chatty", T0 + (AUDIT_RETENTION_DAYS + 1) * 86_400))
            .unwrap();
        assert_eq!(log.list().unwrap().len(), 1);
        assert_eq!(
            std::fs::read_to_string(log.path()).unwrap().lines().count(),
            1
        );
        let later = AuditLog::with_clock(
            dir.path(),
            Arc::new(|| T0 + (2 * AUDIT_RETENTION_DAYS + 5) * 86_400),
        );
        assert_eq!(
            later.list().unwrap(),
            Vec::new(),
            "everything is too old on load"
        );
    }

    #[test]
    fn an_unloadable_log_fails_closed_and_is_never_overwritten() {
        for contents in [
            "not json\n",
            "{\"version\":2,\"id\":\"x\"}\n",
            &format!(
                "{}\n{{\"version\":1}}\n",
                serde_json::to_string(&receipt("peer-a", T0)).unwrap()
            ),
            &format!(
                "{}\n",
                serde_json::to_string(&receipt("peer-a", T0))
                    .unwrap()
                    .replace("\"summary\"", "\"body\":\"hello\",\"summary\"")
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let log = AuditLog::with_clock(dir.path(), system_clock());
            std::fs::create_dir_all(log.path().parent().unwrap()).unwrap();
            std::fs::write(log.path(), contents).unwrap();
            let read = log.list().unwrap_err();
            assert!(
                matches!(&read, FederationError::Io { path, .. } if *path == log.path()),
                "{contents}: {read:?}"
            );
            assert!(
                !read.to_string().contains("hello"),
                "the error must not quote the file"
            );
            let write = log.record(receipt("peer-b", T0)).unwrap_err();
            assert!(matches!(write, FederationError::Io { .. }));
            assert_eq!(std::fs::read_to_string(log.path()).unwrap(), contents);
        }
        // An empty file and a trailing blank line are fine.
        let dir = tempfile::tempdir().unwrap();
        let log = AuditLog::with_clock(dir.path(), system_clock());
        std::fs::create_dir_all(log.path().parent().unwrap()).unwrap();
        std::fs::write(log.path(), "").unwrap();
        assert_eq!(log.list().unwrap(), Vec::new());
        std::fs::write(
            log.path(),
            format!(
                "{}\n\n",
                serde_json::to_string(&receipt("peer-a", T0)).unwrap()
            ),
        )
        .unwrap();
        let fresh = AuditLog::with_clock(dir.path(), system_clock());
        assert_eq!(fresh.list().unwrap().len(), 1);
    }

    #[test]
    fn an_oversize_log_is_unloadable() {
        let dir = tempfile::tempdir().unwrap();
        let log = AuditLog::with_clock(dir.path(), system_clock());
        std::fs::create_dir_all(log.path().parent().unwrap()).unwrap();
        let line = serde_json::to_string(&receipt("peer-a", T0)).unwrap();
        let mut text = String::new();
        while text.len() <= MAX_AUDIT_FILE_BYTES as usize {
            text.push_str(&line);
            text.push('\n');
        }
        std::fs::write(log.path(), &text).unwrap();
        assert!(matches!(log.list(), Err(FederationError::Io { .. })));
        assert_eq!(std::fs::read_to_string(log.path()).unwrap(), text);
    }
}
