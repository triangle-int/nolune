//! Router-level tests for the Resume my work ritual (#83).
//!
//! Included from `app/router.rs`, so every request goes through
//! `build_router` with the real auth and companion middleware. Most
//! desktops register through the registry exactly as the WebSocket route
//! does; the reconnect tests instead serve the router on a local port and
//! connect a desktop over the machine socket itself, so the connect hook is
//! reached the way a real desktop reaches it. Either way a desktop
//! registers with what one from this release sends (#19): the five
//! toolcalls the app executes and the descriptor of the Cua driver whose
//! grants a continuation needs. Either way their toolcall
//! channels stay empty throughout, which is how these tests prove that a
//! suggestion never touches a computer. Records are written the way
//! explicit task activity writes them; the ritual only reads them.

use super::*;
use crate::{
    config::{Config, LlmProvider, ModelPreset},
    domain::{
        companion::CANONICAL_SLUG,
        continuity::{
            ContinuityRecord, ContinuityUpdate, HandoffDecision, Origin, Priority, Provenance,
            ProvenanceSource,
        },
        events::ServerEvent,
        machine::DESKTOP_TOOLCALLS,
        proactive::{ProactivePolicy, QuietHours},
        resume::{MAX_BREAK_MINUTES, MAX_COOLDOWN_SECS},
    },
    services::{
        companion,
        continuity::ContinuityStore,
        machine_registry::MachineInfo,
        resume_ritual::{self, Held, RITUAL_FILE, ResumeRitual},
    },
};
use axum::{
    body::Body,
    http::{Method, Request, StatusCode, header},
};
use chrono::Timelike;
use cua_protocol::{
    Capability, CuaRegistrationEnvelope, DriverVersion, MachineDescriptor, MachineHealth,
    MachineId, MachineLocation, Permission, PermissionState, Platform, ProtocolVersion,
};
use std::{fs, net::SocketAddr, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use tower::ServiceExt;

const TOKEN: &str = "issue-83-resume-token";
const MAX_BODY: usize = 64 * 1024 * 1024;
const MAC_A: &str = "4f3c1c2e-9b5e-4d2b-8f0a-1c2d3e4f5a6b";
const MAC_B: &str = "7e2d0b1a-3c4d-4e5f-9a8b-0c1d2e3f4a5b";
const CHAT: &str = "chat_1";

struct Harness {
    workspace: tempfile::TempDir,
    state: AppState,
}

/// A companion with a chat model configured, so continuation is possible
/// and the handoff checks judge only the destination.
fn configured() -> Config {
    let mut config = Config {
        auth_token: TOKEN.into(),
        ..Default::default()
    };
    config.llm.presets = vec![ModelPreset {
        id: "sonnet".into(),
        name: "Claude Sonnet".into(),
        provider: LlmProvider::Anthropic,
        model: "claude-sonnet-4-6".into(),
    }];
    config.llm.chat_preset = "sonnet".into();
    config.llm.tokens.anthropic = "test-key-never-used".into();
    config
}

async fn harness() -> Harness {
    let workspace = tempfile::tempdir().unwrap();
    let state = AppState::new_in(configured(), workspace.path().to_owned()).await;
    companion::ensure_identity(workspace.path()).unwrap();
    Harness { workspace, state }
}

impl Harness {
    /// The same workspace under a new process: everything on disk stays,
    /// everything in memory (connections, events) is gone.
    async fn restarted(self) -> Harness {
        let Harness { workspace, state } = self;
        drop(state);
        let state = AppState::new_in(configured(), workspace.path().to_owned()).await;
        Harness { workspace, state }
    }
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn api(suffix: &str) -> String {
    format!("/api/instances/{CANONICAL_SLUG}/{suffix}")
}

/// What the desktop app reports as `capabilities`: the toolcalls it
/// executes, and nothing about windows (`computer_use_bridge.rs::CAPABILITIES`).
fn desktop_toolcalls() -> Vec<String> {
    DESKTOP_TOOLCALLS.iter().map(|s| (*s).to_owned()).collect()
}

/// The descriptor of a healthy Cua driver holding both grants, as a desktop
/// registers it beside its toolcalls (#17).
fn driver(machine_id: &str) -> MachineDescriptor {
    MachineDescriptor {
        machine_id: MachineId::try_from(machine_id).unwrap(),
        location: MachineLocation::Desktop,
        platform: Platform::Macos,
        driver_version: DriverVersion::try_from("0.28.2").unwrap(),
        health: MachineHealth::Healthy,
        permissions: PermissionState {
            accessibility: Permission::Granted,
            screen_capture: Permission::Granted,
        },
        capabilities: vec![
            Capability::AppDiscovery,
            Capability::WindowDiscovery,
            Capability::SessionLifecycle,
            Capability::Health,
        ],
    }
}

/// A desktop connected through the registry: the channel every toolcall to
/// it would arrive on.
struct Desktop {
    calls: tokio::sync::mpsc::UnboundedReceiver<String>,
}

impl Desktop {
    fn assert_untouched(&mut self, label: &str) {
        assert!(
            self.calls.try_recv().is_err(),
            "{label}: a toolcall reached the desktop"
        );
    }
}

/// A desktop attached over the machine socket (`/api/agents/ws/machine`),
/// the way the desktop app attaches: a WebSocket handshake with the API
/// token, then the registration message. Frames are the RFC 6455 wire
/// format written by hand, so the test has no client library between it
/// and the route.
struct SocketDesktop {
    stream: TcpStream,
}

impl SocketDesktop {
    async fn connect(addr: SocketAddr, machine_id: &str, hostname: &str) -> Self {
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let request = format!(
            "GET /api/agents/ws/machine HTTP/1.1\r\nHost: {addr}\r\nUpgrade: websocket\r\n\
             Connection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
             Sec-WebSocket-Version: 13\r\nAuthorization: Bearer {TOKEN}\r\n\r\n"
        );
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = Vec::new();
        let mut byte = [0u8; 1];
        while !response.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).await.unwrap();
            response.push(byte[0]);
        }
        let response = String::from_utf8_lossy(&response);
        assert!(response.starts_with("HTTP/1.1 101"), "{response}");

        let mut desktop = Self { stream };
        // The register frame a desktop from this release sends: no grants of
        // the app's own, the toolcalls it executes, and its driver's
        // descriptor in `cua`.
        desktop
            .send_text(
                &serde_json::json!({
                    "type": "register",
                    "machine_id": machine_id,
                    "os": "macos",
                    "hostname": hostname,
                    "capabilities": desktop_toolcalls(),
                    "cua": CuaRegistrationEnvelope {
                        version: ProtocolVersion::V1,
                        machine: driver(machine_id),
                    },
                })
                .to_string(),
            )
            .await;
        let ack = desktop.next_text(Duration::from_secs(5)).await.unwrap();
        assert_eq!(ack["type"], "registered", "{ack}");
        assert_eq!(ack["machine_id"], machine_id);
        assert_eq!(ack["cua"], true, "the typed target attached: {ack}");
        desktop
    }

    /// One masked text frame, as a client must send it.
    async fn send_text(&mut self, text: &str) {
        let payload = text.as_bytes();
        let key = [0x12u8, 0x34, 0x56, 0x78];
        let mut frame = vec![0x81u8];
        if payload.len() < 126 {
            frame.push(0x80 | payload.len() as u8);
        } else {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        }
        frame.extend_from_slice(&key);
        frame.extend(payload.iter().enumerate().map(|(i, b)| b ^ key[i % 4]));
        self.stream.write_all(&frame).await.unwrap();
    }

    /// The next frame from the server: its opcode and payload.
    async fn next_frame(&mut self) -> (u8, Vec<u8>) {
        let mut head = [0u8; 2];
        self.stream.read_exact(&mut head).await.unwrap();
        assert_eq!(head[1] & 0x80, 0, "server frames are never masked");
        let mut len = usize::from(head[1] & 0x7F);
        if len == 126 {
            let mut ext = [0u8; 2];
            self.stream.read_exact(&mut ext).await.unwrap();
            len = usize::from(u16::from_be_bytes(ext));
        } else if len == 127 {
            let mut ext = [0u8; 8];
            self.stream.read_exact(&mut ext).await.unwrap();
            len = usize::try_from(u64::from_be_bytes(ext)).unwrap();
        }
        let mut payload = vec![0u8; len];
        self.stream.read_exact(&mut payload).await.unwrap();
        (head[0] & 0x0F, payload)
    }

    /// The next text frame within `wait`, or `None`. Control frames are skipped.
    async fn next_text(&mut self, wait: Duration) -> Option<serde_json::Value> {
        tokio::time::timeout(wait, async {
            loop {
                let (opcode, payload) = self.next_frame().await;
                if opcode == 0x1 {
                    return serde_json::from_slice(&payload).unwrap();
                }
            }
        })
        .await
        .ok()
    }

    /// Nothing but the registration ack ever reaches the desktop.
    async fn assert_untouched(&mut self, label: &str) {
        assert_eq!(
            self.next_text(Duration::from_millis(200)).await,
            None,
            "{label}: a toolcall reached the desktop"
        );
    }

    /// Drop the connection the way a closed laptop lid does: no close frame.
    async fn disconnect(self) {
        drop(self.stream);
        tokio::task::yield_now().await;
    }
}

