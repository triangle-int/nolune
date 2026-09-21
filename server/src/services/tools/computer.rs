use std::collections::BTreeMap;
use std::fmt;

use base64::Engine;
use cua_protocol::Permission;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::services::machine_registry::{AgentToolCall, MachineInfo, MachineRegistry};
use crate::services::tool::{Tool, ToolDefinition};
use crate::services::tools::{ToolExecError, openai_schema};

// ═══════════════════════════════════════════════════════════════════════════
// Machine targeting (#80) — the computer the user chose for a conversation
// ═══════════════════════════════════════════════════════════════════════════

/// The `machine_id` a chat request sends for the server home: the row the
/// client synthesizes while no server-local target is listed. Mirrors
/// `HOME_SPACE_ID` in client/src/lib/computers/spaces.js.
pub const SERVER_HOME_TARGET: &str = "server-home";

/// What the user chose in the composer for one conversation. The computer
/// tools act on exactly that; nothing here ever picks a computer for them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum TargetSelection {
    /// Nothing chosen: the only connected desktop is used, several are refused.
    #[default]
    Unselected,
    /// The server home, where `run_command` and the file tools already act.
    ServerHome,
    /// One desktop by its stable id.
    Machine(String),
}

impl TargetSelection {
    /// The chat request's `machine_id` checked the way registration checks
    /// one (`validate_machine_id`) before it reaches the prompt, the log or
    /// a refusal: blank passes as nothing chosen; anything else must be a
    /// well-formed id. The error names the rule, never the id.
    pub fn check_request(machine_id: Option<&str>) -> Result<(), String> {
        match machine_id.map(str::trim) {
            None | Some("") => Ok(()),
            Some(id) => crate::domain::machine::validate_machine_id(id),
        }
    }

    /// From the chat request's `machine_id`: blank means nothing chosen, the
    /// synthesized home or a server-local id means the server home.
    pub fn from_request(machine_id: Option<&str>) -> Self {
        match machine_id.map(str::trim) {
            None | Some("") => Self::Unselected,
            Some(id)
                if id == SERVER_HOME_TARGET
                    || id.starts_with(crate::services::cua::host::SERVER_LOCAL_PREFIX) =>
            {
                Self::ServerHome
            }
            Some(id) => Self::Machine(id.to_owned()),
        }
    }
}

/// The selection resolved once per turn, with the display names of the
/// known machines so the trail names computers the way the Computers tab
/// does, and the ids of the desktops connected at that moment so the trail
/// can name the only one when nothing was chosen.
#[derive(Clone, Debug, Default)]
pub struct MachineTarget {
    selection: TargetSelection,
    names: BTreeMap<String, String>,
    live: Vec<String>,
}

/// What a computer tool needs from the desktop before it acts there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Need {
    /// Pointer and keyboard actions.
    Accessibility,
    /// Screenshots.
    ScreenCapture,
    /// Shell and file operations need no desktop permission.
    None,
}

/// The desktop one call acts on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedTarget {
    pub machine_id: String,
    /// The user's name for it, else its hostname, else its id.
    pub label: String,
}

/// Why a computer tool did not act, in words the model relays to the user.
/// Every variant names one thing to do; none of them picks another computer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TargetRefusal {
    /// Several desktops are connected and the user chose none.
    ChooseAComputer { labels: Vec<String> },
    /// No desktop is connected.
    NoneConnected,
    /// The chosen or named desktop is not connected.
    Unavailable { label: String },
    /// Connected, but its heartbeat went stale.
    Unhealthy { label: String, age_secs: i64 },
    /// A permission the action needs is not granted there.
    PermissionDenied {
        label: String,
        permission: &'static str,
        state: Permission,
    },
    /// The user chose the server home, which these tools do not drive.
    ServerHome,
    /// The model named a desktop other than the one the user chose.
    Mismatch { chosen: String, requested: String },
}

impl TargetRefusal {
    /// Stable code the error string starts with.
    pub fn code(&self) -> &'static str {
        match self {
            Self::ChooseAComputer { .. } => "choose_a_computer",
            Self::NoneConnected => "no_computer_connected",
            Self::Unavailable { .. } => "machine_unavailable",
            Self::Unhealthy { .. } => "machine_unhealthy",
            Self::PermissionDenied { .. } => "permission_denied",
            Self::ServerHome => "server_home",
            Self::Mismatch { .. } => "target_mismatch",
        }
    }
}

impl fmt::Display for TargetRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.code())?;
        match self {
            Self::ChooseAComputer { labels } => write!(
                f,
                "several computers are connected ({}) and the user has not chosen one; \
                 ask them to choose a computer in the composer, then try again",
                labels.join(", ")
            ),
            Self::NoneConnected => f.write_str(
                "no computer is connected; the user needs to open the Nolune desktop app \
                 on the computer they want you to use",
            ),
            Self::Unavailable { label } => write!(
                f,
                "{label} is not connected; ask the user to open the Nolune desktop app there, \
                 or to choose a connected computer in the composer"
            ),
            Self::Unhealthy { label, age_secs } => write!(
                f,
                "{label} has not answered for {age_secs} s; ask the user to check that it is \
                 awake and the Nolune desktop app is still running there"
            ),
            Self::PermissionDenied {
                label,
                permission,
                state,
            } => {
                let state = match state {
                    Permission::Denied => "denied",
                    _ => "not been allowed yet",
                };
                write!(
                    f,
                    "{permission} is {state} on {label}; ask the user to grant it to the \
                     Nolune desktop app in System Settings there and reconnect"
                )
            }
            Self::ServerHome => f.write_str(
                "the user chose the server home for this conversation, where run_command \
                 and the file tools already act; computer_use, remote_bash and remote_files \
                 need a desktop, so ask the user to choose one in the composer",
            ),
            Self::Mismatch { chosen, requested } => write!(
                f,
                "the user chose {chosen} for this conversation, not {requested}; act on \
                 {chosen} or ask them to switch computers in the composer"
            ),
        }
    }
}

impl std::error::Error for TargetRefusal {}

impl From<TargetRefusal> for ToolExecError {
    fn from(refusal: TargetRefusal) -> Self {
        ToolExecError(refusal.to_string())
    }
}

impl MachineTarget {
    /// A target with no names and no snapshot: the trail falls back to ids
    /// and never names a desktop nobody chose.
    pub fn new(selection: TargetSelection) -> Self {
        Self {
            selection,
            names: BTreeMap::new(),
            live: Vec::new(),
        }
    }

    /// A selection with the display names the trail uses (`label`).
    pub fn with_names(selection: TargetSelection, names: BTreeMap<String, String>) -> Self {
        Self {
            selection,
            names,
            live: Vec::new(),
        }
    }

    /// The ids of the desktops connected when the target was resolved, so
    /// `describe` names the only one while nothing is chosen.
    pub fn with_live(mut self, live: Vec<String>) -> Self {
        self.live = live;
        self
    }

