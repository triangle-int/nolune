//! The lifecycle of the server-local Cua target (#16): started once by the
//! gateway, registered into the shared `CuaTargets` while the driver is up,
//! driven through bounded per-run sessions, and stopped by the shutdown hook.
//!
//! Nothing here consults the host on construction; `start` does, so tests
//! that build an `AppState` never spawn a driver. A headless host or a driver
//! that cannot start leaves the runtime idle and the server healthy.
//!
//! While the driver runs, one watch task per target keeps the advertised
//! descriptor honest: a child that exits is unregistered at once, and a
//! fresh health report every `[cua].health_interval_secs` carries a
//! permission granted or revoked after startup into the descriptor.

use std::{
    future::Future,
    path::PathBuf,
    sync::{
        Arc, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use anyhow::Context as _;
use cua_protocol::{
    CheckedCuaAdapter, CuaAction, CuaRequestEnvelope, CuaResponse, CuaResponseEnvelope,
    MachineDescriptor, MachineId, ProtocolVersion, RequestId, SessionLabel, SessionRefArgs,
    StartSessionArgs, ValidationError,
};

use super::{
    discovery::{self, DriverLookupError},
    driver::{checked_adapter, describe_machine},
    host::{HostProbe, Skip, server_local_machine_id, startup_plan},
    install,
    session::{OpenSessions, is_run_managed, with_session},
    transport::{DriverTransport, StdioDriverTransport},
};
use crate::{config::CuaConfig, services::machine_registry::CuaTargets};

/// Session labels are `nolune-run-<n>`; request ids hang off them.
const RUN_LABEL_PREFIX: &str = "nolune-run-";

/// Where the runtime is in its life.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeStatus {
    /// `start` has not run.
    NotStarted,
    /// The host has no GUI target to offer; the server runs without one.
    Skipped(Skip),
    /// A driver was found but could not be started or described.
    Failed(String),
    /// The server-local target is registered under this id.
    Running(MachineId),
    /// The shutdown hook ran; the driver is gone.
    Stopped,
}

/// Why a run could not be executed.
#[derive(Debug)]
pub enum RunError {
    /// No server-local target is registered right now.
    Unavailable,
    /// The run outlived `[cua].run_timeout_secs`; its session was ended.
    Timeout(Duration),
    /// The run's body returned an error; its session was ended.
    Failed(anyhow::Error),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => f.write_str("no server-local computer-use target is available"),
            Self::Timeout(limit) => write!(f, "the run exceeded its {limit:?} limit"),
            Self::Failed(error) => write!(f, "the run failed: {error}"),
        }
    }
}

impl std::error::Error for RunError {}

/// Why one action inside a run was not executed.
#[derive(Debug)]
pub enum ExecError {
    /// The target does not advertise the capability, lacks the permission,
    /// is unavailable, or the action is one the run manages itself.
    Refused(String),
    /// The run's session could not be started, so the action never ran.
    SessionFailed(String),
    /// The driver answered, but the answer did not pass the protocol boundary.
    Protocol(ValidationError),
}

impl std::fmt::Display for ExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(reason) => write!(f, "refused: {reason}"),
            Self::SessionFailed(reason) => write!(f, "session could not start: {reason}"),
            Self::Protocol(error) => write!(f, "protocol: {error}"),
        }
    }
}

impl std::error::Error for ExecError {}

/// The registered target: the adapter that executes inside the checked
/// boundary and the transport that owns the driver child.
struct Target {
    machine_id: MachineId,
    adapter: Arc<CheckedCuaAdapter>,
    transport: Arc<dyn DriverTransport>,
    /// The task watching the driver's liveness and refreshing its
    /// descriptor; `shutdown` aborts it first so a deliberate stop is never
    /// taken for a crash.
    watch: tokio::task::AbortHandle,
}

enum State {
    NotStarted,
    Skipped(Skip),
    Failed(String),
    Running(Target),
    Stopped,
}

struct Inner {
    config: CuaConfig,
    targets: CuaTargets,
    /// The workspace root, where `nolune cua install` records the driver it
    /// put under `cua-driver/` (#20).
    workspace_dir: PathBuf,
    state: tokio::sync::Mutex<State>,
    /// Labels of the sessions runs hold open right now; `shutdown` ends every
    /// one and a run that finishes afterwards finds its label gone.
    open: Arc<OpenSessions>,
    next_run: AtomicU64,
}

/// A handle on the server-local runtime; clones share one runtime.
#[derive(Clone)]
pub struct CuaRuntime {
    inner: Arc<Inner>,
}

/// A weak handle the registry keeps (#18): the typed machine tools reach
/// the runtime that runs the server-local target through `CuaTargets`
/// without the registry keeping the runtime alive.
#[derive(Clone)]
pub struct RuntimeHandle(Weak<Inner>);

impl RuntimeHandle {
    /// The runtime, while it exists.
    pub fn upgrade(&self) -> Option<CuaRuntime> {
        self.0.upgrade().map(|inner| CuaRuntime { inner })
    }
}

impl CuaRuntime {
    /// A runtime that has not looked at the host yet. Registers into
    /// `targets` once started; `workspace_dir` is where `nolune cua install`
    /// records the driver it installed, consulted only by `start`. The
    /// targets keep a weak handle on it so the typed machine tools can run
    /// operations on the server-local target (#18).
    pub fn new(config: CuaConfig, targets: CuaTargets, workspace_dir: PathBuf) -> Self {
        let runtime = Self {
            inner: Arc::new(Inner {
                config,
                targets: targets.clone(),
                workspace_dir,
                state: tokio::sync::Mutex::new(State::NotStarted),
                open: Arc::new(OpenSessions::new()),
                next_run: AtomicU64::new(1),
            }),
        };
        targets.set_server_local_runtime(RuntimeHandle(Arc::downgrade(&runtime.inner)));
        runtime
    }

    /// Probe this host, find the driver, and register the server-local
    /// target when there is one. Every outcome is logged once; none of them
    /// fails the server.
    pub async fn start(&self) {
        let host = HostProbe::current();
        let driver = self.locate_driver();
        self.start_from(host, driver).await;
    }

    /// The driver the server would run, from the same lookup `nolune cua
    /// status` reports: `[cua].driver_path`, `NOLUNE_CUA_DRIVER`, the
    /// workspace's own install (#20), then `PATH`. A manifest that cannot be
    /// read is logged and skipped rather than failing startup; `nolune cua
    /// status` explains it.
    fn locate_driver(&self) -> Result<Option<PathBuf>, DriverLookupError> {
        let installed = match install::read_manifest(&self.inner.workspace_dir) {
            Ok(installed) => installed,
            Err(error) => {
                log::warn!(
                    "[cua] ignoring the driver install manifest: {error:#}; run `nolune cua install --force`"
                );
                None
            }
        };
        let located = discovery::discover(
            self.inner.config.driver_path(),
            installed
                .as_ref()
                .map(|installed| installed.driver.as_path()),
        )?;
        Ok(located.map(|located| {
            log::info!(
                "[cua] driver: {} ({})",
                located.path.display(),
                located.source
            );
            located.path
        }))
    }

