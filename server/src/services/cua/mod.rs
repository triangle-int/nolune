//! The server-local Cua Driver runtime (#16): the machine the server itself
//! runs on, driven through one persistent `cua-driver mcp` child.
//!
//! This module owns the transport and the descriptor the target advertises.
//! The wire mapping (action to tool call, payload to result, health report to
//! descriptor) lives in `cua_protocol::driver_mcp` so the desktop runtime
//! shares it. Headless detection, per-run sessions and the `[cua]` config
//! section wire this into `AppState` in the lifecycle slice.

// Wired into `AppState` by the lifecycle slice (#16); until then only tests
// construct these.
#[allow(dead_code)]
pub mod discovery;
#[allow(dead_code)]
pub mod driver;
#[allow(dead_code)]
pub mod transport;
