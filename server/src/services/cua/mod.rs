//! The server-local Cua Driver runtime (#16): the machine the server itself
//! runs on, driven through one persistent `cua-driver mcp` child.
//!
//! This module owns the transport and the descriptor the target advertises,
//! the host probe that decides whether a GUI target can exist here, and the
//! runtime that registers it and keeps per-run sessions bounded. The wire
//! mapping (action to tool call, payload to result, health report to
//! descriptor) lives in `cua_protocol::driver_mcp` so the desktop runtime
//! shares it.

pub mod discovery;
pub mod driver;
pub mod host;
pub mod runtime;
pub mod session;
pub mod transport;
