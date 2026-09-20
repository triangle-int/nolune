//! `nolune gateway install|uninstall|status` (#125), `nolune uninstall` (#126), and their
//! `--profile` forms (#107) through the real binary. launchctl / systemctl are replaced by
//! mock executables on PATH that record their arguments, so nothing touches the developer's
//! real services.
#![cfg(unix)]

use std::{
    fs,
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const BIN: &str = env!("CARGO_BIN_EXE_nolune");

struct Sandbox {
    _tmp: tempfile::TempDir,
    home_dir: PathBuf,
    nolune_home: PathBuf,
    mock_bin: PathBuf,
    calls: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let home_dir = tmp.path().join("home");
        let nolune_home = home_dir.join(".nolune");
        let mock_bin = tmp.path().join("mock-bin");
        let calls = tmp.path().join("calls");
        fs::create_dir_all(&mock_bin).unwrap();
        fs::write(&calls, "").unwrap();
        for tool in ["launchctl", "systemctl", "journalctl"] {
            let script = format!(
                "#!/bin/bash\nprintf '{tool} %s\\n' \"$*\" >> \"$MOCK_CALLS\"\ncase \"$*\" in\n  'print '*) exit 1 ;;\n  '--user is-active '*) echo inactive; exit 3 ;;\nesac\nexit 0\n"
            );
            let path = mock_bin.join(tool);
            fs::write(&path, script).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let sandbox = Self {
            _tmp: tmp,
            home_dir,
            nolune_home,
            mock_bin,
            calls,
        };
        assert!(sandbox.run(&["onboard", "--json"]).status.success());
        sandbox
    }

    /// The binary with the mocked PATH and sandboxed HOME; no workspace chosen yet.
    fn command(&self, args: &[&str]) -> Command {
        let path = format!(
            "{}:{}",
            self.mock_bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut cmd = Command::new(BIN);
        cmd.args(args)
            .env("HOME", &self.home_dir)
            .env_remove("NOLUNE_HOME")
            .env("MOCK_CALLS", &self.calls)
            .env("PATH", path)
            .env_remove("PORT")
            .stdin(std::process::Stdio::null());
        cmd
    }

    /// The default profile, addressed the way the installers do: through NOLUNE_HOME.
    fn run(&self, args: &[&str]) -> Output {
        self.command(args)
            .env("NOLUNE_HOME", &self.nolune_home)
            .output()
            .unwrap()
    }

    /// A named profile: `--profile <name>` alone, its root derived from HOME (#107).
    fn run_profile(&self, profile: &str, args: &[&str]) -> Output {
        self.command(args)
            .args(["--profile", profile])
            .output()
            .unwrap()
    }

    fn onboard_profile(&self, profile: &str) -> serde_json::Value {
        let out = self.run_profile(profile, &["onboard", "--json"]);
        assert!(
            out.status.success(),
            "onboard --profile {profile} failed: {}",
            text(&out.stderr)
        );
        let stdout = text(&out.stdout);
        let last = stdout.lines().last().expect("onboard printed nothing");
        serde_json::from_str(last).unwrap_or_else(|e| panic!("not JSON ({e}): {last:?}"))
    }

    fn profile_home(&self, profile: &str) -> PathBuf {
        self.home_dir.join(".nolune-profiles").join(profile)
    }

    fn profile_definition(&self, profile: &str) -> PathBuf {
        if cfg!(target_os = "macos") {
            self.home_dir
                .join("Library/LaunchAgents")
                .join(format!("dev.nolune.nolune.{profile}.plist"))
        } else {
            self.home_dir
                .join(".config/systemd/user")
                .join(format!("nolune-{profile}.service"))
        }
    }

    /// The name launchd or systemd knows a profile's service by.
    fn service_id(profile: &str) -> String {
        if cfg!(target_os = "macos") {
            format!("dev.nolune.nolune.{profile}")
        } else {
            format!("nolune-{profile}")
        }
    }

    fn calls(&self) -> String {
        fs::read_to_string(&self.calls).unwrap()
    }

    fn definition(&self) -> PathBuf {
        if cfg!(target_os = "macos") {
            self.home_dir
                .join("Library/LaunchAgents/dev.nolune.nolune.plist")
        } else {
            self.home_dir.join(".config/systemd/user/nolune.service")
        }
    }

    fn set_port(&self, port: u16) {
        set_port_in(&self.nolune_home, port);
    }
}

fn set_port_in(home: &Path, port: u16) {
    let config = home.join("config.toml");
    let raw = fs::read_to_string(&config).unwrap();
    let updated: Vec<String> = raw
        .lines()
        .map(|l| {
            if l.starts_with("port =") {
                format!("port = {port}")
            } else {
                l.to_string()
            }
        })
        .collect();
    fs::write(&config, updated.join("\n") + "\n").unwrap();
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn seed_install(home: &Path) {
    fs::create_dir_all(home.join("bin")).unwrap();
    fs::write(home.join("bin/nolune"), "binary").unwrap();
    fs::write(home.join("bin/update"), "#!/bin/bash").unwrap();
    fs::write(home.join("nolune.log"), "log").unwrap();
}

#[test]
fn gateway_install_writes_definition_and_starts_service() {
    let sb = Sandbox::new();

    let out = sb.run(&["gateway", "install"]);

    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let definition = fs::read_to_string(sb.definition()).unwrap();
    assert!(
        definition.contains(BIN),
        "definition should run this binary:\n{definition}"
    );
    assert!(
        definition.contains("gateway"),
        "definition should run `gateway`:\n{definition}"
    );
    assert!(definition.contains(sb.nolune_home.to_str().unwrap()));
    let calls = sb.calls();
    if cfg!(target_os = "macos") {
        assert!(calls.contains("launchctl bootstrap gui/"), "{calls}");
        assert!(calls.contains("launchctl kickstart -k gui/"), "{calls}");
        assert!(calls.contains("dev.nolune.nolune"), "{calls}");
    } else {
        assert!(calls.contains("systemctl --user daemon-reload"), "{calls}");
        assert!(
            calls.contains("systemctl --user enable --now nolune"),
            "{calls}"
        );
    }
    let stdout = text(&out.stdout);
    assert!(
        stdout.contains("gateway logs"),
        "should tell the user how to see logs:\n{stdout}"
    );
}

#[test]
fn gateway_install_refuses_when_a_foreground_gateway_holds_the_port() {
    let sb = Sandbox::new();
    let blocker = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = blocker.local_addr().unwrap().port();
    sb.set_port(port);

    let out = sb.run(&["gateway", "install"]);

    assert!(!out.status.success());
    let stderr = text(&out.stderr);
    assert!(stderr.contains(&port.to_string()), "{stderr}");
    assert!(stderr.contains("already"), "{stderr}");
    assert!(
        !sb.definition().exists(),
        "must not write a definition it cannot start"
    );
    assert!(!sb.calls().contains("bootstrap") && !sb.calls().contains("enable"));
    drop(blocker);
}

#[test]
fn gateway_uninstall_removes_definition_and_status_reports_not_installed() {
    let sb = Sandbox::new();
    assert!(sb.run(&["gateway", "install"]).status.success());
    assert!(sb.definition().exists());

    let out = sb.run(&["gateway", "uninstall"]);

    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(!sb.definition().exists());
    let calls = sb.calls();
    if cfg!(target_os = "macos") {
        assert!(calls.contains("launchctl bootout gui/"), "{calls}");
    } else {
        assert!(
            calls.contains("systemctl --user disable --now nolune"),
            "{calls}"
        );
    }

    let status = sb.run(&["gateway", "status"]);
    assert!(status.status.success());
    assert!(
        text(&status.stdout).contains("not installed"),
        "{}",
        text(&status.stdout)
    );

    // A second uninstall is a no-op, not an error.
    assert!(sb.run(&["gateway", "uninstall"]).status.success());
}

#[test]
fn top_level_service_verbs_still_work_as_hidden_aliases() {
    let sb = Sandbox::new();

    let status = sb.run(&["status"]);
    assert!(status.status.success());
    assert!(text(&status.stdout).contains("not installed"));

    let help = text(&Command::new(BIN).arg("--help").output().unwrap().stdout);
    assert!(help.contains("gateway"));
    assert!(
        !help.contains("\n  start "),
        "top-level start should be hidden:\n{help}"
    );
}

#[test]
fn uninstall_keep_data_removes_bin_and_service_but_keeps_workspace() {
    let sb = Sandbox::new();
    seed_install(&sb.nolune_home);
    assert!(sb.run(&["gateway", "install"]).status.success());

    let out = sb.run(&["uninstall", "--keep-data"]);

    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(
        !sb.definition().exists(),
        "service definition should be removed"
    );
    assert!(!sb.nolune_home.join("bin").exists());
    assert!(!sb.nolune_home.join("nolune.log").exists());
    assert!(sb.nolune_home.join("config.toml").exists());
    assert!(sb.nolune_home.join("instances").exists());
    let stdout = text(&out.stdout);
    assert!(stdout.contains("kept"), "{stdout}");
}

#[test]
fn uninstall_yes_removes_everything() {
    let sb = Sandbox::new();
    seed_install(&sb.nolune_home);

    let out = sb.run(&["uninstall", "--yes"]);

    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(!sb.nolune_home.exists());
    assert!(!sb.definition().exists());
}

#[test]
fn uninstall_refuses_to_delete_data_without_yes_when_not_interactive() {
    let sb = Sandbox::new();
    seed_install(&sb.nolune_home);

    let out = sb.run(&["uninstall"]);

    assert!(!out.status.success());
    assert!(text(&out.stderr).contains("--yes"));
    assert!(sb.nolune_home.join("config.toml").exists());
    assert!(
        sb.nolune_home.join("bin").exists(),
        "must remove nothing when refusing"
    );
}

// ── Profiles (#107) ─────────────────────────────────────────────────────

#[test]
fn onboard_profile_uses_a_sibling_root_and_a_free_port() {
    let sb = Sandbox::new();

    let report = sb.onboard_profile("molinka");

    let dir = PathBuf::from(report["dir"].as_str().unwrap());
    assert_eq!(dir, sb.profile_home("molinka"));
    assert!(
        !dir.starts_with(&sb.nolune_home),
        "profile roots must not nest under ~/.nolune: {}",
        dir.display()
    );
    assert_eq!(report["profile"], "molinka");
    let port = report["port"].as_u64().unwrap();
    assert_ne!(
        port, 26559,
        "a named profile must not take the default port"
    );
    assert_eq!(report["url"], format!("http://localhost:{port}"));
    assert!(dir.join("config.toml").exists());
    assert!(
        fs::read_to_string(dir.join("config.toml"))
            .unwrap()
            .contains(&format!("port = {port}"))
    );
    assert_ne!(
        report["token"],
        sb.onboard_profile("default")["token"],
        "each profile gets its own auth token"
    );

    // Its own token and port are separate from the default workspace, which is unchanged.
    let default_config = fs::read_to_string(sb.nolune_home.join("config.toml")).unwrap();
    assert!(default_config.contains("port = 26559"), "{default_config}");

    // A second profile avoids the first one's port.
    let second = sb.onboard_profile("yuki");
    assert_ne!(second["port"], report["port"]);
    assert_eq!(second["dir"], sb.profile_home("yuki").to_str().unwrap());

    // Rerunning keeps the port, and the human output names the profile.
    let again = sb.onboard_profile("molinka");
    assert_eq!(again["port"], report["port"]);
    let human = sb.run_profile("molinka", &["onboard"]);
    assert!(human.status.success());
    let stdout = text(&human.stdout);
    assert!(
        stdout.contains("--profile molinka"),
        "should tell the user how to start this profile:\n{stdout}"
    );
}

#[test]
fn onboard_accepts_an_explicit_port() {
    let sb = Sandbox::new();

    let report = sb.onboard_profile("molinka");
    let chosen = report["port"].as_u64().unwrap();
    let out = sb.run_profile("molinka", &["onboard", "--json", "--port", "4123"]);

    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let stdout = text(&out.stdout);
    let report: serde_json::Value = serde_json::from_str(stdout.lines().last().unwrap()).unwrap();
    assert_eq!(report["port"], 4123);
    assert_ne!(chosen, 4123);
    assert!(
        fs::read_to_string(sb.profile_home("molinka").join("config.toml"))
            .unwrap()
            .contains("port = 4123")
    );
}

#[test]
fn profile_names_are_validated_and_a_foreign_nolune_home_is_refused() {
    let sb = Sandbox::new();

    let long = "a".repeat(33);
    for bad in ["Molinka", "a_b", "-x", "a/b", long.as_str()] {
        let out = sb.run_profile(bad, &["onboard"]);
        assert!(!out.status.success(), "{bad:?} should be rejected");
        assert!(text(&out.stderr).contains(bad), "{}", text(&out.stderr));
    }
    assert!(
        !sb.home_dir.join(".nolune-profiles").exists(),
        "a rejected name must not create anything"
    );

    // `--profile` together with a NOLUNE_HOME that points somewhere else is ambiguous.
    let out = sb
        .command(&["onboard", "--profile", "molinka"])
        .env("NOLUNE_HOME", &sb.nolune_home)
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = text(&out.stderr);
    assert!(stderr.contains("NOLUNE_HOME"), "{stderr}");
    assert!(stderr.contains("molinka"), "{stderr}");
    assert!(!sb.profile_home("molinka").exists());

    // `--profile default` is the plain workspace, and agreeing values are fine.
    let report = sb.onboard_profile("default");
    assert_eq!(report["dir"], sb.nolune_home.to_str().unwrap());
    let out = sb
        .command(&["onboard", "--json", "--profile", "molinka"])
        .env("NOLUNE_HOME", sb.profile_home("molinka"))
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
}

#[test]
fn two_profiles_install_separate_services_and_uninstall_independently() {
    let sb = Sandbox::new();
    sb.onboard_profile("molinka");
    sb.onboard_profile("yuki");

    let out = sb.run_profile("molinka", &["gateway", "install"]);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let out = sb.run_profile("yuki", &["gateway", "install"]);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));

    let molinka = fs::read_to_string(sb.profile_definition("molinka")).unwrap();
    let yuki = fs::read_to_string(sb.profile_definition("yuki")).unwrap();
    assert!(molinka.contains(sb.profile_home("molinka").to_str().unwrap()));
    assert!(yuki.contains(sb.profile_home("yuki").to_str().unwrap()));
    assert!(!molinka.contains("yuki") && !yuki.contains("molinka"));
    for (definition, name) in [(&molinka, "molinka"), (&yuki, "yuki")] {
        assert!(definition.contains(BIN), "{definition}");
        assert!(
            definition.contains("--profile"),
            "{name}: the service must run its own profile:\n{definition}"
        );
        assert!(
            definition.contains(&Sandbox::service_id(name)),
            "{definition}"
        );
    }
    assert!(
        !sb.definition().exists(),
        "installing a named profile must not touch the default service"
    );
    let calls = sb.calls();
    assert!(calls.contains(&Sandbox::service_id("molinka")), "{calls}");
    assert!(calls.contains(&Sandbox::service_id("yuki")), "{calls}");

    // Status addresses one profile.
    let status = sb.run_profile("yuki", &["gateway", "status"]);
    assert!(status.status.success());
    let stdout = text(&status.stdout);
    assert!(stdout.contains("yuki"), "{stdout}");
    assert!(!stdout.contains("molinka"), "{stdout}");
    assert!(!stdout.contains("not installed"), "{stdout}");
    let default_status = text(&sb.run(&["gateway", "status"]).stdout);
    assert!(default_status.contains("not installed"), "{default_status}");

    // Removing one service leaves the other, and both data roots, alone.
    let out = sb.run_profile("molinka", &["gateway", "uninstall"]);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(!sb.profile_definition("molinka").exists());
    assert!(sb.profile_definition("yuki").exists());
    assert!(sb.profile_home("molinka").join("config.toml").exists());
    let calls = sb.calls();
    let stop_calls: Vec<&str> = calls
        .lines()
        .filter(|line| line.contains("bootout") || line.contains("disable"))
        .collect();
    assert!(!stop_calls.is_empty(), "{calls}");
    assert!(
        stop_calls
            .iter()
            .all(|line| line.contains(&Sandbox::service_id("molinka")) && !line.contains("yuki")),
        "{stop_calls:?}"
    );
    let stdout = text(&out.stdout);
    assert!(stdout.contains("molinka"), "{stdout}");
    let status = text(&sb.run_profile("molinka", &["gateway", "status"]).stdout);
    assert!(status.contains("not installed"), "{status}");
}

#[test]
fn gateway_install_refuses_a_port_shared_with_a_sibling_profile() {
    let sb = Sandbox::new();
    let molinka = sb.onboard_profile("molinka");
    sb.onboard_profile("yuki");
    let port = u16::try_from(molinka["port"].as_u64().unwrap()).unwrap();
    set_port_in(&sb.profile_home("yuki"), port);

    let out = sb.run_profile("yuki", &["gateway", "install"]);

    assert!(!out.status.success());
    let stderr = text(&out.stderr);
    assert!(stderr.contains("molinka"), "{stderr}");
    assert!(stderr.contains("yuki"), "{stderr}");
    assert!(stderr.contains(&port.to_string()), "{stderr}");
    assert!(
        !sb.profile_definition("yuki").exists(),
        "must not write a definition that collides"
    );
    assert!(!sb.calls().contains("bootstrap") && !sb.calls().contains("enable"));

    // The default profile's port is protected the same way.
    set_port_in(&sb.profile_home("yuki"), 26559);
    let out = sb.run_profile("yuki", &["gateway", "install"]);
    assert!(!out.status.success());
    let stderr = text(&out.stderr);
    assert!(
        stderr.contains("default") && stderr.contains("yuki"),
        "{stderr}"
    );
    assert!(!sb.profile_definition("yuki").exists());
}

#[test]
fn gateway_install_refuses_a_data_root_that_belongs_to_another_profile() {
    let sb = Sandbox::new();
    sb.onboard_profile("molinka");

    // NOLUNE_HOME inside a sibling's root addresses the default profile but molinka's data.
    let out = sb
        .command(&["gateway", "install"])
        .env("NOLUNE_HOME", sb.profile_home("molinka"))
        .output()
        .unwrap();

    assert!(!out.status.success());
    let stderr = text(&out.stderr);
    assert!(stderr.contains("molinka"), "{stderr}");
    assert!(stderr.contains("default"), "{stderr}");
    assert!(!sb.definition().exists());
    assert!(!sb.profile_definition("molinka").exists());
    assert!(!sb.calls().contains("bootstrap") && !sb.calls().contains("enable"));
}

#[test]
fn uninstall_profile_removes_only_that_profile() {
    let sb = Sandbox::new();
    sb.onboard_profile("molinka");
    sb.onboard_profile("yuki");
    seed_install(&sb.profile_home("molinka"));
    assert!(
        sb.run_profile("molinka", &["gateway", "install"])
            .status
            .success()
    );

    let out = sb.run_profile("molinka", &["uninstall", "--yes"]);

    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(!sb.profile_home("molinka").exists());
    assert!(!sb.profile_definition("molinka").exists());
    assert!(sb.profile_home("yuki").join("config.toml").exists());
    assert!(sb.nolune_home.join("config.toml").exists());
    let stdout = text(&out.stdout);
    assert!(
        stdout.contains(sb.profile_home("molinka").to_str().unwrap()),
        "{stdout}"
    );

    // Removing the default workspace leaves the remaining profile alone.
    let out = sb.run(&["uninstall", "--yes"]);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(!sb.nolune_home.exists());
    assert!(sb.profile_home("yuki").join("config.toml").exists());

    // And --keep-data on a profile keeps its workspace.
    seed_install(&sb.profile_home("yuki"));
    let out = sb.run_profile("yuki", &["uninstall", "--keep-data"]);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    assert!(!sb.profile_home("yuki").join("bin").exists());
    assert!(sb.profile_home("yuki").join("config.toml").exists());
}
