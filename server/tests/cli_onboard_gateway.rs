//! `nolune onboard` (#123), `nolune gateway` (#124), and two `--profile` gateways side by
//! side (#107), driven through the real binary.

use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const BIN: &str = env!("CARGO_BIN_EXE_nolune");

fn nolune(home: &Path) -> Command {
    let mut cmd = Command::new(BIN);
    cmd.env("NOLUNE_HOME", home)
        .env_remove("PORT")
        .env_remove("NOLUNE_AUTH_TOKEN")
        .env("RUST_LOG", "warn");
    cmd
}

fn onboard(home: &Path) -> serde_json::Value {
    let out = nolune(home).args(["onboard", "--json"]).output().unwrap();
    assert!(
        out.status.success(),
        "onboard failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    let last = stdout.lines().last().expect("onboard printed nothing");
    serde_json::from_str(last).unwrap_or_else(|e| panic!("last line is not JSON ({e}): {last:?}"))
}

fn wait_exit(child: &mut Child, limit: Duration) -> Option<std::process::ExitStatus> {
    let start = Instant::now();
    while start.elapsed() < limit {
        if let Some(status) = child.try_wait().unwrap() {
            return Some(status);
        }
        thread::sleep(Duration::from_millis(100));
    }
    None
}

#[test]
fn help_lists_onboard_and_gateway() {
    let out = Command::new(BIN).arg("--help").output().unwrap();
    let help = String::from_utf8(out.stdout).unwrap();
    assert!(help.contains("onboard"), "{help}");
    assert!(help.contains("gateway"), "{help}");
    assert!(help.contains("--profile"), "{help}");

    let out = Command::new(BIN)
        .args(["gateway", "--help"])
        .output()
        .unwrap();
    let help = String::from_utf8(out.stdout).unwrap();
    assert!(
        help.contains("\n  run "),
        "gateway run should be listed:\n{help}"
    );
    assert!(help.contains("--profile"), "{help}");
}

/// Spawn a gateway on `port` and block until it prints its ready line. Panics with the
/// captured stderr if it never does.
fn spawn_ready(mut cmd: Command, port: u16) -> (Child, thread::JoinHandle<String>) {
    let mut child = cmd
        .env("PORT", port.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let (tx, rx) = mpsc::channel::<String>();
    let stdout = child.stdout.take().unwrap();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let stderr = child.stderr.take().unwrap();
    let stderr_lines = thread::spawn(move || {
        let mut buf = String::new();
        BufReader::new(stderr).read_to_string(&mut buf).ok();
        buf
    });

    let expected = format!("nolune: ready http://localhost:{port}");
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(remaining) {
            Ok(line) if line == expected => break,
            Ok(_) => continue,
            Err(_) => {
                child.kill().ok();
                panic!(
                    "no ready line within timeout; stderr:\n{}",
                    stderr_lines.join().unwrap()
                );
            }
        }
    }
    (child, stderr_lines)
}

/// One raw HTTP/1.1 GET; returns the whole response text.
fn http_get(port: u16, path: &str, bearer: Option<&str>) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let auth = bearer
        .map(|token| format!("Authorization: Bearer {token}\r\n"))
        .unwrap_or_default();
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n{auth}Connection: close\r\n\r\n")
                .as_bytes(),
        )
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

#[cfg(unix)]
fn terminate(child: &mut Child) -> std::process::ExitStatus {
    let killed = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    wait_exit(child, Duration::from_secs(15)).unwrap_or_else(|| {
        child.kill().ok();
        panic!("gateway did not exit within 15s of SIGTERM");
    })
}

#[test]
fn onboard_json_last_line_is_parseable_and_human_output_hides_token() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");

    let report = onboard(&home);
    assert_eq!(report["url"], "http://localhost:26559");
    assert_eq!(report["dir"], home.to_str().unwrap());
    let token = report["token"].as_str().unwrap();
    assert_eq!(token.len(), 32);
    assert!(
        fs::read_to_string(home.join("config.toml"))
            .unwrap()
            .contains(token)
    );

    let out = nolune(&home).arg("onboard").output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        !stdout.contains(token),
        "human output printed the token:\n{stdout}"
    );
    assert!(
        stdout.contains("nolune gateway"),
        "should tell the user how to start:\n{stdout}"
    );
}

#[cfg(unix)]
#[test]
fn gateway_prints_ready_serves_healthz_and_stops_on_sigterm() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    onboard(&home);
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();

    let mut cmd = nolune(&home);
    cmd.arg("gateway");
    let (mut child, _stderr) = spawn_ready(cmd, port);

    let response = http_get(port, "/healthz", None);
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.contains("\"status\":\"ok\""), "{response}");

    let status = terminate(&mut child);
    assert!(status.success(), "gateway exited with {status}");
}

