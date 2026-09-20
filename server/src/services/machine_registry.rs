use cua_protocol::{
    CheckedCuaAdapter, MachineDescriptor, MachineId, MachineLocation, PermissionState, Platform,
    SelectionError, select_machine,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{Mutex, oneshot};

use crate::domain::machine::{KnownMachine, MachineRecord};

/// Info about a connected Tauri agent machine.
#[derive(Clone, Debug, Serialize)]
pub struct MachineInfo {
    pub machine_id: String,
    pub os: String,
    pub hostname: String,
    pub screen_width: u32,
    pub screen_height: u32,
    pub last_seen: i64,
    /// Instance this machine is bound to (if any).
    pub instance_slug: Option<String>,
    /// Derived from `os` at registration (#80); `None` for an OS the protocol does not name.
    pub platform: Option<Platform>,
    /// Every WebSocket agent is a desktop; the server home is a Cua target (#16).
    pub location: MachineLocation,
    /// Desktop permission state as reported at registration; `None` when not reported.
    pub permissions: Option<PermissionState>,
    /// Legacy action names the agent accepts (`normalize_capabilities`).
    pub capabilities: Vec<String>,
}

/// Why the known-machines store refused a read or a change (#80).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MachineError {
    NotFound(String),
    /// The input is not a keepable name or id.
    Invalid(String),
    /// The machines file is not one this server can read; nothing is read
    /// from or written to it until the user fixes or removes it.
    Unsupported(String),
    Io(String),
}

impl MachineError {
    /// Stable machine-readable code for API responses.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "not_found",
            Self::Invalid(_) => "invalid",
            Self::Unsupported(_) => "machines_format_unsupported",
            Self::Io(_) => "storage_error",
        }
    }
}

impl fmt::Display for MachineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(id) => write!(f, "unknown machine {id}"),
            Self::Invalid(message) | Self::Io(message) => f.write_str(message),
            Self::Unsupported(message) => {
                write!(f, "the known machines file is unsupported: {message}")
            }
        }
    }
}

impl std::error::Error for MachineError {}

impl From<std::io::Error> for MachineError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

/// A pending computer use request waiting for an agent to respond.
pub struct PendingAction {
    pub responder: oneshot::Sender<ActionResult>,
}

/// Result from a Tauri agent executing a computer use action.
#[derive(Debug, Clone, Deserialize)]
pub struct ActionResult {
    /// "screenshot" or "action"
    pub result_type: String,
    /// Base64 PNG (only for screenshots)
    pub image: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    #[allow(dead_code)]
    pub scale: Option<f64>,
    /// For action results
    pub success: Option<bool>,
    pub error: Option<String>,
}

/// Message sent to a connected agent over WebSocket.
/// Generic toolcall message — action + arbitrary params.
#[derive(Debug, Clone, Serialize)]
pub struct AgentToolCall {
    pub request_id: String,
    pub action: String,
    /// Action-specific parameters (flattened into the JSON).
    #[serde(flatten)]
    pub params: serde_json::Value,
}

/// Channel to send toolcalls to a connected agent.
type AgentSender = tokio::sync::mpsc::UnboundedSender<String>;

/// Why a Cua target could not be registered.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CuaRegistrationError {
    /// A target with this machine id is already registered.
    DuplicateMachineId(MachineId),
}

impl fmt::Display for CuaRegistrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateMachineId(id) => {
                write!(f, "cua machine '{}' is already registered", id.as_str())
            }
        }
    }
}

impl std::error::Error for CuaRegistrationError {}

/// Cua machine targets (#16): every machine the companion can drive through
/// the shared `cua_protocol` boundary, keyed by machine id.
///
/// A target is the descriptor it advertised at registration plus the checked
/// adapter that executes requests against it; the adapter owns the descriptor,
/// so the two can never disagree. Server-local and desktop targets share one
/// map, which is why duplicate ids are rejected at registration instead of
/// surfacing later as `SelectionError::DuplicateMachineId`.
#[derive(Clone, Default)]
pub struct CuaTargets {
    targets: Arc<Mutex<BTreeMap<MachineId, Arc<CheckedCuaAdapter>>>>,
}

impl CuaTargets {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a target. Fails when its machine id is already registered.
    #[allow(dead_code)] // Registered by the driver runtime (#16, transport slice).
    pub async fn register(&self, adapter: CheckedCuaAdapter) -> Result<(), CuaRegistrationError> {
        let descriptor = adapter.descriptor();
        let id = descriptor.machine_id.clone();
        let mut targets = self.targets.lock().await;
        if targets.contains_key(&id) {
            return Err(CuaRegistrationError::DuplicateMachineId(id));
        }
        log::info!(
            "[machines] cua target registered: {} ({:?}, {:?}, {:?})",
            id.as_str(),
            descriptor.location,
            descriptor.platform,
            descriptor.health
        );
        targets.insert(id, Arc::new(adapter));
        Ok(())
    }

