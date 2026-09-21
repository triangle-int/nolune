//! The one codex app-server per gateway, and what every Codex turn shares:
//! the supervised process, started on the first turn and replaced by the
//! supervisor when it dies; the login state read from it (never the login
//! itself); and the thread bookkeeping the adapter keeps per conversation.
//!
//! [`Runtime::shared`] is the process-wide instance every backend built
//! from config carries. A test builds its own with [`Runtime::for_launch`]
//! and the fake app-server, so the real binary is never started by a test
//! that did not ask for it.

use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

use serde_json::{Value, json};

use super::{
    AppServerError,
    adapter::Threads,
    discovery,
    process::{AppServer, Launch},
};

/// The login the app-server holds, as `account/read` describes it: the
/// kind of account and who it belongs to, never a token.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AccountState {
    /// No login: `codex login` (or the login route) has to run first.
    LoggedOut,
    /// A ChatGPT login; `email` is what codex reports, when it does.
    ChatGpt { email: Option<String>, plan: String },
    /// The app-server is configured with an OpenAI API key of its own.
    ApiKey,
    /// An account type this build does not know.
    Other(String),
}

impl AccountState {
    pub fn is_logged_in(&self) -> bool {
        !matches!(self, Self::LoggedOut)
    }
}

/// Where the binary comes from.
enum Source {
    /// [`discovery::discover`] from the environment, at first use.
    Discover,
    /// A launch handed in, for tests.
    #[cfg_attr(not(test), allow(dead_code))]
    Launch(Launch),
}

struct Inner {
    source: Source,
    /// The supervisor once a child was started; a failed discovery leaves
    /// it empty so the next turn tries again (the user may install codex
    /// or log in meanwhile).
    server: tokio::sync::Mutex<Option<AppServer>>,
    /// The adapter's threads and open turns.
    threads: Mutex<Threads>,
    /// An empty directory of this runtime's own, where the threads of
    /// one-shot runs are placed; made on first use, removed with the
    /// runtime.
    scratch: Mutex<Option<tempfile::TempDir>>,
    /// Set by `close`: nothing starts again.
    closed: AtomicBool,
}

/// The shared codex state. Cheap to clone; every clone is the same process.
#[derive(Clone)]
pub struct Runtime {
    inner: Arc<Inner>,
}

impl Runtime {
    /// The one runtime of this process, discovering the binary on first use.
    pub fn shared() -> Runtime {
        static SHARED: OnceLock<Runtime> = OnceLock::new();
        SHARED
            .get_or_init(|| Self::with_source(Source::Discover))
            .clone()
    }

    /// A runtime of its own that starts `launch`: the fake in tests.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn for_launch(launch: Launch) -> Runtime {
        Self::with_source(Source::Launch(launch))
    }

    fn with_source(source: Source) -> Runtime {
        Runtime {
            inner: Arc::new(Inner {
                source,
                server: tokio::sync::Mutex::new(None),
                threads: Mutex::new(Threads::default()),
                scratch: Mutex::new(None),
                closed: AtomicBool::new(false),
            }),
        }
    }

    /// The supervised app-server, started now if it is not running yet.
    /// Discovery failures come back typed and are retried on the next call.
    pub(crate) async fn app_server(&self) -> Result<AppServer, AppServerError> {
        if self.inner.closed.load(Ordering::SeqCst) {
            return Err(AppServerError::Closed);
        }
        let mut server = self.inner.server.lock().await;
        if let Some(server) = server.as_ref() {
            return Ok(server.clone());
        }
        let launch = match &self.inner.source {
            Source::Launch(launch) => launch.clone(),
            Source::Discover => {
                // A test that reaches the shared runtime by accident must
                // never start the real binary, or spend a login on a turn.
                if cfg!(test) && std::env::var_os(super::LIVE_ENV).is_none() {
                    return Err(AppServerError::Unusable {
                        path: "codex".into(),
                        reason: format!(
                            "the shared runtime does not start the real binary under test (set {} for a live test)",
                            super::LIVE_ENV
                        ),
                    });
                }
                Launch::new(discovery::discover().await?.path)
            }
        };
        let started = AppServer::start(launch).await?;
        if self.inner.closed.load(Ordering::SeqCst) {
            started.close();
            return Err(AppServerError::Closed);
        }
        *server = Some(started.clone());
        Ok(started)
    }

    /// Who the app-server is logged in as, from `account/read`. Nothing but
    /// the account kind, email and plan leaves this function.
    pub(crate) async fn account(&self) -> Result<AccountState, AppServerError> {
        let answer = self
            .app_server()
            .await?
            .request("account/read", json!({}))
            .await?;
        Ok(account_state(&answer))
    }

    /// The adapter's per-conversation bookkeeping.
    pub(super) fn threads(&self) -> &Mutex<Threads> {
        &self.inner.threads
    }

    /// An empty directory of this runtime's own, for the threads of
    /// one-shot runs, which have no conversation and so no workspace: a
    /// thread has to run somewhere, and nowhere Nolune keeps anything.
    pub(super) fn scratch_dir(&self) -> std::io::Result<PathBuf> {
        let mut scratch = self.inner.scratch.lock().unwrap();
        if scratch.is_none() {
            *scratch = Some(tempfile::Builder::new().prefix("nolune-codex-").tempdir()?);
        }
        Ok(scratch
            .as_ref()
            .expect("made just above")
            .path()
            .to_path_buf())
    }

    /// Kill the child and refuse restarts; a runtime built for a test is
    /// closed when the test is done with it.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn close(&self) {
        // The lock is held only while a child starts; a start that sees the
        // flag afterwards closes its own child.
        self.inner.closed.store(true, Ordering::SeqCst);
        let server = match self.inner.server.try_lock() {
            Ok(mut server) => server.take(),
            Err(_) => None,
        };
        match server {
            Some(server) => server.close(),
            None => log::debug!("[codex] close: nothing running"),
        }
        self.inner.threads.lock().unwrap().clear();
    }
}

