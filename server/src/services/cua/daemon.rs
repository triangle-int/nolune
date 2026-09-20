//! The app daemon behind `cua-driver mcp` on macOS (#20).
//!
//! On macOS `cua-driver mcp` is a proxy: it talks to the `CuaDriver.app`
//! daemon (`cua-driver serve`) that owns the login session's socket, and
//! when none runs it starts one by name through LaunchServices (`open -a
//! CuaDriver`). By name means whichever `CuaDriver.app` LaunchServices
//! resolves, which on a fresh Mac is none and on a Mac with the upstream
//! installer is `/Applications/CuaDriver.app`, never the copy `nolune cua
//! install` verified. So Nolune starts the daemon itself, by path, from the
//! bundle the located driver runs from, and only when none is running: the
//! driver keeps one daemon per login session (its pid file is not per
//! socket), so a running foreign daemon is reported, not replaced.
//!
//! The daemon has to be launched through LaunchServices, not spawned: only
//! then does macOS attribute Accessibility and Screen Recording to the
//! bundle (`com.trycua.driver`) instead of the terminal or Nolune.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::Context as _;

/// The bundle identifier of the genuine driver app.
pub const BUNDLE_ID: &str = "com.trycua.driver";

/// How often the daemon is asked whether it is up while it starts.
const POLL_INTERVAL: Duration = Duration::from_millis(250);
/// How long `<driver> status` may take to answer.
const STATUS_TIMEOUT: Duration = Duration::from_secs(5);

/// Whether the daemon `driver` proxies to is up.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DaemonState {
    Running,
    NotRunning,
    /// `<driver> status` answered something else; the text says what.
    Unknown(String),
}

/// The genuine `CuaDriver.app` bundle `driver` runs from: the `.app`
/// ancestor of its `Contents/MacOS/` directory whose `Info.plist` declares
/// [`BUNDLE_ID`]. `None` for a bare binary, a bundle of another app, or a
/// bundle without a readable plist. Symlinks (the upstream installer's
/// `~/.local/bin/cua-driver`) are followed first.
pub fn app_bundle(driver: &Path) -> Option<PathBuf> {
    let driver = fs::canonicalize(driver).ok()?;
    // <bundle>/Contents/MacOS/<binary>
    let macos = driver.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    if macos.file_name()? != "MacOS"
        || contents.file_name()? != "Contents"
        || bundle.extension()? != "app"
    {
        return None;
    }
    let plist = fs::read_to_string(contents.join("Info.plist")).ok()?;
    (bundle_identifier(&plist).as_deref() == Some(BUNDLE_ID)).then(|| bundle.to_path_buf())
}

/// `CFBundleIdentifier` from an XML `Info.plist`.
pub fn bundle_identifier(info_plist: &str) -> Option<String> {
    let key = "<key>CFBundleIdentifier</key>";
    let after_key = &info_plist[info_plist.find(key)? + key.len()..];
    let value = after_key.trim_start().strip_prefix("<string>")?;
    let end = value.find("</string>")?;
    let id = value[..end].trim();
    (!id.is_empty()).then(|| id.to_owned())
}

