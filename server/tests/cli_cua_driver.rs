//! `nolune cua install` and `nolune cua status` (#20) through the real binary.
//!
//! The pinned release is served by a local HTTP file server: nothing here
//! reaches the network, and the driver inside the served archive is a shell
//! script that answers `--version` and, for `mcp`, a canned health report.
//! The pinned digest is 70 MB of real driver, so the success paths override
//! it through the debug-only `NOLUNE_CUA_TEST_PIN` seam; the wrong-checksum
//! path uses the real pin (which refuses whatever the server sends) and then
//! the seam with the right size and a wrong digest.
#![cfg(unix)]

use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::{Arc, Mutex},
    thread,
};

use cua_protocol::cua_driver_pin::{PINNED_VERSION, RELEASE_TAG, Target, asset_for, sha256_hex};

const BIN: &str = env!("CARGO_BIN_EXE_nolune");

/// A local HTTP/1.1 file server standing in for the GitHub release: one
/// request per connection, files served from a directory, every request
/// path recorded.
struct MockRelease {
    base_url: String,
    requests: Arc<Mutex<Vec<String>>>,
    /// When set, every response announces this `Content-Length` instead of
    /// the file's real size (the body is still the file).
    announce: Arc<Mutex<Option<u64>>>,
}

impl MockRelease {
    fn serve(dir: PathBuf) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = requests.clone();
        let announce = Arc::new(Mutex::new(None));
        let announced = announce.clone();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut head = Vec::new();
                let mut byte = [0u8; 1];
                while stream.read(&mut byte).map(|n| n == 1).unwrap_or(false) {
                    head.push(byte[0]);
                    if head.ends_with(b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&head).into_owned();
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("/")
                    .to_owned();
                log.lock().unwrap().push(path.clone());
                let file = path
                    .strip_prefix("/release/")
                    .map(|name| dir.join(name))
                    .filter(|file| file.is_file());
                let response = match file.and_then(|file| fs::read(file).ok()) {
                    Some(body) => {
                        let length = announced.lock().unwrap().unwrap_or(body.len() as u64);
                        let mut response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n"
                        )
                        .into_bytes();
                        response.extend_from_slice(&body);
                        response
                    }
                    None => {
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .to_vec()
                    }
                };
                let _ = stream.write_all(&response);
            }
        });
        Self {
            base_url: format!("http://127.0.0.1:{port}/release"),
            requests,
            announce,
        }
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }

    /// Lie about the size of every asset from now on.
    fn announce(&self, length: u64) {
        *self.announce.lock().unwrap() = Some(length);
    }
}

/// Whether the host running the tests has a graphical session for `nolune
/// cua status` to probe a driver in: Linux tests set `DISPLAY` themselves,
/// a Mac has one when launchd runs the test inside an Aqua session (a
/// terminal), not over SSH.
fn host_has_gui() -> bool {
    if cfg!(target_os = "macos") {
        Command::new("launchctl")
            .arg("managername")
            .output()
            .map(|out| text(&out.stdout).trim() == "Aqua")
            .unwrap_or(false)
    } else {
        true
    }
}

/// The pinned asset for the host running this test; every test installs
/// "for the current target" exactly as a user would.
fn host() -> Target {
    Target::current().expect("the test host is a release target")
}

/// A driver stand-in: `--version` prints `version`, `mcp` speaks just enough
/// MCP to answer `initialize` and one `tools/call` with the JSON in
/// `report_file`. Anything else exits 2.
fn fake_driver_script(version: &str, report_file: &Path) -> String {
    format!(
        r#"#!/bin/sh
case "$1" in
  --version) echo "cua-driver {version}"; exit 0 ;;
  mcp) ;;
  *) exit 2 ;;
esac
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"protocolVersion":"2025-03-26","capabilities":{{"tools":{{}}}},"serverInfo":{{"name":"fake-cua-driver","version":"{version}"}}}}}}\n' "$id"
      ;;
    *'"method":"tools/call"'*)
      id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"content":[{{"type":"text","text":"health"}}],"structuredContent":' "$id"
      cat '{report}'
      printf '}}}}\n'
      ;;
  esac
done
"#,
        report = report_file.display()
    )
}

