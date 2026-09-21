//! `nolune federation …` (#108, PR 4) driven through the real binary: two
//! profiles on one host pair through the wire only, the invite line is
//! printed once and never logged, `accept` takes it from an argument or
//! stdin and never from a URL, and `peers`, `confirm`, `revoke`, and
//! `rotate` drive the owner routes. Source guards keep the CLI, the
//! Companions settings section, and `docs/federation.md` honest.

#[path = "../test-support/source_scan.rs"]
mod source_scan;

use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use source_scan::without_cfg_test_items;

const BIN: &str = env!("CARGO_BIN_EXE_nolune");
const INVITE_PREFIX: &str = "nolune-invite-v1.";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn read(relative: &str) -> String {
    fs::read_to_string(repo().join(relative)).unwrap_or_else(|_| panic!("{relative} is missing"))
}

/// The binary addressing `profile` under a sandboxed `HOME`, with the same
/// port the profile's gateway listens on (so the CLI reaches it) and no
/// inherited overrides.
fn profile_cmd(home_dir: &Path, profile: &str, port: Option<u16>) -> Command {
    let mut cmd = Command::new(BIN);
    cmd.env("HOME", home_dir)
        .env_remove("NOLUNE_HOME")
        .env_remove("PORT")
        .env_remove("NOLUNE_AUTH_TOKEN")
        .env_remove("NOLUNE_PUBLIC_URL")
        .env("RUST_LOG", "info")
        .args(["--profile", profile]);
    if let Some(port) = port {
        cmd.env("PORT", port.to_string());
    }
    cmd
}

/// Run a CLI command to completion; returns (exit code, stdout, stderr).
fn run(cmd: &mut Command, stdin: Option<&str>) -> (i32, String, String) {
    cmd.stdin(if stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    if let Some(text) = stdin {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }
    let out = child.wait_with_output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Keep the gateways these tests spawn from driving this machine (#16), as
/// `cli_onboard_gateway.rs` does.
fn without_computer_use(dir: &Path) {
    let config = dir.join("config.toml");
    let mut raw = fs::read_to_string(&config).unwrap();
    assert!(
        !raw.contains("[cua]"),
        "onboard wrote a [cua] section:\n{raw}"
    );
    raw.push_str("\n[cua]\nenabled = false\n");
    fs::write(&config, raw).unwrap();
}

fn onboard_profile(home_dir: &Path, profile: &str) -> (PathBuf, String) {
    let (code, stdout, stderr) = run(
        profile_cmd(home_dir, profile, None).args(["onboard", "--json"]),
        None,
    );
    assert_eq!(code, 0, "onboard --profile {profile} failed: {stderr}");
    let report: serde_json::Value = serde_json::from_str(stdout.lines().last().unwrap()).unwrap();
    let root = PathBuf::from(report["dir"].as_str().unwrap());
    without_computer_use(&root);
    (root, report["token"].as_str().unwrap().to_owned())
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Spawn a gateway on `port` and block until it prints its ready line; the
/// returned handle collects everything it writes to stderr (its log).
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
    let log = thread::spawn(move || {
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
                    "no ready line within timeout; log:\n{}",
                    log.join().unwrap()
                );
            }
        }
    }
    (child, log)
}

#[cfg(unix)]
fn terminate(child: &mut Child) -> std::process::ExitStatus {
    let killed = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(15) {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        thread::sleep(Duration::from_millis(100));
    }
    child.kill().ok();
    panic!("gateway did not exit within 15s of SIGTERM");
}

/// One raw HTTP/1.1 request; returns the whole response text.
fn http(port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\n");
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str(&format!(
        "Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    ));
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

/// One HTTP/1.1 request read whole off `stream` (head, then the body its
/// Content-Length announces); returns the head. The caller decides whether
/// to answer.
fn read_request(stream: &mut TcpStream) -> String {
    let mut reader = BufReader::new(stream);
    let mut head = String::new();
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
            break;
        }
        if let Some(value) = line
            .split_once(':')
            .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            .map(|(_, value)| value.trim())
        {
            content_length = value.parse().unwrap();
        }
        head.push_str(&line);
    }
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body).unwrap();
    head
}

/// A well-formed invite line built in the test around `secret`, so a
/// run can prove the secret shows up nowhere.
fn invite_line_carrying(secret: &str) -> String {
    let doc = serde_json::json!({
        "origin": "https://a.test",
        "secret": secret,
        "issuer": serde_json::from_str::<serde_json::Value>(&read("server/tests/fixtures/federation/identity_v1.json")).unwrap(),
    });
    format!(
        "{INVITE_PREFIX}{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&doc).unwrap())
    )
}

/// The invite line `nolune federation invite` printed, and the secret inside
/// it (decoded here, in the test, to prove it never shows up anywhere else).
fn invite_line_and_secret(stdout: &str) -> (String, String) {
    let lines: Vec<&str> = stdout
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with(INVITE_PREFIX))
        .collect();
    assert_eq!(
        lines.len(),
        1,
        "the invite is printed exactly once:\n{stdout}"
    );
    let token = lines[0].to_owned();
    let payload = URL_SAFE_NO_PAD
        .decode(&token[INVITE_PREFIX.len()..])
        .expect("invite payload is base64url");
    let body: serde_json::Value = serde_json::from_slice(&payload).unwrap();
    (token, body["secret"].as_str().unwrap().to_owned())
}