/// The `account/read` answer as an [`AccountState`]: `account` is null when
/// nobody is logged in, else an object whose `type` says what it is.
fn account_state(answer: &Value) -> AccountState {
    let account = &answer["account"];
    match account["type"].as_str() {
        None => AccountState::LoggedOut,
        Some("chatgpt") => AccountState::ChatGpt {
            email: account["email"].as_str().map(str::to_owned),
            plan: account["planType"].as_str().unwrap_or("unknown").to_owned(),
        },
        Some("apiKey") => AccountState::ApiKey,
        Some(other) => AccountState::Other(other.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::super::fake;
    use super::*;

    #[tokio::test]
    async fn the_account_is_read_without_its_tokens() {
        let runtime = Runtime::for_launch(fake::launch(None));
        let account = runtime.account().await.unwrap();
        assert_eq!(
            account,
            AccountState::ChatGpt {
                email: Some("companion@example.test".into()),
                plan: "plus".into(),
            }
        );
        assert!(account.is_logged_in());
        let json = serde_json::to_value(&account).unwrap();
        assert_eq!(json["kind"], "chat_gpt");
        assert_eq!(json["email"], "companion@example.test");
        runtime.close();

        let mut launch = fake::launch(None);
        launch.env.push((fake::ACCOUNT_ENV.into(), "none".into()));
        let runtime = Runtime::for_launch(launch);
        let account = runtime.account().await.unwrap();
        assert_eq!(account, AccountState::LoggedOut);
        assert!(!account.is_logged_in());
        assert_eq!(
            serde_json::to_value(&account).unwrap(),
            serde_json::json!({"kind": "logged_out"})
        );
        runtime.close();
    }

    #[tokio::test]
    async fn the_app_server_starts_once_and_close_ends_it() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pids");
        let runtime = Runtime::for_launch(fake::launch(Some(&pid_file)));
        let first = runtime.app_server().await.unwrap();
        let again = runtime.app_server().await.unwrap();
        assert_eq!(first.pid(), again.pid(), "one child for the runtime");
        assert_eq!(
            std::fs::read_to_string(&pid_file).unwrap().lines().count(),
            1
        );
        runtime.close();
        assert!(matches!(
            runtime.app_server().await,
            Err(AppServerError::Closed)
        ));
    }

    /// The shared runtime discovers the real binary, which no test may
    /// start by accident: under test it refuses unless the live tests
    /// opted in.
    #[tokio::test]
    async fn the_shared_runtime_never_starts_the_real_binary_under_test() {
        if std::env::var_os(super::super::LIVE_ENV).is_some() {
            return;
        }
        let shared = Runtime::shared();
        assert!(matches!(
            shared.app_server().await,
            Err(AppServerError::Unusable { .. })
        ));
        assert!(matches!(
            shared.account().await,
            Err(AppServerError::Unusable { .. })
        ));
    }
}
