//! The one codex app-server per gateway, and what every Codex turn shares:
//! the supervised process, started on the first turn and replaced by the
//! supervisor when it dies; the login state read from it (never the login
//! itself); and the thread bookkeeping the adapter keeps per conversation.
//!
//! [`Runtime::shared`] is the process-wide instance every backend built
//! from config carries. A test builds its own with [`Runtime::for_launch`]
//! and the fake app-server, so the real binary is never started by a test
//! that did not ask for it.

use std::sync::{Arc, Mutex, OnceLock};

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
    pub fn for_launch(launch: Launch) -> Runtime {
        Self::with_source(Source::Launch(launch))
    }

    fn with_source(source: Source) -> Runtime {
        Runtime {
            inner: Arc::new(Inner {
                source,
                server: tokio::sync::Mutex::new(None),
                threads: Mutex::new(Threads::default()),
            }),
        }
    }

    /// The supervised app-server, started now if it is not running yet.
    /// Discovery failures come back typed and are retried on the next call.
    pub(crate) async fn app_server(&self) -> Result<AppServer, AppServerError> {
        let _ = (&self.inner.source, &self.inner.server, discovery::discover);
        let _ = Launch::new("");
        todo!("27c")
    }

    /// Who the app-server is logged in as, from `account/read`. Nothing but
    /// the account kind, email and plan leaves this function.
    pub(crate) async fn account(&self) -> Result<AccountState, AppServerError> {
        let _: Value = json!({});
        todo!("27c")
    }

    /// The adapter's per-conversation bookkeeping.
    pub(super) fn threads(&self) -> &Mutex<Threads> {
        &self.inner.threads
    }

    /// Kill the child and refuse restarts; a runtime built for a test is
    /// closed when the test is done with it.
    pub fn close(&self) {
        let _ = &self.inner.server;
        todo!("27c")
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
