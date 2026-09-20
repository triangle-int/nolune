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
    /// The machine is connected right now, so it cannot be forgotten.
    Online(String),
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
            Self::Online(_) => "machine_online",
            Self::Unsupported(_) => "machines_format_unsupported",
            Self::Io(_) => "storage_error",
        }
    }
}

impl fmt::Display for MachineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(id) => write!(f, "unknown machine {id}"),
            Self::Online(id) => write!(f, "machine {id} is connected"),
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
    targets: Arc<Mutex<BTreeMap<MachineId, CuaTarget>>>,
}

/// One registered target and the labels its descriptor does not carry.
struct CuaTarget {
    adapter: Arc<CheckedCuaAdapter>,
    /// The host's name, known for the server-local target only.
    hostname: Option<String>,
    registered_at: i64,
}

/// A server-local target as `GET /machines` lists it: the descriptor plus
/// the labels a descriptor does not carry.
#[derive(Clone, Debug, PartialEq)]
pub struct ServerLocalEntry {
    pub descriptor: MachineDescriptor,
    /// The host's name for display; the id is derived from it (#16).
    pub hostname: String,
    /// Unix seconds of the registration: `first_seen` for the row.
    pub registered_at: i64,
}

impl CuaTargets {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a server-local target with the hostname the API shows for it and
    /// the time it registered. Fails when its machine id is already registered.
    pub async fn register_server_local(
        &self,
        adapter: CheckedCuaAdapter,
        hostname: &str,
        registered_at: i64,
    ) -> Result<(), CuaRegistrationError> {
        self.insert(adapter, Some(hostname.to_owned()), registered_at)
            .await
    }

    /// The server-local targets as the known-machines listing shows them.
    pub async fn server_local_entries(&self) -> Vec<ServerLocalEntry> {
        self.targets
            .lock()
            .await
            .values()
            .filter(|target| target.adapter.descriptor().location == MachineLocation::ServerLocal)
            .map(|target| ServerLocalEntry {
                descriptor: target.adapter.descriptor().clone(),
                hostname: target.hostname.clone().unwrap_or_else(|| {
                    target
                        .adapter
                        .descriptor()
                        .machine_id
                        .as_str()
                        .trim_start_matches(crate::services::cua::host::SERVER_LOCAL_PREFIX)
                        .to_owned()
                }),
                registered_at: target.registered_at,
            })
            .collect()
    }

    /// Add a target. Fails when its machine id is already registered.
    #[allow(dead_code)] // Desktop targets register here (#17).
    pub async fn register(&self, adapter: CheckedCuaAdapter) -> Result<(), CuaRegistrationError> {
        self.insert(adapter, None, chrono::Utc::now().timestamp())
            .await
    }

    async fn insert(
        &self,
        adapter: CheckedCuaAdapter,
        hostname: Option<String>,
        registered_at: i64,
    ) -> Result<(), CuaRegistrationError> {
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
        targets.insert(
            id,
            CuaTarget {
                adapter: Arc::new(adapter),
                hostname,
                registered_at,
            },
        );
        Ok(())
    }

    /// Remove a target; returns whether it was registered.
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
            .map(|target| target.adapter.descriptor().clone())
            .collect()
    }

    /// Resolve a target through `cua_protocol::select_machine`: the requested
    /// id when given, otherwise the only target, never a guess between several.
    #[allow(dead_code)] // Used by the typed machine tools (#17/#18).
    pub async fn select(
        &self,
        requested: Option<&MachineId>,
    ) -> Result<Arc<CheckedCuaAdapter>, SelectionError> {
        let targets = self.targets.lock().await;
        let descriptors: Vec<MachineDescriptor> = targets
            .values()
            .map(|target| target.adapter.descriptor().clone())
            .collect();
        let chosen = select_machine(&descriptors, requested)?;
        targets
            .get(&chosen.machine_id)
            .map(|target| target.adapter.clone())
            .ok_or(SelectionError::NotFound)
    }
}

/// The persisted known machines (#80): one bounded record per machine that
/// ever registered, under `instances/{slug}/machines.json`, so a disconnected
/// desktop stays listed as offline with its last-seen time and a reconnect
/// under the same stable id updates one record instead of adding another.
///
/// The file is read on every access and every change is written back through
/// a temp file and a rename, so a file the user fixes or removes takes effect
/// without a restart. A file with another version, another slug, unknown
/// fields, duplicate ids, or invalid JSON fails closed: reads answer
/// `MachineError::Unsupported` and nothing is ever written over it. Every
/// access first puts back the connected agents the store has no record for,
/// so a desktop that connected while the file was unusable is recorded as
/// soon as the file is readable again. Without a path
/// (`MachineRegistry::new`) the records live in memory only.
#[derive(Clone)]
struct KnownMachines {
    path: Option<PathBuf>,
    slug: String,
    /// Serializes every read-modify-write cycle.
    state: Arc<std::sync::Mutex<StoreState>>,
}

#[derive(Default)]
struct StoreState {
    /// The records of a store without a path.
    memory: BTreeMap<String, MachineRecord>,
    /// Why the last read failed, so one broken file is logged once.
    last_failure: Option<String>,
}

/// A connected agent as the store needs it: enough to rebuild its record.
#[derive(Clone)]
struct LiveSnapshot {
    info: MachineInfo,
    /// When this connection registered; `first_seen` for a record built from it.
    registered_at: i64,
}

/// Ids whose records a change dropped: a legacy record migrated into a
/// stable one, or the longest-offline record evicted to make room. Clients
/// hear about each as `machine_forgotten`.
type Forgotten = Vec<String>;

/// Larger than any file the bounded store writes (`save` refuses more), so
/// anything bigger is not ours.
const MAX_MACHINES_FILE_BYTES: usize = 1024 * 1024;
/// A connected machine's heartbeat reaches its record at least this often,
/// so a crash leaves `last_seen` at most a minute behind.
const HEARTBEAT_PERSIST_SECS: i64 = 60;
/// How often the health watch looks for heartbeats that went stale; a third
/// of `STALE_HEARTBEAT_SECS`, so clients hear within 15 seconds of the change.
const HEALTH_WATCH_SECS: u64 = 15;