/// Ask `<driver> status` whether the daemon it proxies to is running.
pub async fn state(driver: &Path) -> DaemonState {
    let output = tokio::time::timeout(
        STATUS_TIMEOUT,
        tokio::process::Command::new(driver)
            .arg("status")
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await;
    let output = match output {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => {
            return DaemonState::Unknown(format!(
                "cannot run {} status: {error}",
                driver.display()
            ));
        }
        Err(_elapsed) => {
            return DaemonState::Unknown(format!(
                "{} status did not answer within {STATUS_TIMEOUT:?}",
                driver.display()
            ));
        }
    };
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if output.status.success() {
        DaemonState::Running
    } else if text.contains("not running") {
        DaemonState::NotRunning
    } else {
        DaemonState::Unknown(format!(
            "{} status answered ({}): {}",
            driver.display(),
            output.status,
            text.split_whitespace().collect::<Vec<_>>().join(" ")
        ))
    }
}

/// The LaunchServices launch of `bundle`'s daemon: `open -n -g <bundle>
/// --args serve`, by path so no other `CuaDriver.app` can answer.
pub fn launch_command(bundle: &Path) -> tokio::process::Command {
    let mut command = tokio::process::Command::new("open");
    command
        .arg("-n")
        .arg("-g")
        .arg(bundle)
        .arg("--args")
        .arg("serve")
        .stdin(std::process::Stdio::null());
    command
}

/// Run `launch`, then wait until `<driver> status` reports the daemon as
/// running, at most `wait`.
pub async fn start(
    driver: &Path,
    mut launch: tokio::process::Command,
    wait: Duration,
) -> anyhow::Result<()> {
    let describe = |command: &tokio::process::Command| {
        let command = command.as_std();
        std::iter::once(command.get_program())
            .chain(command.get_args())
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ")
    };
    let launched = describe(&launch);
    let output = launch
        .output()
        .await
        .with_context(|| format!("cannot run `{launched}`"))?;
    if !output.status.success() {
        anyhow::bail!(
            "`{launched}` failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let deadline = Instant::now() + wait;
    loop {
        match state(driver).await {
            DaemonState::Running => return Ok(()),
            DaemonState::NotRunning | DaemonState::Unknown(_) if Instant::now() < deadline => {
                tokio::time::sleep(POLL_INTERVAL).await;
            }
            DaemonState::NotRunning => anyhow::bail!(
                "the daemon did not answer within {wait:?} of `{launched}`; if macOS is asking \
                 to grant Accessibility or Screen Recording to Cua Driver, grant them and run \
                 the status again"
            ),
            DaemonState::Unknown(why) => anyhow::bail!(
                "the daemon did not answer within {wait:?} of `{launched}` ({why}); if macOS \
                 is asking to grant Accessibility or Screen Recording to Cua Driver, grant \
                 them and run the status again"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDisplayName</key>
	<string>Cua Driver</string>
	<key>CFBundleExecutable</key>
	<string>cua-driver</string>
	<key>CFBundleIdentifier</key>
	<string>com.trycua.driver</string>
	<key>CFBundleName</key>
	<string>Cua Driver</string>
	<key>LSUIElement</key>
	<true/>
</dict>
</plist>
"#;

    #[test]
    fn bundle_identifier_reads_the_drivers_plist_and_nothing_else() {
        assert_eq!(
            bundle_identifier(REAL_PLIST).as_deref(),
            Some("com.trycua.driver")
        );
        assert_eq!(
            bundle_identifier(
                "<plist><dict><key>CFBundleIdentifier</key>\n  <string> com.example.other </string></dict></plist>"
            )
            .as_deref(),
            Some("com.example.other"),
            "whitespace around the value is not part of it"
        );
        assert_eq!(bundle_identifier("<plist/>"), None);
        assert_eq!(bundle_identifier(""), None);
        assert_eq!(
            bundle_identifier("<key>CFBundleName</key><string>com.trycua.driver</string>"),
            None,
            "another key naming the id is not the id"
        );
    }

    fn bundle_with(dir: &Path, plist: Option<&str>) -> PathBuf {
        let bundle = dir.join("CuaDriver.app");
        let macos = bundle.join("Contents/MacOS");
        fs::create_dir_all(&macos).unwrap();
        fs::write(macos.join("cua-driver"), "#!/bin/sh\n").unwrap();
        if let Some(plist) = plist {
            fs::write(bundle.join("Contents/Info.plist"), plist).unwrap();
        }
        bundle
    }

    #[test]
    fn app_bundle_is_the_genuine_bundle_the_driver_runs_from() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = bundle_with(dir.path(), Some(REAL_PLIST));
        let driver = bundle.join("Contents/MacOS/cua-driver");
        assert_eq!(
            app_bundle(&driver).map(|found| fs::canonicalize(found).unwrap()),
            Some(fs::canonicalize(&bundle).unwrap())
        );
    }

    #[test]
    fn app_bundle_is_none_for_bare_binaries_and_other_bundles() {
        let dir = tempfile::tempdir().unwrap();
        let bare = dir.path().join("cua-driver");
        fs::write(&bare, "#!/bin/sh\n").unwrap();
        assert_eq!(app_bundle(&bare), None, "a bare binary has no bundle");

        let unnamed = bundle_with(&dir.path().join("no-plist"), None);
        assert_eq!(
            app_bundle(&unnamed.join("Contents/MacOS/cua-driver")),
            None,
            "the test archives' empty plist is not the driver app"
        );

        let other = bundle_with(
            &dir.path().join("other"),
            Some(
                "<plist><dict><key>CFBundleIdentifier</key><string>com.example.other</string></dict></plist>",
            ),
        );
        assert_eq!(app_bundle(&other.join("Contents/MacOS/cua-driver")), None);

        let outside = bundle_with(&dir.path().join("outside"), Some(REAL_PLIST));
        // A binary beside the bundle, not inside it.
        let beside = outside.parent().unwrap().join("cua-driver");
        fs::write(&beside, "#!/bin/sh\n").unwrap();
        assert_eq!(app_bundle(&beside), None);

        assert_eq!(app_bundle(&dir.path().join("missing")), None);
    }

    #[cfg(unix)]
    #[test]
    fn app_bundle_follows_a_symlink_into_the_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = bundle_with(dir.path(), Some(REAL_PLIST));
        let link = dir.path().join("cua-driver");
        std::os::unix::fs::symlink(bundle.join("Contents/MacOS/cua-driver"), &link).unwrap();
        assert_eq!(
            app_bundle(&link).map(|found| fs::canonicalize(found).unwrap()),
            Some(fs::canonicalize(&bundle).unwrap())
        );
    }

    #[cfg(unix)]
    fn fake_driver(dir: &Path, status_body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        fs::create_dir_all(dir).unwrap();
        let path = dir.join("cua-driver");
        fs::write(
            &path,
            format!(
                "#!/bin/sh\ncase \"$1\" in\n  status) {status_body} ;;\n  *) exit 2 ;;\nesac\n"
            ),
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn state_reads_the_drivers_own_status_command() {
        let dir = tempfile::tempdir().unwrap();
        let running = fake_driver(
            &dir.path().join("running"),
            "echo 'Cua Driver daemon is running'; exit 0",
        );
        assert_eq!(state(&running).await, DaemonState::Running);

        let stopped = fake_driver(
            &dir.path().join("stopped"),
            "echo 'Cua Driver daemon is not running'; exit 1",
        );
        assert_eq!(state(&stopped).await, DaemonState::NotRunning);

        let odd = fake_driver(
            &dir.path().join("odd"),
            "echo 'Cua Driver daemon is unhealthy: socket refused' >&2; exit 1",
        );
        let DaemonState::Unknown(text) = state(&odd).await else {
            panic!("an answer that is neither running nor not running is unknown");
        };
        assert!(text.contains("unhealthy"), "{text}");

        let DaemonState::Unknown(text) = state(&dir.path().join("missing")).await else {
            panic!("a driver that cannot run has no known daemon");
        };
        assert!(text.contains("missing"), "{text}");
    }

    #[test]
    fn launch_command_opens_the_bundle_by_path_in_the_background() {
        let bundle = Path::new("/Users/me/.nolune/cua-driver/releases/0.28.2/x/CuaDriver.app");
        let command = launch_command(bundle);
        let std_command = command.as_std();
        assert_eq!(std_command.get_program(), "open");
        let args: Vec<_> = std_command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            ["-n", "-g", bundle.to_str().unwrap(), "--args", "serve"],
            "by path, a new instance, not in the foreground, no other serve flags"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn start_waits_for_the_daemon_the_launch_brings_up() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("up");
        // `status` answers running once the marker exists.
        let driver = dir.path().join("cua-driver");
        fs::write(
            &driver,
            format!(
                "#!/bin/sh\ncase \"$1\" in\n  status) if [ -e '{}' ]; then echo running; exit 0; else echo 'not running'; exit 1; fi ;;\n  *) exit 2 ;;\nesac\n",
                marker.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&driver, fs::Permissions::from_mode(0o755)).unwrap();

        // A launch that brings the daemon up after a moment.
        let mut launch = tokio::process::Command::new("sh");
        launch.args([
            "-c",
            &format!(
                "(sleep 0.4; touch '{}') >/dev/null 2>&1 &",
                marker.display()
            ),
        ]);
        let started = Instant::now();
        start(&driver, launch, Duration::from_secs(10))
            .await
            .expect("the daemon came up");
        assert!(marker.exists());
        assert!(started.elapsed() < Duration::from_secs(5));

        // A launch that fails outright is reported with its stderr.
        fs::remove_file(&marker).unwrap();
        let mut failing = tokio::process::Command::new("sh");
        failing.args(["-c", "echo 'Unable to find application' >&2; exit 1"]);
        let error = start(&driver, failing, Duration::from_secs(10))
            .await
            .unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("Unable to find application"), "{message}");

        // A launch after which the daemon never answers gives up at `wait`.
        let mut silent = tokio::process::Command::new("true");
        silent.arg("nothing");
        let started = Instant::now();
        let error = start(&driver, silent, Duration::from_millis(600))
            .await
            .unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(5));
        let message = format!("{error:#}");
        assert!(message.contains("600ms"), "names the wait: {message}");
        assert!(
            message.contains("Accessibility"),
            "hints at the TCC gate: {message}"
        );
    }
}
