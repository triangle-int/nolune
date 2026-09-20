//! The server-local Cua Driver runtime (#16): the machine the server itself
//! runs on, driven through one persistent `cua-driver mcp` child.
//!
//! This module owns the transport and the descriptor the target advertises.
//! The wire mapping (action to tool call, payload to result, health report to
//! descriptor) lives in `cua_protocol::driver_mcp` so the desktop runtime
//! shares it. Headless detection, per-run sessions and the `[cua]` config
//! section wire this into `AppState` in the lifecycle slice. `install` and
//! `host` back `nolune cua install|status` (#20): the verified install of
//! the pinned driver under the workspace, and what this host can run.

// Wired into `AppState` by the lifecycle slice (#16); until then only tests
// and the `nolune cua` commands construct these.
#[allow(dead_code)]
pub mod discovery;
#[allow(dead_code)]
pub mod driver;
pub mod host;
pub mod install;
#[allow(dead_code)]
pub mod transport;