    /// The request's `machine_id` resolved against the registry: the
    /// selection plus a snapshot of every known machine's display name and
    /// of which desktops are connected right now.
    pub async fn resolve(registry: &MachineRegistry, machine_id: Option<&str>) -> Self {
        // The record's display name is the user's; a desktop the store cannot
        // read right now (an unsupported file) is still named by its hostname.
        let mut names: BTreeMap<String, String> = match registry.known().await {
            Ok(known) => known
                .into_iter()
                .map(|machine| (machine.machine_id, machine.display_name))
                .collect(),
            Err(error) => {
                log::warn!("[machines] known machines unavailable for the trail: {error}");
                BTreeMap::new()
            }
        };
        let mut live = Vec::new();
        for connected in registry.list().await {
            live.push(connected.machine_id.clone());
            names
                .entry(connected.machine_id)
                .or_insert(connected.hostname);
        }
        Self::with_names(TargetSelection::from_request(machine_id), names).with_live(live)
    }

    pub fn selection(&self) -> &TargetSelection {
        &self.selection
    }

    /// The display name of a machine, else its id.
    pub fn label(&self, machine_id: &str) -> String {
        self.names
            .get(machine_id)
            .cloned()
            .unwrap_or_else(|| machine_id.to_owned())
    }

    /// Where a call acts, for the activity trail: "on <name>". The user's
    /// choice outranks the `machine_id` the model passed: with a computer or
    /// the home chosen the tool only ever acts there or refuses, so that is
    /// what the line says. With nothing chosen, the computer the model named
    /// is the one the call is about; otherwise the only desktop connected
    /// when the turn started is the one that acts, so it is named, and the
    /// generic wording stays only while the choice is genuinely open (none
    /// or several connected).
    pub fn describe(&self, requested: Option<&str>) -> String {
        match (&self.selection, requested) {
            (TargetSelection::Machine(id), _) => format!("on {}", self.label(id)),
            (TargetSelection::ServerHome, _) => "on the server home".to_owned(),
            (TargetSelection::Unselected, Some(id)) => format!("on {}", self.label(id)),
            (TargetSelection::Unselected, None) => match self.live.as_slice() {
                [only] => format!("on {}", self.label(only)),
                _ => "on the connected computer".to_owned(),
            },
        }
    }

    /// The system-prompt sentence about the chosen computer.
    pub fn prompt_line(&self, connected_desktops: usize) -> String {
        match &self.selection {
            TargetSelection::Machine(id) => format!(
                "the user chose {} for this conversation: computer_use, remote_bash and \
                 remote_files act there and nowhere else.",
                self.label(id)
            ),
            TargetSelection::ServerHome => "the user chose the server home for this \
                 conversation: run_command and the file tools act there; computer_use, \
                 remote_bash and remote_files are refused until they choose a desktop."
                .to_owned(),
            TargetSelection::Unselected => match connected_desktops {
                0 => "no desktop is connected; computer_use, remote_bash and remote_files \
                      will refuse."
                    .to_owned(),
                1 => "the user has not chosen a computer; the only connected desktop is used \
                      by computer_use, remote_bash and remote_files."
                    .to_owned(),
                _ => "several desktops are connected and the user has not chosen one; \
                      computer_use, remote_bash and remote_files will refuse until they \
                      choose a computer in the composer, so ask them to choose one before \
                      using those tools."
                    .to_owned(),
            },
        }
    }

    /// The desktop one call acts on: the chosen one, or the only connected
    /// one, checked for health and the permission the action needs. Never a
    /// guess between several, never a fallback to another computer.
    pub async fn desktop(
        &self,
        registry: &MachineRegistry,
        requested: Option<&str>,
        need: Need,
    ) -> Result<ResolvedTarget, TargetRefusal> {
        let live = registry.list().await;
        self.desktop_at(&live, requested, need, chrono::Utc::now().timestamp())
    }

