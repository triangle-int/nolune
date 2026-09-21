//! Permission onboarding for the desktop's Cua Driver (#20): what the
//! settings window says about Accessibility and Screen Recording as macOS
//! grants them to the driver's own bundle (`com.trycua.driver`), read from
//! the driver's health report through the runtime and never from this
//! app's grants; the workspace install and the reported version against
//! the pin; the hosts where there is nothing to grant (Linux and Windows,
//! a session without a display); and the grant action, which drives
//! `cua-driver permissions grant` so the prompts name the driver, and
//! opens the System Settings pane. Capture is one-shot: the driver
//! snapshots a window when an action asks for one, and nothing here
//! records, streams or watches a screen.

use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

use cua_protocol::{
    cua_driver_pin::{check_driver_version, Target, PINNED_VERSION},
    driver_mcp::permissions_from_health,
    HealthCheckData, HealthCheckStatus, HealthOverall, HealthReportResult, Permission,
};
use serde::Serialize;

use crate::cua_runtime::{self, CuaRuntime};

/// The command that installs the pinned driver under the workspace.
pub const INSTALL_COMMAND: &str = "nolune cua install";
/// The bundle macOS attributes the driver's grants to.
pub const DRIVER_BUNDLE: &str = "com.trycua.driver";

// ---------------------------------------------------------------------------
// The host: whether there is anything to grant here
// ---------------------------------------------------------------------------

/// What the host says about itself, gathered without starting anything:
/// the same facts the server's `nolune cua status` prints.
#[derive(Clone, Debug, Default)]
pub struct HostFacts<'a> {
    /// `std::env::consts::OS`.
    pub os: &'a str,
    /// The pinned target this build runs on, `None` on a host the pin does
    /// not cover.
    pub target: Option<Target>,
    /// Linux: `DISPLAY`.
    pub display: Option<&'a OsStr>,
    /// Linux: `WAYLAND_DISPLAY`.
    pub wayland_display: Option<&'a OsStr>,
    /// Windows: `SESSIONNAME`, which only an interactive session has.
    pub session_name: Option<&'a OsStr>,
    /// macOS: what `launchctl managername` answers (`Aqua` in a graphical
    /// login, `Background` over SSH, `System` for daemons); `None` when it
    /// could not be asked.
    pub launchd_manager: Option<&'a str>,
}

/// Whether this host can grant anything to a driver at all.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlatformState {
    /// Nolune drives computers here with the pinned driver.
    Supported { os: String, triple: String },
    /// It does not (Linux, Windows, a host without a pinned driver); the
    /// reason is fit for the page.
    Unsupported {
        os: String,
        triple: String,
        reason: String,
    },
    /// No graphical session: the driver is never started here.
    Headless {
        os: String,
        triple: String,
        reason: String,
    },
}

impl PlatformState {
    /// Whether the driver is probed here at all.
    pub fn probes(&self) -> bool {
        matches!(self, Self::Supported { .. })
    }
}

/// The platform state of a host with `facts`: the same rules as the
/// server's `services::cua::host`, worded for the page.
pub fn platform_state(facts: &HostFacts<'_>) -> PlatformState {
    let os = facts.os.to_owned();
    let triple = facts.target.map_or("this host", Target::triple).to_owned();
    let Some(_) = facts.target else {
        return PlatformState::Unsupported {
            os,
            reason: format!(
                "Nolune ships no Cua Driver for {triple}, so computer use is unavailable on \
                 this computer and there is nothing to grant."
            ),
            triple,
        };
    };
    let set = |name: &str, value: Option<&OsStr>| {
        value
            .filter(|value| !value.is_empty())
            .map(|value| format!("{name}={}", value.to_string_lossy()))
    };
    let named = match facts.os {
        "linux" => "Linux",
        "windows" => "Windows",
        other => other,
    };
    // A session without a display comes first: nothing runs there, whatever
    // the platform policy says.
    let headless = match facts.os {
        "linux" => set("WAYLAND_DISPLAY", facts.wayland_display)
            .or_else(|| set("DISPLAY", facts.display))
            .is_none()
            .then(|| "No display session (DISPLAY and WAYLAND_DISPLAY are unset)".to_owned()),
        // A graphical login runs its processes under launchd's Aqua manager;
        // SSH sessions and daemons run under Background or System, where no
        // window server is reachable. When launchctl cannot be asked, the
        // driver's own report decides.
        "macos" => match facts.launchd_manager.map(str::trim) {
            Some("Aqua") | None => None,
            Some(manager) => Some(format!(
                "No graphical session (launchctl managername reports {manager}, an SSH or \
                 background session, not Aqua)"
            )),
        },
        "windows" => set("SESSIONNAME", facts.session_name)
            .is_none()
            .then(|| "No interactive session (SESSIONNAME is unset)".to_owned()),
        _ => None,
    };
    if let Some(why) = headless {
        return PlatformState::Headless {
            os,
            triple,
            reason: format!(
                "{why}: the driver is never started here and there is nothing to grant. \
                 Headless installs need nothing from this page."
            ),
        };
    }
    if facts.os == "macos" {
        return PlatformState::Supported { os, triple };
    }
    PlatformState::Unsupported {
        os,
        triple,
        reason: format!(
            "Computer use is not available on {named} yet: Nolune drives computers on macOS \
             only for now. The pinned driver still installs with `{INSTALL_COMMAND}` so a \
             later release can turn it on without changing the pin; there is nothing to \
             grant here."
        ),
    }
}

