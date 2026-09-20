//! Wire mapping for the Cua Driver MCP surface (0.28.x).
//!
//! Pure functions, no transport: a [`CuaAction`] becomes the `tools/call` name
//! and arguments the driver's `list-tools` schemas accept, a driver payload
//! becomes a validated [`CuaResponseEnvelope`], and a `health_report` becomes
//! the [`MachineDescriptor`] a target advertises. The server's stdio transport
//! and the desktop runtime share this one copy so both encode and decode
//! identically.
//!
//! Every driver schema rejects unknown properties, so each action emits
//! exactly the keys its tool accepts and nothing for an unset option.

use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::{
    BoundedText, Capability, CuaAction, CuaRequestEnvelope, CuaResponse, CuaResponseEnvelope,
    CuaRuntimeError, ElementAddress, ElementCondition, ElementRef, HealthCheckStatus,
    HealthOverall, HealthPlatform, HealthReportResult, MachineDescriptor, MachineHealth, MachineId,
    MachineLocation, Permission, PermissionKind, PermissionState, Platform, RuntimeErrorCode,
    SessionLabel, ValidationError, VerifyPredicate, WindowTarget,
};

/// The driver tool that produces a [`HealthReportResult`].
pub const HEALTH_REPORT_TOOL: &str = "health_report";

/// Longest runtime error message forwarded from the driver.
const MAX_ERROR_MESSAGE_BYTES: usize = 1_024;

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
    /// The driver did not answer within the transport's deadline and the
    /// request was cancelled; the driver itself is still there.
    Timeout(String),
    /// The driver answered `isError: true`; `code` is its structured error
    /// code when it sent one.
    Tool {
        code: Option<String>,
        message: String,
    },
    /// The driver answered, but not with a payload this protocol can read.
    Malformed(String),
}

/// Arguments under construction: only set options are ever inserted.
struct Arguments(Map<String, Value>);

impl Arguments {
    fn new() -> Self {
        Self(Map::new())
    }

    fn set<T: Serialize>(mut self, key: &str, value: &T) -> Self {
        self.0.insert(
            key.to_owned(),
            serde_json::to_value(value).expect("protocol types serialize"),
        );
        self
    }

    fn set_if<T: Serialize>(self, key: &str, value: Option<&T>) -> Self {
        match value {
            Some(value) => self.set(key, value),
            None => self,
        }
    }

    fn set_unless_empty<T: Serialize>(self, key: &str, values: &[T]) -> Self {
        if values.is_empty() {
            self
        } else {
            self.set(key, &values)
        }
    }

    /// Flat `pid` and `window_id`, the form every window tool but
    /// `move_cursor` takes.
    fn window(self, target: WindowTarget) -> Self {
        self.set("pid", &target.pid)
            .set("window_id", &target.window_id)
    }

    fn session(self, session: Option<&SessionLabel>) -> Self {
        self.set_if("session", session)
    }

    /// An element address flattened the way the driver spells it: the token,
    /// the index plus its snapshot, or window-local pixel coordinates.
    fn address(self, address: &ElementAddress) -> Self {
        match address {
            ElementAddress::ElementToken { element_token } => {
                self.set("element_token", element_token)
            }
            ElementAddress::ElementIndex {
                element_index,
                snapshot_id,
            } => self
                .set("element_index", element_index)
                .set("snapshot_id", snapshot_id),
            ElementAddress::Point(point) => self.set("x", &point.x).set("y", &point.y),
        }
    }

    fn element(self, element: &ElementRef) -> Self {
        match element {
            ElementRef::ElementToken { element_token } => self.set("element_token", element_token),
            ElementRef::ElementIndex {
                element_index,
                snapshot_id,
            } => self
                .set("element_index", element_index)
                .set("snapshot_id", snapshot_id),
        }
    }

    fn finish(self, name: &'static str) -> DriverToolCall {
        DriverToolCall {
            name,
            arguments: self.0,
        }
    }
}

