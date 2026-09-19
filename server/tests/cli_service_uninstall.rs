//! `nolune gateway install|uninstall|status` (#125) and `nolune uninstall` (#126) through the
//! real binary. launchctl / systemctl are replaced by mock executables on PATH that record
//! their arguments, so nothing touches the developer's real services.
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

    fn run(&self, args: &[&str]) -> Output {
        let path = format!(
            "{}:{}",
            self.mock_bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        Command::new(BIN)
            .args(args)
            .env("HOME", &self.home_dir)
            .env("NOLUNE_HOME", &self.nolune_home)
            .env("MOCK_CALLS", &self.calls)
            .env("PATH", path)
            .env_remove("PORT")
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap()
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
        let config = self.nolune_home.join("config.toml");
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