// ---------------------------------------------------------------------------
// The workspace install, checked against the pin
// ---------------------------------------------------------------------------

/// What `nolune cua install` left under the workspace, against the pin.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InstallState {
    /// The pinned driver is installed at `driver`.
    Pinned { version: String, driver: String },
    /// Another version is installed: the pin moved, or the install is old.
    Stale { version: String, driver: String },
    /// The manifest names a binary that is gone.
    Missing { version: String, driver: String },
    /// No manifest: nothing was installed under the workspace.
    None,
    /// A manifest that cannot be read.
    Unreadable { detail: String },
}

/// The install recorded under `workspace` (`cua-driver/install.json`, the
/// manifest `nolune cua install` writes), checked against the pin.
pub fn install_state(workspace: &Path) -> InstallState {
    let manifest = workspace.join(cua_runtime::INSTALL_MANIFEST);
    let raw = match std::fs::read_to_string(&manifest) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return InstallState::None,
        Err(error) => {
            return InstallState::Unreadable {
                detail: format!("cannot read {}: {error}", manifest.display()),
            };
        }
    };
    let parsed = serde_json::from_str::<serde_json::Value>(&raw).ok();
    let (version, driver) = match parsed.as_ref().and_then(|manifest| {
        Some((
            manifest.get("version")?.as_str()?.to_owned(),
            PathBuf::from(manifest.get("driver")?.as_str()?),
        ))
    }) {
        Some(found) => found,
        None => {
            return InstallState::Unreadable {
                detail: format!("{} is not a driver manifest", manifest.display()),
            };
        }
    };
    let driver_shown = driver.display().to_string();
    if !driver.is_file() {
        InstallState::Missing {
            version,
            driver: driver_shown,
        }
    } else if version != PINNED_VERSION {
        InstallState::Stale {
            version,
            driver: driver_shown,
        }
    } else {
        InstallState::Pinned {
            version,
            driver: driver_shown,
        }
    }
}

// ---------------------------------------------------------------------------
// The driver: what it reports, against the pin
// ---------------------------------------------------------------------------

/// The driver's overall health, as its report says.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Health {
    Ok,
    Degraded,
    Failed,
}

/// One check the driver reports as failed, with its own hint.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FailedCheck {
    pub name: String,
    pub message: String,
    pub hint: Option<String>,
}

/// Where the driver stands.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DriverState {
    /// Not probed: the host cannot run it (see [`PlatformState`]).
    Skipped,
    /// No driver anywhere: not `NOLUNE_CUA_DRIVER`, not the workspace
    /// install, not on `PATH`.
    Absent,
    /// A driver was found but could not report (a failed handshake, a
    /// stalled report); `error` repeats what it said.
    Unreachable { path: String, error: String },
    /// The driver reported.
    Reported {
        path: String,
        version: String,
        /// Whether `version` is the pin.
        compatible: bool,
        /// Why not, with the remedy, when it is not.
        incompatibility: Option<String>,
        health: Health,
        /// The bundle the report attributes its grants to.
        bundle: Option<String>,
        failed_checks: Vec<FailedCheck>,
    },
}

/// Accessibility and Screen Recording as the driver's bundle holds them.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DriverPermissions {
    pub accessibility: Permission,
    pub screen_recording: Permission,
}

/// The driver at `path` and what it reported, or why it could not.
pub fn driver_state(path: &Path, probe: Result<&HealthReportResult, String>) -> DriverState {
    let path = path.display().to_string();
    let report = match probe {
        Ok(report) => report,
        Err(error) => return DriverState::Unreachable { path, error },
    };
    let incompatibility = check_driver_version(&report.driver_version)
        .err()
        .map(|incompatible| {
            format!("{incompatible} (`{INSTALL_COMMAND}` installs the pinned release)")
        });
    let health = match report.overall {
        HealthOverall::Ok => Health::Ok,
        HealthOverall::Degraded => Health::Degraded,
        HealthOverall::Failed => Health::Failed,
    };
    // The bundle the TCC checks name; the identity check when they carry none.
    let bundle = report
        .checks
        .iter()
        .find_map(|check| match &check.data {
            Some(HealthCheckData::BundlePermission(data)) => {
                Some(data.bundle_identifier.as_str().to_owned())
            }
            _ => None,
        })
        .or_else(|| {
            report.checks.iter().find_map(|check| match &check.data {
                Some(HealthCheckData::BundleIdentity(data)) => {
                    Some(data.bundle_identifier.as_str().to_owned())
                }
                _ => None,
            })
        });
    let failed_checks = report
        .checks
        .iter()
        .filter(|check| check.status == HealthCheckStatus::Fail)
        .map(|check| FailedCheck {
            name: check.name.as_str().to_owned(),
            message: check.message.as_str().to_owned(),
            hint: check.hint.as_ref().map(|hint| hint.as_str().to_owned()),
        })
        .collect();
    DriverState::Reported {
        path,
        version: report.driver_version.as_str().to_owned(),
        compatible: incompatibility.is_none(),
        incompatibility,
        health,
        bundle,
        failed_checks,
    }
}