/// Two profiles on one host (#107): separate roots, ports, and tokens, started with
/// `gateway run --profile <name>` from one binary at the same time.
#[cfg(unix)]
#[test]
fn two_profiles_run_concurrently_with_separate_roots_ports_and_tokens() {
    let tmp = tempfile::tempdir().unwrap();
    let home_dir = tmp.path().join("home");
    fs::create_dir_all(&home_dir).unwrap();
    let profile_cmd = |profile: &str| {
        let mut cmd = Command::new(BIN);
        cmd.env("HOME", &home_dir)
            .env_remove("NOLUNE_HOME")
            .env_remove("PORT")
            .env_remove("NOLUNE_AUTH_TOKEN")
            .env("RUST_LOG", "warn")
            .args(["--profile", profile]);
        cmd
    };
    let onboard_profile = |profile: &str| -> serde_json::Value {
        let out = profile_cmd(profile)
            .args(["onboard", "--json"])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "onboard --profile {profile} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8(out.stdout).unwrap();
        serde_json::from_str(stdout.lines().last().unwrap()).unwrap()
    };

    let molinka = onboard_profile("molinka");
    let yuki = onboard_profile("yuki");
    let molinka_root = Path::new(molinka["dir"].as_str().unwrap()).to_path_buf();
    let yuki_root = Path::new(yuki["dir"].as_str().unwrap()).to_path_buf();
    assert_ne!(molinka_root, yuki_root);
    assert!(molinka_root.starts_with(home_dir.join(".nolune-profiles")));
    assert!(yuki_root.starts_with(home_dir.join(".nolune-profiles")));
    assert_ne!(molinka["port"], yuki["port"]);
    assert_ne!(molinka["token"], yuki["token"]);

    // Ephemeral ports keep this test independent of what else listens on the machine.
    let free_port = || {
        TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    };
    let (molinka_port, yuki_port) = (free_port(), free_port());
    assert_ne!(molinka_port, yuki_port);

    let mut molinka_cmd = profile_cmd("molinka");
    molinka_cmd.args(["gateway", "run"]);
    let mut yuki_cmd = profile_cmd("yuki");
    yuki_cmd.args(["gateway", "run"]);
    let (mut molinka_gateway, _) = spawn_ready(molinka_cmd, molinka_port);
    let (mut yuki_gateway, _) = spawn_ready(yuki_cmd, yuki_port);

    for port in [molinka_port, yuki_port] {
        let response = http_get(port, "/healthz", None);
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    }

    // Each token opens only its own profile; the sibling is a stranger.
    let molinka_token = molinka["token"].as_str().unwrap();
    let yuki_token = yuki["token"].as_str().unwrap();
    let own = http_get(molinka_port, "/api/companion", Some(molinka_token));
    assert!(own.starts_with("HTTP/1.1 200"), "{own}");
    let crossed = http_get(yuki_port, "/api/companion", Some(molinka_token));
    assert!(crossed.starts_with("HTTP/1.1 401"), "{crossed}");
    let crossed = http_get(molinka_port, "/api/companion", Some(yuki_token));
    assert!(crossed.starts_with("HTTP/1.1 401"), "{crossed}");

    // Each server wrote only into its own root.
    assert!(molinka_root.join("instances").is_dir());
    assert!(yuki_root.join("instances").is_dir());
    assert!(
        !home_dir.join(".nolune").exists(),
        "a named profile must never touch the default root"
    );

    // Stopping one leaves the other serving.
    let status = terminate(&mut molinka_gateway);
    assert!(status.success(), "molinka exited with {status}");
    let response = http_get(yuki_port, "/healthz", None);
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    let status = terminate(&mut yuki_gateway);
    assert!(status.success(), "yuki exited with {status}");
}

#[test]
fn gateway_fails_fast_when_port_is_taken() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    onboard(&home);
    let blocker = TcpListener::bind("0.0.0.0:0").unwrap();
    let port = blocker.local_addr().unwrap().port();

    let mut child = nolune(&home)
        .arg("gateway")
        .env("PORT", port.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let status = wait_exit(&mut child, Duration::from_secs(120)).unwrap_or_else(|| {
        child.kill().ok();
        panic!("gateway kept running on a taken port");
    });
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();

    assert!(!status.success());
    assert!(stderr.contains("already in use"), "{stderr}");
    assert!(stderr.contains(&port.to_string()), "{stderr}");
    drop(blocker);
}
