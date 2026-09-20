//! Router-level tests for the Resume my work ritual (#83).
//!
//! Included from `app/router.rs`, so every request goes through
//! `build_router` with the real auth and companion middleware. Desktops
//! register through the registry exactly as the WebSocket route does; their
//! toolcall channels stay empty throughout, which is how these tests prove
//! that a suggestion never touches a computer. Records are written the way
//! explicit task activity writes them; the ritual only reads them.

use super::*;
use crate::{
    config::{Config, LlmProvider, ModelPreset},
    domain::{
        companion::CANONICAL_SLUG,
        continuity::{
            ContinuityRecord, ContinuityUpdate, Origin, Priority, Provenance, ProvenanceSource,
        },
        events::ServerEvent,
        handoff::{COMPUTER_USE_CAPABILITIES, FILE_CAPABILITIES},
        proactive::{ProactivePolicy, QuietHours},
    },
    services::{
        companion,
        continuity::ContinuityStore,
        machine_registry::MachineInfo,
        resume_ritual::{RITUAL_FILE, ResumeRitual},
    },
};
use axum::{
    body::Body,
    http::{Method, Request, StatusCode, header},
};
use chrono::Timelike;
use cua_protocol::{MachineLocation, Permission, PermissionState, Platform};
use std::fs;
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

fn granted() -> Option<PermissionState> {
    Some(PermissionState {
        accessibility: Permission::Granted,
        screen_capture: Permission::Granted,
    })
}

fn full_capabilities() -> Vec<String> {
    COMPUTER_USE_CAPABILITIES
        .iter()
        .chain(FILE_CAPABILITIES.iter())
        .chain(["bash", "right_click", "scroll"].iter())
        .map(|s| (*s).to_owned())
        .collect()
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

impl Harness {
    async fn connect_ready(&self, machine_id: &str, hostname: &str) -> Desktop {
        let (tx, calls) = tokio::sync::mpsc::unbounded_channel();
        self.state
            .machine_registry
            .register(
                MachineInfo {
                    machine_id: machine_id.into(),
                    os: "macos".into(),
                    hostname: hostname.into(),
                    screen_width: 2560,
                    screen_height: 1440,
                    last_seen: now(),
                    instance_slug: None,
                    platform: Some(Platform::Macos),
                    location: MachineLocation::Desktop,
                    permissions: granted(),
                    capabilities: full_capabilities(),
                },
                tx,
            )
            .await;
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
                due_at: Some(now() + 3 * 3_600),
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
async fn a_reconnect_offers_only_work_naming_that_computer_and_never_touches_it() {
    let h = harness().await;
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

    // A computer no waiting work names connects: nothing is offered.
    let mut laptop = h.connect_ready(MAC_B, "laptop").await;
    crate::routes::machine_agents::on_machine_connected(&h.state, MAC_B, Some(CANONICAL_SLUG))
        .await;
    assert!(h.status().await["suggestion"].is_null());

    // The computer the task names reconnects: exactly that task is offered.
    let mut studio = h.connect_ready(MAC_A, "studio").await;
    crate::routes::machine_agents::on_machine_connected(&h.state, MAC_A, Some(CANONICAL_SLUG))
        .await;
    let status = h.status().await;
    let suggestion = &status["suggestion"];
    assert_eq!(suggestion["record_id"], on_a.id, "{status}");
    assert_eq!(suggestion["trigger"]["kind"], "machine_connected");
    assert_eq!(suggestion["trigger"]["machine_id"], MAC_A);
    assert_eq!(
        suggestion["why_now"],
        "studio reconnected, and this task names it."
    );
    assert_eq!(suggestion["destination_id"], MAC_A);
    assert_eq!(suggestion["card"]["origin"]["online"], true);

    studio.assert_untouched("a reconnect");
    laptop.assert_untouched("a reconnect");
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
    h.connect_ready(MAC_A, "studio").await;
    h.enable(30, 0).await;
    let task = h
        .task(
            "sort the receipts",
            &[MAC_A],
            300,
            ContinuityUpdate::default(),
        )
        .await;

    // Quiet hours around this very hour, in the companion's timezone (UTC by default).
    let hour = chrono::Utc::now().hour() as u8;
    h.state
        .proactive
        .set_policy(&ProactivePolicy {
            quiet_hours: Some(QuietHours {
                start_hour: hour,
                end_hour: (hour + 1) % 24,
            }),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(h.status().await["quiet_hours_active"], true);

    h.last_opened(3_600).await;
    assert_eq!(held(&h.opened().await), "quiet_hours");
    crate::routes::machine_agents::on_machine_connected(&h.state, MAC_A, Some(CANONICAL_SLUG))
        .await;
    assert!(h.status().await["suggestion"].is_null());

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