/// The permissions a report proves, under the page's names.
pub fn driver_permissions(report: &HealthReportResult) -> DriverPermissions {
    let permissions = permissions_from_health(report);
    DriverPermissions {
        accessibility: permissions.accessibility,
        screen_recording: permissions.screen_capture,
    }
}

// ---------------------------------------------------------------------------
// The report the settings window renders
// ---------------------------------------------------------------------------

/// Everything the settings window shows about computer-use permissions.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CuaPermissionsReport {
    pub platform: PlatformState,
    /// The driver version this build expects.
    pub pinned_version: &'static str,
    /// The command that installs it.
    pub install_command: &'static str,
    /// The bundle the grants belong to.
    pub driver_bundle: &'static str,
    pub install: InstallState,
    pub driver: DriverState,
    /// `None` until a driver reported.
    pub permissions: Option<DriverPermissions>,
    /// One line saying where things stand and what to do.
    pub summary: String,
}

/// The report for a host in `platform` state with `install` under the
/// workspace and `driver` as probed.
pub fn assemble(
    platform: PlatformState,
    install: InstallState,
    driver: DriverState,
    permissions: Option<DriverPermissions>,
) -> CuaPermissionsReport {
    let summary = summary(&platform, &driver, permissions.as_ref());
    CuaPermissionsReport {
        platform,
        pinned_version: PINNED_VERSION,
        install_command: INSTALL_COMMAND,
        driver_bundle: DRIVER_BUNDLE,
        install,
        driver,
        permissions,
        summary,
    }
}

