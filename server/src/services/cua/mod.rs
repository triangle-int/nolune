//! The server-local Cua Driver runtime (#16): the machine the server itself
//! runs on, driven through one persistent `cua-driver mcp` child.
//!
//! This module owns the transport and the descriptor the target advertises,
//! the host probe that decides whether a GUI target can exist here, and the
//! runtime that registers it and keeps per-run sessions bounded. The wire
//! mapping (action to tool call, payload to result, health report to
//! descriptor) lives in `cua_protocol::driver_mcp` so the desktop runtime
//! shares it. Headless detection, per-run sessions and the `[cua]` config
//! section wire this into `AppState`. `install`, `daemon` and `host` also
//! back `nolune cua install|status` (#20): the verified install of the
//! pinned driver under the workspace, its macOS daemon, and what this host
//! can run. `desktop` is the other kind of target (#17): a desktop app that
//! registers a descriptor over the machine WebSocket and answers typed
//! frames with its own driver.

pub mod daemon;
pub mod desktop;
pub mod discovery;
pub mod driver;
pub mod host;
pub mod install;
pub mod runtime;
pub mod session;
pub mod transport;
