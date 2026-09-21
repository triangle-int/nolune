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

use std::{
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
};

use cua_protocol::{
    CuaAction, CuaActionResult, GetWindowStateArgs as ProtocolWindowStateArgs, MachineHealth,
    MachineId, MachineLocation, VerificationResult, VerifyPredicate,
    VerifyStateArgs as ProtocolVerifyArgs, WindowStateResult, WindowTarget,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::services::cua::orchestrator::{
    ActReport, Failure, Orchestrator, PixelPolicy, Target, VerifySpec, kind_name,
};
use crate::services::machine_registry::MachineRegistry;
use crate::services::resource_access::ResourceAccess;
use crate::services::tool::{Tool, ToolDefinition};
use crate::services::tools::computer::{MachineTarget, TargetRefusal, TargetSelection};
use crate::services::tools::{ToolExecError, openai_schema};

/// How many accessibility elements one `get_window_state` result shows the
/// model at most; the rest is counted and reachable through `query`.
pub const MAX_RENDERED_ELEMENTS: usize = 50;

/// The longest text one rendered field (a label, a value, a role, an
/// action name) carries; the protocol allows 16 KiB per element value.
pub const MAX_RENDERED_TEXT: usize = 200;

/// The longest free text a window state's own fields carry (a degraded
/// reason, a route reason, an escalation reason, a title).
const MAX_RENDERED_NOTE: usize = 400;

/// How many action names one element lists.
const MAX_RENDERED_ACTIONS: usize = 8;

/// The bound on one rendered `get_window_state` result, in bytes: under
/// `ObservableTool`'s 12 000-char truncation with room to spare, so the
/// snapshot id, the pixel policy and the machine always reach the model
/// (serde_json sorts keys, and `elements` sorts before all of them).
pub const MAX_RENDERED_CHARS: usize = 11_000;

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

/// Where a window's one-shot capture is kept so the model can see it: among
/// the companion's uploads, referenced the way every other image reaches
/// the model (a provider-reachable URL carrying its provenance, or the bytes
/// inlined within the provider's bound on a local install).
#[derive(Clone)]
pub struct CaptureStore {
    workspace_dir: PathBuf,
    instance_slug: String,
    public_url: String,
    resources: ResourceAccess,
}

/// A capture kept as an upload: the image block for the model, when one
/// could be built, and the link that shows it to the user.
pub struct KeptCapture {
    pub upload_id: String,
    pub link: String,
    pub block: Option<serde_json::Value>,
}

impl CaptureStore {
    pub fn new(
        workspace_dir: &Path,
        instance_slug: &str,
        public_url: &str,
        resources: &ResourceAccess,
    ) -> Self {
        Self {
            workspace_dir: workspace_dir.to_path_buf(),
            instance_slug: instance_slug.to_owned(),
            public_url: public_url.to_owned(),
            resources: resources.clone(),
        }
    }
}

/// What the four tools share: the registry the targets live in, the
/// conversation's choice, the turn's orchestrator and where captures go.
#[derive(Clone)]
pub struct CuaTools {
    registry: MachineRegistry,
    target: MachineTarget,
    orchestrator: Arc<Orchestrator>,
    #[allow(dead_code)] // The stub of slice 3; the observation keeps captures here next.
    captures: CaptureStore,
}

impl CuaTools {
    pub fn new(registry: MachineRegistry, target: MachineTarget, captures: CaptureStore) -> Self {
        Self {
            registry,
            target,
            orchestrator: Arc::new(Orchestrator::new()),
            captures,
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
        let cua = self.registry.cua();
        let descriptors = cua.list().await;
        let requested = requested.map(str::trim).filter(|id| !id.is_empty());
        // The model naming the home or a server-local id means the server
        // machine either way, as it does for the composer's choice.
        let names_home = matches!(
            TargetSelection::from_request(requested),
            TargetSelection::ServerHome
        );
        let requested_label = |id: &str| {
            if names_home {
                "the server home".to_owned()
            } else {
                self.target.label(id)
            }
        };

        let chosen: MachineId = match self.target.selection() {
            TargetSelection::ServerHome => {
                if let Some(id) = requested
                    && !names_home
                {
                    return Err(TargetRefusal::Mismatch {
                        chosen: "the server home".to_owned(),
                        requested: self.target.label(id),
                    }
                    .into());
                }
                descriptors
                    .iter()
                    .find(|descriptor| descriptor.location == MachineLocation::ServerLocal)
                    .map(|descriptor| descriptor.machine_id.clone())
                    .ok_or(CuaRefusal::NoServerLocalTarget)?
            }
            TargetSelection::Machine(chosen) => {
                if let Some(id) = requested
                    && id != chosen
                {
                    return Err(TargetRefusal::Mismatch {
                        chosen: self.target.label(chosen),
                        requested: requested_label(id),
                    }
                    .into());
                }
                MachineId::try_from(chosen.as_str()).map_err(|_| TargetRefusal::Unavailable {
                    label: self.target.label(chosen),
                })?
            }
            TargetSelection::Unselected => match descriptors.as_slice() {
                [] => return Err(CuaRefusal::NoTargets),
                [only] => {
                    if let Some(id) = requested {
                        let names_it = id == only.machine_id.as_str()
                            || (names_home && only.location == MachineLocation::ServerLocal);
                        if !names_it {
                            return Err(TargetRefusal::Unavailable {
                                label: requested_label(id),
                            }
                            .into());
                        }
                    }
                    only.machine_id.clone()
                }
                several => {
                    // The model naming one of them is not the user choosing it.
                    let mut labels: Vec<String> = several
                        .iter()
                        .map(|descriptor| self.target.label(descriptor.machine_id.as_str()))
                        .collect();
                    labels.sort();
                    return Err(TargetRefusal::ChooseAComputer { labels }.into());
                }
            },
        };

        let label = self.target.label(chosen.as_str());
        let Ok(adapter) = cua.select(Some(&chosen)).await else {
            // Connected without a descriptor, or not connected at all.
            let connected = self
                .registry
                .list()
                .await
                .iter()
                .any(|machine| machine.machine_id == chosen.as_str());
            return Err(if connected {
                CuaRefusal::NoCuaDriver { label }
            } else {
                TargetRefusal::Unavailable { label }.into()
            });
        };
        let descriptor = adapter.descriptor();
        if descriptor.health == MachineHealth::Unavailable {
            return Err(CuaRefusal::DriverUnavailable { label });
        }
        let target = if descriptor.location == MachineLocation::ServerLocal {
            // Every operation on the server machine is one run of its
            // runtime; without the runtime there is no target to run on.
            let runtime = cua
                .server_local_runtime()
                .ok_or(CuaRefusal::NoServerLocalTarget)?;
            Target::server_local(adapter, runtime)
        } else {
            Target::direct(adapter)
        };
        Ok(Resolved { target, label })
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

/// A user-facing argument error, typed like the refusals.
fn invalid(error: String) -> ToolExecError {
    ToolExecError(format!("invalid_arguments: {error}"))
}

/// The action the JSON describes, through the protocol's own decoder so
/// every bound and shape rule applies before anything is sent.
fn protocol_action(tool: &str, args: serde_json::Value) -> Result<CuaAction, String> {
    let action = serde_json::json!({"tool": tool, "args": args});
    CuaAction::from_json(&action.to_string()).map_err(|error| error.to_string())
}

/// The discovery action the arguments describe, validated by the protocol.
pub fn discovery_action(args: &DiscoverWindowsArgs) -> Result<CuaAction, String> {
    match args.mode {
        DiscoverMode::ListApps => protocol_action("list_apps", serde_json::json!({})),
        DiscoverMode::ListWindows => {
            if args.pid == Some(0) {
                return Err("pid must be positive".into());
            }
            protocol_action(
                "list_windows",
                serde_json::json!({
                    "pid": args.pid,
                    "on_screen_only": args.on_screen_only.unwrap_or(true),
                }),
            )
        }
        DiscoverMode::LaunchApp => protocol_action(
            "launch_app",
            serde_json::json!({
                "bundle_id": args.bundle_id,
                "name": args.name,
                "creates_new_application_instance":
                    args.creates_new_application_instance.unwrap_or(false),
            }),
        ),
    }
}

/// The observation the arguments describe, validated by the protocol.
pub fn window_state_action(args: &WindowStateArgs) -> Result<ProtocolWindowStateArgs, String> {
    let action = protocol_action(
        "get_window_state",
        serde_json::json!({
            "target": args.target,
            "include_accessibility_tree": args.include_accessibility_tree.unwrap_or(true),
            "include_screenshot": args.include_screenshot.unwrap_or(false),
            "max_elements": args.max_elements,
            "max_depth": args.max_depth,
            "query": args.query,
        }),
    )?;
    match action {
        CuaAction::GetWindowState(args) => Ok(args),
        _ => Err("not a window state".into()),
    }
}

/// The typed action the arguments describe: the target and background
/// delivery filled in, defaults applied, validated by the protocol.
pub fn typed_action(target: WindowTargetArgs, action: &ActionArgs) -> Result<CuaAction, String> {
    use serde_json::json;
    let (tool, mut args) = match action {
        ActionArgs::Click {
            address,
            button,
            action,
            count,
        } => (
            "click",
            json!({
                "address": address,
                "button": button.unwrap_or(ButtonArg::Left),
                "action": action.unwrap_or(ClickActionArg::Press),
                "modifiers": [],
                "count": count,
            }),
        ),
        ActionArgs::DoubleClick { address } => ("double_click", json!({"address": address})),
        ActionArgs::RightClick { address } => {
            ("right_click", json!({"address": address, "modifiers": []}))
        }
        ActionArgs::Drag {
            from,
            to,
            duration_ms,
            steps,
            button,
        } => (
            "drag",
            json!({
                "from": from,
                "to": to,
                "duration_ms": duration_ms.unwrap_or(300),
                "steps": steps.unwrap_or(20),
                "button": button.unwrap_or(ButtonArg::Left),
                "modifiers": [],
            }),
        ),
        ActionArgs::Scroll {
            address,
            direction,
            by,
            amount,
        } => (
            "scroll",
            json!({
                "address": address,
                "direction": direction,
                "by": by.unwrap_or(ScrollByArg::Line),
                "amount": amount.unwrap_or(3),
            }),
        ),
        ActionArgs::TypeText {
            address,
            text,
            delay_ms,
        } => (
            "type_text",
            json!({"address": address, "text": text, "delay_ms": delay_ms.unwrap_or(0)}),
        ),
        ActionArgs::PressKey {
            address,
            key,
            modifiers,
        } => (
            "press_key",
            json!({
                "address": address,
                "key": key,
                "modifiers": modifiers.clone().unwrap_or_default(),
            }),
        ),
        ActionArgs::Hotkey { address, keys } => {
            ("hotkey", json!({"address": address, "keys": keys}))
        }
        ActionArgs::SetValue { element, value } => {
            ("set_value", json!({"element": element, "value": value}))
        }
        ActionArgs::InvokeMenu { path } => ("invoke_menu", json!({"path": path})),
    };
    args["target"] = json!(target);
    // Every delivered action goes out in the background; set_value and
    // invoke_menu are accessibility calls with no delivery at all.
    if !matches!(tool, "set_value" | "invoke_menu") {
        args["delivery_mode"] = json!(DeliveryModeArg::Background);
    }
    protocol_action(tool, args)
}

/// The predicates as the protocol carries them.
pub fn predicates(expect: &[PredicateArgs]) -> Result<Vec<VerifyPredicate>, String> {
    use serde_json::json;
    expect
        .iter()
        .map(|predicate| {
            let value = match predicate {
                PredicateArgs::WindowExists { value } => {
                    json!({"predicate": "window_exists", "value": value})
                }
                PredicateArgs::WindowBounds {
                    x,
                    y,
                    width,
                    height,
                    tolerance_px,
                } => json!({
                    "predicate": "window_bounds",
                    "value": {
                        "bounds": {"x": x, "y": y, "width": width, "height": height},
                        "tolerance_px": tolerance_px.unwrap_or(2.0),
                    },
                }),
                PredicateArgs::Element {
                    role,
                    label_contains,
                    condition,
                    expected,
                } => {
                    if role.is_none() && label_contains.is_none() {
                        return Err(
                            "an element predicate needs a selector: role and/or label_contains"
                                .to_owned(),
                        );
                    }
                    let condition = match (condition, expected) {
                        (ConditionArg::Exists, _) => json!({"condition": "exists"}),
                        (ConditionArg::Enabled, Some(ExpectedArg::Flag(flag))) => {
                            json!({"condition": "enabled", "expected": flag})
                        }
                        (ConditionArg::Selected, Some(ExpectedArg::Flag(flag))) => {
                            json!({"condition": "selected", "expected": flag})
                        }
                        (ConditionArg::ValueEquals, Some(ExpectedArg::Text(text))) => {
                            json!({"condition": "value_equals", "expected": text})
                        }
                        (ConditionArg::Enabled | ConditionArg::Selected, _) => {
                            return Err(
                                "enabled and selected need expected: true or false".to_owned()
                            );
                        }
                        (ConditionArg::ValueEquals, _) => {
                            return Err("value_equals needs expected: the text".to_owned());
                        }
                    };
                    json!({
                        "predicate": "element",
                        "value": {
                            "selector": {"role": role, "label_contains": label_contains},
                            "condition": condition,
                        },
                    })
                }
            };
            serde_json::from_value::<VerifyPredicate>(value)
                .map_err(|error| format!("invalid predicate: {error}"))
        })
        .collect()
}

/// A verification checked by the protocol's rules (predicate count, timing).
fn checked_verification(
    target: WindowTargetArgs,
    expect: Vec<VerifyPredicate>,
    timeout_ms: Option<u16>,
    stable_samples: Option<u8>,
) -> Result<ProtocolVerifyArgs, String> {
    let args = ProtocolVerifyArgs {
        target: target.into(),
        session: None,
        expect,
        include_screenshot: false,
        stable_samples: stable_samples.unwrap_or(DEFAULT_STABLE_SAMPLES),
        timeout_ms: timeout_ms.unwrap_or(DEFAULT_VERIFY_TIMEOUT_MS),
    };
    CuaAction::VerifyState(args.clone())
        .validate()
        .map_err(|error| error.to_string())?;
    Ok(args)
}

/// The verification the `act` arguments ask for, validated by the protocol.
pub fn verify_spec(target: WindowTargetArgs, verify: &VerifyArgs) -> Result<VerifySpec, String> {
    let args = checked_verification(
        target,
        predicates(&verify.expect)?,
        verify.timeout_ms,
        verify.stable_samples,
    )?;
    Ok(VerifySpec {
        expect: args.expect,
        timeout_ms: args.timeout_ms,
        stable_samples: args.stable_samples,
    })
}

/// The standalone verification the arguments describe.
pub fn verify_action(args: &VerifyWindowArgs) -> Result<ProtocolVerifyArgs, String> {
    checked_verification(
        args.target,
        predicates(&args.expect)?,
        args.timeout_ms,
        args.stable_samples,
    )
}

// ═══════════════════════════════════════════════════════════════════════════
// Rendering
// ═══════════════════════════════════════════════════════════════════════════

/// How many windows a discovery result lists; the rest is counted.
const MAX_RENDERED_WINDOWS: usize = 80;

fn rect_json(rect: &cua_protocol::Rect) -> serde_json::Value {
    serde_json::json!([rect.x, rect.y, rect.width, rect.height])
}

/// `text` cut to `max` characters, saying how much was cut.
fn clip(text: &str, max: usize) -> String {
    let total = text.chars().count();
    if total <= max {
        return text.to_owned();
    }
    let mut clipped: String = text.chars().take(max).collect();
    clipped.push_str(&format!("… (+{} chars)", total - max));
    clipped
}

/// One accessibility element as the model reads it: its token, role,
/// clipped label and value, flags, frame and a bounded list of actions.
fn element_json(element: &cua_protocol::AccessibilityElement) -> serde_json::Value {
    use serde_json::json;
    let mut value = json!({
        "element_index": element.element_index,
        "element_token": element.element_token.as_str(),
        "role": clip(element.role.as_str(), MAX_RENDERED_TEXT),
    });
    if let Some(text) = &element.label {
        value["label"] = json!(clip(text.as_str(), MAX_RENDERED_TEXT));
    }
    if let Some(text) = &element.value
        && !text.as_str().is_empty()
    {
        value["value"] = json!(clip(text.as_str(), MAX_RENDERED_TEXT));
    }
    if let Some(enabled) = element.enabled {
        value["enabled"] = json!(enabled);
    }
    if let Some(selected) = element.selected {
        value["selected"] = json!(selected);
    }
    if let Some(frame) = &element.frame {
        value["frame"] = rect_json(frame);
    }
    if !element.actions.is_empty() {
        let mut actions: Vec<String> = element
            .actions
            .iter()
            .take(MAX_RENDERED_ACTIONS)
            .map(|action| clip(action.as_str(), MAX_RENDERED_TEXT))
            .collect();
        if element.actions.len() > MAX_RENDERED_ACTIONS {
            actions.push(format!(
                "… (+{} more)",
                element.actions.len() - MAX_RENDERED_ACTIONS
            ));
        }
        value["actions"] = json!(actions);
    }
    value
}

fn window_json(window: &cua_protocol::WindowRecord) -> serde_json::Value {
    let mut value = serde_json::json!({
        "pid": window.target.pid,
        "window_id": window.target.window_id,
        "app_name": window.app_name.as_str(),
        "bounds": rect_json(&window.bounds),
        "is_on_screen": window.is_on_screen,
    });
    if let Some(title) = &window.title {
        value["title"] = serde_json::json!(title.as_str());
    }
    if let Some(on_current_space) = window.on_current_space {
        value["on_current_space"] = serde_json::json!(on_current_space);
    }
    value
}

/// Up to `MAX_RENDERED_WINDOWS` windows and how many there were.
fn windows_json(
    windows: &[cua_protocol::WindowRecord],
    budget: &mut usize,
) -> (Vec<serde_json::Value>, usize) {
    let shown: Vec<serde_json::Value> = windows.iter().take(*budget).map(window_json).collect();
    *budget -= shown.len();
    (shown, windows.len())
}

/// A discovery result as the model reads it.
pub fn render_discovery(result: &CuaActionResult, label: &str) -> serde_json::Value {
    use serde_json::json;
    let mut budget = MAX_RENDERED_WINDOWS;
    match result {
        CuaActionResult::ListApps(apps) => {
            let rendered: Vec<serde_json::Value> = apps
                .apps
                .iter()
                .map(|app| {
                    let (windows, total) = windows_json(&app.windows, &mut budget);
                    let mut value = json!({
                        "pid": app.pid,
                        "bundle_id": app.bundle_id.as_str(),
                        "name": app.name.as_str(),
                        "running": app.running,
                        "active": app.active,
                        "windows": windows,
                    });
                    if windows.len() < total {
                        value["windows_total"] = json!(total);
                    }
                    value
                })
                .collect();
            let mut value = json!({"machine": label, "apps": rendered});
            if budget == 0 {
                value["note"] = json!(format!(
                    "window lists are cut at {MAX_RENDERED_WINDOWS} in total; list_windows \
                     with a pid shows one app's windows"
                ));
            }
            value
        }
        CuaActionResult::ListWindows(windows) => {
            let (rendered, total) = windows_json(&windows.windows, &mut budget);
            let mut value = json!({
                "machine": label,
                "current_space_id": windows.current_space_id,
                "windows": rendered,
                "windows_total": total,
            });
            if rendered.len() < total {
                value["note"] = json!(format!(
                    "{} more windows not shown; list_windows with a pid narrows the list",
                    total - rendered.len()
                ));
            }
            value
        }
        CuaActionResult::LaunchApp(launched) => {
            let (windows, total) = windows_json(&launched.windows, &mut budget);
            json!({
                "machine": label,
                "launched": {
                    "pid": launched.pid,
                    "bundle_id": launched.bundle_id.as_str(),
                    "name": launched.name.as_str(),
                    "launch_state": launched.launch_state,
                    "windows": windows,
                    "windows_total": total,
                },
            })
        }
        other => json!({"machine": label, "result": other}),
    }
}

/// A window state as the model reads it: the snapshot id, a bounded list
/// of elements with their tokens, the driver's flags, the pixel policy for
/// the window, and a foreground recommendation reported as not applied.
/// Never the screenshot bytes. The whole rendering stays under
/// `MAX_RENDERED_CHARS`: every free text is clipped, and the elements list
/// is cut where it stops fitting beside the rest, with the count of what
/// was left out.
pub fn render_window_state(
    state: &WindowStateResult,
    policy: &PixelPolicy,
    label: &str,
    _capture: Option<&KeptCapture>,
) -> serde_json::Value {
    use serde_json::json;
    let returned = state.elements.len();
    let mut value = json!({
        "machine": label,
        "target": {"pid": state.target.pid, "window_id": state.target.window_id},
        "app_name": state.app_name.as_ref().map(|name| name.as_str()),
        "window_title": state
            .window_title
            .as_ref()
            .map(|title| clip(title.as_str(), MAX_RENDERED_NOTE)),
        "snapshot_id": state.snapshot_id.as_ref().map(|id| id.as_str()),
        "degraded": state.degraded,
        "degraded_reason": state
            .degraded_reason
            .as_ref()
            .map(|reason| clip(reason.as_str(), MAX_RENDERED_NOTE)),
        "elements": [],
        "elements_shown": 0,
        "elements_returned": returned,
        "elements_total": state.total_element_count.or(state.element_count),
        "truncated": state.truncated,
        "pixel_addresses": {"allowed": policy.allowed, "reason": policy.reason},
    });
    if let Some(bounds) = &state.window_bounds {
        value["window_bounds"] = rect_json(bounds);
    }
    if let Some(background) = &state.background_input {
        value["background_input"] = json!({
            "exact_window": background.exact_window.status,
            "routes": background
                .routes
                .iter()
                .map(|route| json!({
                    "route": route.route,
                    "status": route.status,
                    "reason": route
                        .reason
                        .as_ref()
                        .map(|reason| clip(reason.as_str(), MAX_RENDERED_NOTE)),
                }))
                .collect::<Vec<_>>(),
        });
    }
    if let Some(escalation) = &state.escalation {
        value["escalation"] = json!(format!(
            "the driver recommends {} ({}); Nolune never applies it: delivery stays \
             background, a point address is the fallback when pixel_addresses allows it",
            kind_name(escalation.recommended),
            clip(escalation.reason.as_str(), MAX_RENDERED_NOTE)
        ));
    }
    if let Some(screenshot) = &state.screenshot {
        value["screenshot"] = json!({
            "media_type": screenshot.media_type,
            "width": screenshot.width,
            "height": screenshot.height,
            "scale": state.screenshot_scale,
            "shown": false,
            "note": "captured one-shot; the image is not shown to you yet",
        });
    }

    // Everything but the elements is on the page now; the elements get
    // what is left, minus room for the note and the counts.
    const NOTE_RESERVE: usize = 200;
    let mut budget = MAX_RENDERED_CHARS.saturating_sub(value.to_string().len() + NOTE_RESERVE);
    let mut elements = Vec::new();
    for element in state.elements.iter().take(MAX_RENDERED_ELEMENTS) {
        let rendered = element_json(element);
        let cost = rendered.to_string().len() + 1;
        if cost > budget {
            break;
        }
        budget -= cost;
        elements.push(rendered);
    }
    let shown = elements.len();
    value["elements"] = json!(elements);
    value["elements_shown"] = json!(shown);
    if shown < returned {
        value["note"] = json!(format!(
            "{} more elements not shown; narrow with query or max_depth, or use \
             element_index + snapshot_id for an element you already know",
            returned - shown
        ));
    }
    value
}

fn outcome_json(outcome: &cua_protocol::ActionOutcome) -> serde_json::Value {
    use serde_json::json;
    let mut value = json!({
        "effect": outcome.effect,
        "route": outcome.route,
        "delivery": "background",
        "delivered_count": outcome.delivery.as_ref().and_then(|delivery| delivery.delivered_count),
        "evidence": outcome.evidence,
    });
    if let Some(escalation) = &outcome.escalation {
        value["escalation"] = json!(format!(
            "the driver recommends {} ({}); not applied",
            kind_name(escalation.target),
            kind_name(escalation.reason)
        ));
    }
    value
}

fn verification_json(verification: &VerificationResult) -> serde_json::Value {
    use serde_json::json;
    let mut value = json!({
        "overall": verification.overall,
        "predicates": verification
            .predicates
            .iter()
            .map(|predicate| json!({"index": predicate.predicate_index, "status": predicate.status}))
            .collect::<Vec<_>>(),
    });
    if let Some(screenshot) = &verification.screenshot {
        value["screenshot"] = json!({
            "media_type": screenshot.media_type,
            "width": screenshot.width,
            "height": screenshot.height,
            "shown": false,
        });
    }
    value
}

/// A verified action as the model reads it.
pub fn render_act(report: &ActReport, label: &str) -> serde_json::Value {
    serde_json::json!({
        "machine": label,
        "verified": true,
        "outcome": outcome_json(&report.outcome),
        "verification": report.verification.as_ref().map(verification_json),
        "next": "the window may have changed: call get_window_state again before the next \
                 action there",
    })
}

/// A verification as the model reads it.
pub fn render_verification(verification: &VerificationResult, label: &str) -> serde_json::Value {
    let mut value = verification_json(verification);
    value["machine"] = serde_json::json!(label);
    value
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
        let resolved = self.0.resolve(args.machine_id.as_deref()).await?;
        let action = discovery_action(&args).map_err(invalid)?;
        log::info!(
            "[discover_windows] {} on '{}' ({})",
            kind_name(action.kind()),
            resolved.target.machine_id().as_str(),
            resolved.label
        );
        let result = self
            .0
            .orchestrator
            .discover(&resolved.target, action)
            .await
            .map_err(|failure| self.0.failure(&resolved.label, failure))?;
        Ok(render_discovery(&result, &resolved.label).to_string())
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
                narrow large trees; a query that matches nothing is an empty list, not a \
                broken window. pixel_addresses says whether act may use a point address: only \
                when accessibility is unavailable for the window or the last verification there \
                failed, and only after an observation with include_screenshot: true, since a \
                point is read from that capture. The screenshot is captured one-shot and its \
                dimensions reported; the image is not shown to you yet. The output is bounded: \
                long labels and values are clipped, and elements past the bound are counted."
                .into(),
            parameters: openai_schema::<WindowStateArgs>(),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let resolved = self.0.resolve(args.machine_id.as_deref()).await?;
        let observation = window_state_action(&args).map_err(invalid)?;
        let window = observation.target;
        log::info!(
            "[get_window_state] pid {} window {} on '{}' ({})",
            window.pid,
            window.window_id,
            resolved.target.machine_id().as_str(),
            resolved.label
        );
        let orchestrator = self.0.orchestrator();
        let state = orchestrator
            .window_state(&resolved.target, observation)
            .await
            .map_err(|failure| self.0.failure(&resolved.label, failure))?;
        let policy = orchestrator
            .ledger()
            .pixel_policy(resolved.target.machine_id(), window);
        Ok(render_window_state(&state, &policy, &resolved.label, None).to_string())
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
                or the last verification failed, and the latest get_window_state of the window \
                captured a screenshot. Delivery is always background: nothing is \
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
        let resolved = self.0.resolve(args.machine_id.as_deref()).await?;
        let action = typed_action(args.target, &args.action).map_err(invalid)?;
        let verify = args
            .verify
            .as_ref()
            .map(|verify| verify_spec(args.target, verify))
            .transpose()
            .map_err(invalid)?;
        // `delivery_mode` admits background only; the action above already
        // carries it.
        let DeliveryModeArg::Background = args.delivery_mode.unwrap_or(DeliveryModeArg::Background);
        log::info!(
            "[act] {} in pid {} window {} on '{}' ({}, {})",
            kind_name(action.kind()),
            args.target.pid,
            args.target.window_id,
            resolved.target.machine_id().as_str(),
            resolved.label,
            match &verify {
                Some(spec) => format!("{} predicate(s)", spec.expect.len()),
                None => "no predicates".to_owned(),
            }
        );
        let report = self
            .0
            .orchestrator
            .act(&resolved.target, action, verify)
            .await
            .map_err(|failure| self.0.failure(&resolved.label, failure))?;
        Ok(render_act(&report, &resolved.label).to_string())
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
                is an error, and after one a point address is allowed on that window once it is \
                observed with a screenshot. Use it to \
                confirm a state before deciding the next step; act verifies its own action when \
                given verify.expect."
                .into(),
            parameters: openai_schema::<VerifyWindowArgs>(),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let resolved = self.0.resolve(args.machine_id.as_deref()).await?;
        let verification = verify_action(&args).map_err(invalid)?;
        log::info!(
            "[verify_state] {} predicate(s) on pid {} window {} on '{}' ({})",
            verification.expect.len(),
            args.target.pid,
            args.target.window_id,
            resolved.target.machine_id().as_str(),
            resolved.label
        );
        let result = self
            .0
            .orchestrator
            .verify(&resolved.target, verification)
            .await
            .map_err(|failure| self.0.failure(&resolved.label, failure))?;
        Ok(render_verification(&result, &resolved.label).to_string())
    }
}

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
        let workspace = tempfile::tempdir().unwrap();
        let resources = crate::services::resource_access::ResourceAccess::new("control-token");
        let shared = CuaTools::new(
            MachineRegistry::new(),
            MachineTarget::new(TargetSelection::Unselected),
            CaptureStore::new(workspace.path(), "moon", "", &resources),
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
        assert!(
            state.description.contains("shown to you") && !state.description.contains("not shown"),
            "the capture reaches the model now: {}",
            state.description
        );
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
                    snapshot_id: "s00000001".into(),
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
    use crate::services::tools::computer::SERVER_HOME_TARGET;
    use cua_protocol::{
        Capability, CheckedCuaAdapter, CuaRequestEnvelope, CuaResponseEnvelope, DriverVersion,
        MachineDescriptor, Permission, PermissionState, Platform,
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

    /// A `public_url` the provider can fetch from, so a capture travels as
    /// a URL with its provenance.
    const ROUTABLE: &str = "https://public.invalid";
    /// The default install: the provider cannot fetch from it, so a capture
    /// is inlined within the provider's bound.
    const LOCAL: &str = "http://localhost:26559";

    struct Tools {
        discover: DiscoverWindowsTool,
        state: GetWindowStateTool,
        act: ActTool,
        verify: VerifyStateTool,
        /// The workspace the captures are saved under, for the test's life.
        workspace: tempfile::TempDir,
    }

    async fn tools_for(registry: &MachineRegistry, chosen: Option<&str>) -> Tools {
        tools_on(registry, chosen, LOCAL).await
    }

    async fn tools_on(registry: &MachineRegistry, chosen: Option<&str>, public_url: &str) -> Tools {
        let workspace = tempfile::tempdir().unwrap();
        let resources = crate::services::resource_access::ResourceAccess::new("control-token");
        let shared = CuaTools::new(
            registry.clone(),
            MachineTarget::resolve(registry, chosen).await,
            CaptureStore::new(workspace.path(), "moon", public_url, &resources),
        );
        Tools {
            discover: DiscoverWindowsTool::new(shared.clone()),
            state: GetWindowStateTool::new(shared.clone()),
            act: ActTool::new(shared.clone()),
            verify: VerifyStateTool::new(shared),
            workspace,
        }
    }

    /// The text block of a result that carries a capture beside it, parsed.
    fn text_of(blocks: &[Value]) -> Value {
        let text = blocks
            .iter()
            .find(|block| block["type"] == "text")
            .unwrap_or_else(|| panic!("a text block: {blocks:?}"));
        serde_json::from_str(text["text"].as_str().unwrap())
            .unwrap_or_else(|e| panic!("{e}: {text}"))
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
                state("s00000001", vec![element(0, "tok/a", "AXButton", "Save")]),
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
        assert!(state.contains("s00000001"), "{state}");
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
                state("s00000001", vec![element(0, "tok/a", "AXButton", "Save")]),
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
        assert!(state.contains("s00000001"), "{state}");
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
            message.contains("Laptop")
                && message.contains("remote_bash")
                && !message.contains("computer_use"),
            "the refusal names the tools the model still has there: {message}"
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
        let mut degraded_with_capture = degraded.clone();
        degraded_with_capture["screenshot"] = json!({
            "media_type": "png", "base64": tiny_png(), "width": 1, "height": 1,
        });
        let (laptop, _) = scripted(
            descriptor(LAPTOP, MachineLocation::Desktop),
            vec![state("s00000001", many), degraded, degraded_with_capture],
        );
        registry.cua().register(laptop).await.unwrap();
        let tools = tools_on(&registry, None, ROUTABLE).await;

        // Without a capture the result is the one JSON document.
        let output = tools.state.call(observe(None)).await.unwrap();
        let rendered: Value =
            serde_json::from_str(&output).unwrap_or_else(|e| panic!("{e}: {output}"));
        assert!(rendered.is_object(), "one text result: {output}");
        assert_eq!(rendered["snapshot_id"], "s00000001");
        assert_eq!(rendered["target"], json!({"pid": 42, "window_id": 99}));
        assert_eq!(rendered["app_name"], "Notes");
        // The elements are a table: one header, one row per element.
        assert_eq!(
            rendered["element_columns"],
            json!([
                "element_index",
                "element_token",
                "role",
                "label",
                "value",
                "enabled",
                "selected",
                "frame",
                "actions"
            ])
        );
        let elements = rendered["elements"].as_array().unwrap();
        assert_eq!(elements.len(), MAX_RENDERED_ELEMENTS, "bounded");
        assert_eq!(
            elements[0],
            json!([
                0,
                "tok/0",
                "AXButton",
                "Button 0",
                null,
                true,
                null,
                [10.0, 20.0, 80.0, 24.0],
                ["AXPress"]
            ])
        );
        assert_eq!(elements[7][1], "tok/7");
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
        // Accessibility is unavailable, but this observation carried no
        // screenshot to read a point from: the policy says what to do.
        assert_eq!(rendered["pixel_addresses"]["allowed"], false);
        assert!(
            rendered["pixel_addresses"]["reason"]
                .as_str()
                .unwrap()
                .contains("include_screenshot"),
            "{rendered}"
        );
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

        // Observed again with a screenshot: the point route is open, and the
        // image travels beside the text as a URL block, never as bytes.
        let mut args = observe(None);
        args.include_screenshot = Some(true);
        let output = tools.state.call(args).await.unwrap();
        let blocks: Vec<Value> =
            serde_json::from_str(&output).unwrap_or_else(|e| panic!("{e}: {output}"));
        assert_eq!(blocks[0]["type"], "image");
        assert_eq!(blocks[0]["source"]["type"], "url");
        let rendered = text_of(&blocks);
        assert_eq!(rendered["degraded"], true);
        assert_eq!(rendered["pixel_addresses"]["allowed"], true);
        assert_eq!(rendered["screenshot"]["shown"], true);
        assert_eq!(rendered["screenshot"]["width"], 1);
        assert!(!output.contains("base64"), "no image bytes in the output");
    }

    /// #18 slice 3: the capture reaches the model as an image beside the
    /// elements, through the same upload path every other image takes, so
    /// the provider fetches it by a URL that carries its provenance and no
    /// bytes enter the context.
    #[tokio::test]
    async fn get_window_state_shows_the_capture_to_the_model_through_the_upload_path() {
        let registry = MachineRegistry::new();
        let mut captured = state("s00000001", vec![element(0, "tok/a", "AXButton", "Save")]);
        captured["screenshot"] = json!({
            "media_type": "png", "base64": tiny_png(), "width": 1, "height": 1,
        });
        let (laptop, _) = scripted(descriptor(LAPTOP, MachineLocation::Desktop), vec![captured]);
        registry.cua().register(laptop).await.unwrap();
        let tools = tools_on(&registry, None, ROUTABLE).await;
        assert!(
            <GetWindowStateTool as Tool>::TRUSTS_RESOURCE_PROVENANCE,
            "the provenance on the image block is the tool's own, so the URL is renewed \
             on later turns like a screenshot's"
        );

        let mut args = observe(None);
        args.include_screenshot = Some(true);
        let output = tools.state.call(args).await.unwrap();
        let blocks: Vec<Value> =
            serde_json::from_str(&output).unwrap_or_else(|e| panic!("{e}: {output}"));
        assert_eq!(blocks.len(), 2, "the image and the text: {output}");

        let image = &blocks[0];
        assert_eq!(image["type"], "image");
        assert_eq!(image["source"]["type"], "url");
        let url = image["source"]["url"].as_str().unwrap();
        assert!(
            url.starts_with("https://public.invalid/resources/model-provider/files/moon/upload_"),
            "{url}"
        );
        assert_eq!(image["resource_provenance"]["kind"], "uploaded_file");
        assert_eq!(image["resource_provenance"]["slug"], "moon");
        let upload_id = image["resource_provenance"]["id"].as_str().unwrap();
        assert!(
            upload_id.starts_with("upload_") && upload_id.ends_with(".png"),
            "saved with the capture's own media type: {upload_id}"
        );
        let stored = tools
            .workspace
            .path()
            .join("instances/moon/uploads")
            .join(format!("{upload_id}_blob.png"));
        assert_eq!(
            std::fs::read(&stored).unwrap(),
            include_bytes!("../../../../cua-protocol/tests/fixtures/tiny.png"),
            "the capture is an upload of the companion"
        );
        assert!(
            !output.contains(&tiny_png()),
            "no image bytes in the output"
        );

        let rendered = text_of(&blocks);
        assert_eq!(rendered["snapshot_id"], "s00000001");
        assert_eq!(rendered["elements"][0][1], "tok/a");
        assert_eq!(rendered["screenshot"]["shown"], true);
        assert_eq!(rendered["screenshot"]["width"], 1);
        assert_eq!(rendered["screenshot"]["height"], 1);
        assert_eq!(rendered["screenshot"]["media_type"], "png");
        assert_eq!(rendered["screenshot"]["upload_id"], upload_id);
        let link = rendered["screenshot"]["link"].as_str().unwrap();
        assert!(
            link.contains("/resources/browser/files/moon/") && link.contains(upload_id),
            "the user's link to the capture: {link}"
        );
        assert!(
            rendered["screenshot"]["note"]
                .as_str()
                .unwrap()
                .contains("![window]("),
            "{rendered}"
        );
    }

    /// On the default install the provider cannot fetch from `public_url`,
    /// so the capture is inlined the way the other tools inline images:
    /// within the provider's bound, as its own media type.
    #[tokio::test]
    async fn get_window_state_inlines_the_capture_on_a_local_install() {
        let registry = MachineRegistry::new();
        let mut captured = state("s00000001", vec![element(0, "tok/a", "AXButton", "Save")]);
        captured["screenshot"] = json!({
            "media_type": "png", "base64": tiny_png(), "width": 1, "height": 1,
        });
        let (laptop, _) = scripted(descriptor(LAPTOP, MachineLocation::Desktop), vec![captured]);
        registry.cua().register(laptop).await.unwrap();
        let tools = tools_on(&registry, None, LOCAL).await;

        let mut args = observe(None);
        args.include_screenshot = Some(true);
        let output = tools.state.call(args).await.unwrap();
        let blocks: Vec<Value> =
            serde_json::from_str(&output).unwrap_or_else(|e| panic!("{e}: {output}"));
        assert_eq!(blocks.len(), 2);
        let image = &blocks[0];
        assert_eq!(image["type"], "image");
        assert_eq!(image["source"]["type"], "base64");
        assert_eq!(image["source"]["media_type"], "image/png");
        assert_eq!(image["source"]["data"], tiny_png());
        assert!(
            image.get("resource_provenance").is_none(),
            "nothing to renew for inline bytes"
        );
        assert!(!output.contains("localhost"), "{output}");
        let rendered = text_of(&blocks);
        assert_eq!(rendered["screenshot"]["shown"], true);
        assert!(
            tools
                .workspace
                .path()
                .join("instances/moon/uploads")
                .join(format!(
                    "{}_blob.png",
                    rendered["screenshot"]["upload_id"].as_str().unwrap()
                ))
                .is_file(),
            "still kept as an upload for the user's link"
        );
    }

    /// The same orchestration renders identically whichever kind of target
    /// runs it: a desktop through its adapter and the server machine
    /// through the runtime's runs yield the same model-visible output, the
    /// machine's name and the capture's ids aside.
    #[tokio::test]
    async fn a_desktop_and_the_server_machine_render_identically_to_the_model() {
        fn captured() -> Value {
            let mut captured = state(
                "s00000001",
                vec![
                    element(0, "tok/a", "AXButton", "Save"),
                    element(1, "tok/b", "AXTextField", "Name"),
                ],
            );
            captured["screenshot"] = json!({
                "media_type": "png", "base64": tiny_png(), "width": 1, "height": 1,
            });
            captured
        }
        fn satisfied() -> Value {
            json!({
                "overall": "satisfied",
                "predicates": [{"predicate_index": 0, "status": "satisfied"}],
            })
        }
        /// The click result the server-local driver returns inside run `n`.
        fn clicked_in_run(n: u64) -> Value {
            json!({
                "target": {"pid": 42, "window_id": 99},
                "session": format!("nolune-run-{n}"),
                "address": {"kind": "element_token", "element_token": "tok/a"},
                "button": "left", "action": "press",
                "outcome": confirmed()["outcome"],
            })
        }
        /// The output with the machine's name and the capture's ids taken out.
        fn normalized(output: &str, label: &str) -> Value {
            let mut value: Value = serde_json::from_str(output).unwrap();
            fn scrub(value: &mut Value, label: &str) {
                match value {
                    Value::String(text) => *text = text.replace(label, "<machine>"),
                    Value::Array(items) => items.iter_mut().for_each(|item| scrub(item, label)),
                    Value::Object(fields) => {
                        for key in ["upload_id", "link", "url", "id"] {
                            if fields.contains_key(key) {
                                fields[key] = json!("<capture>");
                            }
                        }
                        if let Some(text) = fields.get_mut("text")
                            && let Some(inner) = text.as_str()
                        {
                            let mut inner: Value = serde_json::from_str(inner).unwrap();
                            scrub(&mut inner, label);
                            *text = inner;
                        }
                        fields.values_mut().for_each(|field| scrub(field, label));
                    }
                    _ => {}
                }
            }
            scrub(&mut value, label);
            value
        }
        async fn run_the_loop(tools: &Tools) -> [String; 3] {
            let mut args = observe(None);
            args.include_screenshot = Some(true);
            let observed = tools.state.call(args).await.unwrap();
            let mut act = click(None, "tok/a");
            act.verify = Some(VerifyArgs {
                expect: vec![PredicateArgs::WindowExists { value: true }],
                timeout_ms: None,
                stable_samples: None,
            });
            let acted = tools.act.call(act).await.unwrap();
            let verified = tools.verify.call(verify(None)).await.unwrap();
            [observed, acted, verified]
        }

        // The desktop: straight through its checked adapter.
        let desktop_registry = MachineRegistry::new();
        let (laptop, laptop_log) = scripted(
            descriptor(LAPTOP, MachineLocation::Desktop),
            vec![captured(), confirmed(), satisfied(), satisfied()],
        );
        desktop_registry.cua().register(laptop).await.unwrap();
        let desktop = tools_on(&desktop_registry, None, ROUTABLE).await;
        let from_desktop = run_the_loop(&desktop).await;
        assert_eq!(laptop_log.lock().unwrap().len(), 4);

        // The server machine: every operation is one sessioned run.
        let local_registry = MachineRegistry::new();
        let runtime = attach_server_local(
            &local_registry,
            vec![
                started(1),
                captured(),
                ended(1),
                started(2),
                clicked_in_run(2),
                ended(2),
                started(3),
                satisfied(),
                ended(3),
                started(4),
                satisfied(),
                ended(4),
            ],
        )
        .await;
        let local = tools_on(&local_registry, None, ROUTABLE).await;
        let from_local = run_the_loop(&local).await;
        runtime.shutdown().await;

        for (step, (desktop_output, local_output)) in
            from_desktop.iter().zip(from_local.iter()).enumerate()
        {
            assert_eq!(
                normalized(desktop_output, LAPTOP),
                normalized(local_output, STUDIO),
                "step {step} renders the same for both kinds of target"
            );
        }
        let observed: Vec<Value> = serde_json::from_str(&from_local[0]).unwrap();
        assert_eq!(observed[0]["type"], "image");
        assert_eq!(text_of(&observed)["screenshot"]["shown"], true);
    }

    fn tiny_png() -> String {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(include_bytes!(
            "../../../../cua-protocol/tests/fixtures/tiny.png"
        ))
    }

    #[tokio::test]
    async fn get_window_state_output_stays_under_the_tool_result_bound_with_long_values() {
        // One text area holding 16 KiB (the protocol's bound for a value)
        // beside a button; serde_json sorts keys, so an unbounded value
        // would push machine, pixel_addresses, snapshot_id and the second
        // token past ObservableTool's 12 000-char truncation.
        let registry = MachineRegistry::new();
        let long = "x".repeat(16 * 1024);
        let mut text_area = element(0, "tok/0", "AXTextArea", "Body");
        text_area["value"] = json!(long);
        let mut button = element(1, "tok/1", "AXButton", &"Save ".repeat(100));
        button["value"] = json!("");
        let (laptop, _) = scripted(
            descriptor(LAPTOP, MachineLocation::Desktop),
            vec![state("s00000001", vec![text_area, button])],
        );
        registry.cua().register(laptop).await.unwrap();
        let tools = tools_for(&registry, None).await;

        let output = tools.state.call(observe(None)).await.unwrap();
        assert!(
            output.len() < 12_000,
            "fits the tool result bound: {}",
            output.len()
        );
        let rendered: Value = serde_json::from_str(&output).unwrap();
        assert!(rendered["machine"].as_str().is_some(), "{rendered}");
        assert_eq!(rendered["snapshot_id"], "s00000001");
        assert_eq!(rendered["pixel_addresses"]["allowed"], false);
        let elements = rendered["elements"].as_array().unwrap();
        assert_eq!(
            elements.len(),
            2,
            "both elements fit once their text is clipped"
        );
        assert_eq!(elements[1][1], "tok/1");
        let value = elements[0][4].as_str().unwrap();
        assert!(
            value.chars().count() <= MAX_RENDERED_TEXT + 16 && value.ends_with("chars)"),
            "the value is clipped and says so: {value:?}"
        );
        let label = elements[1][3].as_str().unwrap();
        assert!(label.chars().count() <= MAX_RENDERED_TEXT + 16, "{label:?}");
        assert_eq!(rendered["elements_shown"], 2);
        assert_eq!(rendered["elements_returned"], 2);
    }

    #[tokio::test]
    async fn get_window_state_output_drops_elements_that_do_not_fit_and_says_so() {
        // Fifty elements each carrying the longest clipped label and value
        // exceed the bound together; the list is cut where it stops
        // fitting, and the counts say how many were left out.
        let registry = MachineRegistry::new();
        let many: Vec<Value> = (0..(MAX_RENDERED_ELEMENTS as u32))
            .map(|i| {
                let mut element = element(
                    i,
                    &format!("tok/{i}"),
                    "AXStaticText",
                    &format!("label {i} {}", "l".repeat(400)),
                );
                element["value"] = json!(format!("value {i} {}", "v".repeat(400)));
                element
            })
            .collect();
        let (laptop, _) = scripted(
            descriptor(LAPTOP, MachineLocation::Desktop),
            vec![state("s00000001", many)],
        );
        registry.cua().register(laptop).await.unwrap();
        let tools = tools_for(&registry, None).await;

        let output = tools.state.call(observe(None)).await.unwrap();
        assert!(
            output.len() <= MAX_RENDERED_CHARS,
            "fits the bound: {}",
            output.len()
        );
        let rendered: Value = serde_json::from_str(&output).unwrap();
        let shown = rendered["elements"].as_array().unwrap().len();
        assert!(
            shown > 5 && shown < MAX_RENDERED_ELEMENTS,
            "cut by size: {shown}"
        );
        assert_eq!(rendered["elements"][0][1], "tok/0");
        assert_eq!(rendered["elements_shown"], shown);
        assert_eq!(rendered["elements_returned"], MAX_RENDERED_ELEMENTS);
        assert_eq!(rendered["snapshot_id"], "s00000001");
        assert_eq!(rendered["pixel_addresses"]["allowed"], false);
        let note = rendered["note"].as_str().unwrap();
        assert!(
            note.contains(&format!(
                "{} more elements not shown",
                MAX_RENDERED_ELEMENTS - shown
            )) && note.contains("query"),
            "{note}"
        );
    }

    #[tokio::test]
    async fn act_and_verify_outputs_report_the_outcome_and_the_predicates() {
        let registry = MachineRegistry::new();
        let (laptop, log) = scripted(
            descriptor(LAPTOP, MachineLocation::Desktop),
            vec![
                state(
                    "s00000001",
                    vec![element(1, "tok/b", "AXTextField", "Name")],
                ),
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

    /// Read-only against the driver installed on this machine, like the
    /// runtime's live test: `cargo test --manifest-path server/Cargo.toml
    /// --bin nolune -- live_typed_tools --ignored --nocapture`. Discovers
    /// the apps and windows, observes the first on-screen window with a
    /// bounded tree, and verifies that it exists; nothing is clicked or
    /// typed and no screenshot is taken. The driver's answers go through
    /// the checked adapter, the orchestrator's ledger and the renderers
    /// directly (`Target::direct`), without a run session: on a machine
    /// where another gateway already holds the runtime's `nolune-run-<n>`
    /// label the driver refuses that session to a second transport, which
    /// is the runtime's label scheme (#192), not the tools'.
    #[tokio::test]
    #[ignore]
    async fn live_typed_tools_observe_a_window_on_the_server_machine() {
        use crate::services::cua::orchestrator::Orchestrator;
        use cua_protocol::EmptyArgs;

        let registry = MachineRegistry::new();
        let runtime = CuaRuntime::new(
            crate::config::CuaConfig::default(),
            registry.cua().clone(),
            std::path::PathBuf::new(),
        );
        runtime.start().await;
        let descriptors = registry.cua().list().await;
        let local = &descriptors[0];
        let adapter = registry
            .cua()
            .select(Some(&local.machine_id))
            .await
            .unwrap();
        let target = Target::direct(adapter);
        let orchestrator = Orchestrator::new();
        let label = local.machine_id.as_str();

        let apps = orchestrator
            .discover(&target, CuaAction::ListApps(EmptyArgs {}))
            .await
            .unwrap();
        let rendered = render_discovery(&apps, label);
        println!(
            "{} apps on {label}",
            rendered["apps"].as_array().unwrap().len()
        );

        let windows = orchestrator
            .discover(
                &target,
                discovery_action(&DiscoverWindowsArgs {
                    mode: DiscoverMode::ListWindows,
                    on_screen_only: Some(true),
                    ..discover(None)
                })
                .unwrap(),
            )
            .await
            .unwrap();
        let rendered = render_discovery(&windows, label);
        let listed = rendered["windows"].as_array().unwrap();
        println!("{} on-screen windows; first: {}", listed.len(), listed[0]);

        // Observe on-screen windows until one resolves its accessibility
        // surface (Electron windows often come back degraded), so real
        // tokens flow through the ledger; the degraded ones exercise the
        // pixel policy on the way.
        let mut target_args = None;
        for window in listed.iter().take(8) {
            let candidate = WindowTargetArgs {
                pid: window["pid"].as_u64().unwrap() as u32,
                window_id: window["window_id"].as_u64().unwrap(),
            };
            let observation = window_state_action(&WindowStateArgs {
                target: candidate,
                max_elements: Some(40),
                ..observe(None)
            })
            .unwrap();
            let window_target = observation.target;
            let state = orchestrator
                .window_state(&target, observation)
                .await
                .unwrap();
            let policy = orchestrator
                .ledger()
                .pixel_policy(target.machine_id(), window_target);
            let rendered = render_window_state(&state, &policy, label, None).to_string();
            let value: Value = serde_json::from_str(&rendered).unwrap();
            println!(
                "{:?} ({}/{}): snapshot {}, {} elements shown of {}, degraded {}, pixels {} \
                 ({} chars)",
                value["app_name"],
                candidate.pid,
                candidate.window_id,
                value["snapshot_id"],
                value["elements_shown"],
                value["elements_returned"],
                value["degraded"],
                value["pixel_addresses"]["allowed"],
                rendered.len()
            );
            assert!(!rendered.contains("base64"));
            assert!(rendered.len() < 12_000, "fits the tool result bound");
            if !state.elements.is_empty() {
                assert!(!policy.allowed, "a resolved tree keeps pixels closed");
                let token = state.elements[0].element_token.as_str();
                let click = typed_action(
                    candidate,
                    &ActionArgs::Click {
                        address: AddressArgs::ElementToken {
                            element_token: token.into(),
                        },
                        button: None,
                        action: None,
                        count: None,
                    },
                )
                .unwrap();
                assert_eq!(
                    orchestrator.ledger().check(target.machine_id(), &click),
                    Ok(()),
                    "a live token from the latest snapshot would be forwarded"
                );
                target_args = Some(candidate);
                break;
            }
        }
        let target_args = target_args.expect("an on-screen window with a resolved tree");

        // A capture-only observation of the same window: the one-shot
        // screenshot reaches the renderer as its dimensions, never its bytes.
        let capture = window_state_action(&WindowStateArgs {
            target: target_args,
            include_accessibility_tree: Some(false),
            include_screenshot: Some(true),
            ..observe(None)
        })
        .unwrap();
        let window = capture.target;
        let state = orchestrator.window_state(&target, capture).await.unwrap();
        let policy = orchestrator
            .ledger()
            .pixel_policy(target.machine_id(), window);
        let rendered = render_window_state(&state, &policy, label, None).to_string();
        let value: Value = serde_json::from_str(&rendered).unwrap();
        println!(
            "capture: {}x{} scale {} ({} chars)",
            value["screenshot"]["width"],
            value["screenshot"]["height"],
            value["screenshot"]["scale"],
            rendered.len()
        );
        assert!(!rendered.contains("base64"));
        assert!(rendered.len() < 2_000, "dimensions only");

        let verified = orchestrator
            .verify(
                &target,
                verify_action(&VerifyWindowArgs {
                    target: target_args,
                    ..verify(None)
                })
                .unwrap(),
            )
            .await
            .unwrap();
        println!("{}", render_verification(&verified, label));
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn the_orchestrator_is_shared_by_the_four_tools_of_a_turn() {
        let registry = MachineRegistry::new();
        let (laptop, _) = scripted(
            descriptor(LAPTOP, MachineLocation::Desktop),
            vec![state(
                "s00000001",
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