    /// Remove a target; returns whether it was registered.
    #[allow(dead_code)] // Called when the driver runtime shuts down (#16, transport slice).
    pub async fn unregister(&self, machine_id: &MachineId) -> bool {
        let removed = self.targets.lock().await.remove(machine_id).is_some();
        if removed {
            log::info!(
                "[machines] cua target unregistered: {}",
                machine_id.as_str()
            );
        }
        removed
    }

    /// Descriptors of every registered target, ordered by machine id.
    pub async fn list(&self) -> Vec<MachineDescriptor> {
        self.targets
            .lock()
            .await
            .values()
            .map(|adapter| adapter.descriptor().clone())
            .collect()
    }

    /// Resolve a target through `cua_protocol::select_machine`: the requested
    /// id when given, otherwise the only target, never a guess between several.
    #[allow(dead_code)] // Used by the typed machine tools (#16, transport slice).
    pub async fn select(
        &self,
        requested: Option<&MachineId>,
    ) -> Result<Arc<CheckedCuaAdapter>, SelectionError> {
        let targets = self.targets.lock().await;
        let descriptors: Vec<MachineDescriptor> = targets
            .values()
            .map(|adapter| adapter.descriptor().clone())
            .collect();
        let chosen = select_machine(&descriptors, requested)?;
        targets
            .get(&chosen.machine_id)
            .cloned()
            .ok_or(SelectionError::NotFound)
    }
}

/// The persisted known machines (#80): one bounded record per machine that
/// ever registered, under `instances/{slug}/machines.json`, so a disconnected
/// desktop stays listed as offline with its last-seen time and a reconnect
/// under the same stable id updates one record instead of adding another.
///
/// The file is loaded once, on first use, and every change is written back
/// through a temp file and a rename. A file with another version, another
/// slug, unknown fields, duplicate ids, or invalid JSON fails closed: reads
/// answer `MachineError::Unsupported` and nothing is ever written over it.
/// Without a path (`MachineRegistry::new`) the records live in memory only.
#[derive(Clone)]
struct KnownMachines {
    path: Option<PathBuf>,
    slug: String,
    /// `None` until first use; then the records or the reason the file is unusable.
    state: Arc<std::sync::Mutex<Option<Result<BTreeMap<String, MachineRecord>, String>>>>,
}

/// One connected agent: what it registered, where to send toolcalls, and
/// when its heartbeat was last written to the record.
struct LiveAgent {
    info: MachineInfo,
    sender: AgentSender,
    last_persisted: i64,
}

/// Registry of the machines this server can control: the connected Tauri
/// agents (legacy WebSocket toolcalls), the Cua targets, and the persisted
/// known machines every agent is recorded in.
#[derive(Clone)]
pub struct MachineRegistry {
    /// Connected agents: machine_id → live agent
    agents: Arc<Mutex<HashMap<String, LiveAgent>>>,
    /// Pending action requests: request_id → oneshot sender
    pending: Arc<Mutex<HashMap<String, PendingAction>>>,
    /// Cua targets speaking the shared protocol.
    cua: CuaTargets,
    /// Every machine that ever registered (#80).
    known: KnownMachines,
    /// Record updates for connected clients. None in tests that do not care.
    events: Option<tokio::sync::broadcast::Sender<crate::domain::events::ServerEvent>>,
}

impl MachineRegistry {
    /// An in-memory registry: known machines do not survive the process.
    pub fn new() -> Self {
        let _ = MachineLocation::Desktop;
        todo!("MachineRegistry::new")
    }

    /// A registry whose known machines persist under
    /// `instances/{slug}/machines.json`. The file is read on first use.
    pub fn open(workspace_dir: &Path, slug: &str) -> Self {
        let _ = (workspace_dir, slug);
        todo!("MachineRegistry::open")
    }

    /// Broadcast every known-machine change as `machine_updated`.
    pub fn with_events(
        mut self,
        events: tokio::sync::broadcast::Sender<crate::domain::events::ServerEvent>,
    ) -> Self {
        self.events = Some(events);
        self
    }

    /// The Cua targets registered alongside the legacy agents.
    pub fn cua(&self) -> &CuaTargets {
        &self.cua
    }

    /// Register a new agent connection.
    ///
    /// Machines are contexts of the one companion this server owns; a
    /// registration naming any other slug is bound to the canonical one.
    /// `info.last_seen` is the registration time; it also becomes
    /// `first_seen` for a machine never seen before.
    pub async fn register(&self, info: MachineInfo, sender: AgentSender) {
        let _ = (info, sender);
        todo!("MachineRegistry::register")
    }

    /// Remove a disconnected agent; its record stays, offline, seen now.
    pub async fn unregister(&self, machine_id: &str) {
        self.unregister_at(machine_id, chrono::Utc::now().timestamp())
            .await;
    }