    /// `start` with the host and the driver lookup supplied, so tests decide
    /// what the host looks like without touching the process environment.
    pub(crate) async fn start_from(
        &self,
        host: HostProbe,
        driver: Result<Option<PathBuf>, DriverLookupError>,
    ) {
        match &*self.inner.state.lock().await {
            State::NotStarted => {}
            State::Stopped => {
                log::info!("[cua] not starting: the gateway is already stopping");
                return;
            }
            _ => {
                log::warn!("[cua] start called more than once; ignoring");
                return;
            }
        }
        let driver = match startup_plan(&self.inner.config, &host, driver) {
            Ok(driver) => driver,
            Err(skip) => {
                let reason = skip.reason();
                if skip.is_error() {
                    log::error!("[cua] no server-local computer-use target: {reason}");
                } else {
                    log::info!("[cua] no server-local computer-use target: {reason}");
                }
                *self.inner.state.lock().await = State::Skipped(skip);
                return;
            }
        };
        let timeouts = self.inner.config.timeouts();
        let transport = match StdioDriverTransport::spawn_with(&driver, timeouts).await {
            Ok(transport) => Arc::new(transport),
            Err(error) => {
                let message = format!("{error:#}");
                log::error!("[cua] driver did not start; no server-local target: {message}");
                *self.inner.state.lock().await = State::Failed(message);
                return;
            }
        };
        let machine_id = server_local_machine_id(&host.hostname);
        let refresh = Some(self.inner.config.health_interval());
        if let Err(error) = self
            .attach(transport, machine_id, &host.hostname, refresh)
            .await
        {
            log::error!("[cua] driver could not be described; no server-local target: {error:#}");
        }
    }

    /// Register the target behind an already running transport: ask it for
    /// its health report, build the descriptor, put the checked adapter into
    /// the shared registry and start watching the driver. `hostname` labels
    /// the row the API lists; `refresh` is how often the driver's health is
    /// re-read while it runs (`start` passes `[cua].health_interval_secs`;
    /// `None` keeps the registration report, for tests that pin a run's
    /// exact call log). Liveness is watched either way. On failure the
    /// transport is closed and the runtime reads `Failed`; a runtime that
    /// was stopped meanwhile stays `Stopped` and the late driver is closed
    /// rather than registered.
    pub(crate) async fn attach(
        &self,
        transport: Arc<dyn DriverTransport>,
        machine_id: MachineId,
        hostname: &str,
        refresh: Option<Duration>,
    ) -> anyhow::Result<MachineDescriptor> {
        match self
            .register(transport.clone(), machine_id, hostname, refresh)
            .await
        {
            Ok(descriptor) => {
                log::info!(
                    "[cua] server-local target registered: {} ({:?} {}, {:?}, accessibility {:?}, \
                     screen capture {:?}, {} capabilities)",
                    descriptor.machine_id.as_str(),
                    descriptor.platform,
                    descriptor.driver_version.as_str(),
                    descriptor.health,
                    descriptor.permissions.accessibility,
                    descriptor.permissions.screen_capture,
                    descriptor.capabilities.len()
                );
                Ok(descriptor)
            }
            Err(error) => {
                transport.close();
                let mut state = self.inner.state.lock().await;
                if !matches!(*state, State::Stopped) {
                    *state = State::Failed(format!("{error:#}"));
                }
                Err(error)
            }
        }
    }

    async fn register(
        &self,
        transport: Arc<dyn DriverTransport>,
        machine_id: MachineId,
        hostname: &str,
        refresh: Option<Duration>,
    ) -> anyhow::Result<MachineDescriptor> {
        let descriptor = describe_machine(transport.as_ref(), machine_id.clone())
            .await
            .context("health report")?;
        let adapter = checked_adapter(transport.clone(), descriptor.clone())
            .map_err(|error| anyhow::anyhow!("descriptor refused: {error}"))?;
        // Held across the registration so a shutdown that lands now either
        // runs first (and this driver is closed, not registered) or finds
        // the target fully registered and stops it.
        let mut state = self.inner.state.lock().await;
        if matches!(*state, State::Stopped) {
            anyhow::bail!("the gateway stopped while the driver was starting");
        }
        self.inner
            .targets
            .register_server_local(adapter, hostname, chrono::Utc::now().timestamp())
            .await?;
        let adapter = self
            .inner
            .targets
            .select(Some(&machine_id))
            .await
            .map_err(|error| anyhow::anyhow!("registered target not selectable: {error:?}"))?;
        let watch = tokio::spawn(watch_driver(
            self.clone(),
            transport.clone(),
            machine_id.clone(),
            refresh,
        ));
        *state = State::Running(Target {
            machine_id,
            adapter,
            transport,
            watch: watch.abort_handle(),
        });
        Ok(descriptor)
    }

    /// The driver child is gone: the target is unregistered at once, the
    /// sessions it held are lost with it (there is nobody left to end them),
    /// and the runtime reads `Failed` until the gateway restarts.
    async fn driver_exited(&self, machine_id: &MachineId) {
        let target = {
            let mut state = self.inner.state.lock().await;
            match &*state {
                State::Running(target) if target.machine_id == *machine_id => {}
                _ => return,
            }
            match std::mem::replace(&mut *state, State::Failed("the driver exited".to_owned())) {
                State::Running(target) => target,
                _ => unreachable!("matched Running above"),
            }
        };
        let lost = self.inner.open.take().len();
        self.inner.targets.unregister(&target.machine_id).await;
        target.transport.close();
        log::error!(
            "[cua] driver exited; server-local target {} unregistered, {lost} open session(s) lost \
             with it. Restart the gateway to register it again",
            target.machine_id.as_str()
        );
    }

    /// Ask the running driver for a fresh health report and, when it says
    /// something new, advertise that: a new checked adapter over the same
    /// transport replaces the registered one, so the next run authorizes
    /// against the current permissions. A report that cannot be read keeps
    /// the last one; only an exit drops the target.
    async fn refresh(&self, transport: &Arc<dyn DriverTransport>, machine_id: &MachineId) {
        let current = match &*self.inner.state.lock().await {
            State::Running(target) if target.machine_id == *machine_id => {
                target.adapter.descriptor().clone()
            }
            _ => return,
        };
        let fresh = match describe_machine(transport.as_ref(), machine_id.clone()).await {
            Ok(fresh) => fresh,
            Err(error) => {
                log::warn!(
                    "[cua] health refresh of {} failed; keeping its last report: {error:#}",
                    machine_id.as_str()
                );
                return;
            }
        };
        if fresh == current {
            return;
        }
        let adapter = match checked_adapter(transport.clone(), fresh.clone()) {
            Ok(adapter) => adapter,
            Err(error) => {
                log::warn!(
                    "[cua] refreshed descriptor of {} refused; keeping the last one: {error}",
                    machine_id.as_str()
                );
                return;
            }
        };
        let mut state = self.inner.state.lock().await;
        let State::Running(target) = &mut *state else {
            return;
        };
        if target.machine_id != *machine_id {
            return;
        }
        match self.inner.targets.replace(adapter).await {
            Ok(adapter) => {
                target.adapter = adapter;
                log::info!(
                    "[cua] server-local target {} health changed: {:?} -> {:?} (accessibility {:?}, \
                     screen capture {:?}, {} capabilities)",
                    machine_id.as_str(),
                    current.health,
                    fresh.health,
                    fresh.permissions.accessibility,
                    fresh.permissions.screen_capture,
                    fresh.capabilities.len()
                );
            }
            Err(error) => log::warn!(
                "[cua] refreshed descriptor of {} not applied: {error}",
                machine_id.as_str()
            ),
        }
    }

