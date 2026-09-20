use cua_protocol::{
    CheckedCuaAdapter, MachineDescriptor, MachineId, SelectionError, select_machine,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::Arc;
use tokio::sync::{Mutex, oneshot};

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

/// Registry of the machines this server can control: the connected Tauri
/// agents (legacy WebSocket toolcalls) and the Cua targets.
#[derive(Clone)]
pub struct MachineRegistry {
    /// Connected agents: machine_id → (info, sender)
    agents: Arc<Mutex<HashMap<String, (MachineInfo, AgentSender)>>>,
    /// Pending action requests: request_id → oneshot sender
    pending: Arc<Mutex<HashMap<String, PendingAction>>>,
    /// Cua targets speaking the shared protocol.
    cua: CuaTargets,
}

impl MachineRegistry {
    pub fn new() -> Self {
        Self {
            agents: Arc::new(Mutex::new(HashMap::new())),
            pending: Arc::new(Mutex::new(HashMap::new())),
            cua: CuaTargets::new(),
        }
    }

    /// The Cua targets registered alongside the legacy agents.
    pub fn cua(&self) -> &CuaTargets {
        &self.cua
    }

    /// Register a new agent connection.
    ///
    /// Machines are contexts of the one companion this server owns; a
    /// registration naming any other slug is bound to the canonical one.
    pub async fn register(&self, mut info: MachineInfo, sender: AgentSender) {
        use crate::domain::companion::{CANONICAL_SLUG, is_canonical};
        if let Some(requested) = info.instance_slug.as_deref()
            && !is_canonical(requested)
        {
            log::warn!(
                "[machines] {} requested foreign companion {requested:?}; binding to {CANONICAL_SLUG}",
                info.machine_id
            );
        }
        info.instance_slug = Some(CANONICAL_SLUG.to_owned());
        let id = info.machine_id.clone();
        log::info!("[machines] registered: {} ({})", id, info.os);
        self.agents.lock().await.insert(id, (info, sender));
    }

    /// Remove a disconnected agent.
    pub async fn unregister(&self, machine_id: &str) {
        log::info!("[machines] unregistered: {machine_id}");
        self.agents.lock().await.remove(machine_id);
    }

    /// Update last_seen timestamp.
    pub async fn heartbeat(&self, machine_id: &str) {
        if let Some((info, _)) = self.agents.lock().await.get_mut(machine_id) {
            info.last_seen = chrono::Utc::now().timestamp();
        }
    }

    /// List all connected machines.
    pub async fn list(&self) -> Vec<MachineInfo> {
        self.agents
            .lock()
            .await
            .values()
            .map(|(info, _)| info.clone())
            .collect()
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
                .map(|(_, s)| s.clone())
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
            self.agents.lock().await.remove(machine_id);
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
