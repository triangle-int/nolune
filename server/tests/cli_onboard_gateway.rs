//! `nolune onboard` (#123) and `nolune gateway` (#124) driven through the real binary.

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

    let mut child = nolune(&home)
        .arg("gateway")
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

    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.contains("\"status\":\"ok\""), "{response}");

    let killed = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    let status = wait_exit(&mut child, Duration::from_secs(15)).unwrap_or_else(|| {
        child.kill().ok();
        panic!("gateway did not exit within 15s of SIGTERM");
    });
    assert!(status.success(), "gateway exited with {status}");
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