/// One `expect` entry of `verify_state`, in the driver's shape.
fn predicate(predicate: &VerifyPredicate) -> Value {
    match predicate {
        VerifyPredicate::WindowExists(exists) => json!({"window": {"exists": exists}}),
        VerifyPredicate::WindowBounds {
            bounds,
            tolerance_px,
        } => json!({"window": {"bounds": {
            "x": bounds.x, "y": bounds.y, "width": bounds.width, "height": bounds.height,
            "tolerance_px": tolerance_px,
        }}}),
        VerifyPredicate::Element(element) => {
            let mut selector = Map::new();
            if let Some(role) = &element.selector.role {
                selector.insert("role".into(), json!(role));
            }
            if let Some(label) = &element.selector.label_contains {
                selector.insert("label_contains".into(), json!(label));
            }
            let (key, value) = match &element.condition {
                ElementCondition::Exists => ("exists", json!(true)),
                ElementCondition::Enabled(enabled) => ("enabled", json!(enabled)),
                ElementCondition::Selected(selected) => ("selected", json!(selected)),
                ElementCondition::ValueEquals(value) => ("value_equals", json!(value)),
            };
            json!({"element": {"selector": selector, key: value}})
        }
    }
}

/// The driver tool name and arguments for a validated action.
pub fn tool_call(action: &CuaAction) -> Result<DriverToolCall, ValidationError> {
    action.validate()?;
    let args = Arguments::new();
    let call = match action {
        CuaAction::ListApps(_) => args.finish("list_apps"),
        CuaAction::LaunchApp(a) => args
            .set_if("bundle_id", a.bundle_id.as_ref())
            .set_if("name", a.name.as_ref())
            .set(
                "creates_new_application_instance",
                &a.creates_new_application_instance,
            )
            .finish("launch_app"),
        CuaAction::ListWindows(a) => args
            .set_if("pid", a.pid.as_ref())
            .set("on_screen_only", &a.on_screen_only)
            .finish("list_windows"),
        CuaAction::GetWindowState(a) => args
            .window(a.target)
            .session(a.session.as_ref())
            .set("include_accessibility_tree", &a.include_accessibility_tree)
            .set("include_screenshot", &a.include_screenshot)
            .set_if("max_elements", a.max_elements.as_ref())
            .set_if("max_depth", a.max_depth.as_ref())
            .set_if("max_dimension", a.max_dimension.as_ref())
            .set_if("query", a.query.as_ref())
            .finish("get_window_state"),
        CuaAction::SetWindowFrame(a) => args
            .window(a.target)
            .session(a.session.as_ref())
            .set("x", &a.frame.x)
            .set("y", &a.frame.y)
            .set("width", &a.frame.width)
            .set("height", &a.frame.height)
            .finish("set_window_frame"),
        CuaAction::Click(a) => args
            .window(a.target)
            .session(a.session.as_ref())
            .set("delivery_mode", &a.delivery_mode)
            .address(&a.address)
            .set("button", &a.button)
            .set("action", &a.action)
            .set_unless_empty("modifier", &a.modifiers)
            .set_if("count", a.count.as_ref())
            .finish("click"),
        CuaAction::DoubleClick(a) => args
            .window(a.target)
            .session(a.session.as_ref())
            .set("delivery_mode", &a.delivery_mode)
            .address(&a.address)
            .finish("double_click"),
        CuaAction::RightClick(a) => args
            .window(a.target)
            .session(a.session.as_ref())
            .set("delivery_mode", &a.delivery_mode)
            .address(&a.address)
            .set_unless_empty("modifier", &a.modifiers)
            .finish("right_click"),
        // `move_cursor` has no flat pid/window_id; the window is its `target`.
        CuaAction::MoveCursor(a) => args
            .set(
                "target",
                &json!({"kind": "window", "pid": a.target.pid, "window_id": a.target.window_id}),
            )
            .session(a.session.as_ref())
            .set("x", &a.point.x)
            .set("y", &a.point.y)
            .finish("move_cursor"),
        CuaAction::Drag(a) => args
            .window(a.target)
            .session(a.session.as_ref())
            .set("delivery_mode", &a.delivery_mode)
            .set("from_x", &a.from.x)
            .set("from_y", &a.from.y)
            .set("to_x", &a.to.x)
            .set("to_y", &a.to.y)
            .set("duration_ms", &a.duration_ms)
            .set("steps", &a.steps)
            .set("button", &a.button)
            .set_unless_empty("modifier", &a.modifiers)
            .finish("drag"),
        CuaAction::Scroll(a) => args
            .window(a.target)
            .session(a.session.as_ref())
            .set("delivery_mode", &a.delivery_mode)
            .address(&a.address)
            .set("direction", &a.direction)
            .set("by", &a.by)
            .set("amount", &a.amount)
            .finish("scroll"),
        CuaAction::TypeText(a) => args
            .window(a.target)
            .session(a.session.as_ref())
            .set("delivery_mode", &a.delivery_mode)
            .address(&a.address)
            .set("text", &a.text)
            .set("delay_ms", &a.delay_ms)
            .finish("type_text"),
        // Unlike click and drag, press_key spells its modifier list `modifiers`.
        CuaAction::PressKey(a) => args
            .window(a.target)
            .session(a.session.as_ref())
            .set("delivery_mode", &a.delivery_mode)
            .address(&a.address)
            .set("key", &a.key)
            .set_unless_empty("modifiers", &a.modifiers)
            .finish("press_key"),
        CuaAction::Hotkey(a) => args
            .window(a.target)
            .session(a.session.as_ref())
            .set("delivery_mode", &a.delivery_mode)
            .address(&a.address)
            .set("keys", &a.keys)
            .finish("hotkey"),
        CuaAction::SetValue(a) => args
            .window(a.target)
            .session(a.session.as_ref())
            .element(&a.element)
            .set("value", &a.value)
            .finish("set_value"),
        CuaAction::InvokeMenu(a) => args
            .window(a.target)
            .session(a.session.as_ref())
            .set("path", &a.path)
            .finish("invoke_menu"),
        CuaAction::VerifyState(a) => args
            .window(a.target)
            .session(a.session.as_ref())
            .set(
                "expect",
                &a.expect.iter().map(predicate).collect::<Vec<_>>(),
            )
            .set("include_screenshot", &a.include_screenshot)
            .set("stable_samples", &a.stable_samples)
            .set("timeout_ms", &a.timeout_ms)
            .finish("verify_state"),
        CuaAction::StartSession(a) => args.session(a.session.as_ref()).finish("start_session"),
        CuaAction::GetSession(a) => args.session(a.session.as_ref()).finish("get_session"),
        CuaAction::ListSessions(a) => args
            .set_if("cursor", a.cursor.as_ref())
            .set_if("limit", a.limit.as_ref())
            .finish("list_sessions"),
        CuaAction::EndSession(a) => args.session(a.session.as_ref()).finish("end_session"),
        // An empty `include` would read as "run nothing", so unset filters
        // are omitted rather than sent empty.
        CuaAction::HealthReport(a) => args
            .set_unless_empty("include", &a.include)
            .set_unless_empty("skip", &a.skip)
            .finish(HEALTH_REPORT_TOOL),
    };
    Ok(call)
}

