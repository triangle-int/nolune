//! The local `codex app-server` child behind the codex provider (#27).
//!
//! Nolune never pretends a ChatGPT login is the OpenAI API: it runs one
//! version-pinned `codex app-server` process and speaks its stdio JSONL
//! protocol. This module owns everything between "is codex installed" and
//! "the answer to request 7 arrived": discovery of the binary and the check
//! that it is exactly the release the protocol fixtures were recorded
//! against, the frames on the wire, and the supervised process with its
//! handshake, request correlation, streamed events, crash restart and
//! shutdown. [`adapter`] is the provider on top of it: one thread per
//! conversation, turns streamed into the provider-neutral events, Nolune's
//! tools bridged through `dynamicTools`; [`runtime`] holds the one process
//! and the thread bookkeeping for the whole gateway. The login routes and
//! the settings tile build on the runtime in the following slices.

pub mod adapter;
pub mod discovery;
pub mod process;
pub mod protocol;
pub mod runtime;

#[cfg(test)]
pub(super) mod fake;

pub use adapter::{CAPABILITIES, CodexAdapter};
#[allow(unused_imports)]
pub use runtime::{AccountState, Runtime};

use std::path::PathBuf;
use std::time::Duration;

use super::contract::LlmError;
use protocol::RpcError;

/// The one codex release this module speaks to. `codex --version` prints
/// `codex-cli <version>`; anything else is refused before a process is
/// started, because the app-server protocol is experimental and the
/// fixtures under `fixtures/codex-<version>.jsonl` were recorded against
/// this release only.
pub const CODEX_VERSION: &str = "0.155.0";

/// The binary name looked up on `PATH`.
pub const CODEX_BINARY: &str = "codex";

/// The environment variable that names the binary outright, ahead of the
/// `PATH` lookup.
pub const CODEX_ENV: &str = "NOLUNE_CODEX_BIN";

/// The arguments that turn the binary into the app-server.
pub const APP_SERVER_ARGS: &[&str] = &["app-server"];

/// Set to let a test start the real binary through the shared runtime; the
/// `#[ignore]` live tests set it, nothing else does.
pub const LIVE_ENV: &str = "NOLUNE_CODEX_LIVE";

/// Why the app-server is not there, or why one exchange with it failed.
#[derive(Clone, Debug, PartialEq)]
pub enum AppServerError {
    /// No `codex` binary where it was looked for.
    NotInstalled,
    /// The binary found is not [`CODEX_VERSION`].
    Incompatible { path: PathBuf, found: String },
    /// The binary found could not say which version it is: it does not
    /// run, `--version` failed or timed out, or printed something else.
    Unusable { path: PathBuf, reason: String },
    /// The child did not complete `initialize` within the handshake
    /// deadline, or exited before it did; the text repeats its stderr.
    Handshake(String),
    /// One request outlived its deadline; the child itself is still there.
    Timeout { method: String, after: Duration },
    /// The child exited, on its own or by a kill, before it answered.
    Exited(String),
    /// The supervisor was shut down: nothing is sent and nothing restarts.
    Closed,
    /// The app-server answered with an error object.
    Rpc(RpcError),
    /// The app-server wrote something that is not a frame of the protocol.
    Protocol(String),
}

impl std::fmt::Display for AppServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotInstalled => write!(
                f,
                "codex is not installed: no `{CODEX_BINARY}` on PATH and {CODEX_ENV} is unset"
            ),
            Self::Incompatible { path, found } => write!(
                f,
                "{} is codex {found}; Nolune supports codex {CODEX_VERSION} only",
                path.display()
            ),
            Self::Unusable { path, reason } => {
                write!(f, "{} cannot report its version: {reason}", path.display())
            }
            Self::Handshake(reason) => write!(f, "codex app-server handshake failed: {reason}"),
            Self::Timeout { method, after } => {
                write!(
                    f,
                    "codex app-server did not answer {method} within {after:?}"
                )
            }
            Self::Exited(reason) => write!(f, "{reason}"),
            Self::Closed => write!(f, "codex app-server was shut down"),
            Self::Rpc(error) => write!(
                f,
                "codex app-server error {}: {}",
                error.code, error.message
            ),
            Self::Protocol(reason) => write!(f, "codex app-server spoke out of protocol: {reason}"),
        }
    }
}

impl std::error::Error for AppServerError {}

/// The provider-neutral reading of each failure: a missing or wrong binary
/// is setup the user has to do, a dead or silent child is the transport,
/// an unreadable frame is an invalid response.
impl From<AppServerError> for LlmError {
    fn from(error: AppServerError) -> Self {
        match error {
            AppServerError::NotInstalled
            | AppServerError::Incompatible { .. }
            | AppServerError::Unusable { .. } => LlmError::SetupRequired(error.to_string()),
            AppServerError::Timeout { .. } => LlmError::Timeout,
            AppServerError::Protocol(_) => LlmError::InvalidResponse(error.to_string()),
            AppServerError::Handshake(_)
            | AppServerError::Exited(_)
            | AppServerError::Closed
            | AppServerError::Rpc(_) => LlmError::Transport(error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pin_is_one_semantic_version() {
        let parts: Vec<u32> = CODEX_VERSION
            .split('.')
            .map(|part| part.parse().expect("numeric"))
            .collect();
        assert_eq!(parts.len(), 3, "{CODEX_VERSION}");
    }

    #[test]
    fn discovery_failures_are_setup_the_user_has_to_do() {
        let errors = [
            AppServerError::NotInstalled,
            AppServerError::Incompatible {
                path: "/opt/bin/codex".into(),
                found: "0.1.0".into(),
            },
            AppServerError::Unusable {
                path: "/opt/bin/codex".into(),
                reason: "exit status 2".into(),
            },
        ];
        for error in errors {
            let text = error.to_string();
            let mapped = LlmError::from(error);
            assert!(
                matches!(&mapped, LlmError::SetupRequired(message) if message == &text),
                "{mapped}"
            );
        }
        let incompatible = AppServerError::Incompatible {
            path: "/opt/bin/codex".into(),
            found: "0.1.0".into(),
        }
        .to_string();
        assert!(incompatible.contains("0.1.0"), "{incompatible}");
        assert!(incompatible.contains(CODEX_VERSION), "{incompatible}");
        assert!(incompatible.contains("/opt/bin/codex"), "{incompatible}");
        let missing = AppServerError::NotInstalled.to_string();
        assert!(missing.contains(CODEX_ENV), "{missing}");
    }

    #[test]
    fn process_failures_are_transport_timeout_or_invalid_response() {
        assert!(matches!(
            LlmError::from(AppServerError::Exited("gone".into())),
            LlmError::Transport(_)
        ));
        assert!(matches!(
            LlmError::from(AppServerError::Handshake("silent".into())),
            LlmError::Transport(_)
        ));
        assert!(matches!(
            LlmError::from(AppServerError::Closed),
            LlmError::Transport(_)
        ));
        assert!(matches!(
            LlmError::from(AppServerError::Rpc(RpcError {
                code: -32600,
                message: "Not initialized".into(),
                data: None,
            })),
            LlmError::Transport(message) if message.contains("-32600") && message.contains("Not initialized")
        ));
        assert!(matches!(
            LlmError::from(AppServerError::Timeout {
                method: "turn/start".into(),
                after: Duration::from_secs(1),
            }),
            LlmError::Timeout
        ));
        assert!(matches!(
            LlmError::from(AppServerError::Protocol("not json".into())),
            LlmError::InvalidResponse(_)
        ));
    }
}
