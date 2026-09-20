//! Router-level tests for reviewable handoffs (#82).
//!
//! Included from `app/router.rs`. Every request goes through `build_router`,
//! so the real auth and companion middleware run. Two fake desktops register
//! through the registry exactly as the WebSocket route does; their toolcall
//! channels stay empty throughout, which is how these tests prove that
//! nothing acts on a computer before, or at, the user's acceptance. The
//! task's conversation is simulated as one that is already running: the
//! acceptance queues the handoff there, and the test plays the conversation
//! by appending the trace and releasing the agent task, exactly what the
//! chat loop does when it stops.

use super::*;
use crate::{
    config::{Config, LlmProvider, ModelPreset},
    domain::{
        companion::CANONICAL_SLUG,
        continuity::{
            ContinuityRecord, ContinuityUpdate, HandoffDecision, Origin, Provenance,
            ProvenanceSource, ResourceRef,
        },
        events::ServerEvent,
        handoff::{COMPUTER_USE_CAPABILITIES, FILE_CAPABILITIES},
        proactive::{ProactivePolicy, RunStatus, Trigger},
    },
    services::{
        chat, companion,
        continuity::ContinuityStore,
        llm::{ContentBlock, HistoryEntry, Message},
        machine_registry::MachineInfo,
    },
};
use axum::{
    body::Body,
    http::{Method, Request, StatusCode, header},
};
use cua_protocol::{MachineLocation, Permission, PermissionState, Platform};
use std::{fs, time::Duration};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

const TOKEN: &str = "issue-82-handoff-token";
const MAX_BODY: usize = 64 * 1024 * 1024;
const MAC_A: &str = "4f3c1c2e-9b5e-4d2b-8f0a-1c2d3e4f5a6b";
const MAC_B: &str = "7e2d0b1a-3c4d-4e5f-9a8b-0c1d2e3f4a5b";
const CHAT: &str = "chat_1";

struct Harness {
    workspace: tempfile::TempDir,
    state: AppState,
}

/// A companion with a chat model configured, so continuation is possible.
/// Nothing here talks to a provider: the conversation is simulated.
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

async fn harness_with(config: Config) -> Harness {
    let workspace = tempfile::tempdir().unwrap();
    let state = AppState::new_in(config, workspace.path().to_owned()).await;
    companion::ensure_identity(workspace.path()).unwrap();
    Harness { workspace, state }
}

