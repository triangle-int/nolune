//! Process-wide gate between a companion import and the writers that reach
//! the companion tree through ambient paths (#74).
//!
//! Memory writes hold the per-companion lifecycle gate, which the import
//! holds too. Everything else that writes `instances/companion` does so
//! through plain paths: an admitted mutating request, a chat message being
//! saved, a proactive run recording its receipts. Each of those holds this
//! gate *shared* for as long as it runs; the import holds it *exclusively*
//! from its busy check until the previous tree is discarded. A writer that
//! arrives during an import waits and then lands in the imported tree; an
//! import that finds writers in flight waits a bounded time for them and
//! reports busy otherwise.
//!
//! The gate is deliberately unfair to the import: a waiting import never
//! blocks a shared holder from taking the gate again (a request that starts
//! a proactive run holds it twice), so holders never wait on each other.
//! The bounded wait keeps that from starving the import for long. While it
//! waits the import holds nothing else either: it takes the gate with
//! [`ImportGate::try_import`] once the companion's lifecycle gate is its,
//! and if writers are in flight it lets the lifecycle gate go and waits for
//! [`ImportGate::idle`] before trying again, so a writer that needs the
//! lifecycle gate meanwhile is never queued behind a waiting import.

use std::sync::{Arc, Mutex};

use tokio::sync::Notify;

#[derive(Default)]
struct GateState {
    writers: usize,
    importing: bool,
}

pub struct ImportGate {
    state: Mutex<GateState>,
    notify: Notify,
}

/// A shared hold: an ambient writer is in flight.
pub struct WriterGuard(Arc<ImportGate>);

/// The exclusive hold: an import is replacing the companion tree.
pub struct ImportGuard(Arc<ImportGate>);

impl ImportGate {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(GateState::default()),
            notify: Notify::new(),
        })
    }

    fn state(&self) -> std::sync::MutexGuard<'_, GateState> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    /// Hold the gate shared. Waits while an import holds it, never while
    /// one is merely waiting, so a holder can take it again.
    pub async fn writer(self: &Arc<Self>) -> WriterGuard {
        loop {
            // Registered before the check so a release in between is not lost.
            let released = self.notify.notified();
            if let Some(guard) = self.try_writer() {
                return guard;
            }
            released.await;
        }
    }

    /// Hold the gate shared right now, or `None` while an import holds it.
    pub fn try_writer(self: &Arc<Self>) -> Option<WriterGuard> {
        let mut state = self.state();
        if state.importing {
            return None;
        }
        state.writers += 1;
        Some(WriterGuard(self.clone()))
    }

    /// Hold the gate exclusively right now, or `None` while a writer or
    /// another import holds it.
    pub fn try_import(self: &Arc<Self>) -> Option<ImportGuard> {
        let mut state = self.state();
        if state.importing || state.writers > 0 {
            return None;
        }
        state.importing = true;
        Some(ImportGuard(self.clone()))
    }

    /// Wait until nobody holds the gate. Holds nothing itself; a writer may
    /// take the gate again before the caller does, so the caller tries and
    /// waits again.
    pub async fn idle(&self) {
        loop {
            let released = self.notify.notified();
            {
                let state = self.state();
                if !state.importing && state.writers == 0 {
                    return;
                }
            }
            released.await;
        }
    }

    /// Hold the gate exclusively once every writer in flight has finished,
    /// waiting at most `wait` for them; `None` when they were still writing.
    /// The restore itself interleaves this wait with the lifecycle gate (see
    /// `profile_import::restore`); this is the plain form for the tests.
    #[cfg(test)]
    pub async fn import(self: &Arc<Self>, wait: std::time::Duration) -> Option<ImportGuard> {
        tokio::time::timeout(wait, async {
            loop {
                if let Some(guard) = self.try_import() {
                    return guard;
                }
                self.idle().await;
            }
        })
        .await
        .ok()
    }

    /// Whether an import holds the gate right now.
    #[cfg(test)]
    pub fn importing(&self) -> bool {
        self.state().importing
    }
}

impl Drop for WriterGuard {
    fn drop(&mut self) {
        let mut state = self.0.state();
        state.writers -= 1;
        if state.writers == 0 {
            self.0.notify.notify_waiters();
        }
    }
}

impl Drop for ImportGuard {
    fn drop(&mut self) {
        self.0.state().importing = false;
        self.0.notify.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn an_import_waits_for_writers_in_flight_and_blocks_new_ones() {
        let gate = ImportGate::new();
        let writer = gate.writer().await;
        assert!(!gate.importing());

        let waiting = {
            let gate = gate.clone();
            tokio::spawn(async move { gate.import(Duration::from_secs(5)).await })
        };
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished(), "the import waits for the writer");
        // A queued import never blocks a shared holder from taking the gate again.
        let nested = gate.try_writer().expect("a holder can take the gate again");
        drop(nested);
        drop(writer);

        let import = waiting
            .await
            .unwrap()
            .expect("the import proceeds once writers are done");
        assert!(gate.importing());
        assert!(
            gate.try_writer().is_none(),
            "no writer starts during the import"
        );
        let blocked = {
            let gate = gate.clone();
            tokio::spawn(async move {
                let _guard = gate.writer().await;
                "wrote"
            })
        };
        tokio::task::yield_now().await;
        assert!(!blocked.is_finished(), "a writer waits for the import");
        drop(import);
        assert_eq!(blocked.await.unwrap(), "wrote");
        assert!(!gate.importing());
    }

    #[tokio::test]
    async fn an_import_gives_up_after_the_bounded_wait_and_leaves_the_writer_alone() {
        let gate = ImportGate::new();
        let writer = gate.writer().await;

        let refused = gate.import(Duration::from_millis(50)).await;

        assert!(refused.is_none());
        assert!(!gate.importing());
        assert!(
            gate.try_writer().is_some(),
            "a refused import holds nothing"
        );
        drop(writer);
        assert!(gate.import(Duration::from_millis(50)).await.is_some());
    }

    #[tokio::test]
    async fn two_imports_take_turns() {
        let gate = ImportGate::new();
        let first = gate.import(Duration::from_secs(1)).await.unwrap();
        let second = {
            let gate = gate.clone();
            tokio::spawn(async move { gate.import(Duration::from_secs(5)).await.is_some() })
        };
        tokio::task::yield_now().await;
        assert!(!second.is_finished());
        drop(first);
        assert!(second.await.unwrap());
    }
}