#[test]
fn help_lists_the_federation_subcommands() {
    let out = Command::new(BIN).arg("--help").output().unwrap();
    let help = String::from_utf8(out.stdout).unwrap();
    assert!(help.contains("federation"), "{help}");

    let out = Command::new(BIN)
        .args(["federation", "--help"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let help = String::from_utf8(out.stdout).unwrap();
    for sub in ["invite", "accept", "peers", "confirm", "revoke", "rotate"] {
        assert!(
            help.contains(&format!("\n  {sub} ")),
            "{sub} missing:\n{help}"
        );
    }
    assert!(help.contains("--profile"), "{help}");

    let out = Command::new(BIN)
        .args(["federation", "accept", "--help"])
        .output()
        .unwrap();
    let help = String::from_utf8(out.stdout).unwrap();
    assert!(
        help.contains("stdin"),
        "accept must say it reads stdin:\n{help}"
    );
    assert!(
        !help.to_lowercase().contains("url"),
        "accept never offers a URL:\n{help}"
    );
}

/// Bad input is refused before the server is contacted: a URL is never an
/// invite, junk is not an invite, and an absent server is reported with the
/// command that starts it.
#[test]
fn accept_refuses_urls_and_junk_and_needs_a_running_server() {
    let tmp = tempfile::tempdir().unwrap();
    let home_dir = tmp.path().join("home");
    fs::create_dir_all(&home_dir).unwrap();
    let (root, _token) = onboard_profile(&home_dir, "solo");
    let port = free_port();
    let cmd = || profile_cmd(&home_dir, "solo", Some(port));

    let fake_secret = "not-a-real-secret-but-must-not-be-echoed";
    for url in [
        format!("http://localhost:{port}/federation/v1/pair?secret={fake_secret}"),
        format!("https://a.test/?invite={INVITE_PREFIX}abc"),
        "HTTPS://A.TEST".to_owned(),
    ] {
        let (code, stdout, stderr) = run(cmd().args(["federation", "accept", &url]), None);
        assert_eq!(code, 1, "{url}: {stdout}{stderr}");
        assert!(
            stderr.contains("URL") && stderr.contains("never"),
            "{url}: {stderr}"
        );
        assert!(
            !stderr.contains(fake_secret) && !stdout.contains(fake_secret),
            "echoed the query string: {stdout}{stderr}"
        );
        // Also on stdin.
        let (code, _, stderr) = run(cmd().args(["federation", "accept"]), Some(&url));
        assert_eq!(code, 1);
        assert!(stderr.contains("URL"), "{stderr}");
    }
    for junk in ["hello", "nolune-invite-v1.", "nolune-invite-v1.!!!", ""] {
        let (code, _, stderr) = run(cmd().args(["federation", "accept", junk]), None);
        assert_eq!(code, 1, "{junk:?}");
        assert!(stderr.contains("invite"), "{junk:?}: {stderr}");
        assert!(
            !stderr.contains("nolune gateway"),
            "{junk:?} reached for the server"
        );
    }
    let (code, _, stderr) = run(cmd().args(["federation", "accept"]), Some("   \n"));
    assert_eq!(code, 1);
    assert!(
        stderr.contains("stdin") || stderr.contains("empty"),
        "{stderr}"
    );

    // A well-formed line with no server running: the server is named, the
    // line is not echoed.
    let line = invite_line_carrying(fake_secret);
    let (code, stdout, stderr) = run(cmd().args(["federation", "accept", &line]), None);
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(stderr.contains("nolune gateway --profile solo"), "{stderr}");
    assert!(
        !stderr.contains(fake_secret) && !stderr.contains(&line) && !stdout.contains(fake_secret),
        "{stdout}{stderr}"
    );
    for args in [
        &["federation", "peers"][..],
        &["federation", "invite"][..],
        &["federation", "confirm", "someone"][..],
        &["federation", "revoke", "someone"][..],
        &["federation", "rotate", "--yes"][..],
    ] {
        let (code, _, stderr) = run(cmd().args(args), None);
        assert_eq!(code, 1, "{args:?}");
        assert!(
            stderr.contains("nolune gateway --profile solo"),
            "{args:?}: {stderr}"
        );
    }
    // Rotating is never done without asking outside a terminal.
    let (code, _, stderr) = run(cmd().args(["federation", "rotate"]), None);
    assert_eq!(code, 1);
    assert!(stderr.contains("--yes"), "{stderr}");
    assert!(
        !root.join("federation").exists(),
        "the CLI never touches the keystore itself"
    );
    assert!(
        !home_dir.join(".nolune").exists(),
        "a named profile never touches the default root"
    );
}

/// A gateway that took the request and hung up before answering is not
/// one that is not running: the work may stand there (a rotation's report
/// arrives only once every paired peer has been told, and the server's
/// transport gives each origin its own timeout). The CLI says so and points
/// at `peers`, never at `nolune gateway`, so an owner is not talked into
/// rotating or revoking a second time on the strength of a wrong message.
#[test]
fn a_taken_request_with_no_answer_is_not_reported_as_unreachable() {
    let tmp = tempfile::tempdir().unwrap();
    let home_dir = tmp.path().join("home");
    fs::create_dir_all(&home_dir).unwrap();
    onboard_profile(&home_dir, "quiet");
    // A stand-in for a busy gateway on the profile's port: it takes each
    // request whole and hangs up without a word.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            if tx.send(read_request(&mut stream)).is_err() {
                break;
            }
            // `stream` drops here: hung up, no answer.
        }
    });
    let cmd = || profile_cmd(&home_dir, "quiet", Some(port));
    let fake_secret = "quiet-secret-that-must-not-be-echoed";
    let line = invite_line_carrying(fake_secret);
    for (args, path) in [
        (
            &["federation", "rotate", "--yes"][..],
            "POST /api/federation/rotate ",
        ),
        (
            &["federation", "revoke", "someone"][..],
            "POST /api/federation/peers/someone/revoke ",
        ),
        (
            &["federation", "confirm", "someone"][..],
            "POST /api/federation/peers/someone/confirm ",
        ),
        (
            &["federation", "accept", &line][..],
            "POST /api/federation/accept ",
        ),
        (
            &["federation", "invite"][..],
            "POST /api/federation/invites ",
        ),
        (&["federation", "peers"][..], "GET /api/federation/peers "),
    ] {
        let (code, stdout, stderr) = run(cmd().args(args), None);
        let head = rx
            .recv_timeout(Duration::from_secs(30))
            .unwrap_or_else(|_| panic!("{args:?} never reached the server"));
        assert!(head.starts_with(path), "{args:?} sent:\n{head}");
        assert_eq!(code, 1, "{args:?}: {stdout}{stderr}");
        assert!(
            !stderr.contains("nolune gateway"),
            "{args:?} blamed a server it reached:\n{stderr}"
        );
        assert!(
            stderr.contains("federation peers --profile quiet"),
            "{args:?} must point at the listing:\n{stderr}"
        );
        assert!(
            stderr.contains("may") && stderr.to_lowercase().contains("answer"),
            "{args:?} must say the work may stand and no answer came:\n{stderr}"
        );
        assert!(
            !stdout.contains(fake_secret)
                && !stderr.contains(fake_secret)
                && !stderr.contains(&line),
            "{args:?}: {stdout}{stderr}"
        );
    }
}

