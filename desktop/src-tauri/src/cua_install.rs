//! Installing the Cua Driver from inside the app (#231).
//!
//! Until now the only way to put a driver on a computer was `nolune cua
//! install` in a terminal, which a desktop user has no reason to own: the
//! app's own install (#128) drops the binary in `~/.nolune/bin` without
//! putting it on `PATH`, and a desktop bound to a server somewhere else has
//! no `nolune` at all. So the settings window installs the driver itself,
//! through the same verified installer the CLI runs
//! (`cua_protocol::cua_driver_install`): same pin, same size and sha256
//! check before a byte is kept, same refusal of a driver reporting any
//! other version, same `cua-driver/install.json` under the workspace, so a
//! computer that has one is the same computer either way.
//!
//! What this does not do is update anything on its own. A driver arrives
//! because someone pressed the button, and it stays that version until a
//! Nolune release moves the pin and someone presses it again.

use std::path::PathBuf;

use cua_protocol::{
    cua_driver_install::{
        self, InstallOutcome, InstallRequest, InstallStep, InstalledDriver, RELEASE_URL_ENV,
    },
    cua_driver_pin::{self, Target, PINNED_VERSION},
};
use serde::Serialize;
use tauri::Emitter;

/// The event the settings window listens on while an install runs.
pub const PROGRESS_EVENT: &str = "cua-install-progress";

/// One step of a running install, as the window narrates it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum Progress {
    /// The asset is being fetched; `size` is what the pin says it is.
    Downloading { url: String, size: u64 },
    /// The bytes hashed to the pinned sha256; nothing was written before.
    Verified { sha256: String },
    /// The extracted binary reported the pinned version.
    VersionChecked { version: String },
}

impl From<&InstallStep> for Progress {
    fn from(step: &InstallStep) -> Self {
        match step {
            InstallStep::Downloading { url, size } => Progress::Downloading {
                url: url.clone(),
                size: *size,
            },
            InstallStep::Verified { sha256 } => Progress::Verified {
                sha256: sha256.clone(),
            },
            InstallStep::VersionChecked { version } => Progress::VersionChecked {
                version: version.clone(),
            },
        }
    }
}

/// What an install left behind.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct InstallReport {
    pub version: String,
    pub driver: String,
    /// The pinned driver was already there and nothing was downloaded.
    pub already_installed: bool,
    /// The machine socket was told to register this computer again, so the
    /// companion learns about the driver without the user reconnecting.
    pub reannounced: bool,
    /// The executable of another `CuaDriver.app` whose daemon was stopped
    /// so this driver's could answer.
    pub stopped_daemon: Option<String>,
}

impl InstallReport {
    fn new(outcome: InstallOutcome, reannounced: bool, stopped_daemon: Option<PathBuf>) -> Self {
        let (installed, already_installed): (InstalledDriver, bool) = match outcome {
            InstallOutcome::AlreadyInstalled(installed) => (installed, true),
            InstallOutcome::Installed(installed) => (installed, false),
        };
        Self {
            version: installed.version,
            driver: installed.driver.display().to_string(),
            already_installed,
            reannounced,
            stopped_daemon: stopped_daemon.map(|path| path.display().to_string()),
        }
    }
}

/// Where an install would put the driver and what it would fetch, or why
/// this host cannot have one. Separated from the install itself so the
/// decision is testable without a network.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallPlan {
    pub target: Target,
    pub asset: &'static cua_driver_pin::PinnedAsset,
    pub release_url: String,
    pub workspace: PathBuf,
}

/// The plan for `os`/`workspace`, with `mirror` from [`RELEASE_URL_ENV`].
/// An error is the sentence the window shows: a host the pin covers no
/// driver for can never be given one, whatever the button says.
pub fn plan(
    target: Option<Target>,
    workspace: PathBuf,
    mirror: Option<&str>,
) -> Result<InstallPlan, String> {
    let target = target.ok_or_else(|| {
        format!(
            "Nolune ships no Cua Driver {PINNED_VERSION} for this computer, so there is nothing \
             to install here."
        )
    })?;
    Ok(InstallPlan {
        target,
        asset: cua_driver_pin::asset_for(target),
        release_url: cua_driver_install::release_url(mirror),
        workspace,
    })
}