    pub async fn unregister_at(&self, machine_id: &str, now: i64) {
        let _ = (machine_id, now);
        todo!("MachineRegistry::unregister_at")
    }

    /// Update last_seen timestamp.
    pub async fn heartbeat(&self, machine_id: &str) {
        self.heartbeat_at(machine_id, chrono::Utc::now().timestamp())
            .await;
    }

    pub async fn heartbeat_at(&self, machine_id: &str, now: i64) {
        let _ = (machine_id, now);
        todo!("MachineRegistry::heartbeat_at")
    }

    /// List all connected machines.
    pub async fn list(&self) -> Vec<MachineInfo> {
        self.agents
            .lock()
            .await
            .values()
            .map(|agent| agent.info.clone())
            .collect()
    }

    /// Every known machine, online ones first, then most recently seen.
    pub async fn known(&self) -> Result<Vec<KnownMachine>, MachineError> {
        self.known_at(chrono::Utc::now().timestamp()).await
    }

    pub async fn known_at(&self, now: i64) -> Result<Vec<KnownMachine>, MachineError> {
        let _ = now;
        todo!("MachineRegistry::known_at")
    }

    /// One known machine by id, as `known_at` reports it.
    pub async fn get_known(
        &self,
        machine_id: &str,
        now: i64,
    ) -> Result<KnownMachine, MachineError> {
        let _ = (machine_id, now);
        todo!("MachineRegistry::get_known")
    }

    /// Set or clear (blank) the user's name for a known machine.
    pub async fn rename(
        &self,
        machine_id: &str,
        display_name: Option<&str>,
    ) -> Result<KnownMachine, MachineError> {
        let _ = (machine_id, display_name);
        todo!("MachineRegistry::rename")
    }

    /// Send a toolcall to a specific machine and wait for the result.
    pub async fn execute(
        &self,
        machine_id: &str,
        call: AgentToolCall,
    ) -> Result<ActionResult, String> {
        let request_id = call.request_id.clone();

        // Find the agent's sender
        let sender = {
            let agents = self.agents.lock().await;
            agents
                .get(machine_id)
                .map(|agent| agent.sender.clone())
                .ok_or_else(|| format!("machine '{machine_id}' not connected"))?
        };

        // Create oneshot for the response
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .await
            .insert(request_id.clone(), PendingAction { responder: tx });

        // Send the toolcall to the agent
        let msg = serde_json::to_string(&call).map_err(|e| e.to_string())?;
        if sender.send(msg).is_err() {
            self.pending.lock().await.remove(&request_id);
            self.unregister(machine_id).await;
            return Err(format!("machine '{machine_id}' disconnected (send failed)"));
        }

        log::info!(
            "[machines] sent toolcall {} to '{machine_id}', waiting...",
            &request_id[..8]
        );

        // Wait for response with timeout
        let result = tokio::time::timeout(std::time::Duration::from_secs(120), rx)
            .await
            .map_err(|_| {
                // Clean up pending on timeout
                let pending = self.pending.clone();
                let rid = request_id.clone();
                tokio::spawn(async move {
                    pending.lock().await.remove(&rid);
                });
                format!("computer use action timed out (120s) on '{machine_id}'")
            })?
            .map_err(|_| "agent disconnected before responding".to_string())?;

        log::info!("[machines] result received for {}", &request_id[..8]);

        Ok(result)
    }

    /// Complete a pending action request (called when agent sends result back).
    pub async fn complete(&self, request_id: &str, result: ActionResult) -> bool {
        if let Some(pending) = self.pending.lock().await.remove(request_id) {
            let _ = pending.responder.send(result);
            true
        } else {
            log::warn!("[machines] no pending request for {request_id}");
            false
        }
    }
}

#[cfg(test)]
mod companion_boundary_tests {
    use super::*;
    use crate::domain::companion::CANONICAL_SLUG;

    fn info(instance_slug: Option<&str>) -> MachineInfo {
        MachineInfo {
            machine_id: "machine-1".into(),
            os: "macos".into(),
            hostname: "studio".into(),
            screen_width: 1440,
            screen_height: 900,
            last_seen: 0,
            instance_slug: instance_slug.map(str::to_owned),
            platform: Some(Platform::Macos),
            location: MachineLocation::Desktop,
            permissions: None,
            capabilities: Vec::new(),
        }
    }

    #[tokio::test]
    async fn registered_machines_are_always_bound_to_the_canonical_companion() {
        for provided in [None, Some("alice"), Some("Companion"), Some(CANONICAL_SLUG)] {
            let registry = MachineRegistry::new();
            let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
            registry.register(info(provided), tx).await;
            let listed = registry.list().await;
            assert_eq!(listed.len(), 1, "{provided:?}");
            assert_eq!(
                listed[0].instance_slug.as_deref(),
                Some(CANONICAL_SLUG),
                "{provided:?} must bind to the canonical companion"
            );
        }
    }
}