/// `rotate` waits for the server's report however long telling the peers
/// takes: a gateway with two paired peers that accept and never answer
/// needs the transport timeout twice over before it can report, and the
/// CLI must not give up first and call a rotation that happened
/// "not reachable".
#[test]
fn rotate_outlasts_the_server_telling_silent_peers() {
    // Two peers that accept and never answer cost the server 15 s each,
    // one after another, before the report exists.
    const TWO_SILENT_PEERS: Duration = Duration::from_secs(32);
    let tmp = tempfile::tempdir().unwrap();
    let home_dir = tmp.path().join("home");
    fs::create_dir_all(&home_dir).unwrap();
    onboard_profile(&home_dir, "patient");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let report = serde_json::json!({
        "identity": { "companion_id": "NEWID" },
        "rotation": { "previous": { "companion_id": "OLDID" } },
        "notified": [],
        "unreachable": ["SILENT-ONE", "SILENT-TWO"],
    })
    .to_string();
    let (tx, rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        tx.send(read_request(&mut stream)).unwrap();
        thread::sleep(TWO_SILENT_PEERS);
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{report}",
            report.len()
        )
        .unwrap();
        stream.flush().unwrap();
    });
    let started = Instant::now();
    let (code, stdout, stderr) = run(
        profile_cmd(&home_dir, "patient", Some(port)).args(["federation", "rotate", "--yes"]),
        None,
    );
    let head = rx.recv_timeout(Duration::from_secs(30)).unwrap();
    assert!(head.starts_with("POST /api/federation/rotate "), "{head}");
    assert!(
        started.elapsed() >= TWO_SILENT_PEERS,
        "the CLI gave up after {:?}",
        started.elapsed()
    );
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(
        !stderr.contains("nolune gateway"),
        "a rotation that happened was called unreachable:\n{stderr}"
    );
    assert!(
        stdout.contains("OLDID") && stdout.contains("NEWID"),
        "{stdout}"
    );
    assert!(
        stdout.contains("unreachable: SILENT-ONE, SILENT-TWO"),
        "{stdout}"
    );
}