async fn harness() -> Harness {
    harness_with(configured()).await
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

/// A desktop connected through the registry: its connection number and the
/// channel every toolcall to it would arrive on.
struct Desktop {
    connection: u64,
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
    async fn connect(
        &self,
        machine_id: &str,
        hostname: &str,
        permissions: Option<PermissionState>,
        capabilities: Vec<String>,
        last_seen: i64,
    ) -> Desktop {
        let (tx, calls) = tokio::sync::mpsc::unbounded_channel();
        let connection = self
            .state
            .machine_registry
            .register(
                MachineInfo {
                    machine_id: machine_id.into(),
                    os: "macos".into(),
                    hostname: hostname.into(),
                    screen_width: 2560,
                    screen_height: 1440,
                    last_seen,
                    instance_slug: None,
                    platform: Some(Platform::Macos),
                    location: MachineLocation::Desktop,
                    permissions,
                    capabilities,
                },
                tx,
            )
            .await;
        Desktop { connection, calls }
    }

    async fn connect_ready(&self, machine_id: &str, hostname: &str) -> Desktop {
        self.connect(machine_id, hostname, granted(), full_capabilities(), now())
            .await
    }

    async fn disconnect(&self, machine_id: &str, desktop: &Desktop) {
        self.state
            .machine_registry
            .unregister_connection(machine_id, desktop.connection)
            .await;
    }

    fn store(&self) -> ContinuityStore {
        ContinuityStore::new(self.workspace.path(), CANONICAL_SLUG)
    }

    fn memory_note(&self, path: &str) {
        let file = companion::companion_dir(self.workspace.path())
            .join("memory")
            .join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, "- the trip").unwrap();
    }

    /// A task started on `MAC_A` in `CHAT`, with one memory note and one
    /// file on `MAC_A`, one finished step, a stated blocker, and a next step.
    async fn task(&self, resources: Vec<ResourceRef>) -> ContinuityRecord {
        self.memory_note("notes/trip.md");
        self.store()
            .create(
                "rename the trip photos",
                Origin {
                    chat_id: CHAT.into(),
                    message_id: Some("msg_1".into()),
                },
                &ContinuityUpdate {
                    completed_step: Some("listed the folder".into()),
                    blocker: Some("needs the external drive".into()),
                    next_step: Some("rename IMG_* files".into()),
                    machine_ids: vec![MAC_A.into()],
                    resources,
                    ..Default::default()
                },
                Provenance {
                    source: ProvenanceSource::Chat,
                    at: now() - 600,
                    note: "asked in chat".into(),
                },
                now() - 600,
            )
            .await
            .unwrap()
    }

    fn usual_resources() -> Vec<ResourceRef> {
        vec![
            ResourceRef::Memory {
                path: "notes/trip.md".into(),
            },
            ResourceRef::MachinePath {
                machine_id: MAC_A.into(),
                path: "/Volumes/Trip".into(),
            },
        ]
    }

    /// Pretend the task's conversation is running (a chat loop is active),
    /// so an acceptance queues the handoff instead of starting a model turn.
    async fn conversation_running(&self, chat_id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        self.state
            .agent_tasks
            .lock()
            .await
            .insert(format!("{CANONICAL_SLUG}/{chat_id}"), token.clone());
        token
    }

    /// The conversation stops: exactly what the chat loop does last.
    async fn conversation_stopped(&self, chat_id: &str) {
        self.state
            .agent_tasks
            .lock()
            .await
            .remove(&format!("{CANONICAL_SLUG}/{chat_id}"));
    }

    fn history(&self, chat_id: &str) -> Vec<HistoryEntry> {
        chat::load_rig_history(&chat::rig_history_path(
            self.workspace.path(),
            CANONICAL_SLUG,
            chat_id,
        ))
        .unwrap_or_default()
    }

    fn handoff_messages(&self, chat_id: &str) -> Vec<String> {
        self.history(chat_id)
            .into_iter()
            .filter_map(|entry| match entry.message {
                Message::User { content } => content.into_iter().find_map(|block| match block {
                    ContentBlock::Text { text } if text.starts_with("[handoff]") => Some(text),
                    _ => None,
                }),
                Message::Assistant { .. } => None,
            })
            .collect()
    }

    /// The conversation did one computer action and replied.
    fn play_conversation(&self, chat_id: &str, machine_id: &str) {
        let path = chat::rig_history_path(self.workspace.path(), CANONICAL_SLUG, chat_id);
        chat::append_to_rig_history(
            &path,
            &HistoryEntry::new(
                Message::Assistant {
                    content: vec![ContentBlock::ToolCall {
                        id: "call_1".into(),
                        name: "computer_use".into(),
                        arguments: serde_json::json!({"machine_id": machine_id, "action": "screenshot"}),
                    }],
                },
                "1".into(),
                "msg_play_1".into(),
            ),
        );
        chat::append_to_rig_history(
            &path,
            &HistoryEntry::new(
                Message::assistant("renamed the files."),
                "2".into(),
                "msg_play_2".into(),
            ),
        );
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

    async fn card(&self, id: &str) -> serde_json::Value {
        let (status, card) = self
            .json(Method::GET, &api(&format!("continuity/{id}/handoff")), None)
            .await;
        assert_eq!(status, StatusCode::OK, "{card}");
        card
    }

    async fn preview(&self, id: &str, machine_id: &str) -> (StatusCode, serde_json::Value) {
        self.json(
            Method::GET,
            &api(&format!(
                "continuity/{id}/handoff/preview?machine_id={machine_id}"
            )),
            None,
        )
        .await
    }

    async fn accept(&self, id: &str, machine_id: &str) -> (StatusCode, serde_json::Value) {
        self.json(
            Method::POST,
            &api(&format!("continuity/{id}/handoff/accept")),
            Some(serde_json::json!({ "machine_id": machine_id })),
        )
        .await
    }

    async fn activity(&self) -> Vec<serde_json::Value> {
        let (status, runs) = self.json(Method::GET, &api("activity"), None).await;
        assert_eq!(status, StatusCode::OK);
        runs.as_array().unwrap().clone()
    }

    /// Wait until the card reports a finished continuation.
    async fn wait_for_outcome(&self, id: &str) -> serde_json::Value {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let card = self.card(id).await;
            if !card["decision"]["outcome"].is_null() {
                return card;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "the continuation never finished: {card}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

fn check_kinds(body: &serde_json::Value) -> Vec<String> {
    body["checks"]
        .as_array()
        .unwrap_or_else(|| panic!("no checks in {body}"))
        .iter()
        .map(|check| {
            format!(
                "{}:{}",
                check["severity"].as_str().unwrap(),
                check["kind"]["kind"].as_str().unwrap()
            )
        })
        .collect()
}

#[tokio::test]
async fn a_task_started_on_one_computer_is_reviewed_and_continued_on_another() {
    let h = harness().await;
    let mut a = h.connect_ready(MAC_A, "studio").await;
    let mut b = h.connect_ready(MAC_B, "laptop").await;
    let (_, _) = h
        .json(
            Method::PUT,
            &api(&format!("machines/{MAC_B}")),
            Some(serde_json::json!({ "display_name": "Travel Laptop" })),
        )
        .await;
    let task = h.task(Harness::usual_resources()).await;
    let mut events = h.state.events.subscribe();

    // Review: the card is derived from the record and the machine list.
    let (status, listing) = h.json(Method::GET, &api("handoffs"), None).await;
    assert_eq!(status, StatusCode::OK, "{listing}");
    assert_eq!(listing["errors"], serde_json::json!([]));
    let cards = listing["handoffs"].as_array().unwrap();
    assert_eq!(cards.len(), 1);
    let card = &cards[0];
    assert_eq!(card["record_id"], task.id);
    assert_eq!(card["goal"], "rename the trip photos");
    assert_eq!(card["origin_chat_id"], CHAT);
    assert_eq!(card["origin"]["machine_id"], MAC_A);
    assert_eq!(card["origin"]["display_name"], "studio");
    assert_eq!(card["origin"]["online"], true);
    assert_eq!(
        card["completed_steps"],
        serde_json::json!(["listed the folder"])
    );
    assert_eq!(
        card["blockers"],
        serde_json::json!(["needs the external drive"])
    );
    assert_eq!(card["next_step"], "rename IMG_* files");
    assert_eq!(card["resources"][0]["label"], "memory notes/trip.md");
    assert_eq!(card["resources"][0]["available"], true);
    assert_eq!(
        card["resources"][1]["label"],
        format!("/Volumes/Trip on {MAC_A}")
    );
    assert_eq!(
        card["required"]["permissions"],
        serde_json::json!(["screen_capture", "accessibility"])
    );
    assert!(
        card["required"]["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c == "file_read")
    );
    assert_eq!(card["decision"], serde_json::Value::Null);
    assert_eq!(card["offered"], true);

    // Preview the destination: ready, with the origin's file noted, and
    // nothing started.
    let (status, preview) = h.preview(&task.id, MAC_B).await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["ready"], true, "{preview}");
    assert_eq!(preview["destination"]["machine_id"], MAC_B);
    assert_eq!(preview["destination"]["display_name"], "Travel Laptop");
    assert_eq!(preview["destination"]["online"], true);
    assert_eq!(check_kinds(&preview), vec!["note:resource_elsewhere"]);
    assert!(h.activity().await.is_empty(), "a preview admits no run");
    assert!(h.handoff_messages(CHAT).is_empty());
    a.assert_untouched("preview");
    b.assert_untouched("preview");

    // Accept: bound to B's stable id, one run in the trail, the task handed
    // to its conversation naming B, and still no action on either desktop.
    let token = h.conversation_running(CHAT).await;
    let (status, accepted) = h.accept(&task.id, MAC_B).await;
    assert_eq!(status, StatusCode::OK, "{accepted}");
    assert_eq!(accepted["already_running"], false);
    let run = &accepted["run"];
    assert_eq!(
        run["trigger"],
        serde_json::json!({"kind": "handoff", "handoff_id": task.id})
    );
    assert_eq!(
        run["target"],
        serde_json::json!({"kind": "machine", "machine_id": MAC_B})
    );
    assert_eq!(run["status"]["kind"], "running");
    assert!(
        run["reason"].as_str().unwrap().contains("Travel Laptop"),
        "{run}"
    );
    let run_id = run["id"].as_str().unwrap().to_owned();
    let card = &accepted["card"];
    assert_eq!(card["decision"]["kind"], "accepted");
    assert_eq!(card["decision"]["machine_id"], MAC_B);
    assert_eq!(card["decision"]["run_id"], run_id);
    assert_eq!(card["bound_to"]["display_name"], "Travel Laptop");
    let bound = h.store().get(&task.id).unwrap();
    assert_eq!(bound.machine_ids, vec![MAC_A, MAC_B]);
    assert!(
        matches!(bound.handoff, Some(HandoffDecision::Accepted { ref machine_id, .. }) if machine_id == MAC_B)
    );
    let last = bound.provenance.last().unwrap();
    assert_eq!(last.source, ProvenanceSource::User);
    assert!(last.note.contains("Travel Laptop"), "{}", last.note);
    let messages = h.handoff_messages(CHAT);
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains(MAC_B) && messages[0].contains("rename the trip photos"));
    assert!(messages[0].contains(&task.id));
    let runs = h.activity().await;
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["id"], run_id);
    a.assert_untouched("accept");
    b.assert_untouched("accept");
    assert!(
        !token.is_cancelled(),
        "the running conversation is not interrupted"
    );
    let mut saw_card_event = false;
    while let Ok(event) = events.try_recv() {
        if let ServerEvent::HandoffUpdated { card, .. } = event {
            saw_card_event = true;
            assert_eq!(card.record_id, task.id);
        }
    }
    assert!(saw_card_event, "clients hear handoff_updated on acceptance");

    // The conversation does the work and stops; the receipts land on the
    // run and the outcome on the record, one trail.
    h.play_conversation(CHAT, MAC_B);
    h.conversation_stopped(CHAT).await;
    let card = h.wait_for_outcome(&task.id).await;
    assert_eq!(card["decision"]["outcome"]["status"], "completed");
    assert_eq!(card["decision"]["outcome"]["summary"], "1 action");
    let (status, run) = h
        .json(Method::GET, &api(&format!("activity/{run_id}")), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(run["status"]["kind"], "completed");
    assert_eq!(run["outcome"]["actions"].as_array().unwrap().len(), 1);
    assert_eq!(run["outcome"]["actions"][0]["tool"], "computer_use");
    let finished = h.store().get(&task.id).unwrap();
    let last = finished.provenance.last().unwrap();
    assert_eq!(last.source, ProvenanceSource::Server);
    assert!(
        last.note.contains("Travel Laptop") && last.note.contains("1 action"),
        "{}",
        last.note
    );
    assert_eq!(
        finished.state,
        crate::domain::continuity::ContinuityState::Active
    );
    assert_eq!(
        h.card(&task.id).await["offered"],
        true,
        "a finished continuation is offered again"
    );
}

#[tokio::test]
async fn accepting_twice_returns_the_same_run_and_starts_nothing_new() {
    let h = harness().await;
    let _a = h.connect_ready(MAC_A, "studio").await;
    let _b = h.connect_ready(MAC_B, "laptop").await;
    let task = h.task(Harness::usual_resources()).await;
    let _token = h.conversation_running(CHAT).await;

    let (status, first) = h.accept(&task.id, MAC_B).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let (status, second) = h.accept(&task.id, MAC_B).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["already_running"], true);
    assert_eq!(second["run"]["id"], first["run"]["id"]);
    // Even a different destination cannot start a second continuation while
    // one is running.
    let (status, elsewhere) = h.accept(&task.id, MAC_A).await;
    assert_eq!(status, StatusCode::OK, "{elsewhere}");
    assert_eq!(elsewhere["already_running"], true);
    assert_eq!(elsewhere["run"]["id"], first["run"]["id"]);
    assert_eq!(elsewhere["card"]["decision"]["machine_id"], MAC_B);

    assert_eq!(h.activity().await.len(), 1, "one run in the trail");
    assert_eq!(
        h.handoff_messages(CHAT).len(),
        1,
        "one request in the conversation"
    );
    let record = h.store().get(&task.id).unwrap();
    assert_eq!(
        record
            .provenance
            .iter()
            .filter(|p| p.source == ProvenanceSource::User)
            .count(),
        1,
        "one acceptance in the provenance"
    );

    // Once the run is over, accepting again is new explicit work: a new run.
    h.play_conversation(CHAT, MAC_B);
    h.conversation_stopped(CHAT).await;
    h.wait_for_outcome(&task.id).await;
    let _token = h.conversation_running(CHAT).await;
    let (status, third) = h.accept(&task.id, MAC_B).await;
    assert_eq!(status, StatusCode::OK, "{third}");
    assert_eq!(third["already_running"], false);
    assert_ne!(third["run"]["id"], first["run"]["id"]);
    assert_eq!(h.activity().await.len(), 2);
}

#[tokio::test]
async fn the_card_stays_useful_while_the_origin_is_offline_and_follows_a_reconnect() {
    let h = harness().await;
    let a = h.connect_ready(MAC_A, "studio").await;
    let task = h.task(Harness::usual_resources()).await;
    h.disconnect(MAC_A, &a).await;

    // Offline origin: the card still carries everything, and says so.
    let (status, listing) = h.json(Method::GET, &api("handoffs"), None).await;
    assert_eq!(status, StatusCode::OK);
    let card = &listing["handoffs"][0];
    assert_eq!(card["origin"]["machine_id"], MAC_A);
    assert_eq!(card["origin"]["display_name"], "studio");
    assert_eq!(card["origin"]["known"], true);
    assert_eq!(card["origin"]["online"], false);
    assert_eq!(card["origin"]["health"], "unavailable");
    assert!(card["origin"]["last_seen"].as_i64().is_some());
    assert_eq!(
        card["completed_steps"],
        serde_json::json!(["listed the folder"])
    );
    assert_eq!(card["next_step"], "rename IMG_* files");
    assert_eq!(card["offered"], true);
    let blockers = card["blockers"].as_array().unwrap();
    assert!(
        blockers
            .iter()
            .any(|b| b.as_str().unwrap().contains("not connected")),
        "the reference check's blocker is shown: {blockers:?}"
    );
    assert_eq!(
        card["resources"][1]["available"], false,
        "the file on the offline computer"
    );

    // Continuing on the offline origin is refused with the reason.
    let (status, preview) = h.preview(&task.id, MAC_A).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(preview["ready"], false);
    assert!(
        check_kinds(&preview).contains(&"blocking:machine_offline".to_owned()),
        "{preview}"
    );
    let (status, refused) = h.accept(&task.id, MAC_A).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["error"], "handoff_not_ready");
    assert!(h.activity().await.is_empty());

    // Another computer can continue while A is offline, but A's file is a
    // stop, not a silent gap.
    let _b = h.connect_ready(MAC_B, "laptop").await;
    let (_, preview) = h.preview(&task.id, MAC_B).await;
    assert_eq!(preview["ready"], false);
    assert!(
        check_kinds(&preview).contains(&"blocking:resource_elsewhere".to_owned()),
        "{preview}"
    );

    // A reconnects under the same stable id: the same card, ready again.
    let mut a = h.connect_ready(MAC_A, "studio").await;
    let (_, preview) = h.preview(&task.id, MAC_A).await;
    assert_eq!(preview["ready"], true, "{preview}");
    assert_eq!(preview["card"]["origin"]["online"], true);
    let (_, preview) = h.preview(&task.id, MAC_B).await;
    assert_eq!(preview["ready"], true, "{preview}");
    let (status, listing) = h.json(Method::GET, &api("handoffs"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing["handoffs"].as_array().unwrap().len(), 1);
    assert_eq!(listing["handoffs"][0]["resources"][1]["available"], true);
    a.assert_untouched("reconnect");
}

#[tokio::test]
async fn stale_and_unfit_destinations_are_refused_with_reasons_before_any_work() {
    let h = harness().await;
    let _a = h.connect_ready(MAC_A, "studio").await;
    // B is connected but its heartbeat is stale.
    let mut b = h
        .connect(MAC_B, "laptop", granted(), full_capabilities(), now() - 120)
        .await;
    let task = h.task(Harness::usual_resources()).await;

    let (_, preview) = h.preview(&task.id, MAC_B).await;
    assert_eq!(preview["ready"], false);
    assert!(
        check_kinds(&preview).contains(&"blocking:machine_not_responding".to_owned()),
        "{preview}"
    );
    let (status, refused) = h.accept(&task.id, MAC_B).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["error"], "handoff_not_ready");
    assert!(check_kinds(&refused).contains(&"blocking:machine_not_responding".to_owned()));
    assert!(!refused["message"].as_str().unwrap().is_empty());

    // An unknown computer.
    let (status, refused) = h.accept(&task.id, "ghost-machine").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(check_kinds(&refused), vec!["blocking:machine_unknown"]);

    // A computer that cannot do what the task needs, and denies a permission.
    let denied = Some(PermissionState {
        accessibility: Permission::Granted,
        screen_capture: Permission::Denied,
    });
    let _c = h
        .connect(
            "mac-c",
            "old-mini",
            denied,
            vec!["screenshot".into(), "bash".into()],
            now(),
        )
        .await;
    let (_, preview) = h.preview(&task.id, "mac-c").await;
    let kinds = check_kinds(&preview);
    assert!(
        kinds.contains(&"blocking:capability_missing".to_owned()),
        "{kinds:?}"
    );
    assert!(
        kinds.contains(&"blocking:permission_denied".to_owned()),
        "{kinds:?}"
    );
    let details: Vec<&str> = preview["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["detail"].as_str().unwrap())
        .collect();
    assert!(
        details.iter().any(|d| d.contains("Screen Recording")),
        "{details:?}"
    );
    assert!(
        details.iter().any(|d| d.contains("left_click")),
        "{details:?}"
    );

    // A computer whose desktop will ask: shown as needing approval, not a stop.
    let prompting = Some(PermissionState {
        accessibility: Permission::PromptRequired,
        screen_capture: Permission::Granted,
    });
    let _d = h
        .connect("mac-d", "spare", prompting, full_capabilities(), now())
        .await;
    let (_, preview) = h.preview(&task.id, "mac-d").await;
    assert_eq!(preview["ready"], true, "{preview}");
    assert!(check_kinds(&preview).contains(&"approval:permission_prompt".to_owned()));

    // A missing upload is reported, never skipped.
    let missing = h
        .task(vec![ResourceRef::Upload {
            id: "upload_gone".into(),
        }])
        .await;
    let (_, preview) = h.preview(&missing.id, MAC_A).await;
    assert_eq!(preview["ready"], false);
    assert!(
        check_kinds(&preview).contains(&"blocking:resource_missing".to_owned()),
        "{preview}"
    );
    let (status, _) = h.accept(&missing.id, MAC_A).await;
    assert_eq!(status, StatusCode::CONFLICT);

    // A closed task, an unknown task, and a foreign companion.
    h.store()
        .complete(
            &missing.id,
            Provenance {
                source: ProvenanceSource::User,
                at: now(),
                note: "done".into(),
            },
            now(),
        )
        .await
        .unwrap();
    let (status, refused) = h.accept(&missing.id, MAC_A).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(check_kinds(&refused).contains(&"blocking:record_closed".to_owned()));
    let (status, listing) = h.json(Method::GET, &api("handoffs"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        listing["handoffs"].as_array().unwrap().len(),
        1,
        "closed tasks are not offered"
    );
    let (status, _) = h
        .send(Method::GET, &api("continuity/task_missing/handoff"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = h.accept("task_missing", MAC_A).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, body) = h
        .json(
            Method::POST,
            &format!("/api/instances/alice/continuity/{}/handoff/accept", task.id),
            Some(serde_json::json!({ "machine_id": MAC_A })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "unknown_companion");
    let (status, _) = h
        .send(Method::GET, "/api/instances/alice/handoffs", None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Through all of it nothing started.
    assert!(h.activity().await.is_empty());
    assert!(h.handoff_messages(CHAT).is_empty());
    assert!(h.state.agent_tasks.lock().await.is_empty());
    b.assert_untouched("refusals");
    for record in [&task, &missing] {
        assert_eq!(h.store().get(&record.id).unwrap().handoff, None);
    }
}

#[tokio::test]
async fn a_missing_model_or_initiative_off_is_a_stated_reason_not_a_silent_stop() {
    let h = harness_with(Config {
        auth_token: TOKEN.into(),
        ..Default::default()
    })
    .await;
    let _a = h.connect_ready(MAC_A, "studio").await;
    let task = h.task(Harness::usual_resources()).await;
    let (_, preview) = h.preview(&task.id, MAC_A).await;
    assert_eq!(preview["ready"], false);
    assert_eq!(check_kinds(&preview), vec!["blocking:model_unavailable"]);
    let (status, _) = h.accept(&task.id, MAC_A).await;
    assert_eq!(status, StatusCode::CONFLICT);

    let h = harness().await;
    let _a = h.connect_ready(MAC_A, "studio").await;
    let task = h.task(Harness::usual_resources()).await;
    h.state
        .proactive
        .set_policy(&ProactivePolicy {
            enabled: false,
            ..Default::default()
        })
        .unwrap();
    let (_, preview) = h.preview(&task.id, MAC_A).await;
    assert_eq!(check_kinds(&preview), vec!["blocking:initiative_off"]);
    let (status, refused) = h.accept(&task.id, MAC_A).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(refused["error"], "handoff_not_ready");
    assert!(h.activity().await.is_empty());
}

#[tokio::test]
async fn keep_there_and_dismiss_hide_the_card_until_explicit_work_updates_the_record() {
    let h = harness().await;
    let a = h.connect_ready(MAC_A, "studio").await;
    let task = h.task(Harness::usual_resources()).await;

    // Dismiss: hidden from the handoffs, still a resumable record.
    let (status, card) = h
        .json(
            Method::POST,
            &api(&format!("continuity/{}/handoff/dismiss", task.id)),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{card}");
    assert_eq!(card["offered"], false);
    assert_eq!(card["decision"]["kind"], "dismissed");
    let (_, listing) = h.json(Method::GET, &api("handoffs"), None).await;
    assert_eq!(listing["handoffs"], serde_json::json!([]));
    let (_, resumable) = h
        .json(Method::GET, &api("continuity?resumable=true"), None)
        .await;
    assert_eq!(resumable["records"].as_array().unwrap().len(), 1);
    assert_eq!(resumable["records"][0]["state"], "active");
    assert_eq!(h.card(&task.id).await["offered"], false);
    assert_eq!(
        h.store()
            .get(&task.id)
            .unwrap()
            .provenance
            .last()
            .unwrap()
            .note,
        "handoff dismissed"
    );

    // A reference check (the origin goes offline) is not explicit work.
    h.disconnect(MAC_A, &a).await;
    let (status, record) = h
        .json(Method::GET, &api(&format!("continuity/{}", task.id)), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        record["blockers"].as_array().unwrap().len() >= 2,
        "{record}"
    );
    let (_, listing) = h.json(Method::GET, &api("handoffs"), None).await;
    assert_eq!(listing["handoffs"], serde_json::json!([]), "still hidden");

    // Explicit work brings it back.
    let (status, _) = h
        .json(
            Method::PUT,
            &api(&format!("continuity/{}", task.id)),
            Some(serde_json::json!({ "completed_step": "found the drive", "note": "progress" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (_, listing) = h.json(Method::GET, &api("handoffs"), None).await;
    assert_eq!(listing["handoffs"].as_array().unwrap().len(), 1);
    assert_eq!(listing["handoffs"][0]["decision"], serde_json::Value::Null);

    // Keep there: bound to the origin, hidden the same way.
    let (status, card) = h
        .json(
            Method::POST,
            &api(&format!("continuity/{}/handoff/keep", task.id)),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{card}");
    assert_eq!(card["offered"], false);
    assert_eq!(card["decision"]["kind"], "kept");
    assert_eq!(card["decision"]["machine_id"], MAC_A);
    let kept = h.store().get(&task.id).unwrap();
    assert!(
        kept.provenance.last().unwrap().note.contains("studio"),
        "{:?}",
        kept.provenance.last()
    );
    let (_, listing) = h.json(Method::GET, &api("handoffs"), None).await;
    assert_eq!(listing["handoffs"], serde_json::json!([]));
    let (status, _) = h
        .json(
            Method::PUT,
            &api(&format!("continuity/{}", task.id)),
            Some(serde_json::json!({ "next_step": "rename them", "note": "edited" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (_, listing) = h.json(Method::GET, &api("handoffs"), None).await;
    assert_eq!(listing["handoffs"].as_array().unwrap().len(), 1);

    // Neither decision started anything.
    assert!(h.activity().await.is_empty());
    assert!(h.handoff_messages(CHAT).is_empty());
    let (status, _) = h
        .send(
            Method::POST,
            &api("continuity/task_missing/handoff/keep"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_cancelled_or_interrupted_continuation_is_closed_on_the_trail_and_can_be_retried() {
    let h = harness().await;
    let _a = h.connect_ready(MAC_A, "studio").await;
    let b = h.connect_ready(MAC_B, "laptop").await;
    let task = h.task(Harness::usual_resources()).await;

    // Cancel from the activity view: the conversation is stopped, the run
    // and the record both say cancelled.
    let token = h.conversation_running(CHAT).await;
    let (status, accepted) = h.accept(&task.id, MAC_B).await;
    assert_eq!(status, StatusCode::OK, "{accepted}");
    let run_id = accepted["run"]["id"].as_str().unwrap().to_owned();
    let (status, _) = h
        .send(
            Method::POST,
            &api(&format!("activity/{run_id}/cancel")),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !token.is_cancelled() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the conversation was not stopped"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    h.conversation_stopped(CHAT).await;
    let card = h.wait_for_outcome(&task.id).await;
    assert_eq!(card["decision"]["outcome"]["status"], "cancelled");
    let (_, run) = h
        .json(Method::GET, &api(&format!("activity/{run_id}")), None)
        .await;
    assert_eq!(run["status"]["kind"], "cancelled");

    // Retry from the activity view re-runs the checks and continues on the
    // bound computer as a linked attempt.
    let _token = h.conversation_running(CHAT).await;
    let (status, retried) = h
        .json(
            Method::POST,
            &api(&format!("activity/{run_id}/retry")),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{retried}");
    assert_eq!(retried["retry_of"], run_id);
    assert_eq!(retried["attempt"], 2);
    assert_eq!(retried["target"]["machine_id"], MAC_B);
    let retry_id = retried["id"].as_str().unwrap().to_owned();
    let card = h.card(&task.id).await;
    assert_eq!(card["decision"]["run_id"], retry_id);
    assert_eq!(card["decision"]["outcome"], serde_json::Value::Null);
    assert_eq!(h.handoff_messages(CHAT).len(), 2);

    // A conversation that stops without ever taking the task up is a
    // retryable failure, stated on both records.
    h.conversation_stopped(CHAT).await;
    let card = h.wait_for_outcome(&task.id).await;
    assert_eq!(card["decision"]["outcome"]["status"], "failed");
    let (_, run) = h
        .json(Method::GET, &api(&format!("activity/{retry_id}")), None)
        .await;
    assert_eq!(run["status"]["kind"], "failed");
    assert_eq!(run["status"]["retryable"], true);

    // A retry is refused with the reason when the computer is gone.
    h.disconnect(MAC_B, &b).await;
    let (status, refused) = h
        .json(
            Method::POST,
            &api(&format!("activity/{retry_id}/retry")),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert!(
        h.state
            .proactive
            .running(&Trigger::Handoff {
                handoff_id: task.id.clone()
            })
            .is_none()
    );

    // A restart interrupts a running continuation; the record is still bound
    // and offered, so the user can pick it up again.
    let _token = h.conversation_running(CHAT).await;
    let _c = h.connect_ready(MAC_B, "laptop").await;
    let (status, accepted) = h.accept(&task.id, MAC_B).await;
    assert_eq!(status, StatusCode::OK, "{accepted}");
    let run_id = accepted["run"]["id"].as_str().unwrap().to_owned();
    assert_eq!(h.state.proactive.recover_on_restart(now()), 1);
    let (_, run) = h
        .json(Method::GET, &api(&format!("activity/{run_id}")), None)
        .await;
    assert!(matches!(
        serde_json::from_value::<RunStatus>(run["status"].clone()).unwrap(),
        RunStatus::Failed { .. }
    ));
    let card = h.card(&task.id).await;
    assert_eq!(card["offered"], true);
    assert_eq!(card["decision"]["run_id"], run_id);
    let (_, preview) = h.preview(&task.id, MAC_B).await;
    assert_eq!(preview["ready"], true, "{preview}");
}