impl KnownMachines {
    fn in_memory(slug: &str) -> Self {
        Self {
            path: None,
            slug: slug.to_owned(),
            state: Arc::new(std::sync::Mutex::new(StoreState::default())),
        }
    }

    fn at(workspace_dir: &Path, slug: &str) -> Self {
        Self {
            path: Some(
                workspace_dir
                    .join("instances")
                    .join(slug)
                    .join(crate::domain::machine::MACHINES_FILE),
            ),
            ..Self::in_memory(slug)
        }
    }

    /// Read the records, put back the connected agents that have none, run
    /// `f` over them under the store lock, and write the file back when
    /// anything changed. Returns `f`'s value and the ids that were dropped.
    fn with<T>(
        &self,
        live: &[LiveSnapshot],
        f: impl FnOnce(&mut BTreeMap<String, MachineRecord>) -> (T, bool),
    ) -> Result<(T, Forgotten), MachineError> {
        // A poisoned lock only means a writer panicked; the file is consistent.
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut records = if self.path.is_none() {
            state.memory.clone()
        } else {
            match self.load() {
                Ok(records) => {
                    if state.last_failure.take().is_some() {
                        log::info!("[machines] the known machines file is readable again");
                    }
                    records
                }
                Err(reason) => {
                    if state.last_failure.as_ref() != Some(&reason) {
                        log::error!(
                            "[machines] known machines file is unsupported; nothing is read or written: {reason}"
                        );
                        state.last_failure = Some(reason.clone());
                    }
                    return Err(MachineError::Unsupported(reason));
                }
            }
        };
        let mut forgotten = Vec::new();
        let mut changed = backfill_live(&mut records, live, &mut forgotten);
        let (value, changed_by_f) = f(&mut records);
        changed |= changed_by_f;
        if changed {
            if self.path.is_none() {
                state.memory = records;
            } else {
                self.save(&records)?;
            }
        }
        Ok((value, forgotten))
    }

    fn load(&self) -> Result<BTreeMap<String, MachineRecord>, String> {
        use crate::domain::machine::{MACHINES_FORMAT_VERSION, MachinesFile, validate_machine_id};
        let Some(path) = &self.path else {
            return Ok(BTreeMap::new());
        };
        let raw = match std::fs::read(path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(BTreeMap::new());
            }
            Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
        };
        if raw.len() > MAX_MACHINES_FILE_BYTES {
            return Err(format!(
                "{} is larger than {MAX_MACHINES_FILE_BYTES} bytes",
                path.display()
            ));
        }
        let file: MachinesFile = serde_json::from_slice(&raw)
            .map_err(|error| format!("{} is not a machines file: {error}", path.display()))?;
        if file.version != MACHINES_FORMAT_VERSION {
            return Err(format!(
                "{} has version {}, this server reads version {MACHINES_FORMAT_VERSION}",
                path.display(),
                file.version
            ));
        }
        if file.slug != self.slug {
            return Err(format!(
                "{} belongs to companion {:?}, not {:?}",
                path.display(),
                file.slug,
                self.slug
            ));
        }
        let mut records = BTreeMap::new();
        for record in file.machines {
            validate_machine_id(&record.machine_id)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            if records.insert(record.machine_id.clone(), record).is_some() {
                return Err(format!("{} lists a machine twice", path.display()));
            }
        }
        Ok(records)
    }

    /// Write the records; refused, leaving the previous file in place, when
    /// they would not fit under `MAX_MACHINES_FILE_BYTES`, so this store never
    /// writes a file `load` rejects.
    fn save(&self, records: &BTreeMap<String, MachineRecord>) -> Result<(), MachineError> {
        use crate::domain::machine::{MACHINES_FORMAT_VERSION, MachinesFile};
        let Some(path) = &self.path else {
            return Ok(());
        };
        let file = MachinesFile {
            version: MACHINES_FORMAT_VERSION,
            slug: self.slug.clone(),
            machines: records.values().cloned().collect(),
        };
        let json = serde_json::to_string_pretty(&file).map_err(std::io::Error::other)?;
        if json.len() > MAX_MACHINES_FILE_BYTES {
            return Err(MachineError::Io(format!(
                "refusing to write {}: {} bytes is more than the {MAX_MACHINES_FILE_BYTES} this server reads",
                path.display(),
                json.len()
            )));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        write_atomic(path, &json)?;
        Ok(())
    }
}

fn write_atomic(path: &Path, content: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)
}

/// The record as the API reports it, with what the registry knows right now.
fn known_view(
    record: &MachineRecord,
    live_last_seen: Option<i64>,
    now: i64,
    slug: &str,
) -> KnownMachine {
    use crate::domain::machine::heartbeat_health;
    let online = live_last_seen.is_some();
    let last_seen = live_last_seen.unwrap_or(record.last_seen);
    KnownMachine {
        machine_id: record.machine_id.clone(),
        display_name: record
            .display_name
            .clone()
            .unwrap_or_else(|| record.hostname.clone()),
        custom_name: record.display_name.clone(),
        hostname: record.hostname.clone(),
        os: record.os.clone(),
        platform: record.platform,
        location: record.location,
        screen_width: record.screen_width,
        screen_height: record.screen_height,
        permissions: record.permissions.clone(),
        capabilities: record.capabilities.clone(),
        first_seen: record.first_seen,
        last_seen,
        instance_slug: Some(slug.to_owned()),
        online,
        health: heartbeat_health(online, now - last_seen),
        driver_version: None,
        cua_health: None,
    }
}

/// The server-local target as the API reports it: online while the driver
/// is registered, with the driver's own health, permissions and
/// capabilities; there is no record, so nothing here is written to disk and
/// the row cannot be renamed.
fn server_local_view(entry: &ServerLocalEntry, now: i64, slug: &str) -> KnownMachine {
    let descriptor = &entry.descriptor;
    let os = serde_json::to_value(descriptor.platform)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default();
    KnownMachine {
        machine_id: descriptor.machine_id.as_str().to_owned(),
        display_name: entry.hostname.clone(),
        custom_name: None,
        hostname: entry.hostname.clone(),
        os,
        platform: Some(descriptor.platform),
        location: descriptor.location,
        screen_width: 0,
        screen_height: 0,
        permissions: Some(descriptor.permissions.clone()),
        capabilities: descriptor
            .capabilities
            .iter()
            .filter_map(|capability| {
                serde_json::to_value(capability)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
            })
            .collect(),
        first_seen: entry.registered_at,
        last_seen: now,
        instance_slug: Some(slug.to_owned()),
        online: true,
        health: descriptor.health,
        driver_version: Some(descriptor.driver_version.as_str().to_owned()),
        cua_health: Some(descriptor.health),
    }
}