    /// Where the runtime is; the typed machine tools (#17/#18) report it.
    #[allow(dead_code)]
    pub async fn status(&self) -> RuntimeStatus {
        match &*self.inner.state.lock().await {
            State::NotStarted => RuntimeStatus::NotStarted,
            State::Skipped(skip) => RuntimeStatus::Skipped(skip.clone()),
            State::Failed(message) => RuntimeStatus::Failed(message.clone()),
            State::Running(target) => RuntimeStatus::Running(target.machine_id.clone()),
            State::Stopped => RuntimeStatus::Stopped,
        }
    }

    /// Execute `body` as one run: the first action it executes opens a driver
    /// session, every action carries that session, and the session is ended
    /// when the body completes, fails, or exceeds the run timeout. The typed
    /// machine tools (#18) execute every operation on the server-local
    /// target this way.
    pub async fn run<T, F, Fut>(&self, purpose: &str, body: F) -> Result<T, RunError>
    where
        F: FnOnce(Arc<RunSession>) -> Fut,
        Fut: Future<Output = anyhow::Result<T>>,
    {
        let (machine_id, adapter) = match &*self.inner.state.lock().await {
            State::Running(target) => (target.machine_id.clone(), target.adapter.clone()),
            _ => return Err(RunError::Unavailable),
        };
        let n = self.inner.next_run.fetch_add(1, Ordering::Relaxed);
        let label = SessionLabel::try_from(format!("{RUN_LABEL_PREFIX}{n}"))
            .expect("the prefix plus a counter is an identifier");
        let session = Arc::new(RunSession {
            label,
            machine_id,
            adapter,
            started: tokio::sync::Mutex::new(false),
            open: self.inner.open.clone(),
            next_request: AtomicU64::new(1),
        });
        log::info!("[cua] run {} begins: {purpose}", session.label().as_str());
        let limit = self.inner.config.run_timeout();
        let outcome = tokio::time::timeout(limit, body(session.clone())).await;
        session.finish().await;
        match outcome {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(error)) => {
                log::warn!("[cua] run {} failed: {error:#}", session.label.as_str());
                Err(RunError::Failed(error))
            }
            Err(_elapsed) => {
                log::warn!(
                    "[cua] run {} exceeded {limit:?}; its session was ended",
                    session.label.as_str()
                );
                Err(RunError::Timeout(limit))
            }
        }
    }

    /// End every open session, unregister the target and stop the driver.
    /// Safe to call on a runtime that never started or already stopped.
    pub async fn shutdown(&self) {
        let target = {
            let mut state = self.inner.state.lock().await;
            match std::mem::replace(&mut *state, State::Stopped) {
                State::Running(target) => target,
                _ => return,
            }
        };
        target.watch.abort();
        let open = self.inner.open.take();
        for label in &open {
            end_session(&target.adapter, &target.machine_id, label).await;
        }
        self.inner.targets.unregister(&target.machine_id).await;
        target.transport.close();
        log::info!(
            "[cua] server-local target {} stopped; {} open session(s) ended",
            target.machine_id.as_str(),
            open.len()
        );
    }
}

/// Watch one driver until it is gone: an exit unregisters the target at
/// once; until then, every `refresh` interval re-reads its health.
async fn watch_driver(
    runtime: CuaRuntime,
    transport: Arc<dyn DriverTransport>,
    machine_id: MachineId,
    refresh: Option<Duration>,
) {
    let mut ticks = refresh.map(|every| {
        let mut ticks = tokio::time::interval_at(tokio::time::Instant::now() + every, every);
        ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ticks
    });
    loop {
        let tick = async {
            match ticks.as_mut() {
                Some(ticks) => {
                    ticks.tick().await;
                }
                None => std::future::pending::<()>().await,
            }
        };
        tokio::select! {
            biased;
            () = transport.exited() => {
                runtime.driver_exited(&machine_id).await;
                return;
            }
            () = tick => runtime.refresh(&transport, &machine_id).await,
        }
    }
}

/// End one driver session; a failure is logged, never propagated, because
/// the run is over either way and the child dies with the runtime.
async fn end_session(adapter: &CheckedCuaAdapter, machine_id: &MachineId, label: &SessionLabel) {
    let request = CuaRequestEnvelope {
        version: ProtocolVersion::V1,
        request_id: RequestId::try_from(format!("{}-end", label.as_str()))
            .expect("a run label plus a suffix is an identifier"),
        machine_id: machine_id.clone(),
        action: CuaAction::EndSession(SessionRefArgs {
            session: Some(label.clone()),
        }),
    };
    match adapter.execute(&request).await {
        Ok(envelope) => match envelope.response {
            CuaResponse::Success { .. } => log::info!("[cua] session {} ended", label.as_str()),
            CuaResponse::Error { error } => log::warn!(
                "[cua] session {} did not end cleanly ({:?}): {}",
                label.as_str(),
                error.code,
                error.message.as_str()
            ),
        },
        Err(error) => log::warn!(
            "[cua] session {} did not end cleanly: {error}",
            label.as_str()
        ),
    }
}

/// One run's view of the target: actions go through the checked adapter,
/// labelled with the run's session.
pub struct RunSession {
    label: SessionLabel,
    machine_id: MachineId,
    adapter: Arc<CheckedCuaAdapter>,
    /// Set once `start_session` succeeded.
    started: tokio::sync::Mutex<bool>,
    open: Arc<OpenSessions>,
    next_request: AtomicU64,
}

impl RunSession {
    /// The driver session this run's actions belong to.
    pub fn label(&self) -> &SessionLabel {
        &self.label
    }

    /// The descriptor the run's actions are authorized against.
    pub fn descriptor(&self) -> &MachineDescriptor {
        self.adapter.descriptor()
    }