/// The whole owner flow between two profiles on one host, over real HTTP:
/// invite on molinka, accept on yuki from stdin, confirm, rotate, revoke.
/// The two servers share a machine, a user, a binary, and a loopback
/// address, and none of that admits either: only the invite does.
#[cfg(unix)]
#[test]
fn two_profiles_on_one_host_pair_through_the_wire_only() {
    let tmp = tempfile::tempdir().unwrap();
    let home_dir = tmp.path().join("home");
    fs::create_dir_all(&home_dir).unwrap();
    let (molinka_root, molinka_token) = onboard_profile(&home_dir, "molinka");
    let (yuki_root, yuki_token) = onboard_profile(&home_dir, "yuki");
    let (molinka_port, yuki_port) = (free_port(), free_port());
    assert_ne!(molinka_port, yuki_port);
    let molinka = || profile_cmd(&home_dir, "molinka", Some(molinka_port));
    let yuki = || profile_cmd(&home_dir, "yuki", Some(yuki_port));

    let mut molinka_cmd = molinka();
    molinka_cmd.args(["gateway", "run"]);
    let mut yuki_cmd = yuki();
    yuki_cmd.args(["gateway", "run"]);
    let (mut molinka_gateway, molinka_log) = spawn_ready(molinka_cmd, molinka_port);
    let (mut yuki_gateway, yuki_log) = spawn_ready(yuki_cmd, yuki_port);

    // Sharing a host is not knowing each other.
    let (code, stdout, stderr) = run(molinka().args(["federation", "peers", "--json"]), None);
    assert_eq!(code, 0, "{stderr}");
    let overview: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let molinka_id = overview["companion_id"].as_str().unwrap().to_owned();
    assert_eq!(overview["peers"].as_array().unwrap().len(), 0);
    assert_eq!(overview["invites"].as_array().unwrap().len(), 0);
    let (code, stdout, _) = run(yuki().args(["federation", "peers", "--json"]), None);
    assert_eq!(code, 0);
    let overview: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let yuki_id = overview["companion_id"].as_str().unwrap().to_owned();
    assert_ne!(molinka_id, yuki_id);
    let (code, stdout, _) = run(molinka().args(["federation", "peers"]), None);
    assert_eq!(code, 0);
    assert!(stdout.contains(&molinka_id), "{stdout}");
    assert!(stdout.to_lowercase().contains("no peers"), "{stdout}");

    // Invite on molinka: printed once, to stdout, never to stderr.
    let (code, stdout, stderr) = run(molinka().args(["federation", "invite"]), None);
    assert_eq!(code, 0, "{stderr}");
    let (token, secret) = invite_line_and_secret(&stdout);
    assert!(
        !stderr.contains(INVITE_PREFIX) && !stderr.contains(&secret),
        "{stderr}"
    );
    assert!(!token.contains("://"), "the invite is not a URL");
    assert!(
        stdout.contains("accept"),
        "should say what the other owner runs:\n{stdout}"
    );
    assert!(
        stdout.contains("--profile molinka") && stdout.contains("confirm"),
        "{stdout}"
    );
    // Its JSON form carries the same line and nothing looser.
    let (code, stdout, _) = run(molinka().args(["federation", "invite", "--json"]), None);
    assert_eq!(code, 0);
    let issued: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(
        issued["invite"]
            .as_str()
            .unwrap()
            .starts_with(INVITE_PREFIX)
    );
    assert!(
        issued.get("secret").is_none(),
        "the JSON line carries the invite line only: {issued}"
    );
    assert_eq!(issued["issuer_companion_id"], molinka_id);
    let second_token = issued["invite"].as_str().unwrap().to_owned();

    // The listing shows the invites without either line or secret.
    let (code, stdout, _) = run(molinka().args(["federation", "peers"]), None);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("2 outstanding") || stdout.matches("invite ").count() >= 2,
        "{stdout}"
    );
    assert!(
        !stdout.contains(&secret) && !stdout.contains(INVITE_PREFIX),
        "{stdout}"
    );
    let (code, stdout, _) = run(molinka().args(["federation", "peers", "--json"]), None);
    assert_eq!(code, 0);
    assert!(
        !stdout.contains(&secret) && !stdout.contains(&token) && !stdout.contains(&second_token)
    );

    // A URL carrying the secret is refused by yuki's CLI before anything is sent.
    let (code, stdout, stderr) = run(
        yuki().args([
            "federation",
            "accept",
            &format!("http://localhost:{molinka_port}/federation/v1/pair?secret={secret}"),
        ]),
        None,
    );
    assert_eq!(code, 1);
    assert!(stderr.contains("URL"), "{stderr}");
    assert!(!stdout.contains(&secret) && !stderr.contains(&secret));

    // Accept on yuki, from stdin: yuki posts to molinka over HTTP.
    let (code, stdout, stderr) = run(
        yuki().args(["federation", "accept"]),
        Some(&format!("{token}\n")),
    );
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(stdout.contains(&molinka_id), "{stdout}");
    assert!(stdout.contains("pending"), "{stdout}");
    assert!(
        stdout.contains("--profile yuki") && stdout.contains("confirm"),
        "says what happens next and how to watch it:\n{stdout}"
    );
    assert!(
        !stdout.contains(&secret) && !stderr.contains(&secret) && !stdout.contains(&token),
        "{stdout}{stderr}"
    );

    // Replaying the same line is refused by molinka, and the refusal names
    // the invite, not the secret.
    let (code, stdout, stderr) = run(yuki().args(["federation", "accept", &token]), None);
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(
        stderr.contains("invalid_invite") || stderr.to_lowercase().contains("invite"),
        "{stderr}"
    );
    assert!(!stdout.contains(&secret) && !stderr.contains(&secret) && !stderr.contains(&token));

    // molinka sees yuki pending, as the issuer, with the origin yuki reported.
    let (code, stdout, _) = run(molinka().args(["federation", "peers", "--json"]), None);
    assert_eq!(code, 0);
    let overview: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(
        overview["invites"].as_array().unwrap().len(),
        1,
        "the second invite is still open"
    );
    let pending = &overview["peers"][0];
    assert_eq!(pending["companion_id"], yuki_id);
    assert_eq!(pending["state"], "pending");
    assert_eq!(pending["role"], "issuer");
    assert_eq!(
        pending["pending_origin"],
        format!("http://localhost:{yuki_port}")
    );
    assert!(pending["last_seen_at"].as_u64().is_some());
    let (code, stdout, _) = run(molinka().args(["federation", "peers"]), None);
    assert_eq!(code, 0);
    assert!(
        stdout.contains(&yuki_id) && stdout.contains("pending"),
        "{stdout}"
    );
    assert!(
        stdout.contains("confirm"),
        "the listing says how to confirm:\n{stdout}"
    );

    // Nothing verifies before the confirmation: yuki's owner routes are not
    // molinka's, and a same-host request without a signature is nothing.
    let (code, _, stderr) = run(yuki().args(["federation", "confirm", &molinka_id]), None);
    assert_eq!(code, 1, "the accepter cannot confirm");
    assert!(
        stderr.contains("issu") || stderr.contains("not paired") || stderr.contains("pending"),
        "{stderr}"
    );

    // Confirm on molinka: paired on both, yuki told over HTTP.
    let (code, stdout, stderr) = run(molinka().args(["federation", "confirm", &yuki_id]), None);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(
        stdout.contains("paired") && stdout.contains(&yuki_id),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!("http://localhost:{yuki_port}")),
        "approved origin is reported:\n{stdout}"
    );
    for (mut cmd, other) in [(molinka(), &yuki_id), (yuki(), &molinka_id)] {
        let (code, stdout, _) = run(cmd.args(["federation", "peers", "--json"]), None);
        assert_eq!(code, 0);
        let overview: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        let peer = &overview["peers"][0];
        assert_eq!(peer["companion_id"], *other);
        assert_eq!(peer["state"], "paired");
        assert_eq!(peer["approved_origins"].as_array().unwrap().len(), 1);
        assert!(peer.get("pending_origin").is_none());
    }
    let (_, stdout, _) = run(yuki().args(["federation", "peers"]), None);
    assert!(
        stdout.contains("paired") && stdout.contains(&format!("http://localhost:{molinka_port}")),
        "{stdout}"
    );
    assert!(stdout.contains("last seen"), "{stdout}");

    // Same host, no shortcut: loopback, the Host header, and the sibling's
    // owner token open nothing on the peer routes or the owner routes.
    let response = http(molinka_port, "POST", "/federation/v1/ping", &[], "{}");
    assert!(response.starts_with("HTTP/1.1 400"), "{response}");
    assert!(
        response.contains("\"malformed\"") || response.contains("invalid_body"),
        "{response}"
    );
    let response = http(
        molinka_port,
        "POST",
        &format!("/federation/v1/pair?secret={secret}"),
        &[
            ("Authorization", &format!("Bearer {yuki_token}")),
            ("X-Forwarded-For", "127.0.0.1"),
        ],
        "{\"kind\":\"pair_request\"}",
    );
    assert!(response.starts_with("HTTP/1.1 400"), "{response}");
    assert!(!response.contains(&secret), "{response}");
    let response = http(
        molinka_port,
        "GET",
        "/api/federation/peers",
        &[("Authorization", &format!("Bearer {yuki_token}"))],
        "",
    );
    assert!(response.starts_with("HTTP/1.1 401"), "{response}");
    let response = http(molinka_port, "GET", "/api/federation/peers", &[], "");
    assert!(response.starts_with("HTTP/1.1 401"), "{response}");
    let response = http(
        molinka_port,
        "GET",
        "/api/federation/peers",
        &[("Authorization", &format!("Bearer {molinka_token}"))],
        "",
    );
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");

    // Rotate on yuki: molinka is told at its approved origin and re-keys.
    let (code, stdout, stderr) = run(yuki().args(["federation", "rotate", "--yes"]), None);
    assert_eq!(code, 0, "{stdout}{stderr}");
    let (code, json_out, _) = run(yuki().args(["federation", "peers", "--json"]), None);
    assert_eq!(code, 0);
    let overview: serde_json::Value = serde_json::from_str(json_out.trim()).unwrap();
    let new_yuki_id = overview["companion_id"].as_str().unwrap().to_owned();
    assert_ne!(new_yuki_id, yuki_id, "a rotated companion has a new id");
    assert!(stdout.contains(&new_yuki_id), "{stdout}");
    assert!(
        stdout.contains(&molinka_id) && stdout.contains("notified"),
        "{stdout}"
    );
    assert_eq!(overview["rotations"].as_array().unwrap().len(), 1);
    let (code, stdout, _) = run(molinka().args(["federation", "peers", "--json"]), None);
    assert_eq!(code, 0);
    let overview: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let peer = &overview["peers"][0];
    assert_eq!(peer["companion_id"], new_yuki_id);
    assert_eq!(peer["state"], "paired");
    assert_eq!(peer["rotation_history"].as_array().unwrap().len(), 1);
    assert_eq!(
        peer["approved_origins"][0],
        format!("http://localhost:{yuki_port}")
    );
    let (_, stdout, _) = run(molinka().args(["federation", "peers"]), None);
    assert!(
        stdout.contains("rotated") || stdout.contains("rotation"),
        "{stdout}"
    );

    // Revoke on molinka: yuki is told and both stop trusting.
    let (code, stdout, stderr) = run(molinka().args(["federation", "revoke", &new_yuki_id]), None);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(
        stdout.contains("revoked") && stdout.contains("notified"),
        "{stdout}"
    );
    for mut cmd in [molinka(), yuki()] {
        let (code, stdout, _) = run(cmd.args(["federation", "peers", "--json"]), None);
        assert_eq!(code, 0);
        let overview: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        assert_eq!(overview["peers"][0]["state"], "revoked");
    }
    let (code, _, _) = run(molinka().args(["federation", "revoke", &new_yuki_id]), None);
    assert_eq!(code, 0, "revoking twice is a no-op");
    let (code, _, stderr) = run(molinka().args(["federation", "revoke", "nobody"]), None);
    assert_eq!(code, 1);
    assert!(
        stderr.contains("unknown") || stderr.contains("not a peer"),
        "{stderr}"
    );

    // The stores hold keys and origins, never the secret, the invite line,
    // a profile name, or the sibling's port as anything but an approved URL.
    let status = terminate(&mut molinka_gateway);
    assert!(status.success(), "molinka exited with {status}");
    let status = terminate(&mut yuki_gateway);
    assert!(status.success(), "yuki exited with {status}");
    let molinka_log = molinka_log.join().unwrap();
    let yuki_log = yuki_log.join().unwrap();
    for (name, log) in [("molinka", &molinka_log), ("yuki", &yuki_log)] {
        assert!(
            !log.contains(&secret),
            "{name} logged the invite secret:\n{log}"
        );
        assert!(
            !log.contains(INVITE_PREFIX),
            "{name} logged an invite line:\n{log}"
        );
    }
    for (root, sibling) in [(&molinka_root, "yuki"), (&yuki_root, "molinka")] {
        let peers = fs::read_to_string(root.join("federation/peers.json")).unwrap();
        assert!(
            !peers.contains(&secret) && !peers.contains(INVITE_PREFIX),
            "{peers}"
        );
        assert!(
            !peers.contains(sibling),
            "the peer store names no profile: {peers}"
        );
        assert!(!peers.contains("127.0.0.1"), "{peers}");
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(root.join("federation"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700);
    }
    assert!(
        !home_dir.join(".nolune").exists(),
        "named profiles never touch the default root"
    );
}