#[cfg(test)]
mod cua_targets_tests {
    use super::*;
    use cua_protocol::{
        AppsResult, Capability, CuaAction, CuaActionResult, CuaRequestEnvelope, CuaResponse,
        CuaResponseEnvelope, DriverVersion, EmptyArgs, ListWindowsArgs, MachineHealth,
        MachineLocation, Permission, PermissionState, Platform, ProtocolVersion, RequestId,
    };

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
            capabilities: vec![Capability::AppDiscovery],
        }
    }

    /// An in-memory adapter: no driver binary, every request is answered with
    /// an empty app list correlated to the request it received.
    fn fake_adapter(descriptor: MachineDescriptor) -> CheckedCuaAdapter {
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

    fn request(machine_id: &str, action: CuaAction) -> CuaRequestEnvelope {
        CuaRequestEnvelope {
            version: ProtocolVersion::V1,
            request_id: RequestId::try_from("req-1").unwrap(),
            machine_id: MachineId::try_from(machine_id).unwrap(),
            action,
        }
    }

    fn id(machine_id: &str) -> MachineId {
        MachineId::try_from(machine_id).unwrap()
    }

    #[tokio::test]
    async fn register_lists_the_descriptor_the_adapter_advertised() {
        let targets = CuaTargets::new();
        assert!(targets.list().await.is_empty());

        let advertised = descriptor("server-local:studio", MachineLocation::ServerLocal);
        targets
            .register(fake_adapter(advertised.clone()))
            .await
            .unwrap();

        assert_eq!(targets.list().await, vec![advertised]);
    }

    #[tokio::test]
    async fn duplicate_machine_ids_are_rejected_at_registration() {
        let targets = CuaTargets::new();
        targets
            .register(fake_adapter(descriptor("studio", MachineLocation::Desktop)))
            .await
            .unwrap();

        let again = targets
            .register(fake_adapter(descriptor("studio", MachineLocation::Desktop)))
            .await;
        assert_eq!(
            again,
            Err(CuaRegistrationError::DuplicateMachineId(id("studio")))
        );
        assert_eq!(targets.list().await.len(), 1, "the first target survives");
    }

    #[tokio::test]
    async fn server_and_desktop_on_one_host_keep_distinct_ids_in_stable_order() {
        let targets = CuaTargets::new();
        // A desktop keyed by hostname and the server-local target on the same
        // host must both fit; the list is sorted so callers see a stable order.
        for (machine_id, location) in [
            ("studio", MachineLocation::Desktop),
            ("server-local:studio", MachineLocation::ServerLocal),
        ] {
            targets
                .register(fake_adapter(descriptor(machine_id, location)))
                .await
                .unwrap();
        }

        let listed = targets.list().await;
        let ids: Vec<&str> = listed.iter().map(|m| m.machine_id.as_str()).collect();
        assert_eq!(ids, ["server-local:studio", "studio"]);
    }

    #[tokio::test]
    async fn select_goes_through_the_protocol_selector() {
        let targets = CuaTargets::new();
        assert_eq!(
            targets.select(None).await.err(),
            Some(SelectionError::NoMachines)
        );

        targets
            .register(fake_adapter(descriptor("studio", MachineLocation::Desktop)))
            .await
            .unwrap();
        let only = targets.select(None).await.unwrap();
        assert_eq!(only.descriptor().machine_id, id("studio"));

        targets
            .register(fake_adapter(descriptor(
                "server-local:studio",
                MachineLocation::ServerLocal,
            )))
            .await
            .unwrap();
        assert_eq!(
            targets.select(None).await.err(),
            Some(SelectionError::Ambiguous),
            "two targets and no request must not guess"
        );
        let local = targets
            .select(Some(&id("server-local:studio")))
            .await
            .unwrap();
        assert_eq!(local.descriptor().location, MachineLocation::ServerLocal);
        assert_eq!(
            targets.select(Some(&id("elsewhere"))).await.err(),
            Some(SelectionError::NotFound)
        );
    }

    #[tokio::test]
    async fn selected_adapters_execute_inside_the_checked_boundary() {
        let targets = CuaTargets::new();
        targets
            .register(fake_adapter(descriptor("studio", MachineLocation::Desktop)))
            .await
            .unwrap();
        let adapter = targets.select(Some(&id("studio"))).await.unwrap();

        let allowed = request("studio", CuaAction::ListApps(EmptyArgs {}));
        let response = adapter.execute(&allowed).await.unwrap();
        assert_eq!(response.request_id, allowed.request_id);
        assert!(matches!(response.response, CuaResponse::Success { .. }));

        // The descriptor only advertises app discovery, so the checked adapter
        // refuses window discovery before the fake ever sees it.
        let denied = request(
            "studio",
            CuaAction::ListWindows(ListWindowsArgs {
                pid: None,
                on_screen_only: false,
            }),
        );
        assert!(adapter.execute(&denied).await.is_err());
    }

    #[tokio::test]
    async fn unregister_forgets_the_target() {
        let targets = CuaTargets::new();
        targets
            .register(fake_adapter(descriptor("studio", MachineLocation::Desktop)))
            .await
            .unwrap();

        assert!(targets.unregister(&id("studio")).await);
        assert!(!targets.unregister(&id("studio")).await, "already gone");
        assert!(targets.list().await.is_empty());
        assert_eq!(
            targets.select(None).await.err(),
            Some(SelectionError::NoMachines)
        );
    }

    #[tokio::test]
    async fn the_machine_registry_carries_the_cua_targets_beside_legacy_agents() {
        let registry = MachineRegistry::new();
        registry
            .cua()
            .register(fake_adapter(descriptor(
                "server-local:studio",
                MachineLocation::ServerLocal,
            )))
            .await
            .unwrap();

        assert!(registry.list().await.is_empty(), "legacy map is untouched");
        assert_eq!(registry.cua().list().await.len(), 1);
        assert_eq!(registry.clone().cua().list().await.len(), 1, "clones share");
    }
}

