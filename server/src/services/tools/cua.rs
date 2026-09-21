//! The typed machine tools (#18): `discover_windows`, `get_window_state`,
//! `act` and `verify_state`, the Cua-native surface the model drives any
//! Cua target through, server-local (#16) or desktop (#17).
//!
//! Every call resolves the computer the user chose for the conversation
//! (#80) to a registered Cua target, never a pick between several, and
//! runs through one `Orchestrator` per turn: its snapshot ledger fails
//! closed on stale element tokens, its verification gate reports an action
//! only when it is verified, delivery is always background, and a driver's
//! foreground recommendation is quoted back, never obeyed. The coordinate
//! `computer_use` tool stays beside these for legacy desktops until #19.

use std::{fmt, sync::Arc};

use cua_protocol::{
    CuaAction, CuaActionResult, GetWindowStateArgs as ProtocolWindowStateArgs, MachineDescriptor,
    MachineHealth, MachineId, MachineLocation, VerificationResult, VerifyPredicate,
    VerifyStateArgs as ProtocolVerifyArgs, WindowStateResult, WindowTarget,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::services::cua::orchestrator::{
    ActReport, Failure, Orchestrator, PixelPolicy, Target, VerifySpec,
};
use crate::services::machine_registry::MachineRegistry;
use crate::services::tool::{Tool, ToolDefinition};
use crate::services::tools::computer::{
    MachineTarget, SERVER_HOME_TARGET, TargetRefusal, TargetSelection,
};
use crate::services::tools::{ToolExecError, openai_schema};

/// How many accessibility elements one `get_window_state` result shows the
/// model; the rest is counted and reachable through `query`.
pub const MAX_RENDERED_ELEMENTS: usize = 60;

/// The driver's verification bounds when the model gives none.
const DEFAULT_VERIFY_TIMEOUT_MS: u16 = 2_000;
const DEFAULT_STABLE_SAMPLES: u8 = 1;

// ═══════════════════════════════════════════════════════════════════════════
// Target resolution
// ═══════════════════════════════════════════════════════════════════════════

/// Why a typed tool did not act. The shared refusals are #80's; the rest
/// name what a Cua target needs that the chosen computer lacks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CuaRefusal {
    Shared(TargetRefusal),
    /// No machine with a Cua driver is registered at all.
    NoTargets,
    /// The user chose the server home and it has no server-local target.
    NoServerLocalTarget,
    /// The chosen desktop is connected but registered no Cua driver.
    NoCuaDriver {
        label: String,
    },
    /// The target's driver reports the machine unavailable.
    DriverUnavailable {
        label: String,
    },
}

impl CuaRefusal {
    /// Stable code the message starts with.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Shared(refusal) => refusal.code(),
            Self::NoTargets => "no_cua_target",
            Self::NoServerLocalTarget => "no_server_local_target",
            Self::NoCuaDriver { .. } => "no_cua_driver",
            Self::DriverUnavailable { .. } => "driver_unavailable",
        }
    }
}

impl fmt::Display for CuaRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Shared(refusal) => refusal.fmt(f),
            Self::NoTargets => write!(
                f,
                "{}: no computer with a Cua driver is available; the server machine registers \
                 one when a driver is installed there (see docs/computer-use.md) and a desktop \
                 registers one when its Nolune app runs the driver",
                self.code()
            ),
            Self::NoServerLocalTarget => write!(
                f,
                "{}: the user chose the server home, which has no computer-use driver right now; \
                 run_command and the file tools still act there, or ask the user to choose a \
                 desktop in the composer",
                self.code()
            ),
            Self::NoCuaDriver { label } => write!(
                f,
                "{}: {label} is connected without a Cua driver, so the typed tools cannot drive \
                 it; computer_use, remote_bash and remote_files still work there",
                self.code()
            ),
            Self::DriverUnavailable { label } => write!(
                f,
                "{}: the driver on {label} reports the machine unavailable; ask the user to run \
                 `nolune cua status` there or check the Computers page",
                self.code()
            ),
        }
    }
}

impl From<TargetRefusal> for CuaRefusal {
    fn from(refusal: TargetRefusal) -> Self {
        Self::Shared(refusal)
    }
}

impl From<CuaRefusal> for ToolExecError {
    fn from(refusal: CuaRefusal) -> Self {
        ToolExecError(refusal.to_string())
    }
}

/// The target one call acts on.
pub struct Resolved {
    pub target: Target,
    /// The user's name for the machine, else its hostname, else its id.
    pub label: String,
}

/// What the four tools share: the registry the targets live in, the
/// conversation's choice, and the turn's orchestrator.
#[derive(Clone)]
pub struct CuaTools {
    registry: MachineRegistry,
    target: MachineTarget,
    orchestrator: Arc<Orchestrator>,
}

impl CuaTools {
    pub fn new(registry: MachineRegistry, target: MachineTarget) -> Self {
        Self {
            registry,
            target,
            orchestrator: Arc::new(Orchestrator::new()),
        }
    }

    pub fn orchestrator(&self) -> &Orchestrator {
        &self.orchestrator
    }

    /// The Cua target the user's choice names, checked against what the
    /// model asked for. The chosen desktop or the server home is used
    /// exactly; with nothing chosen, the only registered target is used and
    /// several are refused. Never a pick, never a fallback.
    pub async fn resolve(&self, requested: Option<&str>) -> Result<Resolved, CuaRefusal> {
        let _ = requested;
        todo!("slice 1: resolve the chosen computer to a Cua target")
    }