/// The CLI never reads or builds a URL that carries an invite, never logs,
/// and prints an invite line from one place only.
#[test]
fn federation_cli_never_puts_an_invite_in_a_url_or_a_log() {
    let source = without_cfg_test_items(&read("server/src/cli/federation.rs"));
    for forbidden in [
        "?secret",
        "?invite",
        "?token",
        "&secret=",
        "log::",
        "tracing::",
        "env_logger",
        "reqwest::get(",
        ".query(",
        "get(&",
    ] {
        assert!(
            !source.contains(forbidden),
            "cli/federation.rs contains {forbidden:?}"
        );
    }
    // The invite reaches stdout through one function only; stderr never
    // formats an invite, a token, or a secret.
    let mut invite_prints = 0;
    for line in source.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("eprintln!") || trimmed.starts_with("eprint!") {
            for token in ["{invite", "{token", "{secret", "{line", "expose("] {
                assert!(!line.contains(token), "stderr formats {token:?}: {line}");
            }
        }
        if trimmed.starts_with("println!") && (line.contains("{invite") || line.contains("{token"))
        {
            invite_prints += 1;
        }
    }
    assert!(
        invite_prints <= 1,
        "the invite line is printed from one place, found {invite_prints}"
    );
    assert!(
        source.contains("fn print_invite_once("),
        "the one place that prints an invite must be named"
    );
    assert!(
        source.contains("looks_like_url(") && source.contains("decode_invite("),
        "accept validates the line with the shared codec before contacting anything"
    );
    let cli = without_cfg_test_items(&read("server/src/cli.rs"));
    assert!(
        cli.contains("federation::FederationAction") && cli.contains("federation::run("),
        "cli.rs must route the federation subcommand to cli/federation.rs"
    );
}