#[cfg(test)]
mod known_machines_tests {
    use super::*;
    use crate::domain::companion::CANONICAL_SLUG;
    use crate::domain::events::ServerEvent;
    use crate::domain::machine::{
        LEGACY_DESKTOP_CAPABILITIES, MACHINES_FILE, MACHINES_FORMAT_VERSION, MAX_KNOWN_MACHINES,
        MachinesFile, STALE_HEARTBEAT_SECS,
    };
    use cua_protocol::{MachineHealth, Permission};
    use std::fs;

    /// 2026-01-05 (Monday) 09:00:00 UTC
    const T0: i64 = 1_767_603_600;
    const STABLE_ID: &str = "4f3c1c2e-9b5e-4d2b-8f0a-1c2d3e4f5a6b";

    fn harness() -> (tempfile::TempDir, MachineRegistry) {
        let ws = tempfile::tempdir().unwrap();
        fs::create_dir_all(ws.path().join("instances").join(CANONICAL_SLUG)).unwrap();
        let registry = MachineRegistry::open(ws.path(), CANONICAL_SLUG);
        (ws, registry)
    }

    fn machines_path(ws: &tempfile::TempDir) -> PathBuf {
        ws.path()
            .join("instances")
            .join(CANONICAL_SLUG)
            .join(MACHINES_FILE)
    }