/// A driver stand-in whose `mcp` explains itself on stderr and exits, the
/// shape of the real driver when it cannot reach its app daemon.
fn refusing_driver_script(version: &str, hint: &str) -> String {
    format!(
        r#"#!/bin/sh
case "$1" in
  --version) echo "cua-driver {version}"; exit 0 ;;
  mcp) echo "mcp launched without CuaDriver.app's TCC grants" >&2; echo "{hint}" >&2; exit 1 ;;
  *) exit 2 ;;
esac
"#
    )
}

/// A driver stand-in that records every `mcp` start in `marker` and then
/// behaves like [`fake_driver_script`].
#[cfg(target_os = "linux")]
fn recording_driver_script(version: &str, report_file: &Path, marker: &Path) -> String {
    fake_driver_script(version, report_file).replacen(
        "  mcp) ;;",
        &format!("  mcp) echo started >> '{}' ;;", marker.display()),
        1,
    )
}

/// A health report the protocol accepts, for the platform this host's driver
/// would report, naming `driver_version`.
fn health_report(driver_version: &str) -> serde_json::Value {
    let platform = if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    };
    let mut checks = vec![
        serde_json::json!({"name": "binary_version", "status": "pass", "message": format!("cua-driver {driver_version}")}),
        serde_json::json!({"name": "platform_supported", "status": "pass", "message": "supported", "data": {"architecture": "arm64", "os_version": "27.0"}}),
        serde_json::json!({"name": "session_active", "status": "pass", "message": "MCP session is active."}),
    ];
    if cfg!(target_os = "macos") {
        checks.extend([
            serde_json::json!({"name": "tcc_accessibility", "status": "pass", "message": "Accessibility is granted.", "data": {"bundle_identifier": "com.trycua.driver"}}),
            serde_json::json!({"name": "tcc_screen_recording", "status": "pass", "message": "Screen Recording is granted.", "data": {"bundle_identifier": "com.trycua.driver"}}),
        ]);
    } else {
        checks.extend([
            serde_json::json!({"name": "ax_capability", "status": "pass", "message": "AT-SPI is reachable."}),
            serde_json::json!({"name": "screen_capture_capability", "status": "pass", "message": "Capture works."}),
        ]);
    }
    serde_json::json!({
        "schema_version": "1",
        "platform": platform,
        "driver_version": driver_version,
        "overall": "ok",
        "checks": checks,
    })
}

/// The pinned asset's archive with `script` as the driver, laid out the way
/// the upstream release lays it out: the Mac tarball wraps a `CuaDriver.app`
/// bundle in a stage directory, the Linux tarball carries the binary at its
/// root.
fn fake_archive(target: Target, script: &str) -> Vec<u8> {
    let asset = asset_for(target);
    assert!(
        asset.name.ends_with(".tar.gz"),
        "unix hosts install a tarball, got {}",
        asset.name
    );
    let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
        Vec::new(),
        flate2::Compression::fast(),
    ));
    let mut add = |path: &str, contents: &[u8], mode: u32| {
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(mode);
        header.set_cksum();
        builder.append_data(&mut header, path, contents).unwrap();
    };
    match target {
        Target::Aarch64AppleDarwin | Target::X86_64AppleDarwin => {
            let stage = format!("cua-driver-rs-{PINNED_VERSION}-darwin-universal");
            add(&format!("{stage}/LICENSE"), b"MIT", 0o644);
            add(&format!("{stage}/cua-driver"), script.as_bytes(), 0o755);
            add(
                &format!("{stage}/CuaDriver.app/Contents/Info.plist"),
                b"<plist/>",
                0o644,
            );
            add(
                &format!("{stage}/CuaDriver.app/Contents/MacOS/cua-driver"),
                script.as_bytes(),
                0o755,
            );
        }
        _ => {
            add("cua-driver", script.as_bytes(), 0o755);
            add("libcua_driver_sdk.so", b"not really", 0o644);
        }
    }
    builder.into_inner().unwrap().finish().unwrap()
}