/// Decode a driver payload into the response envelope for `request`, applying
/// every size, shape and correlation check of the protocol.
///
/// The payload is wrapped into the envelope the protocol already knows how to
/// bound and validate, so a driver answer passes through exactly the checks a
/// remote adapter's answer would.
pub fn decode_response(
    request: &CuaRequestEnvelope,
    payload: Value,
) -> Result<CuaResponseEnvelope, ValidationError> {
    let kind = request.action.kind();
    let envelope = json!({
        "version": request.version,
        "request_id": request.request_id,
        "machine_id": request.machine_id,
        "action": kind,
        "response": {
            "status": "success",
            "result": {"action": kind, "result": payload},
        },
    });
    let envelope = CuaResponseEnvelope::from_json(&envelope.to_string())?;
    envelope.validate_response_for(request)?;
    Ok(envelope)
}

/// The runtime error code a driver error code or message text names, if any.
fn tool_error_code(text: &str) -> Option<(RuntimeErrorCode, bool)> {
    let text = text.to_ascii_lowercase();
    let mentions = |needles: &[&str]| needles.iter().any(|needle| text.contains(needle));
    if mentions(&["stale"]) {
        Some((RuntimeErrorCode::StaleSnapshot, false))
    } else if mentions(&["permission", "tcc", "not trusted", "not granted"]) {
        Some((RuntimeErrorCode::PermissionDenied, false))
    } else if mentions(&["session"]) {
        Some((RuntimeErrorCode::SessionUnavailable, false))
    } else if mentions(&["not_found", "not found", "not a live window", "no such"]) {
        Some((RuntimeErrorCode::TargetUnavailable, false))
    } else if mentions(&["timeout", "timed_out", "timed out"]) {
        Some((RuntimeErrorCode::Timeout, true))
    } else {
        None
    }
}