/// Give a connected agent the record it has none of: a desktop upgraded from
/// the hostname-keyed id takes over its offline legacy record (keeping
/// `first_seen` and the user's name), otherwise a new record is added,
/// evicting the longest-offline one when the store is full. Returns whether
/// anything changed; dropped ids land in `forgotten`.
fn insert_record(
    records: &mut BTreeMap<String, MachineRecord>,
    agent: &LiveSnapshot,
    online: &std::collections::HashSet<String>,
    forgotten: &mut Forgotten,
) -> bool {
    use crate::domain::machine::MAX_KNOWN_MACHINES;
    let info = &agent.info;
    // Before #80 the desktop registered under its hostname, as both id and
    // hostname. That offline record is this computer, so it moves under the
    // stable id instead of staying behind as a duplicate.
    let legacy = (info.hostname != info.machine_id && !online.contains(&info.hostname))
        .then(|| records.get(&info.hostname))
        .flatten()
        .filter(|legacy| legacy.hostname == legacy.machine_id)
        .map(|legacy| legacy.machine_id.clone());
    let (display_name, first_seen) = match legacy.and_then(|id| records.remove(&id)) {
        Some(legacy) => {
            log::info!(
                "[machines] '{}' now registers as '{}'; moving its record",
                legacy.machine_id,
                info.machine_id
            );
            forgotten.push(legacy.machine_id);
            (legacy.display_name, legacy.first_seen)
        }
        None => (None, agent.registered_at),
    };
    if records.len() >= MAX_KNOWN_MACHINES {
        let evict = records
            .values()
            .filter(|record| !online.contains(&record.machine_id))
            .min_by_key(|record| (record.last_seen, record.machine_id.clone()))
            .map(|record| record.machine_id.clone());
        match evict {
            Some(id) => {
                log::info!(
                    "[machines] forgetting '{id}' to make room for '{}'",
                    info.machine_id
                );
                records.remove(&id);
                forgotten.push(id);
            }
            None => {
                log::warn!(
                    "[machines] {MAX_KNOWN_MACHINES} machines are online; '{}' is not recorded",
                    info.machine_id
                );
                return false;
            }
        }
    }
    records.insert(
        info.machine_id.clone(),
        MachineRecord {
            machine_id: info.machine_id.clone(),
            display_name,
            hostname: info.hostname.clone(),
            os: info.os.clone(),
            platform: info.platform,
            location: info.location,
            screen_width: info.screen_width,
            screen_height: info.screen_height,
            permissions: info.permissions.clone(),
            capabilities: info.capabilities.clone(),
            first_seen,
            last_seen: info.last_seen,
        },
    );
    true
}

/// Every connected agent with a usable id has a record: the ones missing
/// (registered while the file was unusable, or forgotten while connected)
/// get theirs back. Returns whether anything changed.
fn backfill_live(
    records: &mut BTreeMap<String, MachineRecord>,
    live: &[LiveSnapshot],
    forgotten: &mut Forgotten,
) -> bool {
    use crate::domain::machine::validate_machine_id;
    let online: std::collections::HashSet<String> = live
        .iter()
        .map(|agent| agent.info.machine_id.clone())
        .collect();
    let mut changed = false;
    for agent in live {
        if records.contains_key(&agent.info.machine_id)
            || validate_machine_id(&agent.info.machine_id).is_err()
        {
            continue;
        }
        changed |= insert_record(records, agent, &online, forgotten);
    }
    changed
}

/// Refresh a machine's record from a registration, keeping `first_seen` and
/// the user's name. The record exists by now (`backfill_live`) unless the
/// store is full of connected machines. Returns whether anything changed.
fn refresh_record(
    records: &mut BTreeMap<String, MachineRecord>,
    info: &MachineInfo,
    now: i64,
) -> bool {
    let Some(record) = records.get_mut(&info.machine_id) else {
        return false;
    };
    record.hostname = info.hostname.clone();
    record.os = info.os.clone();
    record.platform = info.platform;
    record.location = info.location;
    record.screen_width = info.screen_width;
    record.screen_height = info.screen_height;
    record.permissions = info.permissions.clone();
    record.capabilities = info.capabilities.clone();
    record.last_seen = now;
    true
}

/// One connected agent: what it registered, where to send toolcalls, which
/// socket it belongs to, and when its heartbeat was last written to the record.
struct LiveAgent {
    info: MachineInfo,
    sender: AgentSender,
    /// Distinguishes a reconnect from the socket it replaced (`unregister_connection`).
    connection: u64,
    /// When this connection registered: `first_seen` if the record has to be rebuilt.
    registered_at: i64,
    last_persisted: i64,
    /// The health clients last heard about, so the health watch and a
    /// recovering heartbeat each report one change once.
    announced_health: cua_protocol::MachineHealth,
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
    /// Hands each registration its own connection number.
    next_connection: Arc<std::sync::atomic::AtomicU64>,
    /// Record updates for connected clients. None in tests that do not care.
    events: Option<tokio::sync::broadcast::Sender<crate::domain::events::ServerEvent>>,
}

impl MachineRegistry {
    /// An in-memory registry: known machines do not survive the process.
    /// Production always opens one under the workspace (`AppState::new_in`).
    #[cfg(test)]
    pub fn new() -> Self {
        Self::with_known(KnownMachines::in_memory(
            crate::domain::companion::CANONICAL_SLUG,
        ))
    }

    /// A registry whose known machines persist under
    /// `instances/{slug}/machines.json`. The file is read on every access.
    pub fn open(workspace_dir: &Path, slug: &str) -> Self {
        Self::with_known(KnownMachines::at(workspace_dir, slug))
    }