    fn desktop(machine_id: &str, hostname: &str, seen: i64) -> MachineInfo {
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
            permissions: Some(PermissionState {
                accessibility: Permission::Granted,
                screen_capture: Permission::Denied,
            }),
            capabilities: vec!["screenshot".into(), "bash".into()],
        }
    }

    async fn connect(registry: &MachineRegistry, info: MachineInfo) -> AgentReceiver {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        registry.register(info, tx).await;
        rx
    }

    type AgentReceiver = tokio::sync::mpsc::UnboundedReceiver<String>;

    #[tokio::test]
    async fn a_disconnected_machine_stays_listed_offline_with_its_last_seen() {
        let (ws, registry) = harness();
        assert_eq!(registry.known_at(T0).await.unwrap(), vec![]);

        let _rx = connect(&registry, desktop(STABLE_ID, "studio", T0)).await;
        let online = registry.known_at(T0 + 5).await.unwrap();
        assert_eq!(online.len(), 1);
        let machine = &online[0];
        assert_eq!(machine.machine_id, STABLE_ID);
        assert!(machine.online);
        assert_eq!(machine.health, MachineHealth::Healthy);
        assert_eq!(machine.display_name, "studio", "hostname until renamed");
        assert_eq!(machine.custom_name, None);
        assert_eq!(machine.location, MachineLocation::Desktop);
        assert_eq!(machine.platform, Some(Platform::Macos));
        assert_eq!(machine.first_seen, T0);
        assert_eq!(machine.last_seen, T0);
        assert_eq!(machine.instance_slug.as_deref(), Some(CANONICAL_SLUG));
        assert_eq!(machine.capabilities, vec!["screenshot", "bash"]);
        assert_eq!(
            machine.permissions.as_ref().unwrap().screen_capture,
            Permission::Denied
        );
        assert_eq!(machine.driver_version, None, "reserved for the Cua driver");
        assert_eq!(machine.cua_health, None, "reserved for the Cua driver");

        registry.unregister_at(STABLE_ID, T0 + 100).await;
        assert!(registry.list().await.is_empty(), "no longer connected");

        let offline = registry.known_at(T0 + 200).await.unwrap();
        assert_eq!(offline.len(), 1, "the record outlives the socket");
        assert!(!offline[0].online);
        assert_eq!(offline[0].health, MachineHealth::Unavailable);
        assert_eq!(offline[0].last_seen, T0 + 100, "seen when it disconnected");
        assert_eq!(offline[0].first_seen, T0);

        // The record survives a restart.
        let reopened = MachineRegistry::open(ws.path(), CANONICAL_SLUG);
        let after_restart = reopened.known_at(T0 + 300).await.unwrap();
        assert_eq!(after_restart, offline);

        let file: MachinesFile =
            serde_json::from_str(&fs::read_to_string(machines_path(&ws)).unwrap()).unwrap();
        assert_eq!(file.version, MACHINES_FORMAT_VERSION);
        assert_eq!(file.slug, CANONICAL_SLUG);
        assert_eq!(file.machines.len(), 1);
        assert_eq!(file.machines[0].machine_id, STABLE_ID);
    }

    #[tokio::test]
    async fn reconnecting_with_the_same_stable_id_updates_one_record() {
        let (ws, registry) = harness();
        let rx = connect(&registry, desktop(STABLE_ID, "studio", T0)).await;
        registry
            .rename(STABLE_ID, Some("Studio Mac"))
            .await
            .unwrap();
        drop(rx);
        registry.unregister_at(STABLE_ID, T0 + 10).await;

        // Same machine, new hostname and screen, after a reboot.
        let mut again = desktop(STABLE_ID, "studio-2", T0 + 500);
        again.screen_width = 2560;
        again.capabilities = Vec::new();
        let _rx = connect(&registry, again).await;

        let known = registry.known_at(T0 + 501).await.unwrap();
        assert_eq!(known.len(), 1, "no duplicate for a reconnect");
        let machine = &known[0];
        assert!(machine.online);
        assert_eq!(machine.hostname, "studio-2", "fresh registration wins");
        assert_eq!(machine.screen_width, 2560);
        assert_eq!(machine.first_seen, T0, "first_seen survives reconnects");
        assert_eq!(machine.last_seen, T0 + 500);
        assert_eq!(
            machine.display_name, "Studio Mac",
            "the user's name survives"
        );
        assert_eq!(machine.custom_name.as_deref(), Some("Studio Mac"));
        assert_eq!(
            machine.capabilities,
            LEGACY_DESKTOP_CAPABILITIES
                .iter()
                .map(|s| (*s).to_owned())
                .collect::<Vec<_>>(),
            "a desktop that reports nothing accepts the legacy action set"
        );

        // Re-registering while still connected replaces the live entry, not the record.
        let _rx2 = connect(&registry, desktop(STABLE_ID, "studio-3", T0 + 600)).await;
        assert_eq!(registry.list().await.len(), 1);
        let known = registry.known_at(T0 + 601).await.unwrap();
        assert_eq!(known.len(), 1);
        assert_eq!(known[0].hostname, "studio-3");
        assert_eq!(known[0].first_seen, T0);

        let file: MachinesFile =
            serde_json::from_str(&fs::read_to_string(machines_path(&ws)).unwrap()).unwrap();
        assert_eq!(file.machines.len(), 1);

        // A second computer is a second record, listed after the online one.
        registry.unregister_at(STABLE_ID, T0 + 700).await;
        let _rx3 = connect(&registry, desktop("other-id", "laptop", T0 + 800)).await;
        let ids: Vec<String> = registry
            .known_at(T0 + 801)
            .await
            .unwrap()
            .into_iter()
            .map(|m| m.machine_id)
            .collect();
        assert_eq!(ids, vec!["other-id".to_owned(), STABLE_ID.to_owned()]);
    }

    #[tokio::test]
    async fn a_corrupt_or_foreign_machines_file_fails_closed() {
        for (label, contents) in [
            ("garbage", "not json".to_owned()),
            (
                "future version",
                serde_json::json!({"version": 2, "slug": CANONICAL_SLUG, "machines": []})
                    .to_string(),
            ),
            (
                "foreign slug",
                serde_json::json!({"version": 1, "slug": "alice", "machines": []}).to_string(),
            ),
            (
                "unknown field",
                serde_json::json!({"version": 1, "slug": CANONICAL_SLUG, "machines": [], "extra": 1})
                    .to_string(),
            ),
            (
                "duplicate ids",
                {
                    let record = serde_json::json!({
                        "machine_id": STABLE_ID, "display_name": null, "hostname": "studio",
                        "os": "macos", "platform": "macos", "location": "desktop",
                        "screen_width": 1, "screen_height": 1, "permissions": null,
                        "capabilities": [], "first_seen": 1, "last_seen": 2
                    });
                    serde_json::json!({"version": 1, "slug": CANONICAL_SLUG, "machines": [record, record]})
                        .to_string()
                },
            ),
        ] {
            let (ws, registry) = harness();
            fs::write(machines_path(&ws), &contents).unwrap();

            let listed = registry.known_at(T0).await;
            assert!(
                matches!(listed, Err(MachineError::Unsupported(_))),
                "{label}: {listed:?}"
            );

            // Live computer use keeps working, but nothing touches the file.
            let _rx = connect(&registry, desktop(STABLE_ID, "studio", T0)).await;
            assert_eq!(registry.list().await.len(), 1, "{label}");
            registry.heartbeat_at(STABLE_ID, T0 + 500).await;
            assert!(
                matches!(
                    registry.rename(STABLE_ID, Some("Studio")).await,
                    Err(MachineError::Unsupported(_))
                ),
                "{label}"
            );
            registry.unregister_at(STABLE_ID, T0 + 600).await;
            assert!(
                matches!(registry.known_at(T0 + 601).await, Err(MachineError::Unsupported(_))),
                "{label}: still refused after live activity"
            );
            assert_eq!(
                fs::read_to_string(machines_path(&ws)).unwrap(),
                contents,
                "{label}: the file must never be rewritten"
            );
            assert!(
                !machines_path(&ws).with_extension("tmp").exists(),
                "{label}: no temp file left behind"
            );
        }
    }

    #[tokio::test]
    async fn health_is_derived_from_heartbeat_age() {
        let (_ws, registry) = harness();
        let _rx = connect(&registry, desktop(STABLE_ID, "studio", T0)).await;

        let fresh = registry.get_known(STABLE_ID, T0 + 10).await.unwrap();
        assert_eq!(fresh.health, MachineHealth::Healthy);

        let stale = registry
            .get_known(STABLE_ID, T0 + STALE_HEARTBEAT_SECS + 1)
            .await
            .unwrap();
        assert!(stale.online, "the socket is still open");
        assert_eq!(stale.health, MachineHealth::Degraded);

        registry
            .heartbeat_at(STABLE_ID, T0 + STALE_HEARTBEAT_SECS + 1)
            .await;
        let recovered = registry
            .get_known(STABLE_ID, T0 + STALE_HEARTBEAT_SECS + 2)
            .await
            .unwrap();
        assert_eq!(recovered.health, MachineHealth::Healthy);
        assert_eq!(recovered.last_seen, T0 + STALE_HEARTBEAT_SECS + 1);

        assert_eq!(
            registry.get_known("nobody", T0).await,
            Err(MachineError::NotFound("nobody".into()))
        );
    }

    #[tokio::test]
    async fn heartbeats_reach_the_record_so_a_crash_keeps_a_recent_last_seen() {
        let (ws, registry) = harness();
        let _rx = connect(&registry, desktop(STABLE_ID, "studio", T0)).await;
        for step in 1..=20 {
            registry.heartbeat_at(STABLE_ID, T0 + step * 15).await;
        }
        // Simulate a crash: reopen without unregistering.
        let reopened = MachineRegistry::open(ws.path(), CANONICAL_SLUG);
        let machine = reopened.get_known(STABLE_ID, T0 + 400).await.unwrap();
        assert!(!machine.online);
        assert!(
            machine.last_seen >= T0 + 240,
            "heartbeats are persisted at least every minute, got {}",
            machine.last_seen - T0
        );
    }

    #[tokio::test]
    async fn rename_is_bounded_and_blank_restores_the_hostname() {
        let (_ws, registry) = harness();
        let _rx = connect(&registry, desktop(STABLE_ID, "studio", T0)).await;

        let renamed = registry
            .rename(STABLE_ID, Some("  Studio Mac "))
            .await
            .unwrap();
        assert_eq!(renamed.display_name, "Studio Mac");
        assert_eq!(renamed.custom_name.as_deref(), Some("Studio Mac"));

        let cleared = registry.rename(STABLE_ID, Some("   ")).await.unwrap();
        assert_eq!(cleared.display_name, "studio");
        assert_eq!(cleared.custom_name, None);

        assert!(matches!(
            registry.rename(STABLE_ID, Some(&"n".repeat(65))).await,
            Err(MachineError::Invalid(_))
        ));
        assert_eq!(
            registry.rename("nobody", Some("x")).await,
            Err(MachineError::NotFound("nobody".into()))
        );

        // Offline machines can be renamed too.
        registry.unregister_at(STABLE_ID, T0 + 1).await;
        let offline = registry.rename(STABLE_ID, Some("Old Mac")).await.unwrap();
        assert!(!offline.online);
        assert_eq!(offline.display_name, "Old Mac");
    }

    #[tokio::test]
    async fn every_change_is_broadcast_as_machine_updated() {
        let (_ws, registry) = harness();
        let (events, mut rx) = tokio::sync::broadcast::channel(16);
        let registry = registry.with_events(events);

        let _agent = connect(&registry, desktop(STABLE_ID, "studio", T0)).await;
        registry.heartbeat_at(STABLE_ID, T0 + 15).await;
        registry.rename(STABLE_ID, Some("Studio")).await.unwrap();
        registry.unregister_at(STABLE_ID, T0 + 30).await;

        let mut seen = Vec::new();
        while let Ok(event) = rx.try_recv() {
            let ServerEvent::MachineUpdated {
                instance_slug,
                machine,
            } = event
            else {
                panic!("unexpected event");
            };
            assert_eq!(instance_slug, CANONICAL_SLUG);
            assert_eq!(machine.machine_id, STABLE_ID);
            seen.push((machine.online, machine.display_name, machine.last_seen));
        }
        assert_eq!(
            seen,
            vec![
                (true, "studio".to_owned(), T0),
                (true, "Studio".to_owned(), T0 + 15),
                (false, "Studio".to_owned(), T0 + 30),
            ],
            "register, rename and disconnect each broadcast once; a healthy heartbeat is silent"
        );

        let json = serde_json::to_string(&ServerEvent::MachineUpdated {
            instance_slug: CANONICAL_SLUG.into(),
            machine: registry.get_known(STABLE_ID, T0 + 31).await.unwrap(),
        })
        .unwrap();
        assert!(json.contains("\"type\":\"machine_updated\""));
        assert!(json.contains("\"health\":\"unavailable\""));
        assert!(json.contains("\"driver_version\":null"));
    }

    #[tokio::test]
    async fn a_recovered_heartbeat_is_broadcast() {
        let (_ws, registry) = harness();
        let (events, mut rx) = tokio::sync::broadcast::channel(16);
        let registry = registry.with_events(events);
        let _agent = connect(&registry, desktop(STABLE_ID, "studio", T0)).await;
        let _ = rx.try_recv().unwrap(); // registration

        registry.heartbeat_at(STABLE_ID, T0 + 15).await;
        assert!(rx.try_recv().is_err(), "healthy to healthy is silent");

        // The heartbeat that ends a stale stretch tells clients the machine recovered.
        registry
            .heartbeat_at(STABLE_ID, T0 + 15 + STALE_HEARTBEAT_SECS + 10)
            .await;
        let ServerEvent::MachineUpdated { machine, .. } = rx.try_recv().unwrap() else {
            panic!("unexpected event");
        };
        assert_eq!(machine.health, MachineHealth::Healthy);
    }

    #[tokio::test]
    async fn the_store_is_bounded_and_forgets_the_oldest_offline_machine_first() {
        let (_ws, registry) = harness();
        for i in 0..MAX_KNOWN_MACHINES {
            let id = format!("machine-{i:03}");
            let _rx = connect(&registry, desktop(&id, "host", T0 + i as i64)).await;
            registry.unregister_at(&id, T0 + i as i64).await;
        }
        assert_eq!(
            registry.known_at(T0 + 1000).await.unwrap().len(),
            MAX_KNOWN_MACHINES
        );

        let _rx = connect(&registry, desktop("machine-new", "host", T0 + 2000)).await;
        let known = registry.known_at(T0 + 2001).await.unwrap();
        assert_eq!(known.len(), MAX_KNOWN_MACHINES, "still bounded");
        let ids: Vec<&str> = known.iter().map(|m| m.machine_id.as_str()).collect();
        assert!(ids.contains(&"machine-new"));
        assert!(
            !ids.contains(&"machine-000"),
            "the oldest offline record made room"
        );
        assert!(ids.contains(&"machine-001"));
    }

    #[tokio::test]
    async fn an_in_memory_registry_keeps_known_machines_for_the_process_only() {
        let registry = MachineRegistry::new();
        let _rx = connect(&registry, desktop(STABLE_ID, "studio", T0)).await;
        registry.unregister_at(STABLE_ID, T0 + 1).await;
        let known = registry.known_at(T0 + 2).await.unwrap();
        assert_eq!(known.len(), 1);
        assert!(!known[0].online);
        assert_eq!(known[0].instance_slug.as_deref(), Some(CANONICAL_SLUG));
    }

    #[tokio::test]
    async fn a_failed_send_marks_the_machine_offline_like_a_disconnect() {
        let (_ws, registry) = harness();
        let (events, mut rx) = tokio::sync::broadcast::channel(16);
        let registry = registry.with_events(events);
        let agent = connect(&registry, desktop(STABLE_ID, "studio", T0)).await;
        let _ = rx.try_recv().unwrap();
        drop(agent);

        let result = registry
            .execute(
                STABLE_ID,
                AgentToolCall {
                    request_id: "req-00000001".into(),
                    action: "screenshot".into(),
                    params: serde_json::json!({}),
                },
            )
            .await;
        assert!(result.is_err());
        let ServerEvent::MachineUpdated { machine, .. } = rx.try_recv().unwrap() else {
            panic!("unexpected event");
        };
        assert!(!machine.online);
        assert!(registry.list().await.is_empty());
    }
}