    fn failure(&self, label: &str, failure: Failure) -> ToolExecError {
        ToolExecError(format!("{failure} (on {label})"))
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Arguments
// ═══════════════════════════════════════════════════════════════════════════

/// The exact window an observation or action is about.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
pub struct WindowTargetArgs {
    /// Process id, from discover_windows.
    pub pid: u32,
    /// Window id, from discover_windows.
    pub window_id: u64,
}

impl From<WindowTargetArgs> for WindowTarget {
    fn from(args: WindowTargetArgs) -> Self {
        Self {
            pid: args.pid,
            window_id: args.window_id,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiscoverMode {
    /// Every running app with its windows.
    ListApps,
    /// Windows, optionally of one process.
    ListWindows,
    /// Launch an app by bundle id or name and list its windows.
    LaunchApp,
}

#[derive(Deserialize, JsonSchema)]
pub struct DiscoverWindowsArgs {
    /// ID of the machine (from list_machines). Omit to act on the computer
    /// the user chose for this conversation.
    #[serde(default)]
    pub machine_id: Option<String>,
    pub mode: DiscoverMode,
    /// list_windows: only this process's windows.
    #[serde(default)]
    pub pid: Option<u32>,
    /// list_windows: only windows on screen (default true).
    #[serde(default)]
    pub on_screen_only: Option<bool>,
    /// launch_app: the app's bundle id (exactly one of bundle_id and name).
    #[serde(default)]
    pub bundle_id: Option<String>,
    /// launch_app: the app's display name (exactly one of bundle_id and name).
    #[serde(default)]
    pub name: Option<String>,
    /// launch_app: start another instance even when the app runs (default false).
    #[serde(default)]
    pub creates_new_application_instance: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub struct WindowStateArgs {
    /// ID of the machine (from list_machines). Omit to act on the computer
    /// the user chose for this conversation.
    #[serde(default)]
    pub machine_id: Option<String>,
    pub target: WindowTargetArgs,
    /// Walk the accessibility tree and issue element tokens (default true).
    #[serde(default)]
    pub include_accessibility_tree: Option<bool>,
    /// Also capture a one-shot screenshot of the window (default false; the
    /// image is not shown to you yet, its dimensions are).
    #[serde(default)]
    pub include_screenshot: Option<bool>,
    /// Cap on the elements the driver walks (1-2000).
    #[serde(default)]
    pub max_elements: Option<u16>,
    /// Cap on the tree depth the driver walks (1-64).
    #[serde(default)]
    pub max_depth: Option<u8>,
    /// Case-insensitive filter: only matching elements and their ancestors.
    #[serde(default)]
    pub query: Option<String>,
}

/// Where an action lands inside the window.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AddressArgs {
    /// An element token from the latest get_window_state of this window
    /// (preferred: exact, works on background windows).
    ElementToken { element_token: String },
    /// An element index with the snapshot id that issued it.
    ElementIndex {
        element_index: u32,
        snapshot_id: String,
    },
    /// Window-local pixel coordinates read from the latest screenshot.
    /// Refused unless accessibility is unavailable for the window or the
    /// last verification there failed.
    Point { x: f64, y: f64 },
}

/// An element by token or index, for set_value.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ElementRefArgs {
    ElementToken {
        element_token: String,
    },
    ElementIndex {
        element_index: u32,
        snapshot_id: String,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
pub struct PointArgs {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ButtonArg {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClickActionArg {
    Press,
    ShowMenu,
    Pick,
    Confirm,
    Cancel,
    Open,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScrollDirectionArg {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScrollByArg {
    Line,
    Page,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModifierArg {
    Cmd,
    Shift,
    Option,
    Ctrl,
    Fn,
}

/// The only delivery Nolune performs: no window is fronted, no focus is
/// stolen. A driver that recommends foreground control is reported, not
/// obeyed.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryModeArg {
    Background,
}

/// The typed actions; every one is delivered in the background.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActionArgs {
    /// Click an element (accessibility action) or a point.
    Click {
        address: AddressArgs,
        /// left (default) or right; middle has no background route.
        #[serde(default)]
        button: Option<ButtonArg>,
        /// The accessibility action: press (default), show_menu, pick,
        /// confirm, cancel, open. Point addresses press only.
        #[serde(default)]
        action: Option<ClickActionArg>,
        /// Click count, point addresses only (1-3).
        #[serde(default)]
        count: Option<u8>,
    },
    DoubleClick {
        address: AddressArgs,
    },
    RightClick {
        address: AddressArgs,
    },
    /// A pixel drag; needs the pixel route like a point address.
    Drag {
        from: PointArgs,
        to: PointArgs,
        /// 0-10000 (default 300).
        #[serde(default)]
        duration_ms: Option<u16>,
        /// 1-200 (default 20).
        #[serde(default)]
        steps: Option<u8>,
        #[serde(default)]
        button: Option<ButtonArg>,
    },
    Scroll {
        address: AddressArgs,
        direction: ScrollDirectionArg,
        /// line (default) or page.
        #[serde(default)]
        by: Option<ScrollByArg>,
        /// 1-50 (default 3).
        #[serde(default)]
        amount: Option<u8>,
    },
    TypeText {
        address: AddressArgs,
        text: String,
        /// Delay between keystrokes, 0-200 ms (default 0).
        #[serde(default)]
        delay_ms: Option<u8>,
    },
    /// One key: a letter or digit, return, tab, escape, up, down, left,
    /// right, space, delete, home, end, pageup, pagedown, f1-f12.
    PressKey {
        address: AddressArgs,
        key: String,
        #[serde(default)]
        modifiers: Option<Vec<ModifierArg>>,
    },
    /// A chord: modifiers first (cmd, shift, option, ctrl, fn), one key last.
    Hotkey {
        address: AddressArgs,
        keys: Vec<String>,
    },
    /// Set an element's value directly (text fields, sliders).
    SetValue {
        element: ElementRefArgs,
        value: String,
    },
    /// Invoke a menu-bar item by its path, e.g. ["File", "Save"].
    InvokeMenu {
        path: Vec<String>,
    },
}

/// The value a condition compares against.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(untagged)]
pub enum ExpectedArg {
    Flag(bool),
    Text(String),
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConditionArg {
    /// The element is present.
    Exists,
    /// enabled equals `expected` (a boolean).
    Enabled,
    /// selected equals `expected` (a boolean).
    Selected,
    /// The element's value equals `expected` (a string).
    ValueEquals,
}

/// What must hold after an action.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(tag = "predicate", rename_all = "snake_case")]
pub enum PredicateArgs {
    /// The window exists (true) or is gone (false).
    WindowExists { value: bool },
    /// The window's bounds, within tolerance_px (default 2).
    WindowBounds {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        #[serde(default)]
        tolerance_px: Option<f64>,
    },
    /// An element selected by role and/or label satisfies the condition.
    Element {
        #[serde(default)]
        role: Option<String>,
        #[serde(default)]
        label_contains: Option<String>,
        condition: ConditionArg,
        #[serde(default)]
        expected: Option<ExpectedArg>,
    },
}

/// Predicates the orchestrator verifies right after the action.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
pub struct VerifyArgs {
    pub expect: Vec<PredicateArgs>,
    /// How long the driver waits for the predicates, 0-10000 ms (default 2000).
    #[serde(default)]
    pub timeout_ms: Option<u16>,
    /// Consecutive samples that must agree, 1-5 (default 1).
    #[serde(default)]
    pub stable_samples: Option<u8>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ActArgs {
    /// ID of the machine (from list_machines). Omit to act on the computer
    /// the user chose for this conversation.
    #[serde(default)]
    pub machine_id: Option<String>,
    pub target: WindowTargetArgs,
    pub action: ActionArgs,
    /// Always background; the only value accepted.
    #[serde(default)]
    pub delivery_mode: Option<DeliveryModeArg>,
    /// Predicates to verify after the action. Without them the action
    /// succeeds only when the driver read its effect back itself.
    #[serde(default)]
    pub verify: Option<VerifyArgs>,
}

#[derive(Deserialize, JsonSchema)]
pub struct VerifyWindowArgs {
    /// ID of the machine (from list_machines). Omit to act on the computer
    /// the user chose for this conversation.
    #[serde(default)]
    pub machine_id: Option<String>,
    pub target: WindowTargetArgs,
    pub expect: Vec<PredicateArgs>,
    /// How long the driver waits for the predicates, 0-10000 ms (default 2000).
    #[serde(default)]
    pub timeout_ms: Option<u16>,
    /// Consecutive samples that must agree, 1-5 (default 1).
    #[serde(default)]
    pub stable_samples: Option<u8>,
}

// ═══════════════════════════════════════════════════════════════════════════
// Conversion to the protocol
// ═══════════════════════════════════════════════════════════════════════════

/// The discovery action the arguments describe, validated by the protocol.
pub fn discovery_action(args: &DiscoverWindowsArgs) -> Result<CuaAction, String> {
    let _ = args;
    todo!("slice 1: discovery arguments")
}

/// The observation the arguments describe, validated by the protocol.
pub fn window_state_action(args: &WindowStateArgs) -> Result<ProtocolWindowStateArgs, String> {
    let _ = args;
    todo!("slice 1: window state arguments")
}

/// The typed action the arguments describe: the target and background
/// delivery filled in, defaults applied, validated by the protocol.
pub fn typed_action(target: WindowTargetArgs, action: &ActionArgs) -> Result<CuaAction, String> {
    let _ = (target, action);
    todo!("slice 1: action arguments")
}

/// The predicates as the protocol carries them.
pub fn predicates(expect: &[PredicateArgs]) -> Result<Vec<VerifyPredicate>, String> {
    let _ = expect;
    todo!("slice 2: predicate arguments")
}

/// The verification the `act` arguments ask for, validated by the protocol.
pub fn verify_spec(target: WindowTargetArgs, verify: &VerifyArgs) -> Result<VerifySpec, String> {
    let _ = (target, verify);
    todo!("slice 2: verification arguments")
}

/// The standalone verification the arguments describe.
pub fn verify_action(args: &VerifyWindowArgs) -> Result<ProtocolVerifyArgs, String> {
    let _ = args;
    todo!("slice 2: verify_state arguments")
}

// ═══════════════════════════════════════════════════════════════════════════
// Rendering
// ═══════════════════════════════════════════════════════════════════════════

/// A discovery result as the model reads it.
pub fn render_discovery(result: &CuaActionResult, label: &str) -> serde_json::Value {
    let _ = (result, label);
    todo!("slice 1: render discovery")
}

/// A window state as the model reads it: the snapshot id, a bounded list
/// of elements with their tokens, the driver's flags, the pixel policy for
/// the window, and a foreground recommendation reported as not applied.
/// Never the screenshot bytes.
pub fn render_window_state(
    state: &WindowStateResult,
    policy: &PixelPolicy,
    label: &str,
) -> serde_json::Value {
    let _ = (state, policy, label);
    todo!("slice 1: render the window state")
}

/// A verified action as the model reads it.
pub fn render_act(report: &ActReport, label: &str) -> serde_json::Value {
    let _ = (report, label);
    todo!("slice 2: render the act report")
}

/// A verification as the model reads it.
pub fn render_verification(verification: &VerificationResult, label: &str) -> serde_json::Value {
    let _ = (verification, label);
    todo!("slice 2: render the verification")
}

// ═══════════════════════════════════════════════════════════════════════════
// Tools
// ═══════════════════════════════════════════════════════════════════════════

pub struct DiscoverWindowsTool(CuaTools);

impl DiscoverWindowsTool {
    pub fn new(shared: CuaTools) -> Self {
        Self(shared)
    }
}

impl Tool for DiscoverWindowsTool {
    const NAME: &'static str = "discover_windows";
    type Error = ToolExecError;
    type Args = DiscoverWindowsArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.into(),
            description: "Find the exact app and window to work in on a computer with a Cua \
                driver (list_machines shows driver_version for those). mode list_apps lists \
                running apps with their windows, list_windows lists windows (optionally of one \
                pid), launch_app starts an app by bundle_id or name and returns its windows. \
                Every window has a pid and window_id: pass both as target to get_window_state \
                before acting in it. Acts on the computer the user chose for this conversation \
                (or the only one with a driver); with several and none chosen it refuses and \
                you must ask the user to choose."
                .into(),
            parameters: openai_schema::<DiscoverWindowsArgs>(),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let _ = args;
        todo!("slice 1: discover_windows")
    }
}

pub struct GetWindowStateTool(CuaTools);

impl GetWindowStateTool {
    pub fn new(shared: CuaTools) -> Self {
        Self(shared)
    }
}

impl Tool for GetWindowStateTool {
    const NAME: &'static str = "get_window_state";
    type Error = ToolExecError;
    type Args = WindowStateArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.into(),
            description: "Observe one window (pid + window_id from discover_windows) before \
                acting in it: returns the snapshot_id and the accessibility elements with their \
                element_token, role, label, value and frame. Call it before every act on that \
                window: element tokens are valid only from the latest snapshot of their window \
                and only until the next action there; anything older is refused. Use query to \
                narrow large trees. pixel_addresses says whether act may use a point address \
                (only when accessibility is unavailable for the window or the last verification \
                there failed). The screenshot, when requested, is captured one-shot and its \
                dimensions reported; the image is not shown to you yet."
                .into(),
            parameters: openai_schema::<WindowStateArgs>(),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let _ = args;
        todo!("slice 1: get_window_state")
    }
}

pub struct ActTool(CuaTools);

impl ActTool {
    pub fn new(shared: CuaTools) -> Self {
        Self(shared)
    }
}

impl Tool for ActTool {
    const NAME: &'static str = "act";
    type Error = ToolExecError;
    type Args = ActArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.into(),
            description: "Perform one typed action in a window: click, double_click, right_click, \
                drag, scroll, type_text, press_key, hotkey, set_value or invoke_menu. Address \
                elements by element_token from the latest get_window_state of that window \
                (preferred) or element_index + snapshot_id; a point address (window-local pixels \
                from the latest screenshot) is refused unless accessibility is unavailable there \
                or the last verification failed. Delivery is always background: nothing is \
                fronted or focused, and a driver that recommends foreground control is refused, \
                not escalated. Give verify.expect predicates for what must hold afterwards; the \
                action is verified right away and reported as success only when they are \
                satisfied. Without predicates it succeeds only when the driver read the effect \
                back itself; refused, unverifiable, suspected_noop and partial outcomes are \
                errors. After an action, call get_window_state again before the next one."
                .into(),
            parameters: openai_schema::<ActArgs>(),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let _ = args;
        todo!("slice 2: act")
    }
}

pub struct VerifyStateTool(CuaTools);

impl VerifyStateTool {
    pub fn new(shared: CuaTools) -> Self {
        Self(shared)
    }
}

impl Tool for VerifyStateTool {
    const NAME: &'static str = "verify_state";
    type Error = ToolExecError;
    type Args = VerifyWindowArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.into(),
            description: "Check predicates against a window without acting: window_exists, \
                window_bounds, or an element (by role and/or label_contains) that exists, is \
                enabled/selected, or has a value. Satisfied is success; unsatisfied or unknown \
                is an error, and after one a point address is allowed on that window. Use it to \
                confirm a state before deciding the next step; act verifies its own action when \
                given verify.expect."
                .into(),
            parameters: openai_schema::<VerifyWindowArgs>(),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let _ = args;
        todo!("slice 2: verify_state")
    }
}

// Names the stubs leave unused until the implementation lands.
#[allow(dead_code)]
fn _uses(
    _: &MachineDescriptor,
    _: MachineHealth,
    _: &MachineId,
    _: MachineLocation,
    _: &str,
) -> &'static str {
    SERVER_HOME_TARGET
}
#[allow(dead_code)]
fn _uses_selection(_: &TargetSelection) {}

#[cfg(test)]
mod schema_tests {
    //! #18: the tool schemas expose the structured Cua surface: exact
    //! window targets, the three address kinds, background-only delivery
    //! and the verification predicates.
    use super::*;
    use serde_json::Value;

    /// The string literals one property schema admits: an `enum` list, a
    /// `const`, or the `const` of each `oneOf` branch (documented variants
    /// render that way).
    fn literals(property: &Value) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(constant) = property.get("const").and_then(Value::as_str) {
            out.push(constant.to_owned());
        }
        if let Some(values) = property.get("enum").and_then(Value::as_array) {
            out.extend(values.iter().filter_map(Value::as_str).map(str::to_owned));
        }
        if let Some(branches) = property.get("oneOf").and_then(Value::as_array) {
            for branch in branches {
                if let Some(constant) = branch.get("const").and_then(Value::as_str) {
                    out.push(constant.to_owned());
                }
            }
        }
        out
    }

    /// Every string literal a property named `name` may take, anywhere in
    /// the schema (the tagged enums render as `oneOf` branches with a
    /// `const` tag).
    fn values_of(schema: &Value, name: &str) -> Vec<String> {
        fn walk(value: &Value, name: &str, out: &mut Vec<String>) {
            match value {
                Value::Object(object) => {
                    if let Some(property) = object.get("properties").and_then(|p| p.get(name)) {
                        out.extend(literals(property));
                    }
                    for child in object.values() {
                        walk(child, name, out);
                    }
                }
                Value::Array(items) => {
                    for item in items {
                        walk(item, name, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        walk(schema, name, &mut out);
        out.sort();
        out.dedup();
        out
    }

    fn property<'a>(schema: &'a Value, path: &[&str]) -> &'a Value {
        let mut at = schema;
        for name in path {
            at = at
                .get("properties")
                .and_then(|p| p.get(name))
                .unwrap_or_else(|| panic!("{path:?} is in the schema: {schema}"));
        }
        at
    }

    fn required(schema: &Value) -> Vec<String> {
        schema["required"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn the_act_schema_exposes_target_addresses_background_delivery_and_predicates() {
        let schema = openai_schema::<ActArgs>();
        assert!(
            schema.get("$defs").is_none(),
            "the schema is self-contained for every provider: {schema}"
        );

        let target = property(&schema, &["target"]);
        assert_eq!(target["properties"]["pid"]["type"], "integer");
        assert_eq!(target["properties"]["window_id"]["type"], "integer");
        assert_eq!(required(target), ["pid", "window_id"]);
        assert_eq!(required(&schema), ["target", "action"]);

        assert_eq!(
            values_of(&schema, "kind"),
            [
                "click",
                "double_click",
                "drag",
                "element_index",
                "element_token",
                "hotkey",
                "invoke_menu",
                "point",
                "press_key",
                "right_click",
                "scroll",
                "set_value",
                "type_text",
            ]
        );
        let text = schema.to_string();
        for field in [
            "\"element_token\"",
            "\"element_index\"",
            "\"snapshot_id\"",
            "\"x\"",
            "\"y\"",
        ] {
            assert!(text.contains(field), "address fields carry {field}: {text}");
        }

        let delivery = property(&schema, &["delivery_mode"]);
        assert_eq!(
            literals(delivery),
            ["background"],
            "delivery is background only: {delivery}"
        );
        assert!(
            !text.contains("foreground"),
            "no foreground delivery: {text}"
        );

        assert_eq!(
            values_of(&schema, "predicate"),
            ["element", "window_bounds", "window_exists"]
        );
        assert_eq!(
            values_of(&schema, "condition"),
            ["enabled", "exists", "selected", "value_equals"]
        );
        let verify = property(&schema, &["verify"]);
        assert!(
            verify.to_string().contains("\"expect\""),
            "verify carries the predicates: {verify}"
        );
        assert_eq!(values_of(&schema, "button"), ["left", "right"]);
        assert_eq!(
            values_of(&schema, "modifiers"),
            Vec::<String>::new(),
            "modifiers are an array"
        );
        assert!(text.contains("\"cmd\"") && text.contains("\"ctrl\""));
    }

    #[test]
    fn the_observation_discovery_and_verification_schemas_name_their_surface() {
        let state = openai_schema::<WindowStateArgs>();
        let target = property(&state, &["target"]);
        assert_eq!(required(target), ["pid", "window_id"]);
        for field in [
            "include_accessibility_tree",
            "include_screenshot",
            "max_elements",
            "max_depth",
            "query",
        ] {
            property(&state, &[field]);
        }
        assert_eq!(required(&state), ["target"]);

        let discover = openai_schema::<DiscoverWindowsArgs>();
        assert_eq!(
            values_of(&discover, "mode"),
            ["launch_app", "list_apps", "list_windows"]
        );
        for field in ["pid", "on_screen_only", "bundle_id", "name"] {
            property(&discover, &[field]);
        }
        assert_eq!(required(&discover), ["mode"]);

        let verify = openai_schema::<VerifyWindowArgs>();
        assert_eq!(
            required(property(&verify, &["target"])),
            ["pid", "window_id"]
        );
        assert_eq!(
            values_of(&verify, "predicate"),
            ["element", "window_bounds", "window_exists"]
        );
        assert_eq!(required(&verify), ["target", "expect"]);
    }

    #[test]
    fn every_typed_tool_takes_the_machine_optionally() {
        for (name, schema) in [
            ("discover_windows", openai_schema::<DiscoverWindowsArgs>()),
            ("get_window_state", openai_schema::<WindowStateArgs>()),
            ("act", openai_schema::<ActArgs>()),
            ("verify_state", openai_schema::<VerifyWindowArgs>()),
        ] {
            let machine = property(&schema, &["machine_id"]);
            assert!(
                machine["type"]
                    .as_array()
                    .is_some_and(|types| types.contains(&Value::from("string"))),
                "{name}: machine_id is a string: {machine}"
            );
            assert!(
                !required(&schema).iter().any(|field| field == "machine_id"),
                "{name}: the chosen computer is the default"
            );
        }
    }

    #[tokio::test]
    async fn the_tool_definitions_state_the_loop() {
        let shared = CuaTools::new(
            MachineRegistry::new(),
            MachineTarget::new(TargetSelection::Unselected),
        );
        let discover = DiscoverWindowsTool::new(shared.clone())
            .definition(String::new())
            .await;
        assert_eq!(discover.name, "discover_windows");
        assert!(discover.description.contains("get_window_state"));
        let state = GetWindowStateTool::new(shared.clone())
            .definition(String::new())
            .await;
        assert_eq!(state.name, "get_window_state");
        assert!(state.description.contains("element_token"));
        let act = ActTool::new(shared.clone()).definition(String::new()).await;
        assert_eq!(act.name, "act");
        for rule in [
            "background",
            "foreground",
            "verify",
            "get_window_state again",
        ] {
            assert!(act.description.contains(rule), "act states {rule:?}");
        }
        let verify = VerifyStateTool::new(shared).definition(String::new()).await;
        assert_eq!(verify.name, "verify_state");
        assert!(verify.description.contains("unsatisfied"));
    }
}

#[cfg(test)]
mod conversion_tests {
    use super::*;
    use cua_protocol::{
        ClickAction, DeliveryMode, ElementAddress, ElementCondition, MouseButton, ScrollDirection,
        ScrollGranularity,
    };

    fn target() -> WindowTargetArgs {
        WindowTargetArgs {
            pid: 42,
            window_id: 99,
        }
    }

    fn token(value: &str) -> AddressArgs {
        AddressArgs::ElementToken {
            element_token: value.into(),
        }
    }

    #[test]
    fn actions_convert_with_background_delivery_and_defaults() {
        let click = typed_action(
            target(),
            &ActionArgs::Click {
                address: token("tok/1"),
                button: None,
                action: None,
                count: None,
            },
        )
        .unwrap();
        let CuaAction::Click(args) = click else {
            panic!("a click");
        };
        assert_eq!(args.target.pid, 42);
        assert_eq!(args.target.window_id, 99);
        assert_eq!(args.delivery_mode, DeliveryMode::Background);
        assert_eq!(args.session, None);
        assert_eq!(args.button, MouseButton::Left);
        assert_eq!(args.action, ClickAction::Press);
        assert!(args.modifiers.is_empty());
        assert_eq!(args.count, None);
        assert!(matches!(args.address, ElementAddress::ElementToken { .. }));

        let scroll = typed_action(
            target(),
            &ActionArgs::Scroll {
                address: AddressArgs::Point { x: 1.0, y: 2.0 },
                direction: ScrollDirectionArg::Down,
                by: None,
                amount: None,
            },
        )
        .unwrap();
        let CuaAction::Scroll(args) = scroll else {
            panic!("a scroll");
        };
        assert_eq!(args.direction, ScrollDirection::Down);
        assert_eq!(args.by, ScrollGranularity::Line);
        assert_eq!(args.amount, 3);
        assert!(matches!(args.address, ElementAddress::Point(_)));

        let typed = typed_action(
            target(),
            &ActionArgs::TypeText {
                address: AddressArgs::ElementIndex {
                    element_index: 3,
                    snapshot_id: "s0000001".into(),
                },
                text: "hello".into(),
                delay_ms: None,
            },
        )
        .unwrap();
        let CuaAction::TypeText(args) = typed else {
            panic!("a type_text");
        };
        assert_eq!(args.text.as_str(), "hello");
        assert_eq!(args.delay_ms, 0);

        let key = typed_action(
            target(),
            &ActionArgs::PressKey {
                address: token("tok/1"),
                key: "return".into(),
                modifiers: Some(vec![ModifierArg::Cmd, ModifierArg::Shift]),
            },
        )
        .unwrap();
        let CuaAction::PressKey(args) = key else {
            panic!("a press_key");
        };
        assert_eq!(args.key.as_str(), "return");
        assert_eq!(args.modifiers.len(), 2);

        let hotkey = typed_action(
            target(),
            &ActionArgs::Hotkey {
                address: token("tok/1"),
                keys: vec!["cmd".into(), "s".into()],
            },
        )
        .unwrap();
        assert!(matches!(hotkey, CuaAction::Hotkey(_)));

        let value = typed_action(
            target(),
            &ActionArgs::SetValue {
                element: ElementRefArgs::ElementToken {
                    element_token: "tok/1".into(),
                },
                value: String::new(),
            },
        )
        .unwrap();
        assert!(matches!(value, CuaAction::SetValue(_)));

        let menu = typed_action(
            target(),
            &ActionArgs::InvokeMenu {
                path: vec!["File".into(), "Save".into()],
            },
        )
        .unwrap();
        let CuaAction::InvokeMenu(args) = menu else {
            panic!("an invoke_menu");
        };
        assert_eq!(args.path.len(), 2);

        let drag = typed_action(
            target(),
            &ActionArgs::Drag {
                from: PointArgs { x: 1.0, y: 1.0 },
                to: PointArgs { x: 9.0, y: 9.0 },
                duration_ms: None,
                steps: None,
                button: None,
            },
        )
        .unwrap();
        let CuaAction::Drag(args) = drag else {
            panic!("a drag");
        };
        assert_eq!(args.duration_ms, 300);
        assert_eq!(args.steps, 20);
        assert!(args.modifiers.is_empty());
    }

    #[test]
    fn the_protocol_refuses_what_it_cannot_deliver_in_the_background() {
        // A hotkey needs modifiers first and one key last.
        let error = typed_action(
            target(),
            &ActionArgs::Hotkey {
                address: token("tok/1"),
                keys: vec!["s".into()],
            },
        )
        .unwrap_err();
        assert!(error.contains("hotkey"), "{error}");
        // A click count is pixel-only.
        let error = typed_action(
            target(),
            &ActionArgs::Click {
                address: token("tok/1"),
                button: None,
                action: None,
                count: Some(2),
            },
        )
        .unwrap_err();
        assert!(error.contains("count"), "{error}");
        // A zero window id is no window.
        let error = typed_action(
            WindowTargetArgs {
                pid: 42,
                window_id: 0,
            },
            &ActionArgs::DoubleClick {
                address: token("tok/1"),
            },
        )
        .unwrap_err();
        assert!(error.contains("window_id"), "{error}");
        // An unknown key name.
        let error = typed_action(
            target(),
            &ActionArgs::PressKey {
                address: token("tok/1"),
                key: "enter-key".into(),
                modifiers: None,
            },
        )
        .unwrap_err();
        assert!(error.to_lowercase().contains("key"), "{error}");
    }

    #[test]
    fn predicates_and_verification_bounds_convert() {
        let expect = vec![
            PredicateArgs::WindowExists { value: true },
            PredicateArgs::WindowBounds {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 600.0,
                tolerance_px: None,
            },
            PredicateArgs::Element {
                role: Some("AXTextField".into()),
                label_contains: Some("Name".into()),
                condition: ConditionArg::ValueEquals,
                expected: Some(ExpectedArg::Text("hello".into())),
            },
            PredicateArgs::Element {
                role: None,
                label_contains: Some("Save".into()),
                condition: ConditionArg::Enabled,
                expected: Some(ExpectedArg::Flag(true)),
            },
            PredicateArgs::Element {
                role: Some("AXButton".into()),
                label_contains: None,
                condition: ConditionArg::Exists,
                expected: None,
            },
        ];
        let converted = predicates(&expect).unwrap();
        assert_eq!(converted.len(), 5);
        assert_eq!(converted[0], VerifyPredicate::WindowExists(true));
        let VerifyPredicate::WindowBounds { tolerance_px, .. } = &converted[1] else {
            panic!("bounds");
        };
        assert_eq!(*tolerance_px, 2.0);
        let VerifyPredicate::Element(element) = &converted[2] else {
            panic!("element");
        };
        assert!(matches!(
            element.condition,
            ElementCondition::ValueEquals(ref value) if value.as_str() == "hello"
        ));
        let VerifyPredicate::Element(element) = &converted[3] else {
            panic!("element");
        };
        assert_eq!(element.condition, ElementCondition::Enabled(true));
        let VerifyPredicate::Element(element) = &converted[4] else {
            panic!("element");
        };
        assert_eq!(element.condition, ElementCondition::Exists);

        let spec = verify_spec(
            target(),
            &VerifyArgs {
                expect: vec![PredicateArgs::WindowExists { value: true }],
                timeout_ms: None,
                stable_samples: None,
            },
        )
        .unwrap();
        assert_eq!(spec.timeout_ms, DEFAULT_VERIFY_TIMEOUT_MS);
        assert_eq!(spec.stable_samples, DEFAULT_STABLE_SAMPLES);

        // A condition without its value, an empty selector, no predicates,
        // and timing out of range are refused before anything is sent.
        let error = predicates(&[PredicateArgs::Element {
            role: Some("AXButton".into()),
            label_contains: None,
            condition: ConditionArg::Enabled,
            expected: None,
        }])
        .unwrap_err();
        assert!(error.contains("expected"), "{error}");
        let error = predicates(&[PredicateArgs::Element {
            role: None,
            label_contains: None,
            condition: ConditionArg::Exists,
            expected: None,
        }])
        .unwrap_err();
        assert!(error.contains("selector"), "{error}");
        let error = verify_spec(
            target(),
            &VerifyArgs {
                expect: vec![],
                timeout_ms: None,
                stable_samples: None,
            },
        )
        .unwrap_err();
        assert!(error.contains("predicate"), "{error}");
        let error = verify_spec(
            target(),
            &VerifyArgs {
                expect: vec![PredicateArgs::WindowExists { value: true }],
                timeout_ms: Some(60_000),
                stable_samples: None,
            },
        )
        .unwrap_err();
        assert!(error.contains("timing"), "{error}");
    }

    #[test]
    fn observation_and_discovery_arguments_convert() {
        let state = window_state_action(&WindowStateArgs {
            machine_id: None,
            target: target(),
            include_accessibility_tree: None,
            include_screenshot: None,
            max_elements: None,
            max_depth: None,
            query: Some("Save".into()),
        })
        .unwrap();
        assert!(state.include_accessibility_tree, "the tree is the default");
        assert!(!state.include_screenshot, "the screenshot is opt-in");
        assert_eq!(state.query.as_ref().map(|q| q.as_str()), Some("Save"));
        let error = window_state_action(&WindowStateArgs {
            machine_id: None,
            target: target(),
            include_accessibility_tree: Some(false),
            include_screenshot: Some(false),
            max_elements: None,
            max_depth: None,
            query: None,
        })
        .unwrap_err();
        assert!(error.contains("modality"), "{error}");

        let apps = discovery_action(&DiscoverWindowsArgs {
            machine_id: None,
            mode: DiscoverMode::ListApps,
            pid: None,
            on_screen_only: None,
            bundle_id: None,
            name: None,
            creates_new_application_instance: None,
        })
        .unwrap();
        assert!(matches!(apps, CuaAction::ListApps(_)));
        let windows = discovery_action(&DiscoverWindowsArgs {
            machine_id: None,
            mode: DiscoverMode::ListWindows,
            pid: Some(42),
            on_screen_only: None,
            bundle_id: None,
            name: None,
            creates_new_application_instance: None,
        })
        .unwrap();
        let CuaAction::ListWindows(args) = windows else {
            panic!("list_windows");
        };
        assert_eq!(args.pid.map(|pid| pid.get()), Some(42));
        assert!(args.on_screen_only);
        let launch = discovery_action(&DiscoverWindowsArgs {
            machine_id: None,
            mode: DiscoverMode::LaunchApp,
            pid: None,
            on_screen_only: None,
            bundle_id: Some("com.apple.Notes".into()),
            name: None,
            creates_new_application_instance: None,
        })
        .unwrap();
        let CuaAction::LaunchApp(args) = launch else {
            panic!("launch_app");
        };
        assert!(!args.creates_new_application_instance);
        let error = discovery_action(&DiscoverWindowsArgs {
            machine_id: None,
            mode: DiscoverMode::LaunchApp,
            pid: None,
            on_screen_only: None,
            bundle_id: None,
            name: None,
            creates_new_application_instance: None,
        })
        .unwrap_err();
        assert!(error.contains("selector"), "{error}");
        let error = discovery_action(&DiscoverWindowsArgs {
            machine_id: None,
            mode: DiscoverMode::ListWindows,
            pid: Some(0),
            on_screen_only: None,
            bundle_id: None,
            name: None,
            creates_new_application_instance: None,
        })
        .unwrap_err();
        assert!(error.contains("pid"), "{error}");
    }
}

#[cfg(test)]
mod tool_tests {
    //! #18 on top of #80: the typed tools act on the computer the user
    //! chose, resolved to its Cua target, and render what the orchestrator
    //! reports without ever passing screenshot bytes to the model.
    use super::*;
    use crate::services::cua::{runtime::CuaRuntime, transport::fake::FakeTransport};
    use crate::services::machine_registry::MachineInfo;
    use cua_protocol::{
        Capability, CheckedCuaAdapter, CuaRequestEnvelope, CuaResponseEnvelope, DriverVersion,
        Permission, PermissionState, Platform,
        driver_mcp::{DriverCallFailure, response_for},
    };
    use serde_json::{Value, json};
    use std::{collections::VecDeque, sync::Mutex};

    const STUDIO: &str = "server-local:studio";
    const LAPTOP: &str = "9a8b7c6d-5e4f-4a3b-9c2d-1e0f9a8b7c6d";
    const TABLET: &str = "4f3c1c2e-9b5e-4d2b-8f0a-1c2d3e4f5a6b";

    const HEALTHY: &str = r#"{
        "schema_version":"1","platform":"darwin","driver_version":"0.28.2","overall":"ok",
        "checks":[
            {"name":"binary_version","status":"pass","message":"cua-driver 0.28.2"},
            {"name":"platform_supported","status":"pass","message":"macOS 27.0 (arm64)","data":{"architecture":"arm64","os_version":"27.0"}},
            {"name":"session_active","status":"pass","message":"MCP session is active."},
            {"name":"bundle_identity","status":"pass","message":"Bundle is com.trycua.driver.","data":{"bundle_identifier":"com.trycua.driver","executable_path":"/Applications/CuaDriver.app/Contents/MacOS/cua-driver","identity_source":"current_process"}},
            {"name":"tcc_accessibility","status":"pass","message":"Accessibility is granted.","data":{"bundle_identifier":"com.trycua.driver"}},
            {"name":"tcc_screen_recording","status":"pass","message":"Screen Recording is granted.","data":{"bundle_identifier":"com.trycua.driver"}},
            {"name":"ax_capability","status":"pass","message":"AX is trusted and reachable."},
            {"name":"screen_capture_capability","status":"skip","message":"Direct capture was not probed."}
        ]
    }"#;

    fn descriptor(machine_id: &str, location: MachineLocation) -> MachineDescriptor {
        MachineDescriptor {
            machine_id: MachineId::try_from(machine_id).unwrap(),
            location,
            platform: Platform::Macos,
            driver_version: DriverVersion::try_from("0.28.2").unwrap(),
            health: MachineHealth::Healthy,
            permissions: PermissionState {
                accessibility: Permission::Granted,
                screen_capture: Permission::Granted,
            },
            capabilities: vec![
                Capability::AppDiscovery,
                Capability::AppLaunch,
                Capability::WindowDiscovery,
                Capability::WindowObservation,
                Capability::Pointer,
                Capability::Keyboard,
                Capability::ElementValue,
                Capability::Menu,
                Capability::Verification,
                Capability::SessionLifecycle,
                Capability::Health,
            ],
        }
    }

    type Log = Arc<Mutex<Vec<CuaRequestEnvelope>>>;

    /// A desktop Cua target answering from a script of raw driver
    /// payloads (an action result echoes the request with the given
    /// outcome under `"outcome"`).
    fn scripted(descriptor: MachineDescriptor, script: Vec<Value>) -> (CheckedCuaAdapter, Log) {
        let script = Arc::new(Mutex::new(VecDeque::from(script)));
        let log: Log = Arc::new(Mutex::new(Vec::new()));
        let seen = log.clone();
        let adapter = CheckedCuaAdapter::new(descriptor, move |request| {
            seen.lock().unwrap().push(request.clone());
            let next = script.lock().unwrap().pop_front();
            Box::pin(async move { answer(&request, next) })
        })
        .unwrap();
        (adapter, log)
    }

    fn answer(request: &CuaRequestEnvelope, payload: Option<Value>) -> CuaResponseEnvelope {
        let outcome = match payload {
            Some(mut payload) => {
                if let Some(outcome) = payload.get("outcome").cloned()
                    && payload.as_object().is_some_and(|o| o.len() == 1)
                {
                    let mut value = serde_json::to_value(&request.action).unwrap();
                    let mut args = value["args"].take();
                    let object = args.as_object_mut().unwrap();
                    object.remove("delivery_mode");
                    object.remove("modifiers");
                    object.insert("outcome".into(), outcome);
                    payload = args;
                }
                Ok(payload)
            }
            None => Err(DriverCallFailure::Transport("no scripted answer".into())),
        };
        response_for(request, outcome)
    }

    fn legacy(machine_id: &str, hostname: &str) -> MachineInfo {
        MachineInfo {
            machine_id: machine_id.into(),
            os: "macos".into(),
            hostname: hostname.into(),
            screen_width: 1440,
            screen_height: 900,
            last_seen: chrono::Utc::now().timestamp(),
            instance_slug: None,
            platform: Some(Platform::Macos),
            location: MachineLocation::Desktop,
            permissions: None,
            capabilities: Vec::new(),
        }
    }

    fn state(snapshot: &str, elements: Vec<Value>) -> Value {
        json!({
            "target": {"pid": 42, "window_id": 99},
            "snapshot_id": snapshot,
            "elements": elements,
            "truncated": false,
            "app_name": "Notes",
            "window_title": "Untitled",
            "element_count": elements.len(),
        })
    }

    fn element(index: u32, token: &str, role: &str, label: &str) -> Value {
        json!({
            "element_index": index, "element_token": token, "role": role, "label": label,
            "value": "", "actions": ["AXPress"], "enabled": true,
            "frame": {"x": 10.0, "y": 20.0, "width": 80.0, "height": 24.0}, "depth": 0
        })
    }

    fn confirmed() -> Value {
        json!({"outcome": {
            "effect": "confirmed", "route": "accessibility",
            "delivery": {"requested": "background", "delivered_count": 1},
            "evidence": ["accessibility_readback"],
        }})
    }

    fn window() -> WindowTargetArgs {
        WindowTargetArgs {
            pid: 42,
            window_id: 99,
        }
    }

    fn discover(machine_id: Option<&str>) -> DiscoverWindowsArgs {
        DiscoverWindowsArgs {
            machine_id: machine_id.map(str::to_owned),
            mode: DiscoverMode::ListApps,
            pid: None,
            on_screen_only: None,
            bundle_id: None,
            name: None,
            creates_new_application_instance: None,
        }
    }

    fn observe(machine_id: Option<&str>) -> WindowStateArgs {
        WindowStateArgs {
            machine_id: machine_id.map(str::to_owned),
            target: window(),
            include_accessibility_tree: None,
            include_screenshot: None,
            max_elements: None,
            max_depth: None,
            query: None,
        }
    }

    fn click(machine_id: Option<&str>, token: &str) -> ActArgs {
        ActArgs {
            machine_id: machine_id.map(str::to_owned),
            target: window(),
            action: ActionArgs::Click {
                address: AddressArgs::ElementToken {
                    element_token: token.into(),
                },
                button: None,
                action: None,
                count: None,
            },
            delivery_mode: None,
            verify: None,
        }
    }

    fn verify(machine_id: Option<&str>) -> VerifyWindowArgs {
        VerifyWindowArgs {
            machine_id: machine_id.map(str::to_owned),
            target: window(),
            expect: vec![PredicateArgs::WindowExists { value: true }],
            timeout_ms: None,
            stable_samples: None,
        }
    }

    struct Tools {
        discover: DiscoverWindowsTool,
        state: GetWindowStateTool,
        act: ActTool,
        verify: VerifyStateTool,
    }

    async fn tools_for(registry: &MachineRegistry, chosen: Option<&str>) -> Tools {
        let shared = CuaTools::new(
            registry.clone(),
            MachineTarget::resolve(registry, chosen).await,
        );
        Tools {
            discover: DiscoverWindowsTool::new(shared.clone()),
            state: GetWindowStateTool::new(shared.clone()),
            act: ActTool::new(shared.clone()),
            verify: VerifyStateTool::new(shared),
        }
    }

    /// Every typed tool refuses with the same code and text.
    async fn every_tool_refuses(tools: &Tools, machine_id: Option<&str>, code: &str) -> String {
        let discover = tools.discover.call(discover(machine_id)).await.unwrap_err();
        let state = tools.state.call(observe(machine_id)).await.unwrap_err();
        let act = tools
            .act
            .call(click(machine_id, "tok/a"))
            .await
            .unwrap_err();
        let verify = tools.verify.call(verify(machine_id)).await.unwrap_err();
        for error in [&discover, &state, &act, &verify] {
            assert!(
                error.0.starts_with(&format!("{code}: ")),
                "expected a {code} refusal, got {error}"
            );
        }
        assert_eq!(discover.0, state.0);
        assert_eq!(state.0, act.0);
        assert_eq!(act.0, verify.0);
        discover.0
    }

    async fn attach_server_local(registry: &MachineRegistry, answers: Vec<Value>) -> CuaRuntime {
        let mut outcomes = vec![Ok(serde_json::from_str(HEALTHY).unwrap())];
        outcomes.extend(answers.into_iter().map(Ok));
        let transport = Arc::new(FakeTransport::answering(outcomes));
        let runtime = CuaRuntime::new(
            crate::config::CuaConfig::default(),
            registry.cua().clone(),
            std::path::PathBuf::new(),
        );
        runtime
            .attach(
                transport,
                MachineId::try_from(STUDIO).unwrap(),
                "studio",
                None,
            )
            .await
            .unwrap();
        runtime
    }

    fn started(n: u64) -> Value {
        json!({
            "active": true, "capture_scope": "auto", "effective_scope": "window",
            "desktop_capture_authorized": true, "desktop_unlocked": true,
            "revived": false, "session": format!("nolune-run-{n}")
        })
    }

    fn ended(n: u64) -> Value {
        json!({"session": format!("nolune-run-{n}"), "active": false})
    }

    #[tokio::test]
    async fn two_targets_and_no_choice_ask_the_user_to_choose() {
        let registry = MachineRegistry::new();
        let (laptop, laptop_log) = scripted(descriptor(LAPTOP, MachineLocation::Desktop), vec![]);
        registry.cua().register(laptop).await.unwrap();
        let runtime = attach_server_local(&registry, vec![]).await;
        let (_tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        registry.register(legacy(LAPTOP, "laptop"), _tx).await;
        registry.rename(LAPTOP, Some("Laptop")).await.unwrap();

        let tools = tools_for(&registry, None).await;
        let message = every_tool_refuses(&tools, None, "choose_a_computer").await;
        assert!(
            message.contains("Laptop") && message.contains("studio"),
            "the refusal names both computers: {message}"
        );
        // The model naming one of them is not the user choosing it.
        every_tool_refuses(&tools, Some(LAPTOP), "choose_a_computer").await;
        every_tool_refuses(&tools, Some(STUDIO), "choose_a_computer").await;
        assert!(laptop_log.lock().unwrap().is_empty());
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn the_chosen_desktop_is_used_exactly() {
        let registry = MachineRegistry::new();
        let (laptop, laptop_log) = scripted(
            descriptor(LAPTOP, MachineLocation::Desktop),
            vec![
                json!({"apps": []}),
                state("s0000001", vec![element(0, "tok/a", "AXButton", "Save")]),
                confirmed(),
            ],
        );
        registry.cua().register(laptop).await.unwrap();
        let (tablet, tablet_log) = scripted(descriptor(TABLET, MachineLocation::Desktop), vec![]);
        registry.cua().register(tablet).await.unwrap();
        let (_tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        registry.register(legacy(LAPTOP, "laptop"), _tx).await;
        registry.rename(LAPTOP, Some("Laptop")).await.unwrap();

        let tools = tools_for(&registry, Some(LAPTOP)).await;
        let apps = tools.discover.call(discover(None)).await.unwrap();
        assert!(
            apps.contains("Laptop"),
            "the result names the computer: {apps}"
        );
        let state = tools.state.call(observe(Some(LAPTOP))).await.unwrap();
        assert!(state.contains("s0000001"), "{state}");
        let acted = tools.act.call(click(None, "tok/a")).await.unwrap();
        assert!(
            acted.contains("confirmed") && acted.contains("Laptop"),
            "{acted}"
        );
        assert_eq!(laptop_log.lock().unwrap().len(), 3);

        // Naming another computer is refused; nobody else is asked.
        let message = every_tool_refuses(&tools, Some(TABLET), "target_mismatch").await;
        assert!(message.contains("Laptop"), "{message}");
        every_tool_refuses(&tools, Some(STUDIO), "target_mismatch").await;
        assert!(tablet_log.lock().unwrap().is_empty());
        assert_eq!(laptop_log.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn the_server_home_drives_the_server_local_target_as_runs() {
        let registry = MachineRegistry::new();
        let runtime = attach_server_local(
            &registry,
            vec![
                started(1),
                json!({"apps": []}),
                ended(1),
                started(2),
                state("s0000001", vec![element(0, "tok/a", "AXButton", "Save")]),
                ended(2),
            ],
        )
        .await;
        let (laptop, laptop_log) = scripted(descriptor(LAPTOP, MachineLocation::Desktop), vec![]);
        registry.cua().register(laptop).await.unwrap();

        let home = tools_for(&registry, Some(SERVER_HOME_TARGET)).await;
        let apps = home.discover.call(discover(None)).await.unwrap();
        assert!(apps.contains("studio"), "{apps}");
        // Naming the desktop while the home is chosen is a mismatch.
        every_tool_refuses(&home, Some(LAPTOP), "target_mismatch").await;
        // Choosing the server-local id is the same choice, and the model
        // naming that id is fine.
        let by_id = tools_for(&registry, Some(STUDIO)).await;
        let state = by_id.state.call(observe(Some(STUDIO))).await.unwrap();
        assert!(state.contains("s0000001"), "{state}");
        assert!(laptop_log.lock().unwrap().is_empty());

        // With the home chosen and no driver there, the typed tools say so.
        runtime.shutdown().await;
        let tools = tools_for(&registry, Some(SERVER_HOME_TARGET)).await;
        let message = every_tool_refuses(&tools, None, "no_server_local_target").await;
        assert!(message.contains("run_command"), "{message}");
    }

    #[tokio::test]
    async fn the_only_target_is_used_without_a_choice_and_none_is_reported() {
        let registry = MachineRegistry::new();
        let tools_none = tools_for(&registry, None).await;
        let message = every_tool_refuses(&tools_none, None, "no_cua_target").await;
        assert!(message.contains("driver"), "{message}");

        let (laptop, laptop_log) = scripted(
            descriptor(LAPTOP, MachineLocation::Desktop),
            vec![json!({"apps": []}), json!({"apps": []})],
        );
        registry.cua().register(laptop).await.unwrap();
        let tools = tools_for(&registry, None).await;
        tools.discover.call(discover(None)).await.unwrap();
        tools.discover.call(discover(Some(LAPTOP))).await.unwrap();
        assert_eq!(laptop_log.lock().unwrap().len(), 2);
        // A computer nobody has is unavailable, by its id.
        let message = every_tool_refuses(&tools, Some(TABLET), "machine_unavailable").await;
        assert!(message.contains(TABLET), "{message}");
        assert_eq!(laptop_log.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_desktop_without_a_driver_and_an_unavailable_driver_are_refused_by_name() {
        let registry = MachineRegistry::new();
        let (_tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        registry.register(legacy(LAPTOP, "laptop"), _tx).await;
        registry.rename(LAPTOP, Some("Laptop")).await.unwrap();
        let tools = tools_for(&registry, Some(LAPTOP)).await;
        let message = every_tool_refuses(&tools, None, "no_cua_driver").await;
        assert!(
            message.contains("Laptop") && message.contains("computer_use"),
            "{message}"
        );

        let mut unavailable = descriptor(TABLET, MachineLocation::Desktop);
        unavailable.health = MachineHealth::Unavailable;
        let (tablet, tablet_log) = scripted(unavailable, vec![]);
        registry.cua().register(tablet).await.unwrap();
        let tools = tools_for(&registry, Some(TABLET)).await;
        let message = every_tool_refuses(&tools, None, "driver_unavailable").await;
        assert!(message.contains(TABLET), "{message}");
        assert!(tablet_log.lock().unwrap().is_empty());

        // An offline chosen desktop never falls back to a connected one.
        let (laptop, laptop_log) = scripted(descriptor(LAPTOP, MachineLocation::Desktop), vec![]);
        registry.cua().register(laptop).await.unwrap();
        let tools = tools_for(&registry, Some("never-seen")).await;
        every_tool_refuses(&tools, None, "machine_unavailable").await;
        assert!(laptop_log.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn get_window_state_output_is_compact_and_states_the_pixel_policy() {
        let registry = MachineRegistry::new();
        let many: Vec<Value> = (0..(MAX_RENDERED_ELEMENTS as u32 + 5))
            .map(|i| element(i, &format!("tok/{i}"), "AXButton", &format!("Button {i}")))
            .collect();
        let fixture =
            include_str!("../../../../cua-protocol/tests/fixtures/degraded-window-state.json");
        let mut degraded: Value = serde_json::from_str(fixture).unwrap();
        degraded["target"] = json!({"pid": 42, "window_id": 99});
        degraded["background_input"]["exact_window"]["pid"] = json!(42);
        degraded["background_input"]["exact_window"]["window_id"] = json!(99);
        let (laptop, _) = scripted(
            descriptor(LAPTOP, MachineLocation::Desktop),
            vec![state("s0000001", many), degraded],
        );
        registry.cua().register(laptop).await.unwrap();
        let tools = tools_for(&registry, None).await;

        let output = tools.state.call(observe(None)).await.unwrap();
        let rendered: Value =
            serde_json::from_str(&output).unwrap_or_else(|e| panic!("{e}: {output}"));
        assert_eq!(rendered["snapshot_id"], "s0000001");
        assert_eq!(rendered["target"], json!({"pid": 42, "window_id": 99}));
        assert_eq!(rendered["app_name"], "Notes");
        let elements = rendered["elements"].as_array().unwrap();
        assert_eq!(elements.len(), MAX_RENDERED_ELEMENTS, "bounded");
        assert_eq!(elements[0]["element_token"], "tok/0");
        assert_eq!(elements[0]["element_index"], 0);
        assert_eq!(elements[0]["role"], "AXButton");
        assert_eq!(elements[0]["label"], "Button 0");
        assert_eq!(elements[0]["frame"], json!([10.0, 20.0, 80.0, 24.0]));
        assert_eq!(rendered["elements_shown"], MAX_RENDERED_ELEMENTS);
        assert_eq!(rendered["elements_returned"], MAX_RENDERED_ELEMENTS + 5);
        assert!(
            rendered["note"].as_str().unwrap().contains("query"),
            "{rendered}"
        );
        assert_eq!(rendered["pixel_addresses"]["allowed"], false);
        assert!(
            output.len() < 12_000,
            "fits the tool result bound: {}",
            output.len()
        );

        let output = tools.state.call(observe(None)).await.unwrap();
        let rendered: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(rendered["degraded"], true);
        assert!(
            rendered["degraded_reason"]
                .as_str()
                .unwrap()
                .contains("ax_window_unresolved")
        );
        assert_eq!(rendered["snapshot_id"], Value::Null);
        assert_eq!(rendered["pixel_addresses"]["allowed"], true);
        let escalation = rendered["escalation"].as_str().unwrap();
        assert!(
            escalation.contains("foreground") && escalation.contains("never"),
            "the recommendation is reported as something Nolune does not do: {escalation}"
        );
        assert_eq!(
            rendered["background_input"]["exact_window"],
            "ax_unresolved"
        );
        assert!(!output.contains("base64"), "no image bytes in the output");
    }

    #[tokio::test]
    async fn act_and_verify_outputs_report_the_outcome_and_the_predicates() {
        let registry = MachineRegistry::new();
        let (laptop, log) = scripted(
            descriptor(LAPTOP, MachineLocation::Desktop),
            vec![
                state("s0000001", vec![element(1, "tok/b", "AXTextField", "Name")]),
                json!({"outcome": {
                    "effect": "unverifiable", "route": "accessibility",
                    "delivery": {"requested": "background", "delivered_count": 1},
                    "evidence": ["delivery_receipt"],
                }}),
                json!({"overall": "satisfied", "predicates": [{"predicate_index": 0, "status": "satisfied"}]}),
                json!({"overall": "unsatisfied", "predicates": [{"predicate_index": 0, "status": "unsatisfied"}]}),
            ],
        );
        registry.cua().register(laptop).await.unwrap();
        let tools = tools_for(&registry, None).await;
        tools.state.call(observe(None)).await.unwrap();

        let output = tools
            .act
            .call(ActArgs {
                machine_id: None,
                target: window(),
                action: ActionArgs::TypeText {
                    address: AddressArgs::ElementToken {
                        element_token: "tok/b".into(),
                    },
                    text: "hello".into(),
                    delay_ms: None,
                },
                delivery_mode: Some(DeliveryModeArg::Background),
                verify: Some(VerifyArgs {
                    expect: vec![PredicateArgs::Element {
                        role: None,
                        label_contains: Some("Name".into()),
                        condition: ConditionArg::ValueEquals,
                        expected: Some(ExpectedArg::Text("hello".into())),
                    }],
                    timeout_ms: None,
                    stable_samples: None,
                }),
            })
            .await
            .unwrap();
        let rendered: Value =
            serde_json::from_str(&output).unwrap_or_else(|e| panic!("{e}: {output}"));
        assert_eq!(rendered["verified"], true);
        assert_eq!(rendered["outcome"]["effect"], "unverifiable");
        assert_eq!(rendered["outcome"]["delivery"], "background");
        assert_eq!(rendered["verification"]["overall"], "satisfied");
        assert!(
            rendered["next"]
                .as_str()
                .unwrap()
                .contains("get_window_state"),
            "{rendered}"
        );

        let error = tools.verify.call(verify(None)).await.unwrap_err();
        assert!(error.0.starts_with("verification_failed: "), "{error}");
        assert_eq!(log.lock().unwrap().len(), 4);
    }

    #[tokio::test]
    async fn the_orchestrator_is_shared_by_the_four_tools_of_a_turn() {
        let registry = MachineRegistry::new();
        let (laptop, _) = scripted(
            descriptor(LAPTOP, MachineLocation::Desktop),
            vec![state(
                "s0000001",
                vec![element(0, "tok/a", "AXButton", "Save")],
            )],
        );
        registry.cua().register(laptop).await.unwrap();
        let tools = tools_for(&registry, None).await;
        // The act tool refuses before any snapshot, then sees the one the
        // state tool recorded.
        let error = tools.act.call(click(None, "tok/a")).await.unwrap_err();
        assert!(error.0.starts_with("snapshot_required: "), "{error}");
        tools.state.call(observe(None)).await.unwrap();
        let error = tools.act.call(click(None, "tok/zzz")).await.unwrap_err();
        assert!(error.0.starts_with("stale_snapshot: "), "{error}");
    }
}