    fn with_known(known: KnownMachines) -> Self {
        Self {
            agents: Arc::new(Mutex::new(HashMap::new())),
            pending: Arc::new(Mutex::new(HashMap::new())),
            cua: CuaTargets::new(),
            known,
            next_connection: Arc::new(std::sync::atomic::AtomicU64::new(1)),
            events: None,
        }
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

    /// Watch heartbeat age in the background: a connected computer whose
    /// heartbeat goes stale is reported as `degraded` within
    /// `HEALTH_WATCH_SECS`, so clients never have to re-derive it from a clock.
    pub fn start_health_watch(&self) {
        let registry = self.clone();
        tokio::spawn(async move {
            let mut ticks =
                tokio::time::interval(std::time::Duration::from_secs(HEALTH_WATCH_SECS));
            ticks.tick().await; // the first tick is immediate
            loop {
                ticks.tick().await;
                registry.sweep_health().await;
            }
        });
    }

    /// One pass of the health watch against the wall clock.
    pub async fn sweep_health(&self) -> Vec<String> {
        self.sweep_health_at(chrono::Utc::now().timestamp()).await
    }

    /// Report every connected machine whose derived health differs from what
    /// clients last heard, as `machine_updated`; returns the ids it broadcast.
    pub async fn sweep_health_at(&self, now: i64) -> Vec<String> {
        use crate::domain::machine::heartbeat_health;
        let changed: Vec<String> = {
            let mut agents = self.agents.lock().await;
            agents
                .values_mut()
                .filter_map(|agent| {
                    let health = heartbeat_health(true, now - agent.info.last_seen);
                    if health == agent.announced_health {
                        return None;
                    }
                    agent.announced_health = health;
                    Some(agent.info.machine_id.clone())
                })
                .collect()
        };
        for machine_id in &changed {
            log::info!("[machines] '{machine_id}' heartbeat health changed");
            self.broadcast(machine_id, now).await;
        }
        changed
    }

    /// Register a new agent connection and return its connection number, which
    /// `unregister_connection` needs so a stale socket closing after a
    /// reconnect never disconnects the live one.
    ///
    /// Machines are contexts of the one companion this server owns; a
    /// registration naming any other slug is bound to the canonical one.
    /// `info.last_seen` is the registration time; it also becomes
    /// `first_seen` for a machine never seen before.
    pub async fn register(&self, mut info: MachineInfo, sender: AgentSender) -> u64 {
        use crate::domain::companion::{CANONICAL_SLUG, is_canonical};
        use crate::domain::machine::{
            normalize_capabilities, normalize_label, validate_machine_id,
        };
        if let Some(requested) = info.instance_slug.as_deref()
            && !is_canonical(requested)
        {
            log::warn!(
                "[machines] {} requested foreign companion {requested:?}; binding to {CANONICAL_SLUG}",
                info.machine_id
            );
        }
        info.instance_slug = Some(CANONICAL_SLUG.to_owned());
        // The one place labels and capabilities are bounded and normalized:
        // the live entry, the record and the file all carry the same values.
        info.hostname = normalize_label(&info.hostname);
        info.os = normalize_label(&info.os);
        info.capabilities = normalize_capabilities(std::mem::take(&mut info.capabilities));
        let id = info.machine_id.clone();
        let now = info.last_seen;
        log::info!(
            "[machines] registered: {} ({}, {})",
            id,
            info.hostname,
            info.os
        );

        let connection = self
            .next_connection
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.agents.lock().await.insert(
            id.clone(),
            LiveAgent {
                info: info.clone(),
                sender,
                connection,
                registered_at: now,
                last_persisted: now,
                announced_health: cua_protocol::MachineHealth::Healthy,
            },
        );

        match validate_machine_id(&id) {
            // The record exists by now (`backfill_live` on this very access);
            // this refreshes it from the registration.
            Ok(()) => {
                self.persist(|records| refresh_record(records, &info, now))
                    .await
            }
            Err(error) => log::warn!("[machines] '{id}' is not recorded: {error}"),
        }
        self.broadcast(&id, now).await;
        connection
    }

    /// Remove a disconnected agent whatever socket it holds; its record
    /// stays, offline, seen now. Production paths know their connection and
    /// use `unregister_connection`.
    #[cfg(test)]
    pub async fn unregister(&self, machine_id: &str) {
        self.unregister_at(machine_id, chrono::Utc::now().timestamp())
            .await;
    }

    #[cfg(test)]
    pub async fn unregister_at(&self, machine_id: &str, now: i64) {
        let removed = self.agents.lock().await.remove(machine_id).is_some();
        self.mark_offline(machine_id, removed, now).await;
    }

    /// Remove the agent only if `connection` is still the one registered
    /// under `machine_id`; a socket that was already replaced by a reconnect
    /// leaves the live entry alone but is logged.
    pub async fn unregister_connection(&self, machine_id: &str, connection: u64) {
        let now = chrono::Utc::now().timestamp();
        let removed = {
            let mut agents = self.agents.lock().await;
            match agents.get(machine_id) {
                Some(agent) if agent.connection == connection => {
                    agents.remove(machine_id);
                    true
                }
                _ => false,
            }
        };
        self.mark_offline(machine_id, removed, now).await;
    }

    async fn mark_offline(&self, machine_id: &str, removed: bool, now: i64) {
        if !removed {
            log::info!("[machines] '{machine_id}' socket closed; a newer connection stays");
            return;
        }
        log::info!("[machines] unregistered: {machine_id}");
        self.persist(|records| match records.get_mut(machine_id) {
            Some(record) => {
                record.last_seen = now;
                true
            }
            None => false,
        })
        .await;
        self.broadcast(machine_id, now).await;
    }

    /// Update last_seen timestamp.
    pub async fn heartbeat(&self, machine_id: &str) {
        self.heartbeat_at(machine_id, chrono::Utc::now().timestamp())
            .await;
    }

    pub async fn heartbeat_at(&self, machine_id: &str, now: i64) {
        use crate::domain::machine::heartbeat_health;
        use cua_protocol::MachineHealth;
        let (persist, recovered) = {
            let mut agents = self.agents.lock().await;
            let Some(agent) = agents.get_mut(machine_id) else {
                return;
            };
            let before = heartbeat_health(true, now - agent.info.last_seen);
            agent.info.last_seen = now;
            let persist = now - agent.last_persisted >= HEARTBEAT_PERSIST_SECS;
            if persist {
                agent.last_persisted = now;
            }
            // Stale by the clock, or already reported stale by the health watch.
            let recovered = before == MachineHealth::Degraded
                || agent.announced_health == MachineHealth::Degraded;
            agent.announced_health = MachineHealth::Healthy;
            (persist, recovered)
        };
        if persist {
            self.persist(|records| match records.get_mut(machine_id) {
                Some(record) => {
                    record.last_seen = now;
                    true
                }
                None => false,
            })
            .await;
        }
        // A healthy heartbeat is routine; the one that ends a stale stretch is news.
        if recovered {
            self.broadcast(machine_id, now).await;
        }
    }

    /// The connected agents as the store needs them.
    async fn live_snapshot(&self) -> Vec<LiveSnapshot> {
        self.agents
            .lock()
            .await
            .values()
            .map(|agent| LiveSnapshot {
                info: agent.info.clone(),
                registered_at: agent.registered_at,
            })
            .collect()
    }

    /// Apply a change to the records; the unsupported-file case was logged by
    /// the store and an I/O failure is logged here, neither stops the live agent.
    async fn persist(&self, change: impl FnOnce(&mut BTreeMap<String, MachineRecord>) -> bool) {
        let live = self.live_snapshot().await;
        match self.known.with(&live, |records| ((), change(records))) {
            Ok(((), forgotten)) => self.broadcast_forgotten(forgotten).await,
            Err(MachineError::Unsupported(_)) => {}
            Err(error) => log::error!("[machines] could not persist known machines: {error}"),
        }
    }

    async fn broadcast(&self, machine_id: &str, now: i64) {
        let Some(events) = &self.events else {
            return;
        };
        if let Ok(machine) = self.get_known(machine_id, now).await {
            let _ = events.send(crate::domain::events::ServerEvent::MachineUpdated {
                instance_slug: self.known.slug.clone(),
                machine,
            });
        }
    }

    async fn broadcast_forgotten(&self, forgotten: Forgotten) {
        let Some(events) = &self.events else {
            return;
        };
        for machine_id in forgotten {
            let _ = events.send(crate::domain::events::ServerEvent::MachineForgotten {
                instance_slug: self.known.slug.clone(),
                machine_id,
            });
        }
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
        let live = self.live_snapshot().await;
        let (records, forgotten) = self.known.with(&live, |records| (records.clone(), false))?;
        self.broadcast_forgotten(forgotten).await;
        let live_seen: HashMap<&str, i64> = live
            .iter()
            .map(|agent| (agent.info.machine_id.as_str(), agent.info.last_seen))
            .collect();
        // The server-local target (#16) is live state, never a record: its
        // row comes from the registered descriptor, and a record that claims
        // its id (the registration grammar allows the prefix) never shadows it.
        let server_local: Vec<KnownMachine> = self
            .cua
            .server_local_entries()
            .await
            .iter()
            .map(|entry| server_local_view(entry, now, &self.known.slug))
            .collect();
        let mut machines: Vec<KnownMachine> = records
            .values()
            .filter(|record| {
                !server_local
                    .iter()
                    .any(|row| row.machine_id == record.machine_id)
            })
            .map(|record| {
                known_view(
                    record,
                    live_seen.get(record.machine_id.as_str()).copied(),
                    now,
                    &self.known.slug,
                )
            })
            .collect();
        machines.extend(server_local);
        machines.sort_by(|a, b| {
            b.online
                .cmp(&a.online)
                .then(b.last_seen.cmp(&a.last_seen))
                .then_with(|| a.machine_id.cmp(&b.machine_id))
        });
        Ok(machines)
    }

    /// One known machine by id, as `known_at` reports it.
    pub async fn get_known(
        &self,
        machine_id: &str,
        now: i64,
    ) -> Result<KnownMachine, MachineError> {
        self.known_at(now)
            .await?
            .into_iter()
            .find(|machine| machine.machine_id == machine_id)
            .ok_or_else(|| MachineError::NotFound(machine_id.to_owned()))
    }

    /// Set or clear (blank) the user's name for a known machine.
    pub async fn rename(
        &self,
        machine_id: &str,
        display_name: Option<&str>,
    ) -> Result<KnownMachine, MachineError> {
        self.rename_at(machine_id, display_name, chrono::Utc::now().timestamp())
            .await
    }

    pub async fn rename_at(
        &self,
        machine_id: &str,
        display_name: Option<&str>,
        now: i64,
    ) -> Result<KnownMachine, MachineError> {
        let name = crate::domain::machine::normalize_display_name(display_name)
            .map_err(MachineError::Invalid)?;
        let live = self.live_snapshot().await;
        let (renamed, forgotten) =
            self.known
                .with(&live, |records| match records.get_mut(machine_id) {
                    Some(record) => {
                        let changed = record.display_name != name;
                        record.display_name = name;
                        (Ok(()), changed)
                    }
                    None => (Err(MachineError::NotFound(machine_id.to_owned())), false),
                })?;
        self.broadcast_forgotten(forgotten).await;
        renamed?;
        self.broadcast(machine_id, now).await;
        self.get_known(machine_id, now).await
    }

    /// Forget an offline known machine: its record and the user's name are
    /// dropped and clients hear `machine_forgotten`. A connected one is
    /// refused; if it connects again later it is simply new.
    pub async fn forget(&self, machine_id: &str) -> Result<(), MachineError> {
        let live = self.live_snapshot().await;
        if live.iter().any(|agent| agent.info.machine_id == machine_id) {
            return Err(MachineError::Online(machine_id.to_owned()));
        }
        let (removed, forgotten) =
            self.known
                .with(&live, |records| match records.remove(machine_id) {
                    Some(_) => (Ok(()), true),
                    None => (Err(MachineError::NotFound(machine_id.to_owned())), false),
                })?;
        self.broadcast_forgotten(forgotten).await;
        removed?;
        log::info!("[machines] forgot '{machine_id}'");
        self.broadcast_forgotten(vec![machine_id.to_owned()]).await;
        Ok(())
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
            // The agent's receive loop is gone. Drop its entry unless a
            // reconnect already replaced it with a live socket.
            let stale = self
                .agents
                .lock()
                .await
                .get(machine_id)
                .map(|agent| agent.sender.same_channel(&sender) as u64 * agent.connection);
            if let Some(connection) = stale.filter(|connection| *connection != 0) {
                self.unregister_connection(machine_id, connection).await;
            }
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
    async fn a_server_local_target_is_a_live_known_machine_row_without_a_record() {
        let registry = MachineRegistry::new();
        let advertised = descriptor("server-local:studio", MachineLocation::ServerLocal);
        registry
            .cua()
            .register_server_local(
                fake_adapter(advertised.clone()),
                "studio.local",
                1_700_000_000,
            )
            .await
            .unwrap();
        assert_eq!(
            registry.cua().server_local_entries().await,
            vec![ServerLocalEntry {
                descriptor: advertised.clone(),
                hostname: "studio.local".into(),
                registered_at: 1_700_000_000,
            }]
        );
        // Registered like any other target: listed, selectable, duplicate-checked.
        assert_eq!(registry.cua().list().await, vec![advertised.clone()]);
        assert_eq!(
            registry
                .cua()
                .register_server_local(fake_adapter(advertised.clone()), "again", 1)
                .await,
            Err(CuaRegistrationError::DuplicateMachineId(id(
                "server-local:studio"
            )))
        );

        let known = registry.known_at(1_700_000_600).await.unwrap();
        assert_eq!(known.len(), 1, "{known:?}");
        let row = &known[0];
        assert_eq!(row.machine_id, "server-local:studio");
        assert_eq!(row.location, MachineLocation::ServerLocal);
        assert_eq!(row.hostname, "studio.local");
        assert_eq!(row.display_name, "studio.local");
        assert_eq!(row.os, "macos");
        assert_eq!(row.platform, Some(Platform::Macos));
        assert_eq!((row.screen_width, row.screen_height), (0, 0));
        assert!(row.online);
        assert_eq!(row.health, MachineHealth::Healthy);
        assert_eq!(row.cua_health, Some(MachineHealth::Healthy));
        assert_eq!(row.driver_version.as_deref(), Some("0.28.2"));
        assert_eq!(row.capabilities, vec!["app_discovery".to_owned()]);
        assert_eq!(row.first_seen, 1_700_000_000);
        assert_eq!(row.last_seen, 1_700_000_600);
        // Never written to the file: it is live state, not a desktop record.
        assert!(
            registry
                .known
                .with(&[], |records| (records.is_empty(), false))
                .unwrap()
                .0,
            "no record was created for the server-local target"
        );
        assert_eq!(
            registry.rename("server-local:studio", Some("Home")).await,
            Err(MachineError::NotFound("server-local:studio".into())),
            "a live row cannot be renamed like a record"
        );

        // Unregistered targets leave no row behind.
        assert!(registry.cua().unregister(&id("server-local:studio")).await);
        assert!(registry.known_at(1_700_000_600).await.unwrap().is_empty());
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
    async fn a_stale_socket_closing_after_a_reconnect_leaves_the_live_one_alone() {
        let (_ws, registry) = harness();
        let (old_tx, _old_rx) = tokio::sync::mpsc::unbounded_channel();
        let first = registry
            .register(desktop(STABLE_ID, "studio", T0), old_tx)
            .await;
        let (new_tx, mut new_rx) = tokio::sync::mpsc::unbounded_channel();
        let second = registry
            .register(desktop(STABLE_ID, "studio", T0 + 5), new_tx)
            .await;
        assert_ne!(first, second);

        // The server notices the old socket died only after the reconnect.
        registry.unregister_connection(STABLE_ID, first).await;
        assert_eq!(registry.list().await.len(), 1, "the reconnect stays live");
        assert!(registry.get_known(STABLE_ID, T0 + 6).await.unwrap().online);

        let call = AgentToolCall {
            request_id: "req-00000002".into(),
            action: "screenshot".into(),
            params: serde_json::json!({}),
        };
        let registry_c = registry.clone();
        let call_c = call.clone();
        let pending = tokio::spawn(async move { registry_c.execute(STABLE_ID, call_c).await });
        let forwarded = new_rx
            .recv()
            .await
            .expect("toolcall reaches the live socket");
        assert!(forwarded.contains("req-00000002"));
        registry
            .complete(
                "req-00000002",
                ActionResult {
                    result_type: "action".into(),
                    image: None,
                    width: None,
                    height: None,
                    scale: None,
                    success: Some(true),
                    error: None,
                },
            )
            .await;
        assert!(pending.await.unwrap().is_ok());

        registry.unregister_connection(STABLE_ID, second).await;
        assert!(registry.list().await.is_empty());
        assert!(!registry.get_known(STABLE_ID, T0 + 7).await.unwrap().online);
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

    /// Review finding: the persisted `os` label was unbounded, so the store could
    /// write a file it would refuse to read back.
    #[tokio::test]
    async fn oversized_labels_are_cut_so_the_file_the_store_writes_is_one_it_reads() {
        use crate::domain::machine::MAX_LABEL_CHARS;
        let (ws, registry) = harness();
        let mut info = desktop(STABLE_ID, "studio", T0);
        info.os = "o".repeat(1_200_000);
        info.hostname = "h".repeat(10_000);
        let _rx = connect(&registry, info).await;

        let live = registry.list().await;
        assert_eq!(live[0].os.chars().count(), MAX_LABEL_CHARS);
        assert_eq!(live[0].hostname.chars().count(), MAX_LABEL_CHARS);

        let known = registry.known_at(T0 + 1).await.unwrap();
        assert_eq!(known.len(), 1);
        assert_eq!(known[0].os.chars().count(), MAX_LABEL_CHARS);
        assert_eq!(known[0].hostname.chars().count(), MAX_LABEL_CHARS);
        assert!(
            fs::metadata(machines_path(&ws)).unwrap().len() < MAX_MACHINES_FILE_BYTES as u64,
            "the written file stays within what load() accepts"
        );

        let reopened = MachineRegistry::open(ws.path(), CANONICAL_SLUG);
        let after_restart = reopened.known_at(T0 + 2).await.unwrap();
        assert_eq!(
            after_restart.len(),
            1,
            "the reopened registry still lists it"
        );
        assert_eq!(after_restart[0].machine_id, STABLE_ID);
    }

    #[test]
    fn the_store_refuses_to_write_a_file_it_could_not_read_back() {
        let ws = tempfile::tempdir().unwrap();
        let store = KnownMachines::at(ws.path(), CANONICAL_SLUG);
        let mut records = BTreeMap::new();
        let mut record = MachineRecord {
            machine_id: STABLE_ID.into(),
            display_name: None,
            hostname: "studio".into(),
            os: "o".repeat(MAX_MACHINES_FILE_BYTES + 1),
            platform: Some(Platform::Macos),
            location: MachineLocation::Desktop,
            screen_width: 1,
            screen_height: 1,
            permissions: None,
            capabilities: Vec::new(),
            first_seen: T0,
            last_seen: T0,
        };
        assert!(
            matches!(store.save(&records), Ok(())),
            "an empty store writes fine"
        );
        records.insert(STABLE_ID.into(), record.clone());
        let refused = store.save(&records);
        assert!(matches!(refused, Err(MachineError::Io(_))), "{refused:?}");
        let path = ws
            .path()
            .join("instances")
            .join(CANONICAL_SLUG)
            .join(MACHINES_FILE);
        assert!(
            fs::metadata(&path).unwrap().len() < 200,
            "the previous (empty) file is left in place"
        );
        assert!(
            !path.with_extension("tmp").exists(),
            "no temp file left behind"
        );

        record.os = "macos".into();
        records.insert(STABLE_ID.into(), record);
        store.save(&records).unwrap();
        assert!(store.load().is_ok(), "what save() writes, load() reads");
    }

    /// Review finding: the fail-closed state was cached for the process lifetime,
    /// so fixing or removing the file did nothing until a restart.
    #[tokio::test]
    async fn a_fixed_or_removed_machines_file_is_read_again_without_a_restart() {
        let (ws, registry) = harness();
        fs::write(machines_path(&ws), "{not json").unwrap();
        assert!(matches!(
            registry.known_at(T0).await,
            Err(MachineError::Unsupported(_))
        ));

        // A desktop connects while the file is broken: live, but not recorded.
        let _rx = connect(&registry, desktop(STABLE_ID, "studio", T0 + 1)).await;
        assert!(matches!(
            registry.known_at(T0 + 2).await,
            Err(MachineError::Unsupported(_))
        ));
        assert_eq!(fs::read_to_string(machines_path(&ws)).unwrap(), "{not json");

        // The user removes the file: the next read works and the connected
        // desktop is recorded without reconnecting.
        fs::remove_file(machines_path(&ws)).unwrap();
        let known = registry.known_at(T0 + 3).await.unwrap();
        assert_eq!(known.len(), 1, "the connected desktop gets its record back");
        assert_eq!(known[0].machine_id, STABLE_ID);
        assert!(known[0].online);
        assert_eq!(
            known[0].first_seen,
            T0 + 1,
            "recorded from its registration time"
        );
        assert!(machines_path(&ws).is_file(), "and it is persisted");

        // The record survives a disconnect and a restart like any other.
        registry.unregister_at(STABLE_ID, T0 + 4).await;
        let reopened = MachineRegistry::open(ws.path(), CANONICAL_SLUG);
        let after = reopened.known_at(T0 + 5).await.unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].last_seen, T0 + 4);

        // Broken again while running: fails closed again, then a fixed file is read.
        fs::write(machines_path(&ws), "{not json").unwrap();
        assert!(matches!(
            registry.known_at(T0 + 6).await,
            Err(MachineError::Unsupported(_))
        ));
        let fixed = MachinesFile {
            version: MACHINES_FORMAT_VERSION,
            slug: CANONICAL_SLUG.into(),
            machines: vec![],
        };
        fs::write(machines_path(&ws), serde_json::to_string(&fixed).unwrap()).unwrap();
        assert_eq!(registry.known_at(T0 + 7).await.unwrap(), vec![]);
    }

    /// Review finding: a desktop upgraded from the hostname-keyed id left a
    /// permanent duplicate next to its new UUID record.
    #[tokio::test]
    async fn a_desktop_upgraded_from_a_hostname_id_keeps_one_record() {
        let (ws, registry) = harness();
        let (events, mut rx) = tokio::sync::broadcast::channel(16);
        let registry = registry.with_events(events);

        // Before the upgrade the desktop registered under its hostname.
        let _old = connect(&registry, desktop("studio", "studio", T0)).await;
        registry.rename("studio", Some("Studio Mac")).await.unwrap();
        registry.unregister_at("studio", T0 + 10).await;
        while rx.try_recv().is_ok() {}

        // After the upgrade it registers under its persisted UUID, same hostname.
        let _new = connect(&registry, desktop(STABLE_ID, "studio", T0 + 500)).await;
        let known = registry.known_at(T0 + 501).await.unwrap();
        assert_eq!(
            known.len(),
            1,
            "the legacy record is migrated, not duplicated"
        );
        let machine = &known[0];
        assert_eq!(machine.machine_id, STABLE_ID);
        assert_eq!(
            machine.first_seen, T0,
            "the old record's first_seen survives"
        );
        assert_eq!(machine.display_name, "Studio Mac", "and the user's name");
        assert_eq!(machine.last_seen, T0 + 500);
        assert!(machine.online);

        // Clients drop the legacy row first, then receive the stable one.
        let ServerEvent::MachineForgotten { machine_id, .. } = rx.try_recv().unwrap() else {
            panic!("unexpected event");
        };
        assert_eq!(machine_id, "studio", "clients drop the legacy row");
        let ServerEvent::MachineUpdated { machine, .. } = rx.try_recv().unwrap() else {
            panic!("unexpected event");
        };
        assert_eq!(machine.machine_id, STABLE_ID);
        assert!(rx.try_recv().is_err(), "nothing else");

        let file: MachinesFile =
            serde_json::from_str(&fs::read_to_string(machines_path(&ws)).unwrap()).unwrap();
        assert_eq!(file.machines.len(), 1);
        assert_eq!(file.machines[0].machine_id, STABLE_ID);

        // Only an offline record keyed by the hostname is migrated: a connected
        // one with that id is another computer, and an existing UUID record wins.
        let _other = connect(&registry, desktop("laptop", "laptop", T0 + 600)).await;
        let _same_name = connect(&registry, desktop("other-uuid", "laptop", T0 + 601)).await;
        let ids: Vec<String> = registry
            .known_at(T0 + 602)
            .await
            .unwrap()
            .into_iter()
            .map(|m| m.machine_id)
            .collect();
        assert!(ids.contains(&"laptop".to_owned()), "{ids:?}");
        assert!(ids.contains(&"other-uuid".to_owned()), "{ids:?}");
        registry.unregister_at("laptop", T0 + 700).await;
        let _again = connect(&registry, desktop("other-uuid", "laptop", T0 + 800)).await;
        let ids: Vec<String> = registry
            .known_at(T0 + 801)
            .await
            .unwrap()
            .into_iter()
            .map(|m| m.machine_id)
            .collect();
        assert_eq!(ids.len(), 3, "{ids:?}");
        assert!(
            ids.contains(&"laptop".to_owned()),
            "a UUID record already exists: nothing to migrate"
        );
    }

    #[tokio::test]
    async fn an_offline_machine_can_be_forgotten() {
        let (ws, registry) = harness();
        let (events, mut rx) = tokio::sync::broadcast::channel(16);
        let registry = registry.with_events(events);
        let _rx = connect(&registry, desktop(STABLE_ID, "studio", T0)).await;
        let _other = connect(&registry, desktop("laptop", "laptop", T0)).await;
        registry.unregister_at("laptop", T0 + 1).await;
        while rx.try_recv().is_ok() {}

        assert_eq!(
            registry.forget(STABLE_ID).await,
            Err(MachineError::Online(STABLE_ID.into())),
            "a connected computer cannot be forgotten"
        );
        assert_eq!(
            registry.forget("nobody").await,
            Err(MachineError::NotFound("nobody".into()))
        );
        assert!(rx.try_recv().is_err(), "refusals are silent");

        registry.forget("laptop").await.unwrap();
        let ids: Vec<String> = registry
            .known_at(T0 + 2)
            .await
            .unwrap()
            .into_iter()
            .map(|m| m.machine_id)
            .collect();
        assert_eq!(ids, vec![STABLE_ID.to_owned()]);
        let ServerEvent::MachineForgotten {
            instance_slug,
            machine_id,
        } = rx.try_recv().unwrap()
        else {
            panic!("unexpected event");
        };
        assert_eq!(instance_slug, CANONICAL_SLUG);
        assert_eq!(machine_id, "laptop");
        assert_eq!(
            registry.forget("laptop").await,
            Err(MachineError::NotFound("laptop".into())),
            "gone"
        );

        let file: MachinesFile =
            serde_json::from_str(&fs::read_to_string(machines_path(&ws)).unwrap()).unwrap();
        assert_eq!(file.machines.len(), 1);

        // A forgotten computer that connects again is simply new.
        let _back = connect(&registry, desktop("laptop", "laptop", T0 + 900)).await;
        let back = registry.get_known("laptop", T0 + 901).await.unwrap();
        assert_eq!(back.first_seen, T0 + 900);
    }

    /// Review finding: health only went `degraded` by time and nothing watched
    /// the clock, so clients never heard about a stale heartbeat.
    #[tokio::test]
    async fn the_health_watch_broadcasts_a_stale_heartbeat_once() {
        let (_ws, registry) = harness();
        let (events, mut rx) = tokio::sync::broadcast::channel(16);
        let registry = registry.with_events(events);
        let _agent = connect(&registry, desktop(STABLE_ID, "studio", T0)).await;
        let _offline = connect(&registry, desktop("laptop", "laptop", T0)).await;
        registry.unregister_at("laptop", T0 + 1).await;
        while rx.try_recv().is_ok() {}

        assert_eq!(
            registry.sweep_health_at(T0 + 10).await,
            Vec::<String>::new()
        );
        assert!(
            rx.try_recv().is_err(),
            "a fresh heartbeat is nothing to report"
        );

        assert_eq!(
            registry
                .sweep_health_at(T0 + STALE_HEARTBEAT_SECS + 1)
                .await,
            vec![STABLE_ID.to_owned()],
            "the offline computer is unavailable already, not degraded"
        );
        let ServerEvent::MachineUpdated { machine, .. } = rx.try_recv().unwrap() else {
            panic!("unexpected event");
        };
        assert_eq!(machine.machine_id, STABLE_ID);
        assert!(machine.online);
        assert_eq!(machine.health, MachineHealth::Degraded);

        assert!(
            registry
                .sweep_health_at(T0 + STALE_HEARTBEAT_SECS + 30)
                .await
                .is_empty(),
            "still stale: announced once"
        );
        assert!(rx.try_recv().is_err());

        // The heartbeat that ends the stale stretch is broadcast, then the
        // watch has nothing to add.
        registry
            .heartbeat_at(STABLE_ID, T0 + STALE_HEARTBEAT_SECS + 40)
            .await;
        let ServerEvent::MachineUpdated { machine, .. } = rx.try_recv().unwrap() else {
            panic!("unexpected event");
        };
        assert_eq!(machine.health, MachineHealth::Healthy);
        assert!(
            registry
                .sweep_health_at(T0 + STALE_HEARTBEAT_SECS + 41)
                .await
                .is_empty()
        );
        assert!(rx.try_recv().is_err());

        // Disconnecting a degraded computer is reported as unavailable, once.
        registry
            .sweep_health_at(T0 + 2 * STALE_HEARTBEAT_SECS + 100)
            .await;
        let _ = rx.try_recv().unwrap();
        registry
            .unregister_at(STABLE_ID, T0 + 2 * STALE_HEARTBEAT_SECS + 101)
            .await;
        let ServerEvent::MachineUpdated { machine, .. } = rx.try_recv().unwrap() else {
            panic!("unexpected event");
        };
        assert_eq!(machine.health, MachineHealth::Unavailable);
        assert!(
            registry
                .sweep_health_at(T0 + 2 * STALE_HEARTBEAT_SECS + 102)
                .await
                .is_empty()
        );
        assert!(rx.try_recv().is_err());
    }
}
