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
//! frames with its own driver. `orchestrator` is the loop policy the typed
//! machine tools (#18) enforce on either kind: the snapshot ledger, the
//! verification gate and background-only delivery.

/// The macOS `CuaDriver.app` daemon `cua-driver mcp` proxies to, shared
/// with the desktop app so both bring it up by path.
pub use cua_protocol::cua_driver_daemon as daemon;
pub mod desktop;
pub mod discovery;
pub mod driver;
pub mod host;
/// The verified install of the pinned driver, shared with the desktop app
/// so both put the same driver under a workspace the same way.
pub use cua_protocol::cua_driver_install as install;
pub mod orchestrator;
pub mod runtime;
pub mod session;
pub mod transport;