/// The runtime error code for a driver failure and whether retrying can help.
///
/// The driver's structured `code` is authoritative when it names a class; the
/// free text only decides when there is no code or the code is unknown (a
/// `window_id_not_found` message also says the id may be "stale").
fn classify(failure: &DriverCallFailure) -> (RuntimeErrorCode, bool) {
    match failure {
        DriverCallFailure::Transport(_) => (RuntimeErrorCode::RuntimeUnavailable, true),
        DriverCallFailure::Timeout(_) => (RuntimeErrorCode::Timeout, true),
        DriverCallFailure::Malformed(_) => (RuntimeErrorCode::DriverFailure, false),
        DriverCallFailure::Tool { code, message } => code
            .as_deref()
            .and_then(tool_error_code)
            .or_else(|| tool_error_code(message))
            .unwrap_or((RuntimeErrorCode::DriverFailure, false)),
    }
}

/// A driver message as bounded protocol text: control characters dropped,
/// length capped, never empty.
fn bounded_message(raw: &str) -> BoundedText {
    let mut message: String = raw
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
        .collect();
    if message.len() > MAX_ERROR_MESSAGE_BYTES {
        let mut end = MAX_ERROR_MESSAGE_BYTES;
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        message.truncate(end);
    }
    if message.is_empty() {
        message.push_str("driver returned no message");
    }
    BoundedText::try_from(message).expect("filtered text within bounds is valid")
}

/// The error envelope for a driver call that produced no usable payload.
pub fn error_response(
    request: &CuaRequestEnvelope,
    failure: &DriverCallFailure,
) -> CuaResponseEnvelope {
    let (code, retryable) = classify(failure);
    let raw = match failure {
        DriverCallFailure::Transport(message)
        | DriverCallFailure::Timeout(message)
        | DriverCallFailure::Tool { message, .. }
        | DriverCallFailure::Malformed(message) => message,
    };
    CuaResponseEnvelope {
        version: request.version,
        request_id: request.request_id.clone(),
        machine_id: request.machine_id.clone(),
        action: request.action.kind(),
        response: CuaResponse::Error {
            error: CuaRuntimeError {
                code,
                message: bounded_message(raw),
                retryable,
            },
        },
    }
}

/// The response envelope for one driver call: a decoded success, or the typed
/// error when the call failed or its payload could not be decoded.
pub fn response_for(
    request: &CuaRequestEnvelope,
    outcome: Result<Value, DriverCallFailure>,
) -> CuaResponseEnvelope {
    let failure = match outcome {
        Ok(payload) => match decode_response(request, payload) {
            Ok(envelope) => return envelope,
            Err(error) => DriverCallFailure::Malformed(error.to_string()),
        },
        Err(failure) => failure,
    };
    error_response(request, &failure)
}

fn check_status(report: &HealthReportResult, name: &str) -> Option<HealthCheckStatus> {
    report
        .checks
        .iter()
        .find(|check| check.name.as_str() == name)
        .map(|check| check.status)
}