    /// Execute one action inside this run's session. The action is
    /// authorized against the target's descriptor before the session is
    /// opened, so a refused action never reaches the driver and never opens
    /// a session on its own.
    pub async fn execute(&self, action: CuaAction) -> Result<CuaResponseEnvelope, ExecError> {
        if is_run_managed(&action) {
            return Err(ExecError::Refused(
                "sessions are opened and closed by the run itself".to_owned(),
            ));
        }
        self.adapter
            .descriptor()
            .authorize(&action)
            .map_err(|error| ExecError::Refused(error.to_string()))?;
        let action = with_session(action, &self.label);
        self.ensure_started().await?;
        let request = self.envelope(action);
        self.adapter
            .execute(&request)
            .await
            .map_err(ExecError::Protocol)
    }

    /// Open the driver session before the first action; later actions find
    /// it open. Holding the lock across the call serializes a run's actions.
    async fn ensure_started(&self) -> Result<(), ExecError> {
        let mut started = self.started.lock().await;
        if *started {
            return Ok(());
        }
        let request = self.envelope(CuaAction::StartSession(StartSessionArgs {
            session: Some(self.label.clone()),
        }));
        let envelope = self
            .adapter
            .execute(&request)
            .await
            .map_err(|error| ExecError::SessionFailed(error.to_string()))?;
        match envelope.response {
            CuaResponse::Success { .. } => {
                *started = true;
                self.open.insert(self.label.clone());
                log::info!("[cua] session {} started", self.label.as_str());
                Ok(())
            }
            CuaResponse::Error { error } => Err(ExecError::SessionFailed(format!(
                "{:?}: {}",
                error.code,
                error.message.as_str()
            ))),
        }
    }

    /// End the session if this run opened it and nothing ended it already.
    async fn finish(&self) {
        if !self.open.remove(&self.label) {
            return;
        }
        end_session(&self.adapter, &self.machine_id, &self.label).await;
    }

