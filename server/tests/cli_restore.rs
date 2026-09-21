//! `nolune restore <archive>` (#74): an operator's explicit local path is posted to the
//! running server as a multipart upload with the bearer token, so the CLI never extracts
//! anything itself and the server's validating restore is the only path into the
//! companion. Driven through the real binary.

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
        .env("RUST_LOG", "warn")
        .stdin(Stdio::null());
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
    let report: serde_json::Value = serde_json::from_str(last)
        .unwrap_or_else(|e| panic!("last line is not JSON ({e}): {last:?}"));
    // Keep the gateway from starting a real Cua driver on a developer Mac (#16).
    let config = home.join("config.toml");
    let mut raw = fs::read_to_string(&config).unwrap();
    assert!(!raw.contains("[cua]"), "{raw}");
    raw.push_str("\n[cua]\nenabled = false\n");
    fs::write(&config, raw).unwrap();
    report
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// A gzip tar of `entries` (path, bytes) written by the system tar, the way an
/// operator would re-pack a backup by hand; directories are created as needed.
fn tar_archive(dir: &Path, entries: &[(&str, &str)]) -> std::path::PathBuf {
    let root = dir.join("source");
    for (path, contents) in entries {
        let full = root.join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, contents).unwrap();
    }
    let archive = dir.join("backup.tar.gz");
    let status = Command::new("tar")
        // macOS bsdtar would otherwise add `._*` AppleDouble entries.
        .env("COPYFILE_DISABLE", "1")
        .arg("-czf")
        .arg(&archive)
        .arg("-C")
        .arg(&root)
        .arg("companion")
        .status()
        .unwrap();
    assert!(status.success());
    archive
}

fn valid_archive(dir: &Path) -> std::path::PathBuf {
    tar_archive(
        dir,
        &[
            (
                "companion/companion.json",
                r#"{"format_version":1,"slug":"companion"}"#,
            ),
            ("companion/soul.md", "restored from the archive"),
            ("companion/memory/notes/tea.md", "- likes oolong"),
        ],
    )
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

/// Spawn a gateway on `port` and block until it prints its ready line.
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
fn help_lists_restore_with_its_confirmation_flag() {
    let out = Command::new(BIN).arg("--help").output().unwrap();
    let help = String::from_utf8(out.stdout).unwrap();
    assert!(help.contains("restore"), "{help}");

    let out = Command::new(BIN)
        .args(["restore", "--help"])
        .output()
        .unwrap();
    let help = String::from_utf8(out.stdout).unwrap();
    assert!(help.contains("<ARCHIVE>"), "{help}");
    assert!(help.contains("--yes"), "{help}");
    assert!(help.contains("--profile"), "{help}");
    assert!(help.to_lowercase().contains("replace"), "{help}");
}

#[test]
fn restore_refuses_without_confirmation_off_a_terminal_and_without_the_archive() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    onboard(&home);
    let archive = valid_archive(tmp.path());

    // stdin is not a terminal here, so the question cannot be asked.
    let out = nolune(&home).arg("restore").arg(&archive).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--yes"), "{stderr}");
    assert!(stderr.contains("nothing was"), "{stderr}");

    let out = nolune(&home)
        .args(["restore", "--yes"])
        .arg(tmp.path().join("missing.tar.gz"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("missing.tar.gz"), "{stderr}");

    // No server is running: say so, and how to start it.
    let out = nolune(&home)
        .args(["restore", "--yes"])
        .arg(&archive)
        .env("PORT", free_port().to_string())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not reachable"), "{stderr}");
    assert!(stderr.contains("nolune gateway"), "{stderr}");
}

/// Something on the port answers 2xx without the import's own JSON (a
/// proxy's page, an empty body): that is not a restore, and the CLI must not
/// print one.
#[test]
fn restore_never_reports_success_for_a_2xx_that_is_not_the_imports_answer() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    onboard(&home);
    let archive = valid_archive(tmp.path());

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        // Read the whole request (headers, then Content-Length bytes) so the
        // upload completes before the reply.
        let mut request = Vec::new();
        let mut buf = [0u8; 4096];
        let header_end = loop {
            let n = stream.read(&mut buf).unwrap();
            assert!(n > 0, "request ended before its headers");
            request.extend_from_slice(&buf[..n]);
            if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                break end + 4;
            }
        };
        let headers = String::from_utf8_lossy(&request[..header_end]).to_string();
        let length: usize = headers
            .lines()
            .find_map(|line| line.strip_prefix("content-length: "))
            .or_else(|| {
                headers
                    .lines()
                    .find_map(|line| line.strip_prefix("Content-Length: "))
            })
            .expect("content-length")
            .trim()
            .parse()
            .unwrap();
        while request.len() < header_end + length {
            let n = stream.read(&mut buf).unwrap();
            assert!(n > 0, "request ended before its body");
            request.extend_from_slice(&buf[..n]);
        }
        let body = "<html>ok</html>";
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
        headers
    });

    let out = nolune(&home)
        .args(["restore", "--yes"])
        .arg(&archive)
        .env("PORT", port.to_string())
        .output()
        .unwrap();
    let headers = server.join().unwrap();
    assert!(
        headers.starts_with("POST /api/instances/companion/import "),
        "{headers}"
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(1),
        "stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(!stdout.contains("restored"), "{stdout}");
    assert!(stderr.contains("unexpected reply"), "{stderr}");
    assert!(stderr.contains("HTTP 200"), "{stderr}");
}