/// What a sequence of checks proves about one permission: the first check
/// that actually ran decides; a check that was skipped or is absent proves
/// nothing, and when none ran the grant is unknown, so a prompt is required.
fn permission_from_checks(report: &HealthReportResult, checks: &[&str]) -> Permission {
    for name in checks {
        match check_status(report, name) {
            Some(HealthCheckStatus::Pass) => return Permission::Granted,
            Some(HealthCheckStatus::Fail) => return Permission::Denied,
            Some(HealthCheckStatus::Skip) | None => {}
        }
    }
    Permission::PromptRequired
}

/// The permissions a health report proves.
///
/// On macOS the TCC checks carry the driver's own bundle identity, so they
/// come first; the capability probes stand in when they were skipped. Other
/// platforms only have the capability probes.
pub fn permissions_from_health(report: &HealthReportResult) -> PermissionState {
    let (accessibility, screen_capture): (&[&str], &[&str]) = match report.platform {
        HealthPlatform::Darwin => (
            &["tcc_accessibility", "ax_capability"],
            &["tcc_screen_recording", "screen_capture_capability"],
        ),
        HealthPlatform::Win32 | HealthPlatform::Linux => {
            (&["ax_capability"], &["screen_capture_capability"])
        }
    };
    PermissionState {
        accessibility: permission_from_checks(report, accessibility),
        screen_capture: permission_from_checks(report, screen_capture),
    }
}

/// The capabilities a target with these permissions can honour: everything
/// the protocol names, minus what a permission that is not granted would make
/// `authorize` refuse anyway.
fn capabilities_for(permissions: &PermissionState) -> Vec<Capability> {
    let granted = |kind: PermissionKind| match kind {
        PermissionKind::Accessibility => permissions.accessibility == Permission::Granted,
        PermissionKind::ScreenCapture => permissions.screen_capture == Permission::Granted,
    };
    let any_modality =
        granted(PermissionKind::Accessibility) || granted(PermissionKind::ScreenCapture);
    [
        Capability::AppDiscovery,
        Capability::AppLaunch,
        Capability::WindowDiscovery,
        Capability::WindowObservation,
        Capability::WindowManagement,
        Capability::Pointer,
        Capability::Keyboard,
        Capability::ElementValue,
        Capability::Menu,
        Capability::Verification,
        Capability::SessionLifecycle,
        Capability::Health,
    ]
    .into_iter()
    .filter(|capability| match capability {
        Capability::WindowObservation => any_modality,
        Capability::WindowManagement
        | Capability::Pointer
        | Capability::Keyboard
        | Capability::ElementValue
        | Capability::Menu
        | Capability::Verification => granted(PermissionKind::Accessibility),
        Capability::AppDiscovery
        | Capability::AppLaunch
        | Capability::WindowDiscovery
        | Capability::SessionLifecycle
        | Capability::Health => true,
    })
    .collect()
}

/// The descriptor a machine advertises given its health report.
///
/// A failed core check makes the machine unavailable; a degraded report, or
/// an ok report whose permissions are not all granted, makes it degraded.
pub fn descriptor_from_health(
    machine_id: MachineId,
    location: MachineLocation,
    report: &HealthReportResult,
) -> MachineDescriptor {
    let permissions = permissions_from_health(report);
    let fully_granted = permissions.accessibility == Permission::Granted
        && permissions.screen_capture == Permission::Granted;
    let health = match report.overall {
        HealthOverall::Failed => MachineHealth::Unavailable,
        HealthOverall::Degraded => MachineHealth::Degraded,
        HealthOverall::Ok if fully_granted => MachineHealth::Healthy,
        HealthOverall::Ok => MachineHealth::Degraded,
    };
    let platform = match report.platform {
        HealthPlatform::Darwin => Platform::Macos,
        HealthPlatform::Win32 => Platform::Windows,
        HealthPlatform::Linux => Platform::Linux,
    };
    MachineDescriptor {
        machine_id,
        location,
        platform,
        driver_version: report.driver_version.clone(),
        health,
        capabilities: capabilities_for(&permissions),
        permissions,
    }
}