    fn envelope(&self, action: CuaAction) -> CuaRequestEnvelope {
        let n = self.next_request.fetch_add(1, Ordering::Relaxed);
        CuaRequestEnvelope {
            version: ProtocolVersion::V1,
            request_id: RequestId::try_from(format!("{}-{n}", self.label.as_str()))
                .expect("a run label plus a counter is an identifier"),
            machine_id: self.machine_id.clone(),
            action,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{
        cua::{
            host::{DisplaySession, PlatformSupport, server_local_machine_id},
            transport::fake::FakeTransport,
        },
        machine_registry::{MachineInfo, MachineRegistry},
        tool::Tool as _,
        tools::computer::{ListMachinesArgs, ListMachinesTool},
    };
    use cua_protocol::*;
    use serde_json::{Value, json};

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

    const ACCESSIBILITY_DENIED: &str = r#"{
        "schema_version":"1","platform":"darwin","driver_version":"0.28.2","overall":"degraded",
        "checks":[
            {"name":"binary_version","status":"pass","message":"cua-driver 0.28.2"},
            {"name":"platform_supported","status":"pass","message":"macOS 27.0 (arm64)","data":{"architecture":"arm64","os_version":"27.0"}},
            {"name":"session_active","status":"pass","message":"MCP session is active."},
            {"name":"bundle_identity","status":"pass","message":"Bundle is com.trycua.driver.","data":{"bundle_identifier":"com.trycua.driver","executable_path":"/Applications/CuaDriver.app/Contents/MacOS/cua-driver","identity_source":"current_process"}},
            {"name":"tcc_accessibility","status":"fail","message":"Accessibility is not granted.","hint":"Run cua-driver permissions grant.","data":{"bundle_identifier":"com.trycua.driver"}},
            {"name":"tcc_screen_recording","status":"pass","message":"Screen Recording is granted.","data":{"bundle_identifier":"com.trycua.driver"}},
            {"name":"ax_capability","status":"fail","message":"AX is not trusted.","hint":"Grant Accessibility."},
            {"name":"screen_capture_capability","status":"skip","message":"Direct capture was not probed."}
        ]
    }"#;

    fn payload(json: &str) -> Value {
        serde_json::from_str(json).unwrap()
    }

    fn label(n: u64) -> Value {
        json!(format!("nolune-run-{n}"))
    }

    fn started(n: u64) -> Value {
        json!({
            "active": true, "capture_scope": "auto", "effective_scope": "window",
            "desktop_capture_authorized": true, "desktop_unlocked": true,
            "revived": false, "session": label(n)
        })
    }

    fn ended(n: u64) -> Value {
        json!({"session": label(n), "active": false})
    }

    fn target() -> WindowTarget {
        WindowTarget {
            pid: 42,
            window_id: 99,
        }
    }

    fn list_apps() -> CuaAction {
        CuaAction::ListApps(EmptyArgs {})
    }

    fn click() -> CuaAction {
        CuaAction::Click(ClickArgs {
            target: target(),
            session: None,
            delivery_mode: DeliveryMode::Background,
            address: ElementAddress::Point(WindowPoint { x: 1.0, y: 2.0 }),
            button: MouseButton::Left,
            action: ClickAction::Press,
            modifiers: vec![],
            count: None,
        })
    }

    fn gui_host() -> HostProbe {
        HostProbe {
            os: "macos",
            support: PlatformSupport::Supported,
            session: DisplaySession::Present("launchd session Aqua".into()),
            container: false,
            hostname: "studio".into(),
        }
    }

    fn config() -> CuaConfig {
        CuaConfig::default()
    }

    fn harness() -> (MachineRegistry, CuaRuntime) {
        let registry = MachineRegistry::new();
        let runtime = CuaRuntime::new(config(), registry.cua().clone(), PathBuf::new());
        (registry, runtime)
    }

    async fn attached(
        registry: &MachineRegistry,
        report: &str,
        answers: Vec<crate::services::cua::transport::CallOutcome>,
    ) -> (CuaRuntime, Arc<FakeTransport>) {
        attached_with(registry, report, answers, None).await
    }

    /// `attached` with a say in how often the driver's health is re-read
    /// while it runs; the session and policy tests attach with `None` so the
    /// call log is exactly the run's.
    async fn attached_with(
        registry: &MachineRegistry,
        report: &str,
        answers: Vec<crate::services::cua::transport::CallOutcome>,
        refresh: Option<Duration>,
    ) -> (CuaRuntime, Arc<FakeTransport>) {
        let mut outcomes = vec![Ok(payload(report))];
        outcomes.extend(answers);
        let transport = Arc::new(FakeTransport::answering(outcomes));
        let runtime = CuaRuntime::new(config(), registry.cua().clone(), PathBuf::new());
        runtime
            .attach(
                transport.clone(),
                server_local_machine_id("studio"),
                "studio",
                refresh,
            )
            .await
            .unwrap();
        (runtime, transport)
    }

    /// Yield until `condition` holds, or fail after a bounded number of turns.
    async fn eventually<F, Fut>(what: &str, mut condition: F)
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = bool>,
    {
        for _ in 0..1000 {
            if condition().await {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("{what} never happened");
    }

    async fn listed_by_tool(registry: &MachineRegistry) -> String {
        ListMachinesTool::new(registry.clone())
            .call(ListMachinesArgs {})
            .await
            .unwrap()
    }

    fn desktop(machine_id: &str, hostname: &str) -> MachineInfo {
        MachineInfo {
            machine_id: machine_id.into(),
            os: "macos".into(),
            hostname: hostname.into(),
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

    #[tokio::test]
    async fn a_headless_host_registers_nothing_and_the_server_stays_healthy() {
        for (host, driver, expected) in [
            (
                HostProbe {
                    session: DisplaySession::Headless("no display session".into()),
                    ..gui_host()
                },
                Ok(Some(PathBuf::from("/usr/bin/cua-driver"))),
                Skip::Headless("no display session".into()),
            ),
            (
                HostProbe {
                    os: "linux",
                    support: PlatformSupport::Unsupported("macOS only for now".into()),
                    ..gui_host()
                },
                Ok(Some(PathBuf::from("/usr/bin/cua-driver"))),
                Skip::Unsupported("macOS only for now".into()),
            ),
            (
                HostProbe {
                    container: true,
                    ..gui_host()
                },
                Ok(Some(PathBuf::from("/usr/bin/cua-driver"))),
                Skip::Container,
            ),
            (gui_host(), Ok(None), Skip::NoDriver),
        ] {
            let (registry, runtime) = harness();
            assert_eq!(runtime.status().await, RuntimeStatus::NotStarted);
            runtime.start_from(host, driver).await;
            assert_eq!(runtime.status().await, RuntimeStatus::Skipped(expected));
            assert!(registry.cua().list().await.is_empty());
            assert!(registry.known_at(1_700_000_000).await.unwrap().is_empty());
            assert!(
                listed_by_tool(&registry)
                    .await
                    .starts_with("No machines connected")
            );
            let refused = runtime.run("check", |_| async { Ok(()) }).await;
            assert!(matches!(refused, Err(RunError::Unavailable)), "{refused:?}");
            // Shutting down a runtime that never started is a no-op.
            runtime.shutdown().await;
            assert_eq!(runtime.status().await, RuntimeStatus::Stopped);
        }
    }

    #[tokio::test]
    async fn a_misconfigured_driver_path_is_reported_and_registers_nothing() {
        let (registry, runtime) = harness();
        let error = DriverLookupError::NotExecutable {
            source: "[cua].driver_path",
            path: PathBuf::from("/typo/cua-driver"),
        };
        runtime.start_from(gui_host(), Err(error.clone())).await;
        assert_eq!(
            runtime.status().await,
            RuntimeStatus::Skipped(Skip::Misconfigured(error))
        );
        assert!(registry.cua().list().await.is_empty());
    }

    /// A driver that never speaks MCP: the runtime gives up within the
    /// handshake deadline and leaves no target behind.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_driver_that_fails_the_handshake_leaves_no_target_behind() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let silent = dir.path().join("silent-driver");
        std::fs::write(
            &silent,
            "#!/bin/sh\n[ \"$1\" = warm-up ] && exit 0\nexec sleep 60\n",
        )
        .unwrap();
        std::fs::set_permissions(&silent, std::fs::Permissions::from_mode(0o755)).unwrap();
        // macOS scans a fresh executable on first run; keep that out of the deadline.
        assert!(
            std::process::Command::new(&silent)
                .arg("warm-up")
                .status()
                .unwrap()
                .success()
        );

        let registry = MachineRegistry::new();
        let runtime = CuaRuntime::new(
            CuaConfig {
                handshake_timeout_secs: 1,
                ..CuaConfig::default()
            },
            registry.cua().clone(),
            PathBuf::new(),
        );
        let started = std::time::Instant::now();
        runtime.start_from(gui_host(), Ok(Some(silent))).await;
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the handshake deadline bounds start, took {:?}",
            started.elapsed()
        );
        match runtime.status().await {
            RuntimeStatus::Failed(message) => {
                assert!(message.contains("handshake"), "{message}");
                assert!(message.contains("silent-driver"), "{message}");
            }
            other => panic!("a driver that never answers is a failure, got {other:?}"),
        }
        assert!(registry.cua().list().await.is_empty());
        assert!(matches!(
            runtime.run("check", |_| async { Ok(()) }).await,
            Err(RunError::Unavailable)
        ));
    }

    #[tokio::test]
    async fn on_a_gui_host_the_server_local_target_is_listed_everywhere() {
        let registry = MachineRegistry::new();
        let (runtime, transport) = attached(&registry, HEALTHY, vec![]).await;
        assert_eq!(
            transport.tools_called(),
            vec!["health_report".to_owned()],
            "attach asks for one health report and nothing else"
        );

        let id = server_local_machine_id("studio");
        assert_eq!(runtime.status().await, RuntimeStatus::Running(id.clone()));

        // The typed registry.
        let targets = registry.cua().list().await;
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].machine_id, id);
        assert_eq!(targets[0].location, MachineLocation::ServerLocal);
        assert_eq!(targets[0].health, MachineHealth::Healthy);
        assert!(targets[0].capabilities.contains(&Capability::Pointer));

        // GET /machines.
        let now = chrono::Utc::now().timestamp();
        let known = registry.known_at(now).await.unwrap();
        assert_eq!(known.len(), 1, "{known:?}");
        let row = &known[0];
        assert_eq!(row.machine_id, "server-local:studio");
        assert_eq!(row.location, MachineLocation::ServerLocal);
        assert_eq!(row.hostname, "studio");
        assert_eq!(row.display_name, "studio");
        assert_eq!(row.custom_name, None);
        assert_eq!(row.os, "macos");
        assert_eq!(row.platform, Some(Platform::Macos));
        assert!(row.online);
        assert_eq!(row.health, MachineHealth::Healthy);
        assert_eq!(row.cua_health, Some(MachineHealth::Healthy));
        assert_eq!(row.driver_version.as_deref(), Some("0.28.2"));
        assert_eq!(
            row.permissions,
            Some(PermissionState {
                accessibility: Permission::Granted,
                screen_capture: Permission::Granted,
            })
        );
        assert!(row.capabilities.contains(&"pointer".to_owned()));
        assert!(row.capabilities.contains(&"session_lifecycle".to_owned()));
        assert_eq!(row.last_seen, now, "a running target was seen now");
        assert!(now - row.first_seen < 60, "first_seen is the registration");
        assert_eq!(
            row.instance_slug.as_deref(),
            Some(crate::domain::companion::CANONICAL_SLUG)
        );

        // The list_machines tool.
        let output = listed_by_tool(&registry).await;
        let entries: Vec<Value> = serde_json::from_str(&output).unwrap();
        let entry = entries
            .iter()
            .find(|entry| entry["machine_id"] == "server-local:studio")
            .expect("list_machines names the server-local target");
        assert_eq!(entry["location"], "server_local");
        assert_eq!(entry["health"], "healthy");
        assert_eq!(entry["driver_version"], "0.28.2");
    }

    #[tokio::test]
    async fn the_server_local_id_never_collides_with_a_desktop_on_the_same_host() {
        let registry = MachineRegistry::new();
        // A desktop from before #80 registered under its bare hostname; a
        // current one under a UUID, still reporting the same hostname.
        for machine_id in ["studio", "4f3c1c2e-9b5e-4d2b-8f0a-1c2d3e4f5a6b"] {
            let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
            registry.register(desktop(machine_id, "studio"), tx).await;
        }
        let (_runtime, _transport) = attached(&registry, HEALTHY, vec![]).await;

        let known = registry.known_at(1_700_000_000).await.unwrap();
        let mut ids: Vec<&str> = known.iter().map(|m| m.machine_id.as_str()).collect();
        ids.sort_unstable();
        assert_eq!(
            ids,
            vec![
                "4f3c1c2e-9b5e-4d2b-8f0a-1c2d3e4f5a6b",
                "server-local:studio",
                "studio"
            ]
        );
        assert!(
            known
                .iter()
                .all(|m| m.hostname == "studio" && m.display_name == "studio"),
            "three rows, one hostname, three ids: {known:?}"
        );
        let entries: Vec<Value> = serde_json::from_str(&listed_by_tool(&registry).await).unwrap();
        assert_eq!(entries.len(), 3);

        // A desktop that claims the server-local id (the WebSocket grammar
        // allows it) does not produce a second row for that id.
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        registry
            .register(desktop("server-local:studio", "impostor"), tx)
            .await;
        let known = registry.known_at(1_700_000_000).await.unwrap();
        let rows: Vec<_> = known
            .iter()
            .filter(|m| m.machine_id == "server-local:studio")
            .collect();
        assert_eq!(rows.len(), 1, "{known:?}");
        assert_eq!(rows[0].location, MachineLocation::ServerLocal);
        assert_eq!(rows[0].hostname, "studio");
    }

    #[tokio::test]
    async fn a_run_opens_its_session_before_the_first_action_and_ends_it_after() {
        let registry = MachineRegistry::new();
        let (runtime, transport) = attached(
            &registry,
            HEALTHY,
            vec![Ok(started(2)), Ok(json!({"apps": []})), Ok(ended(2))],
        )
        .await;

        // A run that executes nothing opens nothing; it still counts as a run.
        let idle: Result<u8, _> = runtime
            .run("idle", |run| async move {
                assert_eq!(run.label().as_str(), "nolune-run-1");
                Ok(7)
            })
            .await;
        assert_eq!(idle.unwrap(), 7);
        assert_eq!(transport.tools_called(), vec!["health_report".to_owned()]);

        let apps = runtime
            .run("list the apps", |run| async move {
                assert_eq!(run.label().as_str(), "nolune-run-2");
                let response = run.execute(list_apps()).await?;
                assert_eq!(response.action, CuaActionKind::ListApps);
                assert!(matches!(response.response, CuaResponse::Success { .. }));
                Ok(response)
            })
            .await
            .unwrap();
        assert_eq!(apps.machine_id.as_str(), "server-local:studio");

        let calls = transport.calls();
        assert_eq!(
            transport.tools_called(),
            vec!["health_report", "start_session", "list_apps", "end_session"]
        );
        assert_eq!(calls[1].1["session"], label(2));
        assert_eq!(calls[3].1["session"], label(2));
        assert!(
            registry.cua().list().await.len() == 1,
            "the target stays registered between runs"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_failing_or_timed_out_run_still_ends_its_session() {
        let registry = MachineRegistry::new();
        let (runtime, transport) = attached(
            &registry,
            HEALTHY,
            vec![
                Ok(started(1)),
                Ok(json!({"apps": []})),
                Ok(ended(1)),
                Ok(started(2)),
                Ok(json!({"apps": []})),
                Ok(ended(2)),
            ],
        )
        .await;

        let failed: Result<(), RunError> = runtime
            .run("fails", |run| async move {
                run.execute(list_apps()).await?;
                anyhow::bail!("the model gave up")
            })
            .await;
        match failed {
            Err(RunError::Failed(error)) => assert!(error.to_string().contains("gave up")),
            other => panic!("expected the body's error, got {other:?}"),
        }
        assert_eq!(
            transport.tools_called(),
            vec!["health_report", "start_session", "list_apps", "end_session"]
        );

        let stuck: Result<(), RunError> = runtime
            .run("hangs", |run| async move {
                run.execute(list_apps()).await?;
                std::future::pending::<()>().await;
                Ok(())
            })
            .await;
        match stuck {
            Err(RunError::Timeout(limit)) => assert_eq!(limit, config().run_timeout()),
            other => panic!("expected a timeout, got {other:?}"),
        }
        let calls = transport.calls();
        assert_eq!(calls.len(), 7, "{calls:?}");
        assert_eq!(calls[6].0, "end_session");
        assert_eq!(calls[6].1["session"], label(2));
    }

    #[tokio::test]
    async fn shutdown_ends_open_sessions_unregisters_the_target_and_closes_the_driver() {
        let registry = MachineRegistry::new();
        let (runtime, transport) = attached(
            &registry,
            HEALTHY,
            vec![
                Ok(started(1)),
                Ok(json!({"apps": []})),
                Ok(started(2)),
                Ok(json!({"apps": []})),
                Ok(ended(1)),
                Ok(ended(2)),
            ],
        )
        .await;

        // Two runs park after their first action, holding their sessions open.
        let release = Arc::new(tokio::sync::Notify::new());
        let mut parked = Vec::new();
        for purpose in ["first", "second"] {
            let runtime = runtime.clone();
            let release = release.clone();
            parked.push(tokio::spawn(async move {
                runtime
                    .run(purpose, |run| async move {
                        run.execute(list_apps()).await?;
                        release.notified().await;
                        Ok(())
                    })
                    .await
            }));
        }
        while transport.tools_called().len() < 5 {
            tokio::task::yield_now().await;
        }

        runtime.shutdown().await;
        assert_eq!(runtime.status().await, RuntimeStatus::Stopped);
        assert!(
            registry.cua().list().await.is_empty(),
            "the target is gone from the registry"
        );
        assert!(
            registry.known_at(1_700_000_000).await.unwrap().is_empty(),
            "and from GET /machines"
        );
        assert!(transport.closed(), "the driver child is killed");
        let ended_labels: Vec<Value> = transport
            .calls()
            .into_iter()
            .filter(|(name, _)| name == "end_session")
            .map(|(_, args)| args["session"].clone())
            .collect();
        assert_eq!(ended_labels, vec![label(1), label(2)]);

        // The parked runs finish afterwards without ending anything twice.
        release.notify_waiters();
        release.notify_one();
        release.notify_one();
        for run in parked {
            run.await.unwrap().unwrap();
        }
        assert_eq!(
            transport
                .tools_called()
                .iter()
                .filter(|name| *name == "end_session")
                .count(),
            2
        );
        assert!(matches!(
            runtime.run("late", |_| async { Ok(()) }).await,
            Err(RunError::Unavailable)
        ));
        // Idempotent.
        runtime.shutdown().await;
        assert_eq!(runtime.status().await, RuntimeStatus::Stopped);
    }

    #[tokio::test]
    async fn a_refused_capability_never_reaches_the_driver() {
        let registry = MachineRegistry::new();
        let (runtime, transport) = attached(
            &registry,
            ACCESSIBILITY_DENIED,
            vec![Ok(started(1)), Ok(json!({"apps": []})), Ok(ended(1))],
        )
        .await;
        let descriptor = &registry.cua().list().await[0];
        assert_eq!(descriptor.health, MachineHealth::Degraded);
        assert!(!descriptor.capabilities.contains(&Capability::Pointer));

        let inner = transport.clone();
        runtime
            .run("click without accessibility", |run| async move {
                let transport = inner;
                let refused = run.execute(click()).await;
                match refused {
                    Err(ExecError::Refused(reason)) => {
                        assert!(reason.contains("capability"), "{reason}");
                    }
                    other => panic!("a dropped capability is refused, got {other:?}"),
                }
                // Sessions are the runtime's job.
                let managed = run
                    .execute(CuaAction::StartSession(StartSessionArgs {
                        session: Some(SessionLabel::try_from("mine").unwrap()),
                    }))
                    .await;
                assert!(matches!(managed, Err(ExecError::Refused(_))), "{managed:?}");
                assert_eq!(
                    transport.tools_called(),
                    vec!["health_report".to_owned()],
                    "no session was opened for refused actions"
                );
                // An advertised action still works in the same run.
                run.execute(list_apps()).await?;
                Ok(())
            })
            .await
            .unwrap();
        assert_eq!(
            transport.tools_called(),
            vec!["health_report", "start_session", "list_apps", "end_session"]
        );
    }

    #[tokio::test]
    async fn a_snapshot_action_verification_round_trip_runs_inside_one_session() {
        let fixture =
            include_str!("../../../../cua-protocol/tests/fixtures/degraded-window-state.json");
        let mut snapshot: Value = serde_json::from_str(fixture).unwrap();
        snapshot["target"] = json!({"pid": 42, "window_id": 99});
        snapshot["background_input"]["exact_window"]["pid"] = json!(42);
        snapshot["background_input"]["exact_window"]["window_id"] = json!(99);
        let clicked = json!({
            "target": {"pid": 42, "window_id": 99},
            "session": label(1),
            "address": {"kind": "point", "x": 1.0, "y": 2.0},
            "button": "left", "action": "press",
            "outcome": {
                "effect": "confirmed", "route": "accessibility",
                "delivery": {"requested": "background", "delivered_count": 1},
                "evidence": ["delivery_receipt"]
            }
        });
        let verified = json!({
            "overall": "satisfied",
            "predicates": [{"predicate_index": 0, "status": "satisfied"}]
        });

        let registry = MachineRegistry::new();
        let (runtime, transport) = attached(
            &registry,
            HEALTHY,
            vec![
                Ok(started(1)),
                Ok(snapshot),
                Ok(clicked),
                Ok(verified),
                Ok(ended(1)),
            ],
        )
        .await;

        let outcome = runtime
            .run("save the document", |run| async move {
                let state = run
                    .execute(CuaAction::GetWindowState(GetWindowStateArgs {
                        target: target(),
                        session: None,
                        include_accessibility_tree: true,
                        include_screenshot: false,
                        max_elements: None,
                        max_depth: None,
                        max_dimension: None,
                        query: None,
                    }))
                    .await?;
                let CuaResponse::Success { result } = state.response else {
                    anyhow::bail!("snapshot failed: {state:?}");
                };
                let CuaActionResult::GetWindowState(window) = *result else {
                    anyhow::bail!("not a window state");
                };
                assert!(window.degraded, "the fixture is the degraded snapshot");

                let click = run.execute(click()).await?;
                let CuaResponse::Success { result } = click.response else {
                    anyhow::bail!("click failed: {click:?}");
                };
                let CuaActionResult::Click(click) = *result else {
                    anyhow::bail!("not a click result");
                };
                assert_eq!(click.outcome.effect, ActionEffect::Confirmed);
                assert_eq!(
                    click.session.as_ref().map(SessionLabel::as_str),
                    Some("nolune-run-1")
                );

                let verify = run
                    .execute(CuaAction::VerifyState(VerifyStateArgs {
                        target: target(),
                        session: None,
                        expect: vec![VerifyPredicate::WindowExists(true)],
                        include_screenshot: false,
                        stable_samples: 1,
                        timeout_ms: 500,
                    }))
                    .await?;
                let CuaResponse::Success { result } = verify.response else {
                    anyhow::bail!("verification failed: {verify:?}");
                };
                let CuaActionResult::VerifyState(verification) = *result else {
                    anyhow::bail!("not a verification");
                };
                Ok(verification.overall)
            })
            .await
            .unwrap();
        assert_eq!(outcome, PredicateStatus::Satisfied);

        let calls = transport.calls();
        assert_eq!(
            transport.tools_called(),
            vec![
                "health_report",
                "start_session",
                "get_window_state",
                "click",
                "verify_state",
                "end_session"
            ]
        );
        for (name, arguments) in &calls[2..=4] {
            assert_eq!(
                arguments["session"],
                label(1),
                "{name} carries the run's session"
            );
        }
        assert_eq!(calls[3].1["x"], 1.0);
        assert_eq!(calls[3].1["y"], 2.0);
    }

    #[tokio::test]
    async fn a_session_that_will_not_start_fails_the_action_and_the_run_cleanly() {
        let registry = MachineRegistry::new();
        let (runtime, transport) = attached(
            &registry,
            HEALTHY,
            vec![Err(cua_protocol::driver_mcp::DriverCallFailure::Tool {
                code: Some("session_limit".into()),
                message: "too many sessions".into(),
            })],
        )
        .await;
        let outcome: Result<(), RunError> = runtime
            .run("no session", |run| async move {
                let failed = run.execute(list_apps()).await;
                match failed {
                    Err(ExecError::SessionFailed(reason)) => {
                        assert!(reason.contains("too many sessions"), "{reason}");
                    }
                    other => panic!("expected SessionFailed, got {other:?}"),
                }
                Ok(())
            })
            .await;
        outcome.unwrap();
        assert_eq!(
            transport.tools_called(),
            vec!["health_report", "start_session"],
            "nothing was executed and nothing is ended for a session that never opened"
        );
    }

    #[tokio::test]
    async fn a_driver_that_dies_after_registration_drops_the_target() {
        let registry = MachineRegistry::new();
        let (runtime, transport) = attached(
            &registry,
            HEALTHY,
            vec![Ok(started(1)), Ok(json!({"apps": []}))],
        )
        .await;
        assert_eq!(registry.cua().list().await.len(), 1);

        // A run parks mid-session, holding the driver session open.
        let release = Arc::new(tokio::sync::Notify::new());
        let parked = tokio::spawn({
            let runtime = runtime.clone();
            let release = release.clone();
            async move {
                runtime
                    .run("parked", |run| async move {
                        run.execute(list_apps()).await?;
                        release.notified().await;
                        Ok(())
                    })
                    .await
            }
        });
        eventually("the parked run opened its session", || async {
            transport.tools_called().len() == 3
        })
        .await;

        // The child dies; nobody asks the runtime anything.
        transport.crash();
        eventually("the target left the registry", || async {
            registry.cua().list().await.is_empty()
        })
        .await;
        assert!(
            registry.known_at(1_700_000_000).await.unwrap().is_empty(),
            "GET /machines no longer lists a dead driver as online"
        );
        assert!(
            listed_by_tool(&registry)
                .await
                .starts_with("No machines connected")
        );
        match runtime.status().await {
            RuntimeStatus::Failed(message) => assert!(message.contains("exited"), "{message}"),
            other => panic!("a dead driver is a failure, got {other:?}"),
        }
        assert!(transport.closed(), "the runtime lets go of the dead child");
        assert!(matches!(
            runtime.run("late", |_| async { Ok(()) }).await,
            Err(RunError::Unavailable)
        ));

        // The parked run finishes; its session died with the driver, so
        // nothing is sent to end it.
        release.notify_one();
        parked.await.unwrap().unwrap();
        assert_eq!(
            transport.tools_called(),
            vec!["health_report", "start_session", "list_apps"]
        );

        // Shutting down afterwards has nothing left to do.
        runtime.shutdown().await;
        assert_eq!(runtime.status().await, RuntimeStatus::Stopped);
    }

    #[tokio::test(start_paused = true)]
    async fn the_descriptor_follows_the_drivers_health_reports_while_it_runs() {
        let registry = MachineRegistry::new();
        let every = Duration::from_secs(60);
        let (runtime, transport) = attached_with(
            &registry,
            HEALTHY,
            vec![
                Ok(payload(ACCESSIBILITY_DENIED)),
                Ok(payload(HEALTHY)),
                Err(cua_protocol::driver_mcp::DriverCallFailure::Timeout(
                    "health_report did not answer within 30s".into(),
                )),
            ],
            Some(every),
        )
        .await;
        let first_seen = registry.known_at(1_700_000_000).await.unwrap()[0].first_seen;
        let health = |registry: &MachineRegistry| {
            let registry = registry.clone();
            async move { registry.cua().list().await[0].health }
        };
        assert_eq!(health(&registry).await, MachineHealth::Healthy);

        // Accessibility is revoked after startup: the next report says so
        // and the target advertises it everywhere a new run would look.
        tokio::time::sleep(every + Duration::from_secs(1)).await;
        eventually("the first refresh landed", || async {
            health(&registry).await == MachineHealth::Degraded
        })
        .await;
        let row = &registry.known_at(1_700_000_000).await.unwrap()[0];
        assert_eq!(row.cua_health, Some(MachineHealth::Degraded));
        assert_eq!(
            row.permissions.as_ref().map(|p| p.accessibility),
            Some(Permission::Denied)
        );
        assert_eq!(
            row.first_seen, first_seen,
            "a refresh is not a new registration"
        );
        let entries: Vec<Value> = serde_json::from_str(&listed_by_tool(&registry).await).unwrap();
        assert_eq!(entries[0]["health"], "degraded");
        let calls_before = transport.tools_called().len();
        runtime
            .run("click after the revoke", |run| async move {
                let refused = run.execute(click()).await;
                assert!(matches!(refused, Err(ExecError::Refused(_))), "{refused:?}");
                Ok(())
            })
            .await
            .unwrap();
        assert_eq!(
            transport.tools_called().len(),
            calls_before,
            "a run authorizes against the refreshed descriptor before any driver call"
        );

        // Granted again: back to healthy.
        tokio::time::sleep(every).await;
        eventually("the second refresh landed", || async {
            health(&registry).await == MachineHealth::Healthy
        })
        .await;
        assert_eq!(registry.cua().list().await.len(), 1, "still one target");

        // A report that fails keeps the last one; only an exit drops the target.
        tokio::time::sleep(every).await;
        eventually("the third refresh was attempted", || async {
            transport.tools_called().len() == 4
        })
        .await;
        assert_eq!(health(&registry).await, MachineHealth::Healthy);
        assert_eq!(
            runtime.status().await,
            RuntimeStatus::Running(server_local_machine_id("studio"))
        );
        assert_eq!(
            transport.tools_called(),
            vec!["health_report"; 4],
            "one report at registration and one per interval, nothing else"
        );

        runtime.shutdown().await;
        assert!(transport.closed());
    }

    #[tokio::test]
    async fn a_driver_that_finishes_starting_after_shutdown_is_closed_not_registered() {
        let registry = MachineRegistry::new();
        let runtime = CuaRuntime::new(config(), registry.cua().clone(), PathBuf::new());
        // The gateway stopped while `start` was still spawning the driver.
        runtime.shutdown().await;
        assert_eq!(runtime.status().await, RuntimeStatus::Stopped);

        let transport = Arc::new(FakeTransport::answering([Ok(payload(HEALTHY))]));
        let error = runtime
            .attach(
                transport.clone(),
                server_local_machine_id("studio"),
                "studio",
                None,
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("stopped"), "{error:#}");
        assert!(transport.closed(), "the late driver is stopped, not leaked");
        assert!(registry.cua().list().await.is_empty());
        assert_eq!(
            runtime.status().await,
            RuntimeStatus::Stopped,
            "a late failure does not overwrite the stop"
        );
        // And a `start` that only now gets its turn does nothing either.
        runtime.start_from(gui_host(), Ok(None)).await;
        assert_eq!(runtime.status().await, RuntimeStatus::Stopped);
    }

    /// Against a real `cua-driver mcp` child; run by hand with `--ignored`
    /// on a host that has the driver. It only reads and ends its own session.
    #[tokio::test]
    #[ignore = "needs an installed cua-driver binary"]
    async fn live_server_local_target_round_trips_a_session() {
        let registry = MachineRegistry::new();
        let runtime = CuaRuntime::new(config(), registry.cua().clone(), PathBuf::new());
        runtime.start().await;
        let RuntimeStatus::Running(id) = runtime.status().await else {
            eprintln!("no server-local target here: {:?}", runtime.status().await);
            return;
        };
        eprintln!("live target: {id:?}");
        let sessions = runtime
            .run("live", |run| async move {
                let response = run
                    .execute(CuaAction::ListSessions(ListSessionsArgs {
                        cursor: None,
                        limit: None,
                    }))
                    .await?;
                Ok(response)
            })
            .await
            .unwrap();
        eprintln!("live sessions: {sessions:?}");
        runtime.shutdown().await;
        assert!(registry.cua().list().await.is_empty());
    }
}