struct Sandbox {
    _tmp: tempfile::TempDir,
    home_dir: PathBuf,
    nolune_home: PathBuf,
    release_dir: PathBuf,
    release: MockRelease,
    report_file: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let home_dir = tmp.path().join("home");
        let nolune_home = home_dir.join(".nolune");
        let release_dir = tmp.path().join("release");
        fs::create_dir_all(&home_dir).unwrap();
        fs::create_dir_all(&release_dir).unwrap();
        let report_file = tmp.path().join("health-report.json");
        fs::write(
            &report_file,
            serde_json::to_vec(&health_report(PINNED_VERSION)).unwrap(),
        )
        .unwrap();
        let release = MockRelease::serve(release_dir.clone());
        Self {
            _tmp: tmp,
            home_dir,
            nolune_home,
            release_dir,
            release,
            report_file,
        }
    }

    /// Serve `bytes` as the pinned asset for this host and return the
    /// `NOLUNE_CUA_TEST_PIN` value that makes the binary accept them.
    fn publish(&self, bytes: &[u8]) -> String {
        let asset = asset_for(host());
        fs::write(self.release_dir.join(asset.name), bytes).unwrap();
        format!("{}:{}", sha256_hex(bytes), bytes.len())
    }

    /// The archive every success path installs: a driver reporting the pin.
    fn publish_pinned_driver(&self) -> String {
        let script = fake_driver_script(PINNED_VERSION, &self.report_file);
        self.publish(&fake_archive(host(), &script))
    }

    /// The binary with a sandboxed HOME, the mock release as the mirror, and
    /// a PATH that cannot contain a real `cua-driver`.
    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(BIN);
        cmd.args(args)
            .env("HOME", &self.home_dir)
            .env("NOLUNE_HOME", &self.nolune_home)
            .env("NOLUNE_CUA_RELEASE_URL", &self.release.base_url)
            .env("PATH", "/usr/bin:/bin")
            .env_remove("NOLUNE_CUA_DRIVER")
            .env_remove("NOLUNE_CUA_TEST_PIN")
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("PORT")
            .env("RUST_LOG", "warn")
            .stdin(std::process::Stdio::null());
        cmd
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    /// The binary in a session that can run a driver: `DISPLAY` is set so
    /// a Linux host (CI) probes instead of reporting headless; a Mac
    /// answers from its own launchd session.
    fn command_with_display(&self, args: &[&str]) -> Command {
        let mut cmd = self.command(args);
        cmd.env("DISPLAY", ":0");
        cmd
    }

    fn run_with_display(&self, args: &[&str]) -> Output {
        self.command_with_display(args).output().unwrap()
    }

    /// Replace what the fake driver answers `health_report` with.
    fn set_health_report(&self, report: &serde_json::Value) {
        fs::write(&self.report_file, serde_json::to_vec(report).unwrap()).unwrap();
    }

    /// Run with the test pin seam pointing at what `publish` returned.
    fn run_pinned(&self, pin: &str, args: &[&str]) -> Output {
        self.command(args)
            .env("NOLUNE_CUA_TEST_PIN", pin)
            .output()
            .unwrap()
    }

    fn install_dir(&self) -> PathBuf {
        self.nolune_home.join("cua-driver")
    }

    fn manifest(&self) -> serde_json::Value {
        let raw = fs::read_to_string(self.install_dir().join("install.json"))
            .expect("install.json is written by a successful install");
        serde_json::from_str(&raw).unwrap()
    }

    fn assert_nothing_installed(&self) {
        let dir = self.install_dir();
        assert!(
            !dir.join("install.json").exists(),
            "install.json must not exist after a refused install"
        );
        assert!(
            !dir.join("releases").exists(),
            "no release directory may be left behind"
        );
        if dir.exists() {
            let leftovers: Vec<_> = fs::read_dir(&dir).unwrap().flatten().collect();
            assert!(
                leftovers.is_empty(),
                "a refused install left {leftovers:?} under {}",
                dir.display()
            );
        }
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn asset_path() -> String {
    format!("/release/{}", asset_for(host()).name)
}

#[test]
fn help_lists_cua_install_and_status() {
    let out = Command::new(BIN).args(["cua", "--help"]).output().unwrap();
    let help = text(&out.stdout);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(help.contains("\n  install "), "{help}");
    assert!(help.contains("\n  status "), "{help}");
    assert!(help.contains("--profile"), "{help}");

    let out = Command::new(BIN).arg("--help").output().unwrap();
    assert!(
        text(&out.stdout).contains("\n  cua "),
        "{}",
        text(&out.stdout)
    );
}

#[test]
fn install_downloads_verifies_and_installs_the_pinned_driver_under_the_workspace() {
    let sb = Sandbox::new();
    let pin = sb.publish_pinned_driver();

    let out = sb.run_pinned(&pin, &["cua", "install"]);
    let stdout = text(&out.stdout);
    assert!(
        out.status.success(),
        "stdout: {stdout}\nstderr: {}",
        text(&out.stderr)
    );
    assert_eq!(
        sb.release.requests(),
        vec![asset_path()],
        "exactly one download of the pinned asset from the mirror"
    );
    assert!(
        stdout.contains(&format!("Cua Driver {PINNED_VERSION}")),
        "{stdout}"
    );
    assert!(stdout.contains(&host().to_string()), "{stdout}");
    assert!(stdout.contains("verified sha256"), "{stdout}");
    assert!(stdout.contains("nolune cua status"), "{stdout}");
    assert!(
        stdout.contains("never updates the driver on its own"),
        "the no-self-update policy is stated: {stdout}"
    );

    let manifest = sb.manifest();
    assert_eq!(manifest["version"], PINNED_VERSION);
    assert_eq!(manifest["target"], host().triple());
    assert_eq!(manifest["asset"], asset_for(host()).name);
    assert_eq!(manifest["sha256"], pin.split(':').next().unwrap());
    let driver = PathBuf::from(manifest["driver"].as_str().unwrap());
    assert!(
        driver.starts_with(sb.install_dir().join("releases").join(PINNED_VERSION)),
        "the driver lives under releases/<version>: {}",
        driver.display()
    );
    assert!(driver.is_file(), "{}", driver.display());
    assert_ne!(
        fs::metadata(&driver).unwrap().permissions().mode() & 0o111,
        0,
        "the extracted driver is executable"
    );
    if cfg!(target_os = "macos") {
        assert!(
            driver.ends_with("CuaDriver.app/Contents/MacOS/cua-driver"),
            "macOS runs the driver from its app bundle so TCC attributes to it: {}",
            driver.display()
        );
    }
    let version = Command::new(&driver).arg("--version").output().unwrap();
    assert_eq!(
        text(&version.stdout).trim(),
        format!("cua-driver {PINNED_VERSION}")
    );

    // Installing again is a no-op until --force asks for a fresh copy.
    let again = sb.run_pinned(&pin, &["cua", "install"]);
    assert!(again.status.success(), "{}", text(&again.stderr));
    assert!(
        text(&again.stdout).contains("already installed"),
        "{}",
        text(&again.stdout)
    );
    assert_eq!(sb.release.requests().len(), 1, "no second download");

    let forced = sb.run_pinned(&pin, &["cua", "install", "--force"]);
    assert!(forced.status.success(), "{}", text(&forced.stderr));
    assert_eq!(sb.release.requests().len(), 2, "--force downloads again");
    assert!(driver.is_file(), "the driver is in place after a reinstall");
}

#[test]
fn install_refuses_a_wrong_checksum_and_leaves_nothing_installed() {
    let sb = Sandbox::new();
    let script = fake_driver_script(PINNED_VERSION, &sb.report_file);
    let archive = fake_archive(host(), &script);
    let pin = sb.publish(&archive);

    // Real pin, wrong bytes: whatever the mirror serves, it is not the
    // driver, and the pinned size already tells.
    let out = sb.run(&["cua", "install"]);
    let stderr = text(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "stderr: {stderr}");
    assert_eq!(sb.release.requests(), vec![asset_path()]);
    assert!(
        stderr.contains(&asset_for(host()).size.to_string()),
        "names the pinned size: {stderr}"
    );
    assert!(stderr.contains("nothing was installed"), "{stderr}");
    sb.assert_nothing_installed();

    // The right size with the wrong digest: only the sha256 can tell, and
    // it does, before anything is written.
    let (_, size) = pin.split_once(':').unwrap();
    let wrong_digest = "0".repeat(64);
    let out = sb.run_pinned(&format!("{wrong_digest}:{size}"), &["cua", "install"]);
    let stderr = text(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "stderr: {stderr}");
    assert_eq!(sb.release.requests().len(), 2);
    assert!(stderr.contains("sha256"), "{stderr}");
    assert!(
        stderr.contains(&wrong_digest),
        "names the pinned digest: {stderr}"
    );
    assert!(
        stderr.contains(&sha256_hex(&archive)),
        "and the digest it got: {stderr}"
    );
    assert!(stderr.contains("nothing was installed"), "{stderr}");
    sb.assert_nothing_installed();

    let status = sb.run(&["cua", "status"]);
    assert!(status.status.success(), "{}", text(&status.stderr));
    assert!(
        text(&status.stdout).contains("installed: none"),
        "{}",
        text(&status.stdout)
    );
}

#[test]
fn install_refuses_a_mirror_that_announces_another_size_before_downloading_it() {
    let sb = Sandbox::new();
    let pin = sb.publish_pinned_driver();
    let announced = asset_for(host()).size + 1;
    sb.release.announce(announced);

    let out = sb.run_pinned(&pin, &["cua", "install"]);
    let stderr = text(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "stderr: {stderr}");
    assert_eq!(sb.release.requests(), vec![asset_path()]);
    assert!(
        stderr.contains(&announced.to_string()),
        "names the announced size: {stderr}"
    );
    assert!(
        stderr.contains("nothing was downloaded"),
        "the body is never read: {stderr}"
    );
    assert!(stderr.contains("nothing was installed"), "{stderr}");
    sb.assert_nothing_installed();
}

#[test]
fn install_refuses_a_driver_that_reports_another_version() {
    let sb = Sandbox::new();
    let script = fake_driver_script("0.99.0", &sb.report_file);
    let pin = sb.publish(&fake_archive(host(), &script));

    let out = sb.run_pinned(&pin, &["cua", "install"]);
    let stderr = text(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "stderr: {stderr}");
    assert!(stderr.contains("0.99.0"), "{stderr}");
    assert!(
        stderr.contains(&format!("is not the pinned {PINNED_VERSION}")),
        "{stderr}"
    );
    assert!(stderr.contains("nothing was installed"), "{stderr}");
    sb.assert_nothing_installed();
}

#[test]
fn install_is_profile_aware() {
    let sb = Sandbox::new();
    let pin = sb.publish_pinned_driver();

    let out = sb
        .command(&["cua", "install", "--profile", "molinka"])
        .env_remove("NOLUNE_HOME")
        .env("NOLUNE_CUA_TEST_PIN", &pin)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));

    let profile_root = sb.home_dir.join(".nolune-profiles").join("molinka");
    let manifest = profile_root.join("cua-driver").join("install.json");
    assert!(manifest.is_file(), "{}", manifest.display());
    let driver = PathBuf::from(
        serde_json::from_str::<serde_json::Value>(&fs::read_to_string(&manifest).unwrap()).unwrap()
            ["driver"]
            .as_str()
            .unwrap(),
    );
    assert!(driver.starts_with(&profile_root), "{}", driver.display());
    assert!(
        !sb.install_dir().exists(),
        "the default workspace is untouched"
    );

    let status = sb
        .command(&["cua", "status", "--profile", "molinka"])
        .env_remove("NOLUNE_HOME")
        .output()
        .unwrap();
    let stdout = text(&status.stdout);
    assert!(stdout.contains("profile molinka"), "{stdout}");
    assert!(
        stdout.contains(&format!("installed: {PINNED_VERSION}")),
        "{stdout}"
    );
}