/// The one line: the platform's reason when there is nothing to grant,
/// else where the driver stands and what to do.
fn summary(
    platform: &PlatformState,
    driver: &DriverState,
    permissions: Option<&DriverPermissions>,
) -> String {
    match platform {
        PlatformState::Unsupported { reason, .. } | PlatformState::Headless { reason, .. } => {
            return reason.clone();
        }
        PlatformState::Supported { .. } => {}
    }
    match driver {
        DriverState::Skipped => "The driver was not probed.".to_owned(),
        DriverState::Absent => format!(
            "No Cua Driver is installed on this computer; run `{INSTALL_COMMAND}`, then check \
             again."
        ),
        DriverState::Unreachable { path, error } => {
            format!("The driver at {path} could not report: {error}")
        }
        DriverState::Reported {
            incompatibility: Some(incompatibility),
            ..
        } => incompatibility.clone(),
        DriverState::Reported {
            version,
            health: Health::Failed,
            failed_checks,
            ..
        } => {
            let failed = failed_checks
                .iter()
                .map(|check| check.message.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            format!("CuaDriver {version} reports a failed check: {failed}")
        }
        DriverState::Reported { version, .. } => {
            let missing = permissions
                .map(|permissions| {
                    [
                        (permissions.accessibility, "Accessibility"),
                        (permissions.screen_recording, "Screen Recording"),
                    ]
                    .into_iter()
                    .filter(|(state, _)| *state != Permission::Granted)
                    .map(|(_, name)| name)
                    .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            match missing.as_slice() {
                [] => format!(
                    "CuaDriver {version} holds Accessibility and Screen Recording; computer \
                     use is ready."
                ),
                [one] => format!("CuaDriver {version} is missing {one}; grant it below."),
                [first, second] => format!(
                    "CuaDriver {version} is missing {first} and {second}; grant them below."
                ),
                _ => unreachable!("two permissions"),
            }
        }
    }
}

/// The report for this host: the platform decides whether the driver is
/// probed at all, `located` is the driver the runtime would run (from
/// [`cua_runtime::locate_driver`]), and the runtime's probe is the
/// driver's own health report, so the permissions are the ones macOS
/// granted to the driver's bundle.
pub async fn gather(
    facts: &HostFacts<'_>,
    workspace: &Path,
    located: Option<PathBuf>,
    runtime: &CuaRuntime,
    machine_id: &str,
) -> CuaPermissionsReport {
    let platform = platform_state(facts);
    let install = install_state(workspace);
    if !platform.probes() {
        return assemble(platform, install, DriverState::Skipped, None);
    }
    let Some(driver) = located else {
        return assemble(platform, install, DriverState::Absent, None);
    };
    let probe = runtime.probe(machine_id).await;
    let permissions = probe.as_ref().ok().map(driver_permissions);
    let driver = driver_state(&driver, probe.as_ref().map_err(Clone::clone));
    assemble(platform, install, driver, permissions)
}

// ---------------------------------------------------------------------------
// The grant action
// ---------------------------------------------------------------------------

/// The System Settings pane a permission is granted in.
pub fn settings_pane_url(permission: &str) -> Option<&'static str> {
    match permission {
        "accessibility" => {
            Some("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        }
        "screen_recording" => {
            Some("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
        }
        _ => None,
    }
}

/// What a grant does on this host: run the driver's own grant flow (so
/// the prompts name the driver's bundle) and open the pane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GrantPlan {
    /// The driver whose `permissions grant` is run, when the host can.
    pub driver_grant: Option<PathBuf>,
    /// The pane opened afterwards, where the driver is enabled when no
    /// prompt appears.
    pub settings_url: &'static str,
}

/// The plan for `permission` on operating system `os` with `driver` the
/// driver the runtime would run: an error names why there is nothing to
/// grant (another platform, no driver).
pub fn grant_plan(os: &str, permission: &str, driver: Option<&Path>) -> Result<GrantPlan, String> {
    let settings_url =
        settings_pane_url(permission).ok_or_else(|| format!("unknown permission: {permission}"))?;
    if os != "macos" {
        return Err(
            "Nolune drives computers on macOS only for now; there is nothing to grant on this \
             computer."
                .to_owned(),
        );
    }
    let driver = driver.ok_or_else(|| {
        format!(
            "No Cua Driver is installed on this computer; run `{INSTALL_COMMAND}` first, so \
             the grant goes to the driver's own bundle."
        )
    })?;
    Ok(GrantPlan {
        driver_grant: Some(driver.to_path_buf()),
        settings_url,
    })
}

/// Carry out `plan`: start the driver's own grant flow, detached and
/// without a terminal (it launches the driver's app through LaunchServices
/// so the prompts name the driver's bundle, then asks macOS for the grants;
/// its terminal text has no reader here), and open the pane where the
/// driver is enabled when no prompt appears.
fn run_grant_plan(permission: &str, plan: &GrantPlan) -> GrantOutcome {
    let driver_grant = plan.driver_grant.as_ref().is_some_and(|driver| {
        match std::process::Command::new(driver)
            .arg("permissions")
            .arg("grant")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(_) => {
                eprintln!(
                    "[cua] {} permissions grant started for {permission}",
                    driver.display()
                );
                true
            }
            Err(error) => {
                eprintln!(
                    "[cua] {} permissions grant could not start: {error}",
                    driver.display()
                );
                false
            }
        }
    });
    let opened_settings = match std::process::Command::new("open")
        .arg(plan.settings_url)
        .spawn()
    {
        Ok(_) => true,
        Err(error) => {
            eprintln!("[cua] System Settings could not be opened: {error}");
            false
        }
    };
    GrantOutcome {
        permission: permission.to_owned(),
        driver_grant,
        opened_settings,
    }
}

/// What the grant action did.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GrantOutcome {
    pub permission: String,
    /// `cua-driver permissions grant` was started.
    pub driver_grant: bool,
    /// The System Settings pane was opened.
    pub opened_settings: bool,
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// `launchctl managername` on macOS; `None` elsewhere or when it fails.
fn launchd_manager_name() -> Option<String> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let output = std::process::Command::new("launchctl")
        .arg("managername")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!name.is_empty()).then_some(name)
}

/// The driver the runtime would run, from the same lookup.
fn located_driver(workspace: &Path) -> Option<PathBuf> {
    cua_runtime::locate_driver(
        workspace,
        std::env::var_os(cua_runtime::DRIVER_ENV).as_deref(),
        std::env::var_os("PATH").as_deref(),
    )
}

/// The permission report the settings window renders (#20): the host's
/// facts, the workspace install, and the driver the app's runtime runs
/// asked for its own report under this computer's stable id.
#[tauri::command]
pub async fn cua_permissions(app: tauri::AppHandle) -> Result<CuaPermissionsReport, String> {
    let machine_id = crate::computer_use_bridge::stable_machine_id(&app)?;
    let workspace = crate::local_server::nolune_home();
    let display = std::env::var_os("DISPLAY");
    let wayland_display = std::env::var_os("WAYLAND_DISPLAY");
    let session_name = std::env::var_os("SESSIONNAME");
    let launchd_manager = launchd_manager_name();
    let facts = HostFacts {
        os: std::env::consts::OS,
        target: Target::current(),
        display: display.as_deref(),
        wayland_display: wayland_display.as_deref(),
        session_name: session_name.as_deref(),
        launchd_manager: launchd_manager.as_deref(),
    };
    let located = located_driver(&workspace);
    Ok(gather(
        &facts,
        &workspace,
        located,
        cua_runtime::runtime(),
        &machine_id,
    )
    .await)
}

/// Grant `permission` (`accessibility` or `screen_recording`) to the
/// driver: its own grant flow, then the System Settings pane.
#[tauri::command]
pub async fn cua_grant_permission(permission: String) -> Result<GrantOutcome, String> {
    let workspace = crate::local_server::nolune_home();
    let located = located_driver(&workspace);
    let plan = grant_plan(std::env::consts::OS, &permission, located.as_deref())?;
    Ok(run_grant_plan(&permission, &plan))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cua_runtime::fake::{FakeTransport, HEALTHY};
    use crate::cua_runtime::DriverTransport;
    use cua_protocol::driver_mcp::DriverCallFailure;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    };

    const STUDIO: &str = "4f3c1c2e-9b5e-4d2b-8f0a-1c2d3e4f5a6b";

    const ACCESSIBILITY_DENIED: &str = r#"{
        "schema_version":"1","platform":"darwin","driver_version":"0.28.2","overall":"degraded",
        "checks":[
            {"name":"binary_version","status":"pass","message":"cua-driver 0.28.2"},
            {"name":"platform_supported","status":"pass","message":"macOS 27.0 (arm64)","data":{"architecture":"arm64","os_version":"27.0"}},
            {"name":"session_active","status":"pass","message":"MCP session is active."},
            {"name":"tcc_accessibility","status":"fail","message":"Accessibility is not granted.","hint":"Run cua-driver permissions grant.","data":{"bundle_identifier":"com.trycua.driver"}},
            {"name":"tcc_screen_recording","status":"pass","message":"Screen Recording is granted.","data":{"bundle_identifier":"com.trycua.driver"}},
            {"name":"ax_capability","status":"fail","message":"AX is not trusted.","hint":"Grant Accessibility."},
            {"name":"screen_capture_capability","status":"skip","message":"Direct capture was not probed."}
        ]
    }"#;

    /// A fresh daemon that was never asked: the TCC checks and the
    /// capability probes were all skipped.
    const NEVER_ASKED: &str = r#"{
        "schema_version":"1","platform":"darwin","driver_version":"0.28.2","overall":"ok",
        "checks":[
            {"name":"binary_version","status":"pass","message":"cua-driver 0.28.2"},
            {"name":"platform_supported","status":"pass","message":"macOS 27.0 (arm64)","data":{"architecture":"arm64","os_version":"27.0"}},
            {"name":"session_active","status":"pass","message":"MCP session is active."},
            {"name":"tcc_accessibility","status":"skip","message":"Accessibility was not probed.","data":{"bundle_identifier":"com.trycua.driver"}},
            {"name":"tcc_screen_recording","status":"skip","message":"Screen Recording was not probed.","data":{"bundle_identifier":"com.trycua.driver"}},
            {"name":"ax_capability","status":"skip","message":"AX was not probed."},
            {"name":"screen_capture_capability","status":"skip","message":"Direct capture was not probed."}
        ]
    }"#;

    fn payload(json: &str) -> serde_json::Value {
        serde_json::from_str(json).unwrap()
    }

    fn report(json: &str) -> HealthReportResult {
        serde_json::from_str(json).unwrap()
    }

    fn healthy_on(version: &str) -> serde_json::Value {
        let mut report = payload(HEALTHY);
        report["driver_version"] = serde_json::Value::String(version.to_owned());
        report["checks"][0]["message"] = serde_json::Value::String(format!("cua-driver {version}"));
        report
    }

    fn macos() -> HostFacts<'static> {
        HostFacts {
            os: "macos",
            target: Some(Target::Aarch64AppleDarwin),
            launchd_manager: Some("Aqua"),
            ..HostFacts::default()
        }
    }

    /// A runtime whose spawner hands out `fakes` in order and counts.
    fn runtime_over(fakes: Vec<Arc<FakeTransport>>) -> (CuaRuntime, Arc<AtomicUsize>) {
        let spawns = Arc::new(AtomicUsize::new(0));
        let queue = Arc::new(Mutex::new(std::collections::VecDeque::from(fakes)));
        let counter = spawns.clone();
        let runtime = CuaRuntime::with_spawner(Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            let next = queue.lock().unwrap().pop_front();
            Box::pin(async move {
                next.map(|fake| fake as Arc<dyn DriverTransport>)
                    .ok_or_else(|| "no driver installed".to_owned())
            })
        }));
        (runtime, spawns)
    }

    /// An executable `bin/cua-driver` under `dir`.
    fn driver_path(dir: &Path) -> PathBuf {
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        let path = dir.join("bin").join("cua-driver");
        std::fs::write(&path, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    fn manifest(workspace: &Path, version: &str, driver: &Path) {
        let dir = workspace.join("cua-driver");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("install.json"),
            serde_json::json!({
                "version": version,
                "target": "aarch64-apple-darwin",
                "asset": format!("cua-driver-rs-{version}-darwin-universal.tar.gz"),
                "sha256": "00",
                "size": 1,
                "driver": driver,
                "installed_at": "2026-09-22T00:00:00Z",
            })
            .to_string(),
        )
        .unwrap();
    }

    async fn gathered(
        fake: Option<Arc<FakeTransport>>,
        located: bool,
    ) -> (
        CuaPermissionsReport,
        Arc<AtomicUsize>,
        Option<Arc<FakeTransport>>,
    ) {
        let workspace = tempfile::tempdir().unwrap();
        let driver = driver_path(workspace.path());
        manifest(workspace.path(), PINNED_VERSION, &driver);
        let (runtime, spawns) = runtime_over(fake.iter().cloned().collect());
        let report = gather(
            &macos(),
            workspace.path(),
            located.then(|| driver.clone()),
            &runtime,
            STUDIO,
        )
        .await;
        (report, spawns, fake)
    }

    fn reported(state: &DriverState) -> (&str, bool, Option<&str>, Health, Option<&str>) {
        match state {
            DriverState::Reported {
                version,
                compatible,
                incompatibility,
                health,
                bundle,
                ..
            } => (
                version,
                *compatible,
                incompatibility.as_deref(),
                *health,
                bundle.as_deref(),
            ),
            other => panic!("expected a reported driver, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_healthy_driver_reports_both_grants_for_the_driver_bundle() {
        let fake = FakeTransport::answering([Ok(payload(HEALTHY))]);
        let (report, spawns, _) = gathered(Some(fake.clone()), true).await;
        assert_eq!(spawns.load(Ordering::SeqCst), 1);
        assert_eq!(fake.tools_called(), vec!["health_report"]);
        assert!(report.platform.probes());
        assert_eq!(report.pinned_version, PINNED_VERSION);
        assert_eq!(report.install_command, INSTALL_COMMAND);
        assert_eq!(report.driver_bundle, DRIVER_BUNDLE);
        assert!(matches!(report.install, InstallState::Pinned { .. }));
        let (version, compatible, incompatibility, health, bundle) = reported(&report.driver);
        assert_eq!(version, "0.28.2");
        assert!(compatible);
        assert_eq!(incompatibility, None);
        assert_eq!(health, Health::Ok);
        assert_eq!(bundle, Some(DRIVER_BUNDLE));
        assert_eq!(
            report.permissions,
            Some(DriverPermissions {
                accessibility: Permission::Granted,
                screen_recording: Permission::Granted,
            })
        );
        assert!(report.summary.contains("0.28.2"), "{}", report.summary);
        assert!(
            report.summary.contains("Accessibility") && report.summary.contains("Screen Recording"),
            "{}",
            report.summary
        );
        // The JSON the page reads: tagged kinds, the protocol's permission words.
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["platform"]["kind"], "supported");
        assert_eq!(json["driver"]["kind"], "reported");
        assert_eq!(json["permissions"]["accessibility"], "granted");
        assert_eq!(json["permissions"]["screen_recording"], "granted");
    }

    #[tokio::test]
    async fn a_denied_permission_is_denied_with_the_drivers_hint() {
        let fake = FakeTransport::answering([Ok(payload(ACCESSIBILITY_DENIED))]);
        let (report, _, _) = gathered(Some(fake), true).await;
        assert_eq!(
            report.permissions,
            Some(DriverPermissions {
                accessibility: Permission::Denied,
                screen_recording: Permission::Granted,
            })
        );
        let DriverState::Reported {
            health,
            failed_checks,
            compatible,
            ..
        } = &report.driver
        else {
            panic!("expected a reported driver, got {:?}", report.driver);
        };
        assert_eq!(*health, Health::Degraded);
        assert!(*compatible);
        assert_eq!(
            failed_checks
                .iter()
                .map(|check| check.name.as_str())
                .collect::<Vec<_>>(),
            vec!["tcc_accessibility", "ax_capability"]
        );
        assert_eq!(
            failed_checks[0].hint.as_deref(),
            Some("Run cua-driver permissions grant.")
        );
        assert!(
            report.summary.contains("Accessibility"),
            "the summary names what is missing: {}",
            report.summary
        );
        assert!(
            !report.summary.contains("Screen Recording"),
            "the summary does not name what is granted: {}",
            report.summary
        );
    }

    #[tokio::test]
    async fn permissions_the_driver_never_asked_for_are_prompt_required() {
        let fake = FakeTransport::answering([Ok(payload(NEVER_ASKED))]);
        let (report, _, _) = gathered(Some(fake), true).await;
        assert_eq!(
            report.permissions,
            Some(DriverPermissions {
                accessibility: Permission::PromptRequired,
                screen_recording: Permission::PromptRequired,
            })
        );
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["permissions"]["accessibility"], "prompt_required");
        assert_eq!(json["permissions"]["screen_recording"], "prompt_required");
        let (_, _, _, health, _) = reported(&report.driver);
        assert_eq!(health, Health::Ok);
    }

    #[tokio::test]
    async fn a_driver_on_another_version_fails_clearly_with_the_install_command() {
        let fake = FakeTransport::answering([Ok(healthy_on("0.27.0"))]);
        let (report, _, _) = gathered(Some(fake), true).await;
        let (version, compatible, incompatibility, _, _) = reported(&report.driver);
        assert_eq!(version, "0.27.0");
        assert!(!compatible);
        let incompatibility = incompatibility.expect("an incompatibility is named");
        assert!(incompatibility.contains("0.27.0"), "{incompatibility}");
        assert!(
            incompatibility.contains(PINNED_VERSION),
            "{incompatibility}"
        );
        assert!(
            incompatibility.contains(INSTALL_COMMAND),
            "{incompatibility}"
        );
        assert_eq!(
            report.summary, incompatibility,
            "the mismatch is the headline"
        );
        // The grants are still what the driver reported.
        assert_eq!(
            report.permissions.as_ref().map(|p| p.accessibility),
            Some(Permission::Granted)
        );
    }

    #[test]
    fn the_workspace_install_is_checked_against_the_pin() {
        let workspace = tempfile::tempdir().unwrap();
        assert_eq!(install_state(workspace.path()), InstallState::None);

        let driver = driver_path(workspace.path());
        manifest(workspace.path(), PINNED_VERSION, &driver);
        assert_eq!(
            install_state(workspace.path()),
            InstallState::Pinned {
                version: PINNED_VERSION.to_owned(),
                driver: driver.display().to_string(),
            }
        );

        manifest(workspace.path(), "0.27.0", &driver);
        assert_eq!(
            install_state(workspace.path()),
            InstallState::Stale {
                version: "0.27.0".to_owned(),
                driver: driver.display().to_string(),
            }
        );

        let gone = workspace.path().join("gone");
        manifest(workspace.path(), PINNED_VERSION, &gone);
        assert_eq!(
            install_state(workspace.path()),
            InstallState::Missing {
                version: PINNED_VERSION.to_owned(),
                driver: gone.display().to_string(),
            }
        );

        std::fs::write(
            workspace.path().join("cua-driver/install.json"),
            "{not json",
        )
        .unwrap();
        match install_state(workspace.path()) {
            InstallState::Unreadable { detail } => {
                assert!(detail.contains("install.json"), "{detail}");
            }
            other => panic!("expected unreadable, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn no_driver_means_nothing_to_grant_and_the_install_command() {
        let (report, spawns, _) = gathered(None, false).await;
        assert_eq!(spawns.load(Ordering::SeqCst), 0, "nothing was started");
        assert_eq!(report.driver, DriverState::Absent);
        assert_eq!(report.permissions, None);
        assert!(
            report.summary.contains(INSTALL_COMMAND),
            "{}",
            report.summary
        );
    }

    #[tokio::test]
    async fn a_driver_that_cannot_report_is_unreachable_with_what_it_said() {
        let fake = FakeTransport::answering([Err(DriverCallFailure::Transport(
            "the MCP handshake timed out; the driver said: \"CuaDriver daemon is not running\""
                .to_owned(),
        ))]);
        let (report, _, _) = gathered(Some(fake), true).await;
        let DriverState::Unreachable { path, error } = &report.driver else {
            panic!("expected unreachable, got {:?}", report.driver);
        };
        assert!(path.ends_with("cua-driver"), "{path}");
        assert!(error.contains("CuaDriver daemon is not running"), "{error}");
        assert_eq!(report.permissions, None);
        assert!(
            report.summary.contains("could not report"),
            "{}",
            report.summary
        );
        assert!(
            report.summary.contains("CuaDriver daemon is not running"),
            "{}",
            report.summary
        );
    }

    #[tokio::test]
    async fn unsupported_and_headless_hosts_never_start_the_driver() {
        let workspace = tempfile::tempdir().unwrap();
        let driver = driver_path(workspace.path());
        for facts in [
            HostFacts {
                os: "linux",
                target: Some(Target::X86_64UnknownLinuxGnu),
                display: Some(OsStr::new(":0")),
                ..HostFacts::default()
            },
            HostFacts {
                os: "windows",
                target: Some(Target::X86_64PcWindowsMsvc),
                session_name: Some(OsStr::new("Console")),
                ..HostFacts::default()
            },
            HostFacts {
                os: "macos",
                target: Some(Target::Aarch64AppleDarwin),
                launchd_manager: Some("Background"),
                ..HostFacts::default()
            },
        ] {
            let fake = FakeTransport::answering([Ok(payload(HEALTHY))]);
            let (runtime, spawns) = runtime_over(vec![fake.clone()]);
            let report = gather(
                &facts,
                workspace.path(),
                Some(driver.clone()),
                &runtime,
                STUDIO,
            )
            .await;
            assert_eq!(
                spawns.load(Ordering::SeqCst),
                0,
                "{}: nothing started",
                facts.os
            );
            assert!(fake.tools_called().is_empty());
            assert!(!report.platform.probes(), "{:?}", report.platform);
            assert_eq!(report.driver, DriverState::Skipped);
            assert_eq!(report.permissions, None);
            let reason = match &report.platform {
                PlatformState::Unsupported { reason, .. }
                | PlatformState::Headless { reason, .. } => reason.clone(),
                PlatformState::Supported { .. } => unreachable!(),
            };
            assert_eq!(report.summary, reason);
        }
    }

    #[test]
    fn linux_windows_and_background_sessions_are_named_explicitly() {
        let linux = HostFacts {
            os: "linux",
            target: Some(Target::X86_64UnknownLinuxGnu),
            display: Some(OsStr::new(":0")),
            ..HostFacts::default()
        };
        let PlatformState::Unsupported { os, triple, reason } = platform_state(&linux) else {
            panic!(
                "linux with a display is unsupported, got {:?}",
                platform_state(&linux)
            );
        };
        assert_eq!(os, "linux");
        assert_eq!(triple, "x86_64-unknown-linux-gnu");
        assert!(reason.contains("macOS only"), "{reason}");
        assert!(reason.contains(INSTALL_COMMAND), "{reason}");
        assert!(reason.contains("nothing to grant"), "{reason}");

        let headless_linux = HostFacts {
            os: "linux",
            target: Some(Target::Aarch64UnknownLinuxGnu),
            ..HostFacts::default()
        };
        let PlatformState::Headless { reason, .. } = platform_state(&headless_linux) else {
            panic!("linux without a display is headless");
        };
        assert!(reason.contains("DISPLAY"), "{reason}");
        assert!(reason.contains("nothing to grant"), "{reason}");

        let windows = HostFacts {
            os: "windows",
            target: Some(Target::X86_64PcWindowsMsvc),
            session_name: Some(OsStr::new("Console")),
            ..HostFacts::default()
        };
        let PlatformState::Unsupported { reason, .. } = platform_state(&windows) else {
            panic!("windows is unsupported");
        };
        assert!(reason.contains("macOS only"), "{reason}");
        let service = HostFacts {
            os: "windows",
            target: Some(Target::X86_64PcWindowsMsvc),
            ..HostFacts::default()
        };
        assert!(
            matches!(platform_state(&service), PlatformState::Headless { .. }),
            "windows without SESSIONNAME is headless"
        );

        let unpinned = HostFacts {
            os: "freebsd",
            target: None,
            ..HostFacts::default()
        };
        let PlatformState::Unsupported { triple, reason, .. } = platform_state(&unpinned) else {
            panic!("a host without a pinned driver is unsupported");
        };
        assert_eq!(triple, "this host");
        assert!(reason.contains("no Cua Driver"), "{reason}");

        assert_eq!(
            platform_state(&macos()),
            PlatformState::Supported {
                os: "macos".to_owned(),
                triple: "aarch64-apple-darwin".to_owned(),
            }
        );
        let ssh = HostFacts {
            launchd_manager: Some("Background"),
            ..macos()
        };
        let PlatformState::Headless { reason, .. } = platform_state(&ssh) else {
            panic!("a macOS background session is headless");
        };
        assert!(reason.contains("Background"), "{reason}");
        let unknown = HostFacts {
            launchd_manager: None,
            ..macos()
        };
        assert!(
            platform_state(&unknown).probes(),
            "when launchctl cannot be asked the driver's report decides"
        );
    }

    #[test]
    fn the_grant_plan_drives_the_driver_and_opens_the_pane_on_macos_only() {
        let driver = Path::new("/tmp/nolune/cua-driver");
        let plan = grant_plan("macos", "accessibility", Some(driver)).unwrap();
        assert_eq!(plan.driver_grant.as_deref(), Some(driver));
        assert!(
            plan.settings_url.contains("Privacy_Accessibility"),
            "{}",
            plan.settings_url
        );
        let plan = grant_plan("macos", "screen_recording", Some(driver)).unwrap();
        assert!(
            plan.settings_url.contains("Privacy_ScreenCapture"),
            "{}",
            plan.settings_url
        );
        assert_eq!(
            settings_pane_url("screen_recording"),
            Some(plan.settings_url)
        );
        assert_eq!(settings_pane_url("clipboard"), None);

        let error = grant_plan("macos", "accessibility", None).unwrap_err();
        assert!(error.contains(INSTALL_COMMAND), "{error}");
        let error = grant_plan("macos", "clipboard", Some(driver)).unwrap_err();
        assert!(error.contains("clipboard"), "{error}");
        for os in ["linux", "windows"] {
            let error = grant_plan(os, "accessibility", Some(driver)).unwrap_err();
            assert!(error.contains("macOS only"), "{os}: {error}");
        }
    }

    #[test]
    fn the_copy_never_promises_more_than_one_shot_capture() {
        // Every sentence this module can put on the page.
        let mut copy = vec![];
        for facts in [
            macos(),
            HostFacts {
                launchd_manager: Some("System"),
                ..macos()
            },
            HostFacts {
                os: "linux",
                target: Some(Target::X86_64UnknownLinuxGnu),
                display: Some(OsStr::new(":0")),
                ..HostFacts::default()
            },
            HostFacts {
                os: "linux",
                target: Some(Target::X86_64UnknownLinuxGnu),
                ..HostFacts::default()
            },
            HostFacts {
                os: "windows",
                target: Some(Target::X86_64PcWindowsMsvc),
                ..HostFacts::default()
            },
        ] {
            copy.push(serde_json::to_string(&platform_state(&facts)).unwrap());
        }
        let driver = Path::new("/tmp/cua-driver");
        for probe in [
            Ok(report(HEALTHY)),
            Ok(report(ACCESSIBILITY_DENIED)),
            Ok(report(NEVER_ASKED)),
            Ok(serde_json::from_value(healthy_on("0.27.0")).unwrap()),
            Err("no answer".to_owned()),
        ] {
            let permissions = probe.as_ref().ok().map(driver_permissions);
            let state = driver_state(driver, probe.as_ref().map_err(Clone::clone));
            let assembled = assemble(
                platform_state(&macos()),
                InstallState::None,
                state,
                permissions,
            );
            copy.push(assembled.summary);
        }
        copy.push(grant_plan("linux", "accessibility", None).unwrap_err());
        copy.push(grant_plan("macos", "accessibility", None).unwrap_err());
        for text in copy {
            let lowered = text.to_lowercase();
            for forbidden in ["continuous", "always on", "always-on", "stream", "watch"] {
                assert!(
                    !lowered.contains(forbidden),
                    "{text:?} implies more than one-shot capture ({forbidden})"
                );
            }
        }
    }
}