/// One install at a time: the button is disabled while one runs, and a
/// second window (or a second click that raced it) is refused rather than
/// downloading the same asset twice into the same staging directory.
static RUNNING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Install the pinned driver under this computer's workspace, narrating
/// each step to the window, then tell the machine socket to register again
/// so the companion sees a computer that can now be driven.
#[tauri::command]
pub async fn cua_install_driver(
    app: tauri::AppHandle,
    force: bool,
) -> Result<InstallReport, String> {
    let Ok(_guard) = RUNNING.try_lock() else {
        return Err("An install is already running; wait for it to finish.".to_owned());
    };
    let plan = plan(
        Target::current(),
        crate::local_server::nolune_home(),
        std::env::var(RELEASE_URL_ENV).ok().as_deref(),
    )?;
    let request = InstallRequest {
        root: &plan.workspace,
        target: plan.target,
        asset: plan.asset,
        release_url: &plan.release_url,
        force,
    };
    let outcome = cua_driver_install::install(&request, &mut |step| {
        let progress = Progress::from(&step);
        eprintln!("[cua] install: {progress:?}");
        let _ = app.emit(PROGRESS_EVENT, &progress);
    })
    .await
    .map_err(|error| format!("{error:#}"))?;
    // The driver that is running is the one that was there before this
    // install: after a reinstall over a stale version, exactly the version
    // this replaced. Let it go so the next probe or request starts the one
    // on disk now, then have the socket register this computer again with
    // that driver's descriptor.
    crate::cua_runtime::runtime().replace_driver().await;
    // The daemon has to be this driver's too: another `CuaDriver.app`'s
    // (the upstream installer's, say) would keep answering, or refusing,
    // whatever was installed here.
    let driver = match &outcome {
        InstallOutcome::AlreadyInstalled(installed) | InstallOutcome::Installed(installed) => {
            installed.driver.clone()
        }
    };
    let stopped_daemon = crate::cua_runtime::take_over_daemon(&driver).await?;
    let reannounced = crate::computer_use_bridge::reannounce();
    Ok(InstallReport::new(outcome, reannounced, stopped_daemon))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_without_a_pinned_driver_is_told_so_instead_of_being_offered_one() {
        let error = plan(None, PathBuf::from("/tmp/ws"), None).unwrap_err();
        assert!(error.contains(PINNED_VERSION), "{error}");
        assert!(error.contains("nothing to install"), "{error}");
    }

    #[test]
    fn the_plan_fetches_this_targets_pinned_asset_from_the_upstream_release() {
        let target = Target::Aarch64AppleDarwin;
        let plan = plan(Some(target), PathBuf::from("/tmp/ws"), None).unwrap();
        assert_eq!(plan.target, target);
        assert_eq!(plan.asset, cua_driver_pin::asset_for(target));
        assert_eq!(
            format!("{}/{}", plan.release_url, plan.asset.name),
            plan.asset.download_url(),
            "the default release directory is the pin's own"
        );
        assert_eq!(plan.workspace, PathBuf::from("/tmp/ws"));
    }

    /// The desktop honours the same mirror the CLI does, so an air-gapped
    /// install is configured once for the computer, not once per entry
    /// point. The checksum is enforced either way.
    #[test]
    fn a_mirror_replaces_the_release_directory_for_the_button_too() {
        let plan = plan(
            Some(Target::X86_64UnknownLinuxGnu),
            PathBuf::from("/tmp/ws"),
            Some("https://mirror.example/cua/"),
        )
        .unwrap();
        assert_eq!(plan.release_url, "https://mirror.example/cua");
    }

    #[test]
    fn a_report_says_whether_anything_was_downloaded() {
        let installed = InstalledDriver {
            version: PINNED_VERSION.to_owned(),
            target: Target::Aarch64AppleDarwin.triple().to_owned(),
            asset: "asset.tar.gz".to_owned(),
            sha256: "0".repeat(64),
            size: 1,
            driver: PathBuf::from("/ws/cua-driver/releases/x/cua-driver"),
            installed_at: "2026-09-23T00:00:00Z".to_owned(),
        };
        let fresh = InstallReport::new(InstallOutcome::Installed(installed.clone()), true, None);
        assert!(!fresh.already_installed);
        assert!(fresh.reannounced);
        assert_eq!(fresh.version, PINNED_VERSION);
        assert_eq!(fresh.driver, "/ws/cua-driver/releases/x/cua-driver");
        assert_eq!(fresh.stopped_daemon, None);

        let again = InstallReport::new(
            InstallOutcome::AlreadyInstalled(installed),
            false,
            Some(PathBuf::from(
                "/Applications/CuaDriver.app/Contents/MacOS/cua-driver",
            )),
        );
        assert!(again.already_installed);
        assert!(!again.reannounced);
        assert_eq!(
            again.stopped_daemon.as_deref(),
            Some("/Applications/CuaDriver.app/Contents/MacOS/cua-driver"),
            "the page says whose daemon made way"
        );
    }
}