/// The Companions section exists on Settings › Connections, is built from
/// pure helpers with tests, never stores or logs an invite in the browser,
/// and is documented where the other sections are.
#[test]
fn companions_section_is_documented_and_never_keeps_an_invite() {
    let page = read("client/src/routes/[slug]/settings/connections/+page.svelte");
    assert!(
        page.contains("Companions"),
        "Connections must show the Companions section"
    );
    assert!(
        page.contains("components/federation/Companions.svelte")
            || page.contains("Companions.svelte"),
        "the section is its own component"
    );
    let component = read("client/src/lib/components/federation/Companions.svelte");
    let row = read("client/src/lib/components/federation/CompanionRow.svelte");
    let helpers = read("client/src/lib/federation/companions.js");
    for (name, source) in [
        ("Companions.svelte", &component),
        ("CompanionRow.svelte", &row),
        ("companions.js", &helpers),
    ] {
        for forbidden in [
            "localStorage",
            "sessionStorage",
            "console.",
            "document.cookie",
            "?invite",
            "?secret",
        ] {
            assert!(!source.contains(forbidden), "{name} contains {forbidden:?}");
        }
    }
    for forbidden in [
        "secret",
        ".invite",
        "invite:",
        "inviteHandoff",
        "IssuedFederationInvite",
        "createFederationInvite",
        "acceptFederationInvite",
    ] {
        assert!(
            !row.contains(forbidden),
            "a peer row never sees an invite: CompanionRow.svelte contains {forbidden:?}"
        );
    }
    for required in ["lastSeen", "approvedOrigins", "canConfirm", "canRevoke"] {
        assert!(
            helpers.contains(required),
            "companions.js is missing {required}"
        );
    }
    assert!(
        read("client/tests/federation-companions.test.mjs").contains("companions.js"),
        "the helpers need node tests"
    );
    for (relative, required) in [
        (
            "client/src/lib/api/client.ts",
            "export function fetchFederation(",
        ),
        (
            "client/src/lib/api/client.ts",
            "export function createFederationInvite(",
        ),
        (
            "client/src/lib/api/client.ts",
            "export function acceptFederationInvite(",
        ),
        (
            "client/src/lib/api/client.ts",
            "export function confirmFederationPeer(",
        ),
        (
            "client/src/lib/api/client.ts",
            "export function revokeFederationPeer(",
        ),
        (
            "client/src/lib/api/client.ts",
            "export function rotateFederationIdentity(",
        ),
        (
            "client/src/lib/api/types.ts",
            "export interface FederationPeer",
        ),
        (
            "client/src/routes/design-system/+page.svelte",
            "CompanionRow",
        ),
    ] {
        assert!(
            read(relative).contains(required),
            "{relative} is missing {required:?}"
        );
    }
    let client = read("client/src/lib/api/client.ts");
    assert!(
        !client.contains("?invite=") && !client.contains("?secret="),
        "the client never puts an invite in a URL"
    );

    let settings = read("docs/settings.md");
    assert!(
        settings.contains("Companions") && settings.contains("federation.md"),
        "docs/settings.md must list the Companions section and link the federation doc"
    );
    let design = read("docs/design-system.md");
    assert!(
        design.contains("Companions"),
        "docs/design-system.md must describe the section"
    );
    let readme = read("README.md");
    assert!(
        readme.contains("`nolune federation invite`") && readme.contains("docs/federation.md"),
        "README must list the federation commands and link the doc"
    );
}