impl Harness {
    /// The router served on a local port, for a desktop on the machine socket.
    async fn serve(&self) -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = build_router(self.state.clone(), None);
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        addr
    }

    /// The current suggestion once one appears, within a few seconds.
    async fn wait_for_suggestion(&self) -> serde_json::Value {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let status = self.status().await;
            if !status["suggestion"].is_null() {
                return status["suggestion"].clone();
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "no suggestion appeared: {status}"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Whether `machine_id` is listed as connected right now.
    async fn is_online(&self, machine_id: &str) -> bool {
        let (_, machines) = self.json(Method::GET, &api("machines"), None).await;
        machines["machines"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["machine_id"] == machine_id && m["online"] == true)
    }

    /// Give a spawned hook every chance to run, then read the status.
    async fn settled_status(&self) -> serde_json::Value {
        tokio::time::sleep(Duration::from_millis(200)).await;
        self.status().await
    }
}

impl Harness {
    /// A desktop registered the way the machine route registers one: its
    /// toolcalls on the legacy registration, then its Cua descriptor
    /// attached to the same connection.
    async fn connect_ready(&self, machine_id: &str, hostname: &str) -> Desktop {
        let (tx, calls) = tokio::sync::mpsc::unbounded_channel();
        let descriptor = driver(machine_id);
        let connection = self
            .state
            .machine_registry
            .register(
                MachineInfo {
                    machine_id: machine_id.into(),
                    os: "macos".into(),
                    hostname: hostname.into(),
                    last_seen: now(),
                    instance_slug: None,
                    platform: Some(Platform::Macos),
                    location: MachineLocation::Desktop,
                    permissions: Some(descriptor.permissions.clone()),
                    capabilities: desktop_toolcalls(),
                },
                tx,
            )
            .await;
        self.state
            .machine_registry
            .attach_desktop_cua(machine_id, connection, descriptor, Duration::from_secs(5))
            .await
            .expect("the driver attaches to the connection that just registered");
        Desktop { calls }
    }

    fn store(&self) -> ContinuityStore {
        ContinuityStore::new(self.workspace.path(), CANONICAL_SLUG)
    }

    fn ritual(&self) -> ResumeRitual {
        ResumeRitual::new(self.workspace.path(), CANONICAL_SLUG)
    }

    fn ritual_file(&self) -> std::path::PathBuf {
        companion::companion_dir(self.workspace.path()).join(RITUAL_FILE)
    }

    /// The companion is onboarded, which the connect check-in requires.
    fn onboarded(&self) {
        let dir = companion::companion_dir(self.workspace.path());
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("soul.md"), "# soul\n").unwrap();
    }

    /// A task started in `CHAT` naming `machine_ids`, updated `age` seconds ago.
    async fn task(
        &self,
        goal: &str,
        machine_ids: &[&str],
        age: i64,
        update: ContinuityUpdate,
    ) -> ContinuityRecord {
        let at = now() - age;
        self.store()
            .create(
                goal,
                Origin {
                    chat_id: CHAT.into(),
                    message_id: Some("msg_1".into()),
                },
                &ContinuityUpdate {
                    next_step: Some("carry on".into()),
                    machine_ids: machine_ids.iter().map(|s| (*s).to_owned()).collect(),
                    ..update
                },
                Provenance {
                    source: ProvenanceSource::Chat,
                    at,
                    note: "asked in chat".into(),
                },
                at,
            )
            .await
            .unwrap()
    }

    async fn send(
        &self,
        method: Method,
        uri: &str,
        body: Option<serde_json::Value>,
    ) -> (StatusCode, Vec<u8>) {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"));
        let body = match body {
            Some(json) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                Body::from(serde_json::to_vec(&json).unwrap())
            }
            None => Body::empty(),
        };
        let response = build_router(self.state.clone(), None)
            .oneshot(request.body(body).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), MAX_BODY)
            .await
            .unwrap();
        (status, bytes.to_vec())
    }

    async fn json(
        &self,
        method: Method,
        uri: &str,
        body: Option<serde_json::Value>,
    ) -> (StatusCode, serde_json::Value) {
        let (status, bytes) = self.send(method, uri, body).await;
        let value = serde_json::from_slice(&bytes).unwrap_or_else(|error| {
            panic!(
                "{uri}: expected JSON body, got {error}: {}",
                String::from_utf8_lossy(&bytes)
            )
        });
        (status, value)
    }

    /// Turn the ritual on with the given break and cooldown.
    async fn enable(&self, break_minutes: u32, cooldown_secs: u64) {
        let (status, policy) = self
            .json(
                Method::PUT,
                &api("resume"),
                Some(serde_json::json!({
                    "enabled": true,
                    "break_minutes": break_minutes,
                    "cooldown_secs": cooldown_secs,
                })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{policy}");
        assert_eq!(policy["enabled"], true);
    }

    async fn resume_now(&self) -> (StatusCode, serde_json::Value) {
        self.json(Method::POST, &api("resume"), None).await
    }

    async fn opened(&self) -> serde_json::Value {
        let (status, body) = self.json(Method::POST, &api("resume/opened"), None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    async fn status(&self) -> serde_json::Value {
        let (status, body) = self.json(Method::GET, &api("resume"), None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    /// Pretend Nolune was last opened `age` seconds ago.
    async fn last_opened(&self, age: i64) {
        self.ritual().record_opened(now() - age).await.unwrap();
    }
}

fn held(body: &serde_json::Value) -> String {
    assert!(
        body["suggestion"].is_null(),
        "expected no suggestion: {body}"
    );
    body["held"]["kind"]
        .as_str()
        .unwrap_or_else(|| panic!("no held reason in {body}"))
        .to_owned()
}

fn resume_events(rx: &mut tokio::sync::broadcast::Receiver<ServerEvent>) -> Vec<Option<String>> {
    let mut out = Vec::new();
    while let Ok(event) = rx.try_recv() {
        if let ServerEvent::ResumeUpdated { suggestion, .. } = event {
            out.push(suggestion.map(|offer| offer.suggestion.record_id));
        }
    }
    out
}

#[tokio::test]
async fn manual_invocation_offers_exactly_one_suggestion_that_explains_its_record_and_why_now() {
    let h = harness().await;
    let mut rx = h.state.events.subscribe();
    let mut studio = h.connect_ready(MAC_A, "studio").await;
    h.enable(120, 3_600).await;
    let plain = h
        .task(
            "sort the receipts",
            &[MAC_A],
            300,
            ContinuityUpdate::default(),
        )
        .await;
    let urgent = h
        .task(
            "file the tax return",
            &[MAC_A],
            7_200,
            ContinuityUpdate {
                priority: Some(Priority::High),
                // Half an hour of slack: the label floors to whole hours
                // against a later clock, and a slow full-suite run must not
                // tip "due in 3 hours" over to "2 hours".
                due_at: Some(now() + 3 * 3_600 + 1_800),
                ..Default::default()
            },
        )
        .await;

    let (status, body) = h.resume_now().await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let suggestion = &body["suggestion"];
    assert!(body["held"].is_null(), "{body}");
    assert_eq!(
        suggestion["record_id"], urgent.id,
        "priority and deadline beat recency"
    );
    assert_eq!(suggestion["goal"], "file the tax return");
    assert_eq!(suggestion["trigger"]["kind"], "manual");
    assert_eq!(suggestion["why_now"], "You asked to resume your work.");
    let why_this = suggestion["why_this"].as_str().unwrap();
    for expected in ["high priority", "due in 3 hours", "studio"] {
        assert!(
            why_this.contains(expected),
            "{why_this:?} lacks {expected:?}"
        );
    }
    assert_eq!(suggestion["destination_id"], MAC_A);
    assert!(suggestion["id"].as_str().unwrap().starts_with("sug_"));
    // The delivery is the record's handoff card, offered and undecided.
    assert_eq!(suggestion["card"]["record_id"], urgent.id);
    assert_eq!(suggestion["card"]["offered"], true);
    assert!(suggestion["card"]["decision"].is_null());
    assert_eq!(suggestion["card"]["origin"]["display_name"], "studio");

    // The same offer is what a fresh read shows, and one event announced it.
    let current = h.status().await;
    assert_eq!(current["suggestion"]["id"], suggestion["id"]);
    assert_eq!(current["policy"]["enabled"], true);
    assert_eq!(resume_events(&mut rx), vec![Some(urgent.id.clone())]);

    // Asking again re-ranks: still one suggestion, the same record; the
    // older offer is replaced, never stacked.
    let (status, again) = h.resume_now().await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["suggestion"]["record_id"], urgent.id);
    assert_ne!(again["suggestion"]["id"], suggestion["id"]);
    assert_eq!(
        h.status().await["suggestion"]["id"],
        again["suggestion"]["id"]
    );

    studio.assert_untouched("a suggestion");
    assert!(h.store().get(&plain.id).is_some());
}

#[tokio::test]
async fn a_disabled_ritual_answers_conflict_and_has_no_side_effects() {
    let h = harness().await;
    let mut rx = h.state.events.subscribe();
    let mut studio = h.connect_ready(MAC_A, "studio").await;
    h.task(
        "sort the receipts",
        &[MAC_A],
        300,
        ContinuityUpdate::default(),
    )
    .await;

    let status = h.status().await;
    assert_eq!(status["policy"]["enabled"], false, "opt-in: off by default");
    assert!(status["suggestion"].is_null());

    let (status, body) = h.resume_now().await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "resume_disabled");

    assert_eq!(held(&h.opened().await), "disabled");
    assert_eq!(held(&h.opened().await), "disabled");

    assert!(
        !h.ritual_file().exists(),
        "a disabled ritual writes nothing, not even the open"
    );
    assert!(resume_events(&mut rx).is_empty());
    assert!(h.status().await["suggestion"].is_null());
    studio.assert_untouched("a disabled ritual");
}

#[tokio::test]
async fn the_settings_edit_is_bounded_and_a_refused_edit_changes_nothing() {
    let h = harness().await;
    h.enable(120, 3_600).await;
    let before = h.status().await["policy"].clone();

    for (label, break_minutes, cooldown_secs) in [
        ("no break at all", 0, 3_600),
        ("a break of more than 30 days", MAX_BREAK_MINUTES + 1, 3_600),
        (
            "a cooldown of more than 30 days",
            120,
            MAX_COOLDOWN_SECS + 1,
        ),
        // Would wrap `since + cooldown` negative and silently end the cooldown.
        ("a cooldown past the end of time", 120, u64::MAX),
    ] {
        let (status, body) = h
            .json(
                Method::PUT,
                &api("resume"),
                Some(serde_json::json!({
                    "enabled": true,
                    "break_minutes": break_minutes,
                    "cooldown_secs": cooldown_secs,
                })),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{label}: {body}");
        assert_eq!(body["error"], "invalid", "{label}: {body}");
        assert_eq!(
            h.status().await["policy"],
            before,
            "{label}: a refused edit changed the policy"
        );
    }

    // The bounds themselves are accepted, as is no cooldown at all ("None").
    h.enable(MAX_BREAK_MINUTES, 0).await;
    let policy = h.status().await["policy"].clone();
    assert_eq!(policy["break_minutes"], MAX_BREAK_MINUTES);
    assert_eq!(policy["cooldown_secs"], 0);
    h.enable(1, MAX_COOLDOWN_SECS).await;
    let policy = h.status().await["policy"].clone();
    assert_eq!(policy["break_minutes"], 1);
    assert_eq!(policy["cooldown_secs"], MAX_COOLDOWN_SECS);
}

#[tokio::test]
async fn opening_after_a_break_offers_one_suggestion_but_not_within_the_break_or_cooldown() {
    let h = harness().await;
    let mut studio = h.connect_ready(MAC_A, "studio").await;
    h.enable(30, 3_600).await;
    let task = h
        .task(
            "sort the receipts",
            &[MAC_A],
            300,
            ContinuityUpdate::default(),
        )
        .await;

    // The first open has nothing to measure from.
    assert_eq!(held(&h.opened().await), "no_break");
    // Opened again within the break: nothing.
    assert_eq!(held(&h.opened().await), "no_break");

    // Back after an hour: one suggestion that says so.
    h.last_opened(3_600).await;
    let body = h.opened().await;
    let suggestion = &body["suggestion"];
    assert_eq!(suggestion["record_id"], task.id, "{body}");
    assert_eq!(suggestion["trigger"]["kind"], "opened_after_break");
    assert_eq!(
        suggestion["trigger"]["away_secs"].as_i64().unwrap() / 60,
        60
    );
    assert_eq!(
        suggestion["why_now"],
        "You opened Nolune after 1 hour away."
    );
    assert!(suggestion["why_this"].as_str().unwrap().contains("studio"));

    // Another break inside the cooldown: held, and the offer stays the same one.
    h.last_opened(3_600).await;
    let body = h.opened().await;
    assert_eq!(held(&body), "cooldown");
    assert_eq!(h.status().await["suggestion"]["id"], suggestion["id"]);
    studio.assert_untouched("opening Nolune");
}

#[tokio::test]
async fn a_reconnect_over_the_machine_socket_offers_only_work_naming_that_computer_and_never_touches_it()
 {
    let h = harness().await;
    let mut rx = h.state.events.subscribe();
    h.onboarded();
    h.enable(120, 0).await;
    let on_a = h
        .task(
            "photos on studio",
            &[MAC_A],
            3_600,
            ContinuityUpdate::default(),
        )
        .await;
    h.task("notes in chat", &[], 60, ContinuityUpdate::default())
        .await;
    let addr = h.serve().await;

    // A computer no waiting work names connects: nothing is offered.
    let mut laptop = SocketDesktop::connect(addr, MAC_B, "laptop").await;
    assert!(h.settled_status().await["suggestion"].is_null());

    // The computer the task names connects over its socket: exactly that
    // task is offered, and the suggestion says which computer came back.
    let mut studio = SocketDesktop::connect(addr, MAC_A, "studio").await;
    let suggestion = h.wait_for_suggestion().await;
    assert_eq!(suggestion["record_id"], on_a.id, "{suggestion}");
    assert_eq!(suggestion["trigger"]["kind"], "machine_connected");
    assert_eq!(suggestion["trigger"]["machine_id"], MAC_A);
    assert_eq!(
        suggestion["why_now"],
        "studio reconnected, and this task names it."
    );
    assert_eq!(suggestion["destination_id"], MAC_A);
    assert_eq!(suggestion["card"]["origin"]["online"], true);
    assert_eq!(resume_events(&mut rx), vec![Some(on_a.id.clone())]);
    studio.assert_untouched("a reconnect").await;
    laptop.assert_untouched("a reconnect").await;

    // The browser's own check-in (machine-hello on every page load) is not a
    // reconnect: with the desktop connected the whole time it offers nothing.
    let (status, _) = h.send(Method::POST, &api("resume/refuse"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = h
        .send(
            Method::POST,
            &api("machine-hello"),
            Some(serde_json::json!({ "machine_id": MAC_A })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        h.settled_status().await["suggestion"].is_null(),
        "opening the companion in a browser is not a reconnect"
    );
    assert_eq!(resume_events(&mut rx), vec![None]);

    // The desktop drops off and comes back: that is a reconnect, and the
    // ritual speaks up again (the cooldown is off here).
    studio.disconnect().await;
    let gone = tokio::time::Instant::now() + Duration::from_secs(5);
    while h.is_online(MAC_A).await {
        assert!(
            tokio::time::Instant::now() < gone,
            "studio never went offline"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let mut studio = SocketDesktop::connect(addr, MAC_A, "studio").await;
    let again = h.wait_for_suggestion().await;
    assert_eq!(again["record_id"], on_a.id);
    assert_ne!(
        again["id"], suggestion["id"],
        "a new suggestion for the new connection"
    );
    assert_eq!(again["trigger"]["kind"], "machine_connected");
    assert_eq!(resume_events(&mut rx), vec![Some(on_a.id.clone())]);
    studio.assert_untouched("a second reconnect").await;
    laptop.assert_untouched("a second reconnect").await;
}

#[tokio::test]
async fn no_suggestion_when_nothing_valid_is_resumable() {
    let h = harness().await;
    h.enable(120, 0).await;

    // Nothing recorded at all.
    let (status, body) = h.resume_now().await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(held(&body), "nothing_to_resume");

    // A record, but its only computer is offline (never connected here).
    let task = h
        .task(
            "photos on studio",
            &[MAC_A],
            300,
            ContinuityUpdate::default(),
        )
        .await;
    let (_, body) = h.resume_now().await;
    assert_eq!(held(&body), "nothing_to_resume");

    // A ready computer, but the record is closed.
    let mut studio = h.connect_ready(MAC_A, "studio").await;
    let (status, done) = h
        .json(
            Method::POST,
            &api(&format!("continuity/{}/complete", task.id)),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{done}");
    let (_, body) = h.resume_now().await;
    assert_eq!(held(&body), "nothing_to_resume");
    assert!(h.status().await["suggestion"].is_null());
    studio.assert_untouched("an empty ritual");
}

#[tokio::test]
async fn work_already_being_continued_is_never_suggested_and_the_next_best_is() {
    let h = harness().await;
    let mut rx = h.state.events.subscribe();
    let mut studio = h.connect_ready(MAC_A, "studio").await;
    h.enable(120, 3_600).await;
    let continuing = h
        .task(
            "rename the trip photos",
            &[MAC_A],
            60,
            ContinuityUpdate::default(),
        )
        .await;
    let plain = h
        .task(
            "sort the receipts",
            &[MAC_A],
            3_600,
            ContinuityUpdate::default(),
        )
        .await;
    // The user accepted the first card and its continuation is running.
    h.store()
        .decide_handoff(
            &continuing.id,
            HandoffDecision::Accepted {
                machine_id: MAC_A.into(),
                run_id: "run_1767603700_0badcafe".into(),
                at: now() - 30,
                outcome: None,
            },
            Provenance {
                source: ProvenanceSource::User,
                at: now() - 30,
                note: "continue here".into(),
            },
            now() - 30,
        )
        .await
        .unwrap();

    let (status, body) = h.resume_now().await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["suggestion"]["record_id"], plain.id,
        "the newer task is being continued already: {body}"
    );
    assert!(body["suggestion"]["card"]["decision"].is_null());
    let current = h.status().await;
    assert_eq!(
        current["suggestion"]["id"], body["suggestion"]["id"],
        "the suggestion is what a fresh read shows"
    );
    assert_eq!(resume_events(&mut rx), vec![Some(plain.id.clone())]);

    // With only the continuing task on file there is nothing to suggest, and
    // the cooldown does not start for a suggestion that was never made.
    let (status, done) = h
        .json(
            Method::POST,
            &api(&format!("continuity/{}/complete", plain.id)),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{done}");
    let (_, body) = h.resume_now().await;
    assert_eq!(held(&body), "nothing_to_resume");
    assert!(h.status().await["suggestion"].is_null());
    assert_eq!(
        h.ritual().state().last_suggested_at,
        Some(current["suggestion"]["suggested_at"].as_i64().unwrap())
    );
    studio.assert_untouched("a continuing task");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn triggers_that_land_together_offer_once_inside_one_cooldown() {
    let h = harness().await;
    let mut rx = h.state.events.subscribe();
    h.connect_ready(MAC_A, "studio").await;
    h.enable(30, 3_600).await;
    let task = h
        .task(
            "sort the receipts",
            &[MAC_A],
            300,
            ContinuityUpdate::default(),
        )
        .await;
    h.last_opened(10_000).await;

    // On one page load the client reports "opened" while the desktop's
    // registration lands: every trigger passes the same cooldown check at
    // once unless the ritual takes them one at a time.
    let go = std::sync::Arc::new(tokio::sync::Barrier::new(4));
    let opened = tokio::spawn({
        let state = h.state.clone();
        let go = go.clone();
        async move {
            go.wait().await;
            resume_ritual::opened(&state, now()).await
        }
    });
    let connects: Vec<_> = (0..3)
        .map(|_| {
            tokio::spawn({
                let state = h.state.clone();
                let go = go.clone();
                async move {
                    go.wait().await;
                    resume_ritual::on_machine_connected(&state, MAC_A).await;
                }
            })
        })
        .collect();
    let opened = opened.await.unwrap();
    for connect in connects {
        connect.await.unwrap();
    }

    let events = resume_events(&mut rx);
    assert_eq!(
        events,
        vec![Some(task.id.clone())],
        "exactly one suggestion was announced"
    );
    let status = h.status().await;
    let suggestion = &status["suggestion"];
    assert_eq!(suggestion["record_id"], task.id);
    match opened {
        Ok(offer) => {
            assert_eq!(suggestion["id"], offer.suggestion.id, "the open report won");
            assert_eq!(suggestion["trigger"]["kind"], "opened_after_break");
        }
        Err(held) => {
            assert!(
                matches!(held, Held::Cooldown { .. }),
                "the open report was held by the reconnect's cooldown, not {held:?}"
            );
            assert_eq!(suggestion["trigger"]["kind"], "machine_connected");
        }
    }
    assert_eq!(
        h.ritual().state().last_suggested_at,
        Some(suggestion["suggested_at"].as_i64().unwrap())
    );
}

#[tokio::test]
async fn refusal_snooze_and_dismiss_are_enforced_across_restarts() {
    let h = harness().await;
    h.connect_ready(MAC_A, "studio").await;
    h.enable(30, 3_600).await;
    let task = h
        .task(
            "sort the receipts",
            &[MAC_A],
            300,
            ContinuityUpdate::default(),
        )
        .await;

    // Not now: the offer is gone and the cooldown runs from the refusal.
    let (_, body) = h.resume_now().await;
    assert_eq!(body["suggestion"]["record_id"], task.id);
    let (status, _) = h.send(Method::POST, &api("resume/refuse"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(h.status().await["suggestion"].is_null());

    let h = h.restarted().await;
    h.connect_ready(MAC_A, "studio").await;
    assert!(
        h.status().await["suggestion"].is_null(),
        "a refusal survives a restart"
    );
    h.last_opened(3_600).await;
    assert_eq!(held(&h.opened().await), "cooldown");

    // Snooze: spontaneous triggers wait, the explicit one does not.
    let until = now() + 7_200;
    let (status, policy) = h
        .json(
            Method::POST,
            &api("resume/snooze"),
            Some(serde_json::json!({ "until": until })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{policy}");
    assert_eq!(policy["snooze_until"], until);
    let (status, refused) = h
        .json(
            Method::POST,
            &api("resume/snooze"),
            Some(serde_json::json!({ "until": now() - 1 })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");

    let h = h.restarted().await;
    h.connect_ready(MAC_A, "studio").await;
    assert_eq!(h.status().await["policy"]["snooze_until"], until);
    h.enable(30, 0).await;
    assert_eq!(
        h.status().await["policy"]["snooze_until"],
        until,
        "a settings edit cannot undo a snooze"
    );
    h.last_opened(3_600).await;
    assert_eq!(held(&h.opened().await), "snoozed");
    let (status, body) = h.resume_now().await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["suggestion"]["record_id"], task.id,
        "asked for, so offered"
    );

    // Ending the snooze brings spontaneous suggestions back.
    let (status, policy) = h
        .json(
            Method::POST,
            &api("resume/snooze"),
            Some(serde_json::json!({ "until": null })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(policy["snooze_until"].is_null());
    h.last_opened(3_600).await;
    assert_eq!(h.opened().await["suggestion"]["record_id"], task.id);

    // Dismiss: never this record again, whatever the trigger.
    let (status, policy) = h
        .json(
            Method::POST,
            &api("resume/dismiss"),
            Some(serde_json::json!({ "record_id": task.id })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{policy}");
    assert_eq!(policy["dismissed_record_ids"], serde_json::json!([task.id]));
    assert!(h.status().await["suggestion"].is_null());

    let h = h.restarted().await;
    h.connect_ready(MAC_A, "studio").await;
    let (status, body) = h.resume_now().await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(held(&body), "nothing_to_resume");
    h.last_opened(3_600).await;
    assert_eq!(held(&h.opened().await), "nothing_to_resume");
    assert_eq!(
        h.status().await["policy"]["dismissed_record_ids"],
        serde_json::json!([task.id])
    );
    // The record itself is untouched: still an offered handoff card.
    let (status, listing) = h.json(Method::GET, &api("handoffs"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing["handoffs"][0]["record_id"], task.id);

    let (status, body) = h
        .json(
            Method::POST,
            &api("resume/dismiss"),
            Some(serde_json::json!({ "record_id": "../etc" })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn quiet_hours_hold_the_spontaneous_triggers_but_not_the_manual_one() {
    let h = harness().await;
    h.onboarded();
    h.enable(30, 0).await;
    let task = h
        .task(
            "sort the receipts",
            &[MAC_A],
            300,
            ContinuityUpdate::default(),
        )
        .await;

    // Quiet hours around this very hour, in the companion's timezone (UTC by
    // default): the hour before through the hour after, so the window cannot
    // end between reading the clock here and the requests below.
    let hour = chrono::Utc::now().hour() as u8;
    h.state
        .proactive
        .set_policy(&ProactivePolicy {
            quiet_hours: Some(QuietHours {
                start_hour: (hour + 23) % 24,
                end_hour: (hour + 2) % 24,
            }),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(h.status().await["quiet_hours_active"], true);

    h.last_opened(3_600).await;
    assert_eq!(held(&h.opened().await), "quiet_hours");
    let addr = h.serve().await;
    let mut studio = SocketDesktop::connect(addr, MAC_A, "studio").await;
    assert!(h.settled_status().await["suggestion"].is_null());
    studio.assert_untouched("quiet hours").await;

    let (status, body) = h.resume_now().await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["suggestion"]["record_id"], task.id);
}

#[tokio::test]
async fn a_suggestion_is_resolved_once_its_card_is_decided_or_the_ritual_is_turned_off() {
    let h = harness().await;
    let mut rx = h.state.events.subscribe();
    h.connect_ready(MAC_A, "studio").await;
    h.enable(120, 0).await;
    let task = h
        .task(
            "sort the receipts",
            &[MAC_A],
            300,
            ContinuityUpdate::default(),
        )
        .await;

    let (_, body) = h.resume_now().await;
    assert_eq!(body["suggestion"]["record_id"], task.id);
    let (status, card) = h
        .json(
            Method::POST,
            &api(&format!("continuity/{}/handoff/keep", task.id)),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{card}");
    assert!(
        h.status().await["suggestion"].is_null(),
        "a kept card is an answered suggestion"
    );
    let (status, body) = h.resume_now().await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        held(&body),
        "nothing_to_resume",
        "kept work is not offered again"
    );

    // Turning the ritual off drops the offer and announces it.
    let other = h
        .task(
            "write the summary",
            &[MAC_A],
            60,
            ContinuityUpdate::default(),
        )
        .await;
    let (_, body) = h.resume_now().await;
    assert_eq!(body["suggestion"]["record_id"], other.id);
    let (status, policy) = h
        .json(
            Method::PUT,
            &api("resume"),
            Some(serde_json::json!({ "enabled": false, "break_minutes": 120, "cooldown_secs": 0 })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{policy}");
    assert!(h.status().await["suggestion"].is_null());
    assert_eq!(
        resume_events(&mut rx),
        vec![Some(task.id.clone()), Some(other.id.clone()), None]
    );
}