#[test]
fn status_without_a_driver_names_the_pin_and_the_next_step() {
    let sb = Sandbox::new();

    let out = sb.run(&["cua", "status"]);
    let stdout = text(&out.stdout);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(
        stdout.contains(&format!("pinned: {PINNED_VERSION} ({RELEASE_TAG}")),
        "{stdout}"
    );
    assert!(stdout.contains(host().triple()), "{stdout}");
    assert!(stdout.contains("installed: none"), "{stdout}");
    assert!(stdout.contains("driver: none found"), "{stdout}");
    assert!(stdout.contains("nolune cua install"), "{stdout}");
    assert!(
        stdout.contains("never updates the driver on its own"),
        "{stdout}"
    );
    if cfg!(target_os = "macos") {
        assert!(stdout.contains("platform: supported"), "{stdout}");
    } else {
        assert!(stdout.contains("platform: unsupported"), "{stdout}");
        assert!(stdout.contains("headless"), "{stdout}");
    }
    assert!(
        !sb.install_dir().exists(),
        "status never creates anything under the workspace"
    );
}

#[test]
fn status_reports_the_installed_driver_its_version_and_its_health() {
    let sb = Sandbox::new();
    let pin = sb.publish_pinned_driver();
    assert!(sb.run_pinned(&pin, &["cua", "install"]).status.success());
    let driver = sb.manifest()["driver"].as_str().unwrap().to_owned();

    let out = sb
        .command_with_display(&["cua", "status"])
        .env("NOLUNE_CUA_TEST_PIN", &pin)
        .output()
        .unwrap();
    let stdout = text(&out.stdout);
    assert!(
        out.status.success(),
        "stdout: {stdout}\nstderr: {}",
        text(&out.stderr)
    );
    assert!(
        stdout.contains(&format!("installed: {PINNED_VERSION}")),
        "{stdout}"
    );
    assert!(
        stdout.contains("verified against the pin"),
        "the manifest's digest is checked against the pin: {stdout}"
    );
    assert!(stdout.contains(&driver), "names the driver path: {stdout}");
    assert!(
        stdout.contains("driver: ") && stdout.contains("Nolune install"),
        "says where the driver came from: {stdout}"
    );
    if host_has_gui() {
        // A GUI host probes the driver: the fake answers the health report.
        assert!(
            stdout.contains(&format!("version: {PINNED_VERSION} matches the pin")),
            "{stdout}"
        );
        assert!(stdout.contains("health: ok"), "{stdout}");
        assert!(stdout.contains("accessibility: granted"), "{stdout}");
        assert!(stdout.contains("screen recording: granted"), "{stdout}");
        assert!(
            !stdout.contains("answered by"),
            "a report without a bundle identity names no daemon: {stdout}"
        );
    } else {
        assert!(stdout.contains("health: not probed"), "{stdout}");
    }

    // Without the test seam the manifest's digest is not the real pin's,
    // and status says so instead of calling the install verified.
    let out = sb.run(&["cua", "status"]);
    let stdout = text(&out.stdout);
    assert!(
        stdout.contains("its checksum is not the pinned one"),
        "{stdout}"
    );
    assert!(stdout.contains("--force"), "names the fix: {stdout}");
    assert!(!stdout.contains("verified against the pin"), "{stdout}");
}