/// `docs/federation.md` describes identity, pairing, the envelope, rotation,
/// the CLI and the section, and the no-implicit-trust rule for profiles on
/// one host, in the vocabulary the identity-language guard allows.
#[test]
fn federation_doc_covers_identity_pairing_envelope_rotation_and_same_host_rule() {
    let doc = read("docs/federation.md");
    for required in [
        "#108",
        "peer companion",
        "companion_id",
        "identity.json",
        "signing_key.json",
        "peers.json",
        "rotations.json",
        "nolune federation invite",
        "nolune federation accept",
        "nolune federation peers",
        "nolune federation confirm",
        "nolune federation revoke",
        "nolune federation rotate",
        "--profile",
        "nolune-invite-v1.",
        "stdin",
        "Settings → Connections",
        "Companions",
        "/api/federation/",
        "/federation/v1/",
        "nonce",
        "expires_at",
        "replay",
        "rotation",
        "revoke",
        "same host",
        "loopback",
        "no implicit trust",
        "companion-storage.md",
    ] {
        assert!(
            doc.contains(required),
            "docs/federation.md is missing {required:?}"
        );
    }
    for forbidden in ["each companion", "dedicated server", "per instance"] {
        assert!(
            !doc.contains(forbidden),
            "docs/federation.md says {forbidden:?}"
        );
    }
    assert!(
        !doc.contains("?secret=") && !doc.contains("?invite="),
        "the doc must not show a secret in a URL, even as an example"
    );
}
