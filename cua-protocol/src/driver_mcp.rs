//! Wire mapping for the Cua Driver MCP surface (0.28.x).
//!
//! Pure functions, no transport: a [`CuaAction`] becomes the `tools/call` name
//! and arguments the driver's `list-tools` schemas accept, a driver payload
//! becomes a validated [`CuaResponseEnvelope`], and a `health_report` becomes
//! the [`MachineDescriptor`] a target advertises. The server's stdio transport
//! and the desktop runtime share this one copy so both encode and decode
//! identically.

use serde_json::{Map, Value};

use crate::{
    CuaAction, CuaRequestEnvelope, CuaResponseEnvelope, HealthReportResult, MachineDescriptor,
    MachineId, MachineLocation, PermissionState, ValidationError,
};

/// The driver tool that produces a [`HealthReportResult`].
pub const HEALTH_REPORT_TOOL: &str = "health_report";

/// A `tools/call` request for the driver.
#[derive(Clone, Debug, PartialEq)]
pub struct DriverToolCall {
    pub name: &'static str,
    pub arguments: Map<String, Value>,
}

/// Why a driver call produced no payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DriverCallFailure {
    /// The driver could not be reached or the RPC itself failed.
    Transport(String),
    /// The driver answered `isError: true`; `code` is its structured error
    /// code when it sent one.
    Tool {
        code: Option<String>,
        message: String,
    },
    /// The driver answered, but not with a payload this protocol can read.
    Malformed(String),
}

/// The driver tool name and arguments for a validated action.
pub fn tool_call(action: &CuaAction) -> Result<DriverToolCall, ValidationError> {
    let _ = action;
    todo!("slice 2: CuaAction -> driver tool call")
}

/// Decode a driver payload into the response envelope for `request`, applying
/// every size, shape and correlation check of the protocol.
pub fn decode_response(
    request: &CuaRequestEnvelope,
    payload: Value,
) -> Result<CuaResponseEnvelope, ValidationError> {
    let _ = (request, payload);
    todo!("slice 2: driver payload -> CuaResponseEnvelope")
}

/// The error envelope for a driver call that produced no usable payload.
pub fn error_response(
    request: &CuaRequestEnvelope,
    failure: &DriverCallFailure,
) -> CuaResponseEnvelope {
    let _ = (request, failure);
    todo!("slice 2: DriverCallFailure -> CuaResponseEnvelope")
}

/// The response envelope for one driver call: a decoded success, or the typed
/// error when the call failed or its payload could not be decoded.
pub fn response_for(
    request: &CuaRequestEnvelope,
    outcome: Result<Value, DriverCallFailure>,
) -> CuaResponseEnvelope {
    let _ = (request, outcome);
    todo!("slice 2: call outcome -> CuaResponseEnvelope")
}

/// The permissions a health report proves.
pub fn permissions_from_health(report: &HealthReportResult) -> PermissionState {
    let _ = report;
    todo!("slice 2: health checks -> permissions")
}

/// The descriptor a machine advertises given its health report.
pub fn descriptor_from_health(
    machine_id: MachineId,
    location: MachineLocation,
    report: &HealthReportResult,
) -> MachineDescriptor {
    let _ = (machine_id, location, report);
    todo!("slice 2: health report -> MachineDescriptor")
}