    fn desktop_at(
        &self,
        live: &[MachineInfo],
        requested: Option<&str>,
        need: Need,
        now: i64,
    ) -> Result<ResolvedTarget, TargetRefusal> {
        // `live` is every connected desktop; the registry drops one the
        // moment its socket closes, so absence here means not connected.
        let connected = |id: &str| live.iter().find(|machine| machine.machine_id == id);
        let label = |machine: &MachineInfo| {
            self.names
                .get(&machine.machine_id)
                .cloned()
                .unwrap_or_else(|| machine.hostname.clone())
        };
        let machine = match &self.selection {
            TargetSelection::ServerHome => return Err(TargetRefusal::ServerHome),
            TargetSelection::Machine(chosen) => {
                if let Some(other) = requested.filter(|id| *id != chosen) {
                    return Err(TargetRefusal::Mismatch {
                        chosen: self.label(chosen),
                        requested: self.label(other),
                    });
                }
                connected(chosen).ok_or_else(|| TargetRefusal::Unavailable {
                    label: self.label(chosen),
                })?
            }
            TargetSelection::Unselected => {
                if live.len() > 1 {
                    // The model naming one of them is not the user choosing it.
                    let mut labels: Vec<String> = live.iter().map(label).collect();
                    labels.sort();
                    return Err(TargetRefusal::ChooseAComputer { labels });
                }
                match requested {
                    Some(id) => connected(id).ok_or_else(|| TargetRefusal::Unavailable {
                        label: self.label(id),
                    })?,
                    None => match live {
                        [only] => only,
                        _ => return Err(TargetRefusal::NoneConnected),
                    },
                }
            }
        };

        let label = label(machine);
        let age_secs = now - machine.last_seen;
        if crate::domain::machine::heartbeat_health(true, age_secs)
            == cua_protocol::MachineHealth::Degraded
        {
            return Err(TargetRefusal::Unhealthy { label, age_secs });
        }
        // `unavailable` is the platform saying it cannot report, not a refusal
        // (the same reading as the Computers tab).
        let needed = match (need, &machine.permissions) {
            (Need::Accessibility, Some(state)) => Some(("Accessibility", state.accessibility)),
            (Need::ScreenCapture, Some(state)) => Some(("Screen recording", state.screen_capture)),
            _ => None,
        };
        if let Some((permission, state @ (Permission::Denied | Permission::PromptRequired))) =
            needed
        {
            return Err(TargetRefusal::PermissionDenied {
                label,
                permission,
                state,
            });
        }
        Ok(ResolvedTarget {
            machine_id: machine.machine_id.clone(),
            label,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// list_machines — returns connected Tauri agents and Cua targets
// ═══════════════════════════════════════════════════════════════════════════

pub struct ListMachinesTool {
    registry: MachineRegistry,
}

impl ListMachinesTool {
    pub fn new(registry: MachineRegistry) -> Self {
        Self { registry }
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct ListMachinesArgs {}

impl Tool for ListMachinesTool {
    const NAME: &'static str = "list_machines";
    type Error = ToolExecError;
    type Args = ListMachinesArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "list_machines".into(),
            description: "List all machines you can control, one entry per machine_id with its \
                location (desktop or server_local) and os. Connected desktop apps carry hostname, \
                screen dimensions and last_seen; use their machine_id with computer_use, \
                remote_bash and remote_files. Machines with a Cua driver also carry \
                driver_version, health, permissions (accessibility, screen_capture) and \
                capabilities: those are the ones discover_windows, get_window_state, act and \
                verify_state drive, and they only accept actions their capabilities and granted \
                permissions allow. The user's choice in the composer decides which machine the \
                tools act on; this list is for reading, not for picking."
                .into(),
            parameters: openai_schema::<ListMachinesArgs>(),
        }
    }

    async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {
        let agents = self.registry.list().await;
        let targets = self.registry.cua().list().await;
        if agents.is_empty() && targets.is_empty() {
            return Ok(
                "No machines connected. The user needs to open the Nolune desktop app first."
                    .into(),
            );
        }
        // One entry per machine id: a desktop that registered a Cua
        // descriptor (#17) shows its legacy fields and its driver together.
        let mut info: BTreeMap<String, serde_json::Value> = agents
            .iter()
            .map(|m| {
                (
                    m.machine_id.clone(),
                    serde_json::json!({
                        "machine_id": m.machine_id,
                        "location": cua_protocol::MachineLocation::Desktop,
                        "os": m.os,
                        "hostname": m.hostname,
                        "screen": format!("{}x{}", m.screen_width, m.screen_height),
                        "last_seen": m.last_seen,
                    }),
                )
            })
            .collect();
        for m in &targets {
            let entry = info
                .entry(m.machine_id.as_str().to_owned())
                .or_insert_with(|| {
                    serde_json::json!({
                        "machine_id": m.machine_id,
                        "location": m.location,
                        "os": m.platform,
                    })
                });
            entry["driver_version"] = serde_json::json!(m.driver_version);
            entry["health"] = serde_json::json!(m.health);
            entry["permissions"] = serde_json::json!(m.permissions);
            entry["capabilities"] = serde_json::json!(m.capabilities);
        }
        let info: Vec<serde_json::Value> = info.into_values().collect();
        serde_json::to_string_pretty(&info).map_err(|e| ToolExecError(e.to_string()))
    }
}

/// The image block a screenshot contributes to the tool result.
///
/// With a provider-reachable `public_url` the provider fetches the saved upload
/// by URL, keeping base64 out of the context. Localhost installs inline the
/// bytes instead, and drop the image (leaving the caption) when the encoded
/// payload exceeds the provider's inline limit.
fn screenshot_image_block(
    public_url: &str,
    instance_slug: &str,
    upload_id: &str,
    image_b64: &str,
    resources: &crate::services::resource_access::ResourceAccess,
) -> Option<serde_json::Value> {
    if let Some(base) = crate::config::provider_reachable_public_url(public_url) {
        // URL-based image — no base64 in context, no truncation, no context bloat
        let full_url = super::public_file_url(base, instance_slug, upload_id, resources);
        return Some(serde_json::json!({
            "type": "image",
            "source": {
                "type": "url",
                "url": full_url,
            },
            "resource_provenance": {
                "kind": "uploaded_file",
                "version": 1,
                "slug": instance_slug,
                "id": upload_id,
            }
        }));
    }
    if image_b64.len() > crate::services::llm::MAX_INLINE_IMAGE_BASE64_BYTES {
        log::warn!(
            "screenshot {upload_id}: {} base64 bytes exceed the inline limit and no \
             provider-reachable public_url is configured; sending caption only",
            image_b64.len()
        );
        return None;
    }
    Some(serde_json::json!({
        "type": "image",
        "source": {
            "type": "base64",
            "media_type": "image/jpeg",
            "data": image_b64,
        }
    }))
}

// ═══════════════════════════════════════════════════════════════════════════
// computer_use — route action to a specific machine agent
// ═══════════════════════════════════════════════════════════════════════════

pub struct ComputerUseTool {
    registry: MachineRegistry,
    target: MachineTarget,
    workspace_dir: std::path::PathBuf,
    instance_slug: String,
    public_url: String,
    resources: crate::services::resource_access::ResourceAccess,
}

impl ComputerUseTool {
    pub fn new(
        registry: MachineRegistry,
        target: MachineTarget,
        workspace_dir: &std::path::Path,
        instance_slug: &str,
        public_url: &str,
        resources: &crate::services::resource_access::ResourceAccess,
    ) -> Self {
        Self {
            registry,
            target,
            workspace_dir: workspace_dir.to_path_buf(),
            instance_slug: instance_slug.to_string(),
            public_url: public_url.to_string(),
            resources: resources.clone(),
        }
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct ComputerUseArgs {
    /// ID of the machine to control (from list_machines). Omit to act on the
    /// computer the user chose for this conversation.
    #[serde(default)]
    pub machine_id: Option<String>,
    /// Action to perform: "screenshot", "left_click", "right_click", "middle_click",
    /// "double_click", "mouse_move", "type", "key", "scroll".
    pub action: String,
    /// [x, y] coordinates for click/move/scroll actions (in screen pixels).
    #[serde(default)]
    pub coordinate: Option<[i32; 2]>,
    /// Text to type (for "type" action).
    #[serde(default)]
    pub text: Option<String>,
    /// Key or key combination to press (for "key" action, e.g. "ctrl+c", "Return").
    #[serde(default)]
    pub key: Option<String>,
    /// Scroll direction: "up", "down", "left", "right".
    #[serde(default)]
    pub scroll_direction: Option<String>,
    /// Number of scroll clicks (default 3).
    #[serde(default)]
    pub scroll_amount: Option<i32>,
}

impl Tool for ComputerUseTool {
    const NAME: &'static str = "computer_use";
    const TRUSTS_RESOURCE_PROVENANCE: bool = true;
    type Error = ToolExecError;
    type Args = ComputerUseArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "computer_use".into(),
            description: "Control a connected desktop machine — take screenshots, click, type, press keys, scroll. \
                It acts on the computer the user chose for this conversation (or the only connected one); \
                with several connected and none chosen it refuses and you must ask the user to choose. \
                Always take a screenshot first to see the current state. \
                Coordinates are in the screenshot's pixel space. \
                Available actions: screenshot, left_click, right_click, middle_click, double_click, \
                mouse_move, type, key, scroll, switch_desktop. \
                \n\nmacOS tips: \
                - Switch desktop/Space: use action 'switch_desktop' with scroll_direction 'left' or 'right'. \
                - Mission Control: key 'ctrl+up'. \
                - App Exposé: key 'ctrl+down'. \
                - Spotlight: key 'cmd+space'. \
                - Close window: key 'cmd+w'. \
                - Quit app: key 'cmd+q'. \
                - Switch app: key 'cmd+tab'."
                .into(),
            parameters: openai_schema::<ComputerUseArgs>(),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let request_id = uuid::Uuid::new_v4().to_string();

        let mut params = serde_json::json!({});
        if let Some(c) = &args.coordinate {
            params["coordinate"] = serde_json::json!(c);
        }
        if let Some(t) = &args.text {
            params["text"] = serde_json::json!(t);
        }
        if let Some(k) = &args.key {
            params["key"] = serde_json::json!(k);
        }
        if let Some(d) = &args.scroll_direction {
            params["scroll_direction"] = serde_json::json!(d);
        }
        if let Some(a) = &args.scroll_amount {
            params["scroll_amount"] = serde_json::json!(a);
        }

        let call = AgentToolCall {
            request_id: request_id.clone(),
            action: args.action.clone(),
            params,
        };

        let need = if args.action == "screenshot" {
            Need::ScreenCapture
        } else {
            Need::Accessibility
        };
        let target = self
            .target
            .desktop(&self.registry, args.machine_id.as_deref(), need)
            .await?;
        log::info!(
            "[computer_use] {} on machine '{}' ({}, req={})",
            args.action,
            target.machine_id,
            target.label,
            &request_id[..8]
        );

        let result = self
            .registry
            .execute(&target.machine_id, call)
            .await
            .map_err(ToolExecError)?;

        match result.result_type.as_str() {
            "screenshot" => {
                let image_b64 = result.image.unwrap_or_default();
                let w = result.width.unwrap_or(0);
                let h = result.height.unwrap_or(0);

                // Save screenshot as upload file
                let saved = base64::engine::general_purpose::STANDARD
                    .decode(&image_b64)
                    .ok()
                    .and_then(|bytes| {
                        crate::services::uploads::save_upload(
                            &self.workspace_dir,
                            &self.instance_slug,
                            "screenshot.jpg",
                            &bytes,
                        )
                        .ok()
                    });

                if let Some(meta) = saved {
                    let chat_url = self.resources.url("", &self.instance_slug,
                        crate::services::resource_capability::CapabilityResource::uploaded_file(&meta.id).map_err(|e| ToolExecError(e.to_string()))?,
                        crate::services::resource_capability::CapabilityAudience::Browser).map_err(|e| ToolExecError(e.to_string()))?;
                    let caption = serde_json::json!({
                        "type": "text",
                        "text": format!(
                            "Screenshot captured ({}x{}) on {}. Show to user: ![screenshot]({})",
                            w, h, target.label, chat_url
                        ),
                    });

                    let image_block = screenshot_image_block(
                        &self.public_url,
                        &self.instance_slug,
                        &meta.id,
                        &image_b64,
                        &self.resources,
                    );
                    let blocks = match image_block {
                        Some(image_block) => serde_json::json!([image_block, caption]),
                        None => serde_json::json!([caption]),
                    };
                    Ok(blocks.to_string())
                } else {
                    Err(ToolExecError("failed to save screenshot".into()))
                }
            }
            "action" => {
                if result.success.unwrap_or(false) {
                    Ok(format!(
                        "Action '{}' executed successfully on {}.",
                        args.action, target.label
                    ))
                } else {
                    let err = result.error.unwrap_or_else(|| "unknown error".to_string());
                    Err(ToolExecError(format!(
                        "Action '{}' failed on {}: {}",
                        args.action, target.label, err
                    )))
                }
            }
            // bash/file results return output as text
            "output" => {
                let output = result.error.unwrap_or_default(); // reuse error field for output text
                Ok(output)
            }
            other => Err(ToolExecError(format!("unexpected result type: {other}"))),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// remote_bash — run a shell command on a connected machine
// ═══════════════════════════════════════════════════════════════════════════

pub struct RemoteBashTool {
    registry: MachineRegistry,
    target: MachineTarget,
}

impl RemoteBashTool {
    pub fn new(registry: MachineRegistry, target: MachineTarget) -> Self {
        Self { registry, target }
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct RemoteBashArgs {
    /// ID of the machine (from list_machines). Omit to act on the computer
    /// the user chose for this conversation.
    #[serde(default)]
    pub machine_id: Option<String>,
    /// Shell command to execute.
    pub command: String,
    /// Working directory (optional, defaults to home).
    #[serde(default)]
    pub cwd: Option<String>,
}

impl Tool for RemoteBashTool {
    const NAME: &'static str = "remote_bash";
    type Error = ToolExecError;
    type Args = RemoteBashArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "remote_bash".into(),
            description: "Execute a shell command on a connected desktop machine. \
                Returns stdout+stderr. Use for installing software, running scripts, \
                checking system state, etc. Commands run in the user's shell."
                .into(),
            parameters: openai_schema::<RemoteBashArgs>(),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let call = AgentToolCall {
            request_id: request_id.clone(),
            action: "bash".into(),
            params: serde_json::json!({
                "command": args.command,
                "cwd": args.cwd,
            }),
        };

        let target = self
            .target
            .desktop(&self.registry, args.machine_id.as_deref(), Need::None)
            .await?;
        log::info!(
            "[remote_bash] '{}' on '{}' ({})",
            args.command,
            target.machine_id,
            target.label
        );

        let result = self
            .registry
            .execute(&target.machine_id, call)
            .await
            .map_err(ToolExecError)?;

        if result.success.unwrap_or(false) {
            Ok(result.error.unwrap_or_default()) // output in error field
        } else {
            let err = result.error.unwrap_or_else(|| "command failed".into());
            Err(ToolExecError(err))
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// remote_files — read/write/list files on a connected machine
// ═══════════════════════════════════════════════════════════════════════════

pub struct RemoteFilesTool {
    registry: MachineRegistry,
    target: MachineTarget,
}

impl RemoteFilesTool {
    pub fn new(registry: MachineRegistry, target: MachineTarget) -> Self {
        Self { registry, target }
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct RemoteFilesArgs {
    /// ID of the machine (from list_machines). Omit to act on the computer
    /// the user chose for this conversation.
    #[serde(default)]
    pub machine_id: Option<String>,
    /// Operation: "read", "write", "list".
    pub operation: String,
    /// File or directory path.
    pub path: String,
    /// Content to write (only for "write" operation).
    #[serde(default)]
    pub content: Option<String>,
}

impl Tool for RemoteFilesTool {
    const NAME: &'static str = "remote_files";
    type Error = ToolExecError;
    type Args = RemoteFilesArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "remote_files".into(),
            description: "Read, write, or list files on a connected desktop machine. \
                Operations: 'read' returns file content, 'write' creates/overwrites a file, \
                'list' returns directory listing. Paths can be absolute or ~ for home."
                .into(),
            parameters: openai_schema::<RemoteFilesArgs>(),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let call = AgentToolCall {
            request_id: request_id.clone(),
            action: format!("file_{}", args.operation),
            params: serde_json::json!({
                "path": args.path,
                "content": args.content,
            }),
        };

        let target = self
            .target
            .desktop(&self.registry, args.machine_id.as_deref(), Need::None)
            .await?;
        log::info!(
            "[remote_files] {} '{}' on '{}' ({})",
            args.operation,
            args.path,
            target.machine_id,
            target.label
        );

        let result = self
            .registry
            .execute(&target.machine_id, call)
            .await
            .map_err(ToolExecError)?;

        if result.success.unwrap_or(false) {
            Ok(result.error.unwrap_or_default()) // output in error field
        } else {
            let err = result
                .error
                .unwrap_or_else(|| "file operation failed".into());
            Err(ToolExecError(err))
        }
    }
}

#[cfg(test)]
mod screenshot_block_tests {
    use super::screenshot_image_block;

    #[test]
    fn local_public_url_inlines_the_screenshot() {
        let resources = crate::services::resource_access::ResourceAccess::new("control-token");
        for public_url in ["http://localhost:26559", "http://0.0.0.0:26559", ""] {
            let block =
                screenshot_image_block(public_url, "moon", "shot.jpg", "aGVsbG8=", &resources)
                    .unwrap();
            assert_eq!(block["source"]["type"], "base64", "{public_url}");
            assert_eq!(block["source"]["media_type"], "image/jpeg");
            assert_eq!(block["source"]["data"], "aGVsbG8=");
            assert!(block.get("resource_provenance").is_none());
            assert!(!block.to_string().contains("localhost"));
        }
    }

    #[test]
    fn routable_public_url_hands_the_provider_a_url() {
        let resources = crate::services::resource_access::ResourceAccess::new("control-token");
        let block = screenshot_image_block(
            "https://public.invalid",
            "moon",
            "shot.jpg",
            "aGVsbG8=",
            &resources,
        )
        .unwrap();
        assert_eq!(block["source"]["type"], "url");
        assert!(
            block["source"]["url"]
                .as_str()
                .unwrap()
                .starts_with("https://public.invalid/resources/model-provider/files/moon/shot.jpg"),
            "{block}"
        );
        assert_eq!(block["resource_provenance"]["id"], "shot.jpg");
    }

    #[test]
    fn oversized_inline_screenshot_is_dropped() {
        let resources = crate::services::resource_access::ResourceAccess::new("control-token");
        let huge = "A".repeat(crate::services::llm::MAX_INLINE_IMAGE_BASE64_BYTES + 1);
        assert!(
            screenshot_image_block(
                "http://localhost:26559",
                "moon",
                "shot.jpg",
                &huge,
                &resources
            )
            .is_none()
        );
    }
}

#[cfg(test)]
mod list_machines_tests {
    use super::*;
    use crate::services::machine_registry::MachineInfo;
    use cua_protocol::{
        AppsResult, Capability, CheckedCuaAdapter, CuaActionResult, CuaResponse,
        CuaResponseEnvelope, DriverVersion, MachineDescriptor, MachineHealth, MachineId,
        MachineLocation, Permission, PermissionState, Platform,
    };

    fn legacy_agent() -> MachineInfo {
        MachineInfo {
            machine_id: "studio".into(),
            os: "macos".into(),
            hostname: "studio".into(),
            screen_width: 1440,
            screen_height: 900,
            last_seen: 1_700_000_000,
            instance_slug: None,
            platform: Some(Platform::Macos),
            location: MachineLocation::Desktop,
            permissions: None,
            capabilities: Vec::new(),
        }
    }

    fn server_local_target() -> CheckedCuaAdapter {
        let descriptor = MachineDescriptor {
            machine_id: MachineId::try_from("server-local:studio").unwrap(),
            location: MachineLocation::ServerLocal,
            platform: Platform::Linux,
            driver_version: DriverVersion::try_from("0.28.2").unwrap(),
            health: MachineHealth::Degraded,
            permissions: PermissionState {
                accessibility: Permission::Granted,
                screen_capture: Permission::Denied,
            },
            capabilities: vec![Capability::AppDiscovery, Capability::Health],
        };
        CheckedCuaAdapter::new(descriptor, |request| {
            let response = CuaResponseEnvelope {
                version: request.version,
                request_id: request.request_id,
                machine_id: request.machine_id,
                action: request.action.kind(),
                response: CuaResponse::Success {
                    result: Box::new(CuaActionResult::ListApps(AppsResult { apps: vec![] })),
                },
            };
            Box::pin(async move { response })
        })
        .unwrap()
    }

    async fn listed(registry: &MachineRegistry) -> Vec<serde_json::Value> {
        let output = ListMachinesTool::new(registry.clone())
            .call(ListMachinesArgs {})
            .await
            .unwrap();
        serde_json::from_str(&output).unwrap_or_else(|e| panic!("{e}: {output}"))
    }

    #[tokio::test]
    async fn legacy_agents_keep_their_fields_and_gain_a_location() {
        let registry = MachineRegistry::new();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        registry.register(legacy_agent(), tx).await;

        let machines = listed(&registry).await;
        assert_eq!(machines.len(), 1);
        let agent = &machines[0];
        assert_eq!(agent["machine_id"], "studio");
        assert_eq!(agent["os"], "macos");
        assert_eq!(agent["hostname"], "studio");
        assert_eq!(agent["screen"], "1440x900");
        assert_eq!(agent["last_seen"], 1_700_000_000);
        assert_eq!(agent["location"], "desktop");
    }

    #[tokio::test]
    async fn cua_targets_advertise_health_permissions_and_capabilities() {
        let registry = MachineRegistry::new();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        registry.register(legacy_agent(), tx).await;
        registry
            .cua()
            .register(server_local_target())
            .await
            .unwrap();

        let machines = listed(&registry).await;
        assert_eq!(machines.len(), 2, "{machines:?}");
        let local = machines
            .iter()
            .find(|m| m["machine_id"] == "server-local:studio")
            .expect("server-local target is listed");
        assert_eq!(local["location"], "server_local");
        assert_eq!(local["os"], "linux");
        assert_eq!(local["driver_version"], "0.28.2");
        assert_eq!(local["health"], "degraded");
        assert_eq!(
            local["permissions"],
            serde_json::json!({ "accessibility": "granted", "screen_capture": "denied" })
        );
        assert_eq!(
            local["capabilities"],
            serde_json::json!(["app_discovery", "health"])
        );
        assert!(
            local.get("hostname").is_none(),
            "no hostname was advertised"
        );
    }

    /// #18: a desktop that registered a Cua descriptor (#17) is one entry,
    /// its legacy fields and its descriptor together, never two entries
    /// under the same id.
    #[tokio::test]
    async fn a_desktop_with_a_driver_is_one_entry_with_both_sides() {
        let registry = MachineRegistry::new();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        registry.register(legacy_agent(), tx).await;
        let descriptor = MachineDescriptor {
            machine_id: MachineId::try_from("studio").unwrap(),
            location: MachineLocation::Desktop,
            platform: Platform::Macos,
            driver_version: DriverVersion::try_from("0.28.2").unwrap(),
            health: MachineHealth::Healthy,
            permissions: PermissionState {
                accessibility: Permission::Granted,
                screen_capture: Permission::Granted,
            },
            capabilities: vec![Capability::AppDiscovery, Capability::Pointer],
        };
        let adapter = CheckedCuaAdapter::new(descriptor, |request| {
            let response = CuaResponseEnvelope {
                version: request.version,
                request_id: request.request_id,
                machine_id: request.machine_id,
                action: request.action.kind(),
                response: CuaResponse::Success {
                    result: Box::new(CuaActionResult::ListApps(AppsResult { apps: vec![] })),
                },
            };
            Box::pin(async move { response })
        })
        .unwrap();
        registry.cua().register(adapter).await.unwrap();

        let machines = listed(&registry).await;
        assert_eq!(machines.len(), 1, "one entry per machine: {machines:?}");
        let studio = &machines[0];
        assert_eq!(studio["machine_id"], "studio");
        assert_eq!(studio["location"], "desktop");
        assert_eq!(studio["hostname"], "studio");
        assert_eq!(studio["screen"], "1440x900");
        assert_eq!(studio["last_seen"], 1_700_000_000);
        assert_eq!(studio["driver_version"], "0.28.2");
        assert_eq!(studio["health"], "healthy");
        assert_eq!(
            studio["capabilities"],
            serde_json::json!(["app_discovery", "pointer"])
        );
        assert_eq!(
            studio["permissions"],
            serde_json::json!({ "accessibility": "granted", "screen_capture": "granted" })
        );
    }

    #[tokio::test]
    async fn no_targets_of_either_kind_reads_as_no_machines() {
        let output = ListMachinesTool::new(MachineRegistry::new())
            .call(ListMachinesArgs {})
            .await
            .unwrap();
        assert!(output.starts_with("No machines connected"), "{output}");
    }
}

#[cfg(test)]
mod target_tests {
    //! #80: the computer tools act on the computer the user chose, or the
    //! only connected one, and refuse everything else with one thing to do.
    use super::*;
    use crate::services::machine_registry::ActionResult;
    use cua_protocol::{MachineLocation, Permission, PermissionState, Platform};

    type AgentReceiver = tokio::sync::mpsc::UnboundedReceiver<String>;

    const STUDIO: &str = "4f3c1c2e-9b5e-4d2b-8f0a-1c2d3e4f5a6b";
    const LAPTOP: &str = "9a8b7c6d-5e4f-4a3b-9c2d-1e0f9a8b7c6d";

    fn now() -> i64 {
        chrono::Utc::now().timestamp()
    }

    fn granted() -> PermissionState {
        PermissionState {
            accessibility: Permission::Granted,
            screen_capture: Permission::Granted,
        }
    }

    fn desktop(
        machine_id: &str,
        hostname: &str,
        seen: i64,
        permissions: Option<PermissionState>,
    ) -> MachineInfo {
        MachineInfo {
            machine_id: machine_id.into(),
            os: "macos".into(),
            hostname: hostname.into(),
            screen_width: 1440,
            screen_height: 900,
            last_seen: seen,
            instance_slug: None,
            platform: Some(Platform::Macos),
            location: MachineLocation::Desktop,
            permissions,
            capabilities: Vec::new(),
        }
    }

    async fn connect(registry: &MachineRegistry, info: MachineInfo) -> AgentReceiver {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        registry.register(info, tx).await;
        rx
    }

    /// Answer every toolcall the agent receives as a successful action and
    /// hand back the actions it saw.
    fn answering(
        registry: MachineRegistry,
        mut rx: AgentReceiver,
    ) -> std::sync::Arc<std::sync::Mutex<Vec<String>>> {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            while let Some(message) = rx.recv().await {
                let call: serde_json::Value = serde_json::from_str(&message).unwrap();
                log.lock()
                    .unwrap()
                    .push(call["action"].as_str().unwrap().to_owned());
                registry
                    .complete(
                        call["request_id"].as_str().unwrap(),
                        ActionResult {
                            result_type: "action".into(),
                            image: None,
                            width: None,
                            height: None,
                            scale: None,
                            success: Some(true),
                            error: Some("ok".into()),
                        },
                    )
                    .await;
            }
        });
        seen
    }

    fn nothing_received(rx: &mut AgentReceiver) -> bool {
        matches!(
            rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        )
    }

    struct Tools {
        _ws: tempfile::TempDir,
        computer: ComputerUseTool,
        bash: RemoteBashTool,
        files: RemoteFilesTool,
    }

    fn harness(registry: &MachineRegistry, target: MachineTarget) -> Tools {
        let ws = tempfile::tempdir().unwrap();
        let resources = crate::services::resource_access::ResourceAccess::new("control-token");
        Tools {
            computer: ComputerUseTool::new(
                registry.clone(),
                target.clone(),
                ws.path(),
                "companion",
                "",
                &resources,
            ),
            bash: RemoteBashTool::new(registry.clone(), target.clone()),
            files: RemoteFilesTool::new(registry.clone(), target),
            _ws: ws,
        }
    }

    fn click(machine_id: Option<&str>) -> ComputerUseArgs {
        ComputerUseArgs {
            machine_id: machine_id.map(str::to_owned),
            action: "left_click".into(),
            coordinate: Some([10, 10]),
            text: None,
            key: None,
            scroll_direction: None,
            scroll_amount: None,
        }
    }

    fn screenshot(machine_id: Option<&str>) -> ComputerUseArgs {
        ComputerUseArgs {
            action: "screenshot".into(),
            coordinate: None,
            ..click(machine_id)
        }
    }

    fn bash(machine_id: Option<&str>) -> RemoteBashArgs {
        RemoteBashArgs {
            machine_id: machine_id.map(str::to_owned),
            command: "uname -a".into(),
            cwd: None,
        }
    }

    fn files(machine_id: Option<&str>) -> RemoteFilesArgs {
        RemoteFilesArgs {
            machine_id: machine_id.map(str::to_owned),
            operation: "list".into(),
            path: "~/Documents".into(),
            content: None,
        }
    }

    /// Every one of the three tools answers a refusal with the same code and
    /// text, so the model relays one message whichever tool it reached for.
    async fn every_tool_refuses(tools: &Tools, machine_id: Option<&str>, code: &str) -> String {
        let computer = tools.computer.call(click(machine_id)).await.unwrap_err();
        let bash = tools.bash.call(bash(machine_id)).await.unwrap_err();
        let files = tools.files.call(files(machine_id)).await.unwrap_err();
        for error in [&computer, &bash, &files] {
            assert!(
                error.0.starts_with(&format!("{code}: ")),
                "expected a {code} refusal, got {error}"
            );
        }
        assert_eq!(computer.0, bash.0);
        assert_eq!(bash.0, files.0);
        computer.0
    }

    #[test]
    fn the_request_names_a_selection() {
        assert_eq!(
            TargetSelection::from_request(None),
            TargetSelection::Unselected
        );
        assert_eq!(
            TargetSelection::from_request(Some("")),
            TargetSelection::Unselected
        );
        assert_eq!(
            TargetSelection::from_request(Some("  ")),
            TargetSelection::Unselected
        );
        assert_eq!(
            TargetSelection::from_request(Some(SERVER_HOME_TARGET)),
            TargetSelection::ServerHome
        );
        assert_eq!(
            TargetSelection::from_request(Some("server-local:studio")),
            TargetSelection::ServerHome
        );
        assert_eq!(
            TargetSelection::from_request(Some(STUDIO)),
            TargetSelection::Machine(STUDIO.into())
        );
    }

    #[tokio::test]
    async fn two_connected_desktops_and_no_choice_ask_the_user_to_choose() {
        let registry = MachineRegistry::new();
        let mut studio = connect(&registry, desktop(STUDIO, "studio", now(), None)).await;
        let mut laptop = connect(&registry, desktop(LAPTOP, "laptop", now(), None)).await;
        registry.rename(STUDIO, Some("Studio Mac")).await.unwrap();
        let target = MachineTarget::resolve(&registry, None).await;
        assert_eq!(target.selection(), &TargetSelection::Unselected);
        let tools = harness(&registry, target);

        let message = every_tool_refuses(&tools, None, "choose_a_computer").await;
        assert!(
            message.contains("Studio Mac") && message.contains("laptop"),
            "the refusal names the computers to choose from: {message}"
        );
        assert!(
            message.contains("choose a computer"),
            "the refusal asks for a choice, never a pick: {message}"
        );

        // The model naming one of them is not the user choosing it.
        let message = every_tool_refuses(&tools, Some(STUDIO), "choose_a_computer").await;
        assert!(message.contains("Studio Mac"), "{message}");

        assert!(nothing_received(&mut studio), "studio was never asked");
        assert!(nothing_received(&mut laptop), "laptop was never asked");
    }

    #[tokio::test]
    async fn the_chosen_desktop_is_used_exactly() {
        let registry = MachineRegistry::new();
        let studio = connect(&registry, desktop(STUDIO, "studio", now(), Some(granted()))).await;
        let mut laptop = connect(&registry, desktop(LAPTOP, "laptop", now(), None)).await;
        registry.rename(STUDIO, Some("Studio Mac")).await.unwrap();
        let seen = answering(registry.clone(), studio);
        let target = MachineTarget::resolve(&registry, Some(STUDIO)).await;
        assert_eq!(target.selection(), &TargetSelection::Machine(STUDIO.into()));
        assert_eq!(target.label(STUDIO), "Studio Mac");
        assert_eq!(target.label(LAPTOP), "laptop");
        assert_eq!(target.label("unknown-id"), "unknown-id");
        let tools = harness(&registry, target);

        // With or without the model naming it, the chosen computer is the one asked.
        let output = tools.computer.call(click(None)).await.unwrap();
        assert!(
            output.contains("Studio Mac"),
            "the result names the computer: {output}"
        );
        tools.computer.call(click(Some(STUDIO))).await.unwrap();
        tools.bash.call(bash(None)).await.unwrap();
        tools.files.call(files(Some(STUDIO))).await.unwrap();
        tokio::task::yield_now().await;
        assert_eq!(
            *seen.lock().unwrap(),
            ["left_click", "left_click", "bash", "file_list"]
        );

        // Naming another computer is refused; the chosen one is never swapped for it.
        let message = every_tool_refuses(&tools, Some(LAPTOP), "target_mismatch").await;
        assert!(
            message.contains("Studio Mac") && message.contains("laptop"),
            "{message}"
        );
        assert!(nothing_received(&mut laptop), "laptop was never asked");
        assert_eq!(seen.lock().unwrap().len(), 4, "studio was not asked either");
    }

    #[tokio::test]
    async fn the_only_connected_desktop_is_used_without_a_choice() {
        let registry = MachineRegistry::new();
        let studio = connect(&registry, desktop(STUDIO, "studio", now(), Some(granted()))).await;
        let seen = answering(registry.clone(), studio);
        let tools = harness(&registry, MachineTarget::resolve(&registry, None).await);

        let output = tools.computer.call(screenshot(None)).await.unwrap();
        assert!(output.contains("on studio"), "{output}");
        tools.bash.call(bash(None)).await.unwrap();
        tools.files.call(files(Some(STUDIO))).await.unwrap();
        tokio::task::yield_now().await;
        assert_eq!(*seen.lock().unwrap(), ["screenshot", "bash", "file_list"]);
    }

    #[tokio::test]
    async fn no_connected_desktop_is_reported_not_guessed() {
        let registry = MachineRegistry::new();
        let tools = harness(&registry, MachineTarget::resolve(&registry, None).await);
        let message = every_tool_refuses(&tools, None, "no_computer_connected").await;
        assert!(message.contains("desktop app"), "{message}");
    }

    #[tokio::test]
    async fn an_unavailable_target_fails_and_never_falls_back() {
        let registry = MachineRegistry::new();
        let mut studio = connect(&registry, desktop(STUDIO, "studio", now(), None)).await;
        let laptop = connect(&registry, desktop(LAPTOP, "laptop", now(), None)).await;
        registry.rename(LAPTOP, Some("Laptop")).await.unwrap();
        drop(laptop);
        registry.unregister(LAPTOP).await;

        // The user chose the laptop; it went offline; studio is not used instead.
        let tools = harness(
            &registry,
            MachineTarget::resolve(&registry, Some(LAPTOP)).await,
        );
        let message = every_tool_refuses(&tools, None, "machine_unavailable").await;
        assert!(message.contains("Laptop"), "{message}");
        assert!(
            !message.contains("studio"),
            "no other computer is offered: {message}"
        );

        // Nothing chosen, the model names the offline laptop: same answer.
        let tools = harness(&registry, MachineTarget::resolve(&registry, None).await);
        let message = every_tool_refuses(&tools, Some(LAPTOP), "machine_unavailable").await;
        assert!(message.contains("Laptop"), "{message}");

        // A computer nobody has ever seen is unavailable too, by its id.
        let message = every_tool_refuses(&tools, Some("never-seen"), "machine_unavailable").await;
        assert!(message.contains("never-seen"), "{message}");

        assert!(nothing_received(&mut studio), "studio was never asked");
    }

    #[tokio::test]
    async fn a_desktop_that_stopped_answering_is_refused() {
        let registry = MachineRegistry::new();
        let stale = now() - crate::domain::machine::STALE_HEARTBEAT_SECS - 30;
        let mut studio = connect(&registry, desktop(STUDIO, "studio", stale, None)).await;
        let tools = harness(
            &registry,
            MachineTarget::resolve(&registry, Some(STUDIO)).await,
        );
        let message = every_tool_refuses(&tools, None, "machine_unhealthy").await;
        assert!(
            message.contains("studio") && message.contains("awake"),
            "{message}"
        );
        assert!(nothing_received(&mut studio));
    }

    #[tokio::test]
    async fn a_missing_permission_refuses_the_actions_that_need_it() {
        let registry = MachineRegistry::new();
        let permissions = PermissionState {
            accessibility: Permission::Denied,
            screen_capture: Permission::PromptRequired,
        };
        let studio = connect(
            &registry,
            desktop(STUDIO, "studio", now(), Some(permissions)),
        )
        .await;
        let seen = answering(registry.clone(), studio);
        let tools = harness(
            &registry,
            MachineTarget::resolve(&registry, Some(STUDIO)).await,
        );

        let click_error = tools.computer.call(click(None)).await.unwrap_err().0;
        assert!(
            click_error.starts_with("permission_denied: ")
                && click_error.contains("Accessibility")
                && click_error.contains("denied")
                && click_error.contains("System Settings"),
            "{click_error}"
        );
        let shot_error = tools.computer.call(screenshot(None)).await.unwrap_err().0;
        assert!(
            shot_error.starts_with("permission_denied: ")
                && shot_error.contains("Screen recording")
                && shot_error.contains("not been allowed"),
            "{shot_error}"
        );

        // Shell and file work need neither, and a report of "unavailable" is
        // the platform saying it cannot tell, not a refusal.
        tools.bash.call(bash(None)).await.unwrap();
        tools.files.call(files(None)).await.unwrap();
        tokio::task::yield_now().await;
        assert_eq!(*seen.lock().unwrap(), ["bash", "file_list"]);

        let registry = MachineRegistry::new();
        let unknown = PermissionState {
            accessibility: Permission::Unavailable,
            screen_capture: Permission::Unavailable,
        };
        let studio = connect(&registry, desktop(STUDIO, "studio", now(), Some(unknown))).await;
        let seen = answering(registry.clone(), studio);
        let tools = harness(&registry, MachineTarget::resolve(&registry, None).await);
        tools.computer.call(click(None)).await.unwrap();
        tokio::task::yield_now().await;
        assert_eq!(*seen.lock().unwrap(), ["left_click"]);
    }

    #[tokio::test]
    async fn the_server_home_is_not_driven_by_the_desktop_tools() {
        let registry = MachineRegistry::new();
        let mut studio = connect(&registry, desktop(STUDIO, "studio", now(), None)).await;
        for chosen in [SERVER_HOME_TARGET, "server-local:studio"] {
            let tools = harness(
                &registry,
                MachineTarget::resolve(&registry, Some(chosen)).await,
            );
            let message = every_tool_refuses(&tools, None, "server_home").await;
            assert!(
                message.contains("run_command") && message.contains("choose"),
                "{message}"
            );
            // The model naming the connected desktop does not override the user's choice.
            every_tool_refuses(&tools, Some(STUDIO), "server_home").await;
        }
        assert!(nothing_received(&mut studio));
    }

    #[tokio::test]
    async fn the_trail_and_the_prompt_name_the_chosen_computer() {
        let registry = MachineRegistry::new();
        let _studio = connect(&registry, desktop(STUDIO, "studio", now(), None)).await;
        registry.rename(STUDIO, Some("Studio Mac")).await.unwrap();

        let chosen = MachineTarget::resolve(&registry, Some(STUDIO)).await;
        assert_eq!(chosen.describe(None), "on Studio Mac");
        assert_eq!(chosen.describe(Some(STUDIO)), "on Studio Mac");
        // The model naming another computer does not move the trail line:
        // the tool only ever acts on the user's choice or refuses.
        assert_eq!(chosen.describe(Some(LAPTOP)), "on Studio Mac");
        assert!(
            chosen.prompt_line(1).contains("Studio Mac"),
            "{}",
            chosen.prompt_line(1)
        );

        // Nothing chosen and one desktop connected: the trail names the
        // desktop that will act, not "the connected computer".
        let open = MachineTarget::resolve(&registry, None).await;
        assert_eq!(open.describe(None), "on Studio Mac");
        assert_eq!(open.describe(Some(STUDIO)), "on Studio Mac");
        assert!(open.prompt_line(1).contains("only connected desktop"));
        assert!(
            open.prompt_line(2).contains("choose a computer"),
            "{}",
            open.prompt_line(2)
        );
        assert!(open.prompt_line(0).contains("no desktop is connected"));

        let home = MachineTarget::resolve(&registry, Some(SERVER_HOME_TARGET)).await;
        assert_eq!(home.describe(None), "on the server home");
        assert_eq!(
            home.describe(Some(STUDIO)),
            "on the server home",
            "nothing runs on a desktop the model names while the home is chosen"
        );
        assert!(home.prompt_line(1).contains("run_command"));

        // Without a snapshot the trail falls back to ids, never to a guess.
        assert_eq!(
            MachineTarget::new(TargetSelection::Machine(STUDIO.into())).describe(None),
            format!("on {STUDIO}")
        );
    }

    #[tokio::test]
    async fn the_trail_names_the_only_connected_desktop_and_stays_generic_when_the_choice_is_open()
    {
        // No desktop connected: nothing to name.
        let registry = MachineRegistry::new();
        let none = MachineTarget::resolve(&registry, None).await;
        assert_eq!(none.describe(None), "on the connected computer");

        // One connected: the trail says which one, by the user's name.
        let _studio = connect(&registry, desktop(STUDIO, "studio", now(), None)).await;
        registry.rename(STUDIO, Some("Studio Mac")).await.unwrap();
        let only = MachineTarget::resolve(&registry, None).await;
        assert_eq!(only.describe(None), "on Studio Mac");
        assert_eq!(
            tool_summary_line(&only),
            "left_click on Studio Mac",
            "the trail line the tool announces names the desktop that acts"
        );

        // Two connected and none chosen: the choice is open, the generic
        // wording stays (the call itself is refused with choose_a_computer).
        let _laptop = connect(&registry, desktop(LAPTOP, "laptop", now(), None)).await;
        let open = MachineTarget::resolve(&registry, None).await;
        assert_eq!(open.describe(None), "on the connected computer");
        assert_eq!(open.describe(Some(LAPTOP)), "on laptop");

        // A snapshot without names still names the only desktop, by id.
        let bare =
            MachineTarget::new(TargetSelection::Unselected).with_live(vec![STUDIO.to_owned()]);
        assert_eq!(bare.describe(None), format!("on {STUDIO}"));
        // Without a snapshot at all nothing is guessed.
        assert_eq!(
            MachineTarget::new(TargetSelection::Unselected).describe(None),
            "on the connected computer"
        );
    }

    fn tool_summary_line(target: &MachineTarget) -> String {
        crate::services::tools::tool_summary_on(
            "computer_use",
            r#"{"action":"left_click"}"#,
            target,
        )
    }

    #[test]
    fn resolution_is_pure_and_never_picks() {
        let target = MachineTarget::new(TargetSelection::Unselected);
        let live = [
            desktop(STUDIO, "studio", 1_000, None),
            desktop(LAPTOP, "laptop", 1_000, None),
        ];
        assert_eq!(
            target.desktop_at(&live, None, Need::None, 1_000),
            Err(TargetRefusal::ChooseAComputer {
                labels: vec!["laptop".into(), "studio".into()]
            })
        );
        assert_eq!(
            target.desktop_at(&live[..1], None, Need::Accessibility, 1_000),
            Ok(ResolvedTarget {
                machine_id: STUDIO.into(),
                label: "studio".into()
            })
        );
        assert_eq!(
            target.desktop_at(&[], None, Need::None, 1_000),
            Err(TargetRefusal::NoneConnected)
        );
        assert_eq!(
            target.desktop_at(&live[..1], Some(LAPTOP), Need::None, 1_000),
            Err(TargetRefusal::Unavailable {
                label: LAPTOP.into()
            })
        );
        let stale = 1_000 + crate::domain::machine::STALE_HEARTBEAT_SECS + 1;
        assert_eq!(
            target.desktop_at(&live[..1], None, Need::None, stale),
            Err(TargetRefusal::Unhealthy {
                label: "studio".into(),
                age_secs: crate::domain::machine::STALE_HEARTBEAT_SECS + 1
            })
        );
        let chosen = MachineTarget::new(TargetSelection::Machine(LAPTOP.into()));
        assert_eq!(
            chosen.desktop_at(&live, Some(STUDIO), Need::None, 1_000),
            Err(TargetRefusal::Mismatch {
                chosen: LAPTOP.into(),
                requested: STUDIO.into()
            })
        );
        assert_eq!(
            MachineTarget::new(TargetSelection::ServerHome).desktop_at(
                &live,
                None,
                Need::None,
                1_000
            ),
            Err(TargetRefusal::ServerHome)
        );
        for refusal in [
            TargetRefusal::ChooseAComputer { labels: vec![] },
            TargetRefusal::NoneConnected,
            TargetRefusal::Unavailable { label: "x".into() },
            TargetRefusal::Unhealthy {
                label: "x".into(),
                age_secs: 1,
            },
            TargetRefusal::PermissionDenied {
                label: "x".into(),
                permission: "Accessibility",
                state: Permission::Denied,
            },
            TargetRefusal::ServerHome,
            TargetRefusal::Mismatch {
                chosen: "a".into(),
                requested: "b".into(),
            },
        ] {
            let text = refusal.to_string();
            assert!(
                text.starts_with(&format!("{}: ", refusal.code())),
                "every refusal is typed by its code: {text}"
            );
        }
    }
}