#[test]
fn status_fails_clearly_when_the_driver_reports_another_version() {
    let sb = Sandbox::new();
    let pin = sb.publish_pinned_driver();
    assert!(sb.run_pinned(&pin, &["cua", "install"]).status.success());
    // The installed binary passed its `--version` check; what it reports
    // over MCP is what counts at runtime.
    sb.set_health_report(&health_report("0.27.0"));

    let out = sb.run_with_display(&["cua", "status"]);
    let stdout = text(&out.stdout);
    if host_has_gui() {
        assert_eq!(out.status.code(), Some(1), "stdout: {stdout}");
        assert!(
            stdout.contains(&format!("0.27.0 is not the pinned {PINNED_VERSION}")),
            "{stdout}"
        );
        assert!(stdout.contains("nolune cua install"), "{stdout}");
    } else {
        assert!(
            out.status.success(),
            "headless hosts do not probe: {stdout}"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn status_never_starts_the_driver_on_a_headless_host() {
    let sb = Sandbox::new();
    let marker = sb.home_dir.join("mcp-started");
    let script = recording_driver_script(PINNED_VERSION, &sb.report_file, &marker);
    let pin = sb.publish(&fake_archive(host(), &script));
    assert!(sb.run_pinned(&pin, &["cua", "install"]).status.success());

    // `command` unsets DISPLAY and WAYLAND_DISPLAY.
    let out = sb.run(&["cua", "status"]);
    let stdout = text(&out.stdout);
    assert!(out.status.success(), "stdout: {stdout}");
    assert!(stdout.contains("display: no display session"), "{stdout}");
    assert!(stdout.contains("health: not probed"), "{stdout}");
    assert!(stdout.contains("headless"), "{stdout}");
    assert!(
        !marker.exists(),
        "the driver was started on a headless host: {stdout}"
    );

    // The same install, same host, with a display: probed.
    let out = sb.run_with_display(&["cua", "status"]);
    let stdout = text(&out.stdout);
    assert!(out.status.success(), "stdout: {stdout}");
    assert!(stdout.contains("display: DISPLAY=:0"), "{stdout}");
    assert!(stdout.contains("health: ok"), "{stdout}");
    assert!(marker.exists(), "the driver was probed: {stdout}");
}

#[test]
fn status_repeats_what_the_driver_said_when_it_cannot_report() {
    if !host_has_gui() {
        eprintln!("skipped: no graphical session to probe in");
        return;
    }
    let sb = Sandbox::new();
    let hint =
        "grant Accessibility + Screen Recording to CuaDriver.app in System Settings and retry";
    let script = refusing_driver_script(PINNED_VERSION, hint);
    let pin = sb.publish(&fake_archive(host(), &script));
    assert!(sb.run_pinned(&pin, &["cua", "install"]).status.success());

    let out = sb.run_with_display(&["cua", "status"]);
    let stdout = text(&out.stdout);
    assert_eq!(out.status.code(), Some(1), "stdout: {stdout}");
    assert!(
        stdout.contains("health: the driver could not report"),
        "{stdout}"
    );
    assert!(
        stdout.contains(hint),
        "the driver's own hint is part of the status line, not only inherited stderr: {stdout}"
    );
}

#[test]
fn status_names_the_daemon_that_answered_when_it_is_not_the_installed_driver() {
    if !host_has_gui() {
        eprintln!("skipped: no graphical session to probe in");
        return;
    }
    let sb = Sandbox::new();
    let pin = sb.publish_pinned_driver();
    assert!(sb.run_pinned(&pin, &["cua", "install"]).status.success());
    let driver = sb.manifest()["driver"].as_str().unwrap().to_owned();

    // The daemon that answered is the installed driver: named, no warning.
    let mut report = health_report(PINNED_VERSION);
    report["checks"].as_array_mut().unwrap().push(serde_json::json!({
        "name": "bundle_identity", "status": "pass", "message": "Bundle is com.trycua.driver.",
        "data": {"bundle_identifier": "com.trycua.driver", "executable_path": driver, "identity_source": "current_process"}
    }));
    sb.set_health_report(&report);
    let out = sb.run_with_display(&["cua", "status"]);
    let stdout = text(&out.stdout);
    assert!(out.status.success(), "stdout: {stdout}");
    assert!(
        stdout.contains(&format!("answered by: {driver}")),
        "{stdout}"
    );
    assert!(!stdout.contains("not the driver above"), "{stdout}");

    // Another CuaDriver.app owns the login session's daemon: the health is
    // that daemon's, and status says so and how to get the installed one.
    let foreign = "/Applications/CuaDriver.app/Contents/MacOS/cua-driver";
    let mut report = health_report(PINNED_VERSION);
    report["checks"].as_array_mut().unwrap().push(serde_json::json!({
        "name": "bundle_identity", "status": "pass", "message": "Bundle is com.trycua.driver.",
        "data": {"bundle_identifier": "com.trycua.driver", "executable_path": foreign, "identity_source": "current_process"}
    }));
    sb.set_health_report(&report);
    let out = sb.run_with_display(&["cua", "status"]);
    let stdout = text(&out.stdout);
    assert!(
        out.status.success(),
        "a foreign daemon of the pinned version is a warning, not a failure: {stdout}"
    );
    assert!(
        stdout.contains(&format!("answered by: {foreign}")),
        "{stdout}"
    );
    assert!(stdout.contains("not the driver above"), "{stdout}");
    assert!(
        stdout.contains(&format!("{foreign} stop")),
        "says how to stop it: {stdout}"
    );
    assert!(
        stdout.contains("nolune cua status"),
        "and how to start the installed one: {stdout}"
    );
}

#[test]
fn status_reports_a_driver_named_by_the_environment() {
    let sb = Sandbox::new();
    let named = sb.home_dir.join("my-cua-driver");
    fs::write(&named, fake_driver_script(PINNED_VERSION, &sb.report_file)).unwrap();
    fs::set_permissions(&named, fs::Permissions::from_mode(0o755)).unwrap();

    let out = sb
        .command(&["cua", "status"])
        .env("NOLUNE_CUA_DRIVER", &named)
        .output()
        .unwrap();
    let stdout = text(&out.stdout);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(stdout.contains("installed: none"), "{stdout}");
    assert!(
        stdout.contains(&format!("driver: {} (NOLUNE_CUA_DRIVER)", named.display())),
        "{stdout}"
    );
}