#[cfg(unix)]
#[test]
fn restore_posts_the_archive_to_the_running_server_which_serves_the_restored_companion() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let report = onboard(&home);
    let token = report["token"].as_str().unwrap().to_owned();
    let port = free_port();
    let mut cmd = nolune(&home);
    cmd.arg("gateway");
    let (mut gateway, _stderr) = spawn_ready(cmd, port);

    // A hostile archive (a symlink entry) is refused by the server and the CLI says so.
    let hostile_dir = tmp.path().join("hostile");
    fs::create_dir_all(hostile_dir.join("source/companion")).unwrap();
    std::os::unix::fs::symlink("/etc/passwd", hostile_dir.join("source/companion/soul.md"))
        .unwrap();
    let hostile = tar_archive(
        &hostile_dir,
        &[(
            "companion/companion.json",
            r#"{"format_version":1,"slug":"companion"}"#,
        )],
    );
    let out = nolune(&home)
        .args(["restore", "--yes"])
        .arg(&hostile)
        .env("PORT", port.to_string())
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("refused"), "{stderr}");
    assert!(!home.join("instances/companion/soul.md").exists());

    // A valid archive replaces the companion the server answers for.
    let archive = valid_archive(tmp.path());
    let out = nolune(&home)
        .args(["restore", "--yes"])
        .arg(&archive)
        .env("PORT", port.to_string())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stdout: {stdout}\nstderr: {stderr}");
    assert!(stdout.contains("restored"), "{stdout}");
    assert!(stdout.contains("3 files"), "{stdout}");
    assert!(
        stdout.contains("pending"),
        "no embedding provider is configured, so the index must be reported as pending: {stdout}"
    );

    let soul = http_get(port, "/api/instances/companion/soul", Some(&token));
    assert!(soul.starts_with("HTTP/1.1 200"), "{soul}");
    assert!(soul.contains("restored from the archive"), "{soul}");
    assert_eq!(
        fs::read_to_string(home.join("instances/companion/memory/notes/tea.md")).unwrap(),
        "- likes oolong"
    );
    let imports: Vec<_> = fs::read_dir(home.join("imports"))
        .map(|entries| entries.map(|e| e.unwrap().file_name()).collect())
        .unwrap_or_default();
    assert!(imports.is_empty(), "imports/ left behind: {imports:?}");

    let status = terminate(&mut gateway);
    assert!(status.success(), "gateway exited with {status}");
}
