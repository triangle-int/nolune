//! Router-level tests for the one-companion storage boundary (#103).
//!
//! Included from `app/router.rs`. Every request goes through `build_router`,
//! so these tests exercise the real auth and companion middleware stack.

use super::*;
use crate::{
    domain::companion::{CANONICAL_SLUG, IDENTITY_FILE, STORAGE_FORMAT_VERSION},
    services::{companion, machine_registry::MachineInfo},
};
use axum::{
    body::Body,
    http::{Method, Request, StatusCode, header},
};
use std::{fs, path::PathBuf, sync::Arc};
use tower::ServiceExt;

const TOKEN: &str = "issue-103-control-token";
const MAX_BODY: usize = 64 * 1024 * 1024;

struct Harness {
    workspace: tempfile::TempDir,
    state: AppState,
}

async fn harness() -> Harness {
    let workspace = tempfile::tempdir().unwrap();
    let config = crate::config::Config {
        auth_token: TOKEN.into(),
        ..Default::default()
    };
    let mut state = AppState::new(config).await;
    state.workspace_dir = workspace.path().to_owned();
    state.vector_store =
        Arc::new(crate::services::vector::VectorStore::connect(workspace.path()).await);
    state.proactive =
        crate::services::proactive::ProactiveLoop::new(workspace.path(), CANONICAL_SLUG)
            .with_events(state.events.clone());
    state.machine_registry =
        crate::services::machine_registry::MachineRegistry::open(workspace.path(), CANONICAL_SLUG)
            .with_events(state.events.clone());
    Harness { workspace, state }
}

impl Harness {
    fn instances(&self) -> PathBuf {
        self.workspace.path().join("instances")
    }

    fn companion(&self) -> PathBuf {
        self.instances().join(CANONICAL_SLUG)
    }

    fn instance_dirs(&self) -> Vec<String> {
        let Ok(entries) = fs::read_dir(self.instances()) else {
            return Vec::new();
        };
        let mut names = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_dir())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    fn seed_obsolete(&self, slug: &str) {
        let dir = self.instances().join(slug);
        fs::create_dir_all(dir.join("memory")).unwrap();
        fs::write(dir.join("soul.md"), format!("soul of {slug}")).unwrap();
        fs::write(dir.join("memory/facts.md"), "- likes tea").unwrap();
    }

    fn assert_obsolete_untouched(&self, slug: &str) {
        let dir = self.instances().join(slug);
        assert_eq!(
            fs::read_to_string(dir.join("soul.md")).unwrap(),
            format!("soul of {slug}"),
            "{slug}: soul.md changed"
        );
        assert_eq!(
            fs::read_to_string(dir.join("memory/facts.md")).unwrap(),
            "- likes tea",
            "{slug}: memory changed"
        );
        assert!(
            !dir.join(IDENTITY_FILE).exists(),
            "{slug}: must never receive an identity marker"
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
}

#[tokio::test]
async fn companion_context_and_meta_report_only_the_canonical_companion() {
    let h = harness().await;

    let (status, context) = h.json(Method::GET, "/api/companion", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        context,
        serde_json::json!({
            "slug": CANONICAL_SLUG,
            "exists": false,
            "companion_name": "",
            "soul_exists": false,
        })
    );

    h.seed_obsolete("alice");
    h.seed_obsolete("bob");
    let (_, context) = h.json(Method::GET, "/api/companion", None).await;
    assert_eq!(
        context["exists"], false,
        "obsolete directories never stand in for the companion"
    );
    assert_eq!(context["slug"], CANONICAL_SLUG);
    let (_, meta) = h.json(Method::GET, "/api/meta", None).await;
    assert_eq!(meta["instances_count"], 0);
    assert_eq!(meta["companion_slug"], CANONICAL_SLUG);

    companion::ensure_identity(h.workspace.path()).unwrap();
    let (_, context) = h.json(Method::GET, "/api/companion", None).await;
    assert_eq!(context["exists"], true);
    assert_eq!(context["slug"], CANONICAL_SLUG);
    let (_, meta) = h.json(Method::GET, "/api/meta", None).await;
    assert_eq!(meta["instances_count"], 1);

    h.assert_obsolete_untouched("alice");
    h.assert_obsolete_untouched("bob");
}

#[tokio::test]
async fn multi_instance_routes_are_gone_and_unknown_api_paths_are_404_json() {
    let h = harness().await;
    companion::ensure_identity(h.workspace.path()).unwrap();
    fs::write(h.companion().join("soul.md"), "keep me").unwrap();

    for (method, uri) in [
        (Method::GET, "/api/instances"),
        (Method::GET, "/api/instances/companion/thoughts"),
        (Method::DELETE, "/api/instances/companion"),
        (Method::GET, "/api/instances/companion"),
        (Method::GET, "/api/no-such-route"),
        (Method::DELETE, "/api/instances/alice"),
    ] {
        let label = format!("{method} {uri}");
        let (status, value) = h.json(method, uri, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{label}");
        assert_eq!(value["error"], "not_found", "{label}");
    }

    assert_eq!(
        fs::read_to_string(h.companion().join("soul.md")).unwrap(),
        "keep me",
        "a removed delete route must not delete anything"
    );
    assert_eq!(h.instance_dirs(), vec![CANONICAL_SLUG]);

    // Retired memory debug routes (#96): the vectors path now resolves as an
    // ordinary (missing) memory file and reindex has no POST handler.
    let (status, _) = h
        .send(Method::GET, "/api/instances/companion/memory/vectors", None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = h
        .send(
            Method::POST,
            "/api/instances/companion/memory/reindex",
            None,
        )
        .await;
    assert!(
        status == StatusCode::NOT_FOUND || status == StatusCode::METHOD_NOT_ALLOWED,
        "{status}"
    );

    // Authentication still runs before the API 404 fallback.
    let response = build_router(h.state.clone(), None)
        .oneshot(
            Request::builder()
                .uri("/api/instances")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn foreign_slugs_fail_closed_on_every_surface_without_side_effects() {
    let h = harness().await;
    h.seed_obsolete("alice");

    let json = |value: serde_json::Value| Some(value);
    let cases: Vec<(Method, &str, Option<serde_json::Value>)> = vec![
        (
            Method::PUT,
            "/api/instances/alice/companion-name",
            json(serde_json::json!({"name": "Alice"})),
        ),
        (
            Method::PUT,
            "/api/instances/luna/companion-name",
            json(serde_json::json!({"name": "Luna"})),
        ),
        (
            Method::PUT,
            "/api/instances/alice/soul",
            json(serde_json::json!({"content": "rewritten"})),
        ),
        (
            Method::PUT,
            "/api/instances/luna/timezone",
            json(serde_json::json!({"timezone": "UTC"})),
        ),
        (Method::GET, "/api/instances/alice/soul", None),
        (Method::GET, "/api/instances/alice/memory", None),
        (Method::GET, "/api/instances/alice/export", None),
        (Method::GET, "/api/instances/alice/scheduled", None),
        (Method::POST, "/api/instances/alice/machine-hello", None),
        (Method::GET, "/api/chat/alice/chats", None),
        (
            Method::POST,
            "/api/chat",
            json(serde_json::json!({"instance_slug": "alice", "content": "hi"})),
        ),
        (
            Method::POST,
            "/api/chat",
            json(serde_json::json!({"instance_slug": "Companion", "content": "hi"})),
        ),
        (Method::GET, "/api/instances/Companion/soul", None),
        (
            Method::GET,
            "/public/memory/alice/sky.png?token=anything",
            None,
        ),
        (Method::GET, "/public/files/alice/upload-1", None),
    ];

    for (method, uri, body) in cases {
        let label = format!("{method} {uri}");
        let (status, bytes) = h.send(method, uri, body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{label}");
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| panic!("{label}: {}", String::from_utf8_lossy(&bytes)));
        assert_eq!(value["error"], "unknown_companion", "{label}");
    }

    assert_eq!(
        h.instance_dirs(),
        vec!["alice"],
        "no directory created or removed"
    );
    h.assert_obsolete_untouched("alice");
    assert!(!h.instances().join("luna").exists());
    assert!(
        !h.companion().exists(),
        "foreign requests never create the companion"
    );
}

#[tokio::test]
async fn canonical_reads_open_nothing_and_canonical_writes_create_the_one_companion() {
    let h = harness().await;
    let soul_uri = format!("/api/instances/{CANONICAL_SLUG}/soul");

    let (status, soul) = h.json(Method::GET, &soul_uri, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(soul["exists"], false);
    assert!(
        h.instance_dirs().is_empty(),
        "GET must not create the companion"
    );

    let (status, _) = h
        .send(
            Method::PUT,
            &format!("/api/instances/{CANONICAL_SLUG}/companion-name"),
            Some(serde_json::json!({"name": "Luna"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        companion::read_identity(h.workspace.path()),
        Ok(Some(
            crate::domain::companion::CompanionIdentity::canonical()
        )),
        "first write creates the identity marker"
    );

    let (status, _) = h
        .send(
            Method::PUT,
            &soul_uri,
            Some(serde_json::json!({"content": "calm and curious"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    let (_, context) = h.json(Method::GET, "/api/companion", None).await;
    assert_eq!(context["slug"], CANONICAL_SLUG);
    assert_eq!(context["exists"], true);
    assert_eq!(context["companion_name"], "Luna");
    assert_eq!(context["soul_exists"], true);
    assert_eq!(h.instance_dirs(), vec![CANONICAL_SLUG]);
}

#[tokio::test]
async fn unsupported_identity_marker_fails_closed_for_reads_and_writes() {
    let h = harness().await;
    fs::create_dir_all(h.companion()).unwrap();
    let raw = format!(
        r#"{{"format_version":{},"slug":"{CANONICAL_SLUG}"}}"#,
        STORAGE_FORMAT_VERSION + 1
    );
    fs::write(h.companion().join(IDENTITY_FILE), &raw).unwrap();

    for (method, uri, body) in [
        (Method::GET, "/api/companion".to_owned(), None),
        (
            Method::GET,
            format!("/api/instances/{CANONICAL_SLUG}/soul"),
            None,
        ),
        (
            Method::PUT,
            format!("/api/instances/{CANONICAL_SLUG}/companion-name"),
            Some(serde_json::json!({"name": "Luna"})),
        ),
    ] {
        let label = format!("{method} {uri}");
        let (status, value) = h.json(method, &uri, body).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{label}");
        assert_eq!(value["error"], "companion_format_unsupported", "{label}");
    }

    assert_eq!(
        fs::read_to_string(h.companion().join(IDENTITY_FILE)).unwrap(),
        raw,
        "marker must not be rewritten"
    );
    assert!(!h.companion().join("project_state.json").exists());
}

#[tokio::test]
async fn every_persisted_subsystem_is_owned_by_the_canonical_companion() {
    let h = harness().await;
    let ws = h.workspace.path();
    let api = |suffix: &str| format!("/api/instances/{CANONICAL_SLUG}/{suffix}");

    // Settings.
    let (status, _) = h
        .send(
            Method::PUT,
            &api("companion-name"),
            Some(serde_json::json!({"name": "Luna"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = h
        .send(
            Method::PUT,
            &api("timezone"),
            Some(serde_json::json!({"timezone": "Europe/Berlin"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    // Identity / soul.
    let (status, _) = h
        .send(
            Method::PUT,
            &api("soul"),
            Some(serde_json::json!({"content": "calm and curious"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    // History.
    crate::services::chat::save_user_message(ws, CANONICAL_SLUG, "default", "hello there").unwrap();

    // Memory.
    let media = h.state.vector_store.media_store();
    media
        .write_instance_text(CANONICAL_SLUG, "memory/notes/tea.md", "- likes oolong")
        .unwrap();
    let (status, memory) = h.json(Method::GET, &api("memory"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        memory.to_string().contains("tea.md"),
        "memory listing must see the canonical library: {memory}"
    );

    // Scheduler.
    let scheduled_dir = companion::companion_dir(ws).join("scheduled");
    fs::create_dir_all(&scheduled_dir).unwrap();
    let task = crate::services::tools::ScheduledTask {
        id: "task-1".into(),
        task: "water the plants".into(),
        deliver_at: 0,
        created_at: 0,
    };
    fs::write(
        scheduled_dir.join("task-1.json"),
        serde_json::to_string(&task).unwrap(),
    )
    .unwrap();
    let due = crate::services::scheduler::due_scheduled_tasks(ws, 1);
    assert_eq!(due.len(), 1);
    assert!(due[0].0.starts_with(companion::companion_dir(ws)));
    let (status, scheduled) = h.json(Method::GET, &api("scheduled"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(scheduled.to_string().contains("water the plants"));

    // Machines: a registration carrying a foreign slug is bound to the canonical companion.
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    h.state
        .machine_registry
        .register(
            MachineInfo {
                machine_id: "mac-mini".into(),
                os: "macos".into(),
                hostname: "studio".into(),
                screen_width: 1920,
                screen_height: 1080,
                last_seen: 0,
                instance_slug: Some("alice".into()),
                platform: None,
                location: cua_protocol::MachineLocation::Desktop,
                permissions: None,
                capabilities: Vec::new(),
            },
            tx,
        )
        .await;
    let machines = h.state.machine_registry.list().await;
    assert_eq!(machines.len(), 1);
    assert_eq!(machines[0].instance_slug.as_deref(), Some(CANONICAL_SLUG));

    // Everything above landed under one directory.
    assert_eq!(h.instance_dirs(), vec![CANONICAL_SLUG]);
    let dir = companion::companion_dir(ws);
    for relative in [
        IDENTITY_FILE,
        "soul.md",
        "project_state.json",
        "memory/notes/tea.md",
        "scheduled/task-1.json",
    ] {
        assert!(dir.join(relative).is_file(), "missing {relative}");
    }
    assert!(
        dir.join("chats").is_dir(),
        "history lives under the companion"
    );

    // Export archive is rooted at the canonical slug.
    let (status, archive) = h.send(Method::GET, &api("export"), None).await;
    assert_eq!(status, StatusCode::OK);
    let archive_path = h.workspace.path().join("export.tar.gz");
    fs::write(&archive_path, &archive).unwrap();
    let listing = std::process::Command::new("tar")
        .arg("-tzf")
        .arg(&archive_path)
        .output()
        .unwrap();
    assert!(
        listing.status.success(),
        "{}",
        String::from_utf8_lossy(&listing.stderr)
    );
    let entries = String::from_utf8_lossy(&listing.stdout);
    let prefix = format!("{CANONICAL_SLUG}/");
    for entry in entries.lines().filter(|line| !line.is_empty()) {
        assert!(
            entry == CANONICAL_SLUG || entry.starts_with(&prefix),
            "archive entry outside the companion root: {entry}"
        );
    }
    assert!(entries.contains(&format!("{prefix}{IDENTITY_FILE}")));
    assert!(entries.contains(&format!("{prefix}soul.md")));
}

#[tokio::test]
async fn stats_dashboard_route_is_gone_and_rhythm_tracking_has_an_opt_out() {
    let h = harness().await;
    companion::ensure_identity(h.workspace.path()).unwrap();
    let ws = h.workspace.path();
    let dir = companion::companion_dir(ws);

    let (status, value) = h
        .json(
            Method::GET,
            &format!("/api/instances/{CANONICAL_SLUG}/stats"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(value["error"], "not_found");

    let rhythm_uri = format!("/api/instances/{CANONICAL_SLUG}/rhythm");
    let (status, value) = h.json(Method::GET, &rhythm_uri, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value, serde_json::json!({ "enabled": true }));

    crate::services::chat::save_user_message(ws, CANONICAL_SLUG, "default", "hello").unwrap();
    assert!(
        dir.join("rhythm.json").is_file(),
        "messages feed the aggregate"
    );
    assert!(
        !dir.join("stats").exists(),
        "no per-day aggregate store may be written"
    );

    let (status, _) = h
        .send(
            Method::PUT,
            &rhythm_uri,
            Some(serde_json::json!({ "enabled": false })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !dir.join("rhythm.json").exists(),
        "opting out deletes the aggregate"
    );
    let (_, value) = h.json(Method::GET, &rhythm_uri, None).await;
    assert_eq!(value["enabled"], false);

    crate::services::chat::save_user_message(ws, CANONICAL_SLUG, "default", "still here").unwrap();
    assert!(
        !dir.join("rhythm.json").exists(),
        "no recording while opted out"
    );

    let (status, _) = h
        .send(
            Method::PUT,
            &rhythm_uri,
            Some(serde_json::json!({ "enabled": true })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    crate::services::chat::save_user_message(ws, CANONICAL_SLUG, "default", "back").unwrap();
    assert!(
        dir.join("rhythm.json").is_file(),
        "recording resumes after opting back in"
    );
}

#[tokio::test]
async fn proactive_activity_api_lists_cancels_retries_and_exposes_policy() {
    use crate::domain::proactive::{RunOutcome, RunStatus, Target, Trigger};
    use crate::services::proactive::Admission;

    let h = harness().await;
    companion::ensure_identity(h.workspace.path()).unwrap();
    let api = |suffix: &str| format!("/api/instances/{CANONICAL_SLUG}/{suffix}");

    // Policy round trip.
    let (status, policy) = h.json(Method::GET, &api("proactive"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(policy["enabled"], true);
    assert_eq!(policy["daily_reach_out_budget"], 6);
    let (status, _) = h
        .send(
            Method::PUT,
            &api("proactive"),
            Some(serde_json::json!({
                "enabled": true,
                "quiet_hours": {"start_hour": 22, "end_hour": 7},
                "cooldown_secs": 60,
                "daily_reach_out_budget": 3,
                "retention_max": 50,
                "retention_days": 7
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (_, policy) = h.json(Method::GET, &api("proactive"), None).await;
    assert_eq!(policy["quiet_hours"]["start_hour"], 22);
    assert_eq!(policy["daily_reach_out_budget"], 3);
    let (status, _) = h
        .send(
            Method::PUT,
            &api("proactive"),
            Some(serde_json::json!({"quiet_hours": {"start_hour": 25, "end_hour": 7}})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "hours are validated");

    // Quiet hours are judged against the wall clock, so clear them before
    // starting runs; otherwise this test fails whenever CI runs at night.
    let (status, _) = h
        .send(
            Method::PUT,
            &api("proactive"),
            Some(serde_json::json!({
                "enabled": true,
                "quiet_hours": null,
                "cooldown_secs": 60,
                "daily_reach_out_budget": 3,
                "retention_max": 50,
                "retention_days": 7
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    // Records created by the loop are visible, bounded, and controllable.
    let Admission::Admitted(running) = h.state.proactive.begin(
        Trigger::Heartbeat {
            agent: "companion".into(),
        },
        "hourly check-in",
        Target::Companion,
    ) else {
        panic!("expected admission");
    };
    let running_id = running.id().to_owned();
    let Admission::Admitted(failed) = h.state.proactive.begin(
        Trigger::Schedule {
            task_id: "t1".into(),
        },
        "water the plants",
        Target::Chat {
            chat_id: "default".into(),
        },
    ) else {
        panic!("expected admission");
    };
    let failed = failed.fail("provider offline", true);

    let (status, list) = h.json(Method::GET, &api("activity?limit=10"), None).await;
    assert_eq!(status, StatusCode::OK);
    let list = list.as_array().unwrap();
    assert_eq!(list.len(), 2);
    for run in list {
        assert!(run.get("outcome").is_none() || run["outcome"].get("summary").is_none());
        assert!(run.get("trace").is_none(), "no raw traces on the wire");
    }

    let (status, one) = h
        .json(Method::GET, &api(&format!("activity/{running_id}")), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(one["status"]["kind"], "running");
    assert_eq!(one["reason"], "hourly check-in");

    let (status, _) = h
        .send(
            Method::POST,
            &api(&format!("activity/{running_id}/cancel")),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(running.is_cancelled());
    let cancelled = running.cancel();
    assert_eq!(cancelled.status, RunStatus::Cancelled);
    let (status, _) = h
        .send(
            Method::POST,
            &api(&format!("activity/{running_id}/cancel")),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "nothing left to cancel");

    let (status, retried) = h
        .json(
            Method::POST,
            &api(&format!("activity/{}/retry", failed.id)),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(retried["attempt"], 2);
    assert_eq!(retried["retry_of"], failed.id);
    let retry_id = retried["id"].as_str().unwrap().to_owned();
    h.state.proactive.cancel(&retry_id);
    let (status, _) = h
        .send(Method::POST, &api("activity/run_missing/retry"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Every record lives under the canonical companion.
    assert!(
        companion::companion_dir(h.workspace.path())
            .join("activity")
            .join(format!("{running_id}.json"))
            .is_file()
    );
    let _ = RunOutcome::default();
}

#[tokio::test]
async fn model_presets_api_validates_seeds_and_pins_per_chat() {
    let h = harness().await;
    companion::ensure_identity(h.workspace.path()).unwrap();
    h.state.config.write().await.llm.tokens.anthropic = "anthropic-key".into();

    // Defaults: the Anthropic seeds, both slots filled, only Anthropic keyed.
    let (status, body) = h.json(Method::GET, "/api/config/models", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["chat_preset"], "sonnet");
    assert_eq!(body["background_preset"], "haiku");
    assert_eq!(body["keyed_providers"], serde_json::json!(["anthropic"]));
    assert_eq!(body["presets"].as_array().unwrap().len(), 3);

    // A slot pointing at a provider without a key is rejected as one unit.
    let mut presets = body["presets"].clone();
    presets.as_array_mut().unwrap().push(serde_json::json!({
        "id": "gpt", "name": "GPT-5.4", "provider": "openai", "model": "gpt-5.4"
    }));
    let (status, body) = h
        .json(
            Method::PUT,
            "/api/config/models",
            Some(serde_json::json!({
                "presets": presets,
                "chat_preset": "gpt",
                "background_preset": "haiku",
            })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "invalid_presets");
    assert!(
        body["message"].as_str().unwrap().contains("OpenAI"),
        "{body}"
    );
    assert_eq!(
        h.state.config.read().await.llm.chat_preset,
        "sonnet",
        "a rejected update changes nothing"
    );

    // A valid update replaces presets and slots atomically and rebuilds backends.
    let (status, body) = h
        .json(
            Method::PUT,
            "/api/config/models",
            Some(serde_json::json!({
                "presets": [
                    {"id": "opus", "name": "Claude Opus", "provider": "anthropic", "model": "claude-opus-4-6"},
                    {"id": "haiku", "name": "Claude Haiku", "provider": "anthropic", "model": "claude-haiku-4-5-20251001"}
                ],
                "chat_preset": "opus",
                "background_preset": "haiku",
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["presets"].as_array().unwrap().len(), 2);
    assert_eq!(
        h.state.llm.read().await.as_ref().unwrap().model,
        "claude-opus-4-6"
    );
    assert_eq!(
        h.state.background_llm.read().await.as_ref().unwrap().model,
        "claude-haiku-4-5-20251001"
    );
    let persisted: crate::config::Config =
        toml::from_str(&fs::read_to_string(h.workspace.path().join("config.toml")).unwrap())
            .unwrap();
    assert_eq!(persisted.llm.chat_preset, "opus");
    assert_eq!(persisted.llm.presets.len(), 2);

    // Seeding adds a provider's defaults without touching chosen slots.
    let (status, body) = h
        .json(
            Method::POST,
            "/api/config/models/seed",
            Some(serde_json::json!({ "provider": "openai" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["added"], 2);
    assert_eq!(body["chat_preset"], "opus");
    let (status, body) = h
        .json(
            Method::POST,
            "/api/config/models/seed",
            Some(serde_json::json!({ "provider": "gemini" })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "unknown_provider");

    // Per-conversation pins: absent by default, validated, clearable.
    let (status, body) = h
        .json(Method::GET, "/api/chat/companion/thread-1/preset", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["preset"], serde_json::Value::Null);
    assert_eq!(body["effective_preset"], "opus");
    let (status, _) = h
        .send(
            Method::PUT,
            "/api/chat/companion/thread-1/preset",
            Some(serde_json::json!({ "preset": "nope" })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, body) = h
        .json(
            Method::PUT,
            "/api/chat/companion/thread-1/preset",
            Some(serde_json::json!({ "preset": "haiku" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["preset"], "haiku");
    assert_eq!(body["effective_preset"], "haiku");
    assert_eq!(
        crate::services::chat::get_chat_preset(h.workspace.path(), "companion", "thread-1")
            .unwrap()
            .as_deref(),
        Some("haiku")
    );
    let (status, body) = h
        .json(
            Method::PUT,
            "/api/chat/companion/thread-1/preset",
            Some(serde_json::json!({ "preset": null })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["preset"], serde_json::Value::Null);
    assert_eq!(body["effective_preset"], "opus");

    // Foreign companions fail closed on every new route.
    for (method, uri) in [
        (Method::GET, "/api/chat/alice/thread-1/preset"),
        (Method::PUT, "/api/chat/alice/thread-1/preset"),
    ] {
        let (status, body) = h
            .json(method, uri, Some(serde_json::json!({ "preset": null })))
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}: {body}");
        assert_eq!(body["error"], "unknown_companion");
    }
    let (status, _) = h.send(Method::PUT, "/api/config/model-mode", None).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "model-mode route must be gone"
    );
    let (status, _) = h.send(Method::PUT, "/api/config/provider", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "provider route must be gone");
}

#[tokio::test]
async fn connected_computers_are_listed_for_the_one_companion_only() {
    let h = harness().await;
    companion::ensure_identity(h.workspace.path()).unwrap();

    let (status, body) = h
        .json(Method::GET, "/api/instances/companion/machines", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["machines"], serde_json::json!([]));

    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
    h.state
        .machine_registry
        .register(
            MachineInfo {
                machine_id: "mac-mini".into(),
                os: "macos".into(),
                hostname: "studio".into(),
                screen_width: 2560,
                screen_height: 1440,
                last_seen: 1_700_000_000,
                instance_slug: None,
                platform: Some(cua_protocol::Platform::Macos),
                location: cua_protocol::MachineLocation::Desktop,
                permissions: Some(cua_protocol::PermissionState {
                    accessibility: cua_protocol::Permission::Granted,
                    screen_capture: cua_protocol::Permission::Denied,
                }),
                capabilities: vec!["screenshot".into(), "bash".into()],
            },
            sender,
        )
        .await;

    let (status, body) = h
        .json(Method::GET, "/api/instances/companion/machines", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let machines = body["machines"].as_array().unwrap();
    assert_eq!(machines.len(), 1);
    assert_eq!(machines[0]["machine_id"], "mac-mini");
    assert_eq!(machines[0]["hostname"], "studio");
    assert_eq!(machines[0]["display_name"], "studio");
    assert_eq!(machines[0]["custom_name"], serde_json::Value::Null);
    assert_eq!(machines[0]["os"], "macos");
    assert_eq!(machines[0]["platform"], "macos");
    assert_eq!(machines[0]["location"], "desktop");
    assert_eq!(machines[0]["screen_width"], 2560);
    assert_eq!(machines[0]["last_seen"], 1_700_000_000);
    assert_eq!(machines[0]["first_seen"], 1_700_000_000);
    assert_eq!(machines[0]["online"], true);
    // The registration is far in the past against the wall clock: open socket, stale heartbeat.
    assert_eq!(machines[0]["health"], "degraded");
    assert_eq!(
        machines[0]["permissions"],
        serde_json::json!({"accessibility": "granted", "screen_capture": "denied"})
    );
    assert_eq!(
        machines[0]["capabilities"],
        serde_json::json!(["screenshot", "bash"])
    );
    assert_eq!(
        machines[0]["driver_version"],
        serde_json::Value::Null,
        "reserved until the Cua driver reports"
    );
    assert_eq!(machines[0]["cua_health"], serde_json::Value::Null);
    assert_eq!(
        machines[0]["instance_slug"], CANONICAL_SLUG,
        "every computer is a context of the one companion"
    );

    // The user names it; the name is stored on the server.
    let (status, renamed) = h
        .json(
            Method::PUT,
            "/api/instances/companion/machines/mac-mini",
            Some(serde_json::json!({"display_name": "Studio Mac"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{renamed}");
    assert_eq!(renamed["display_name"], "Studio Mac");
    assert_eq!(renamed["custom_name"], "Studio Mac");
    assert_eq!(
        renamed["hostname"], "studio",
        "the hostname stays for display"
    );

    let (status, invalid) = h
        .json(
            Method::PUT,
            "/api/instances/companion/machines/mac-mini",
            Some(serde_json::json!({"display_name": "x".repeat(65)})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{invalid}");
    assert_eq!(invalid["error"], "invalid");

    let (status, missing) = h
        .json(
            Method::PUT,
            "/api/instances/companion/machines/nobody",
            Some(serde_json::json!({"display_name": "Ghost"})),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{missing}");
    assert_eq!(missing["error"], "not_found");

    // Disconnecting keeps the computer listed, offline, with its name and last-seen time.
    h.state.machine_registry.unregister("mac-mini").await;
    let (status, body) = h
        .json(Method::GET, "/api/instances/companion/machines", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let machines = body["machines"].as_array().unwrap();
    assert_eq!(machines.len(), 1, "a disconnected computer does not vanish");
    assert_eq!(machines[0]["machine_id"], "mac-mini");
    assert_eq!(machines[0]["online"], false);
    assert_eq!(machines[0]["health"], "unavailable");
    assert_eq!(machines[0]["display_name"], "Studio Mac");
    assert!(machines[0]["last_seen"].as_i64().unwrap() > 1_700_000_000);
    assert!(
        h.companion().join("machines.json").is_file(),
        "known machines live under the companion directory"
    );

    // Foreign slugs fail closed like every other companion route.
    let (status, body) = h
        .json(Method::GET, "/api/instances/alice/machines", None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["error"], "unknown_companion");
    let (status, body) = h
        .json(
            Method::PUT,
            "/api/instances/alice/machines/mac-mini",
            Some(serde_json::json!({"display_name": "Studio Mac"})),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["error"], "unknown_companion");
}

#[tokio::test]
async fn a_broken_machines_file_answers_503_instead_of_an_empty_list() {
    let h = harness().await;
    companion::ensure_identity(h.workspace.path()).unwrap();
    fs::write(h.companion().join("machines.json"), "{not json").unwrap();

    let (status, body) = h
        .json(Method::GET, "/api/instances/companion/machines", None)
        .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["error"], "machines_format_unsupported");
    assert_eq!(
        fs::read_to_string(h.companion().join("machines.json")).unwrap(),
        "{not json",
        "the file is never rewritten"
    );

    // A desktop connects while the file is broken; renaming it is refused.
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
    h.state
        .machine_registry
        .register(
            MachineInfo {
                machine_id: "mac-mini".into(),
                os: "macos".into(),
                hostname: "studio".into(),
                screen_width: 1440,
                screen_height: 900,
                last_seen: 1_700_000_000,
                instance_slug: None,
                platform: Some(cua_protocol::Platform::Macos),
                location: cua_protocol::MachineLocation::Desktop,
                permissions: None,
                capabilities: Vec::new(),
            },
            sender,
        )
        .await;
    let (status, body) = h
        .json(
            Method::PUT,
            "/api/instances/companion/machines/mac-mini",
            Some(serde_json::json!({"display_name": "Studio Mac"})),
        )
        .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");

    // Removing the file takes effect without a restart, and the connected
    // desktop is recorded.
    fs::remove_file(h.companion().join("machines.json")).unwrap();
    let (status, body) = h
        .json(Method::GET, "/api/instances/companion/machines", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let machines = body["machines"].as_array().unwrap();
    assert_eq!(machines.len(), 1, "{body}");
    assert_eq!(machines[0]["machine_id"], "mac-mini");
    assert_eq!(machines[0]["online"], true);
    assert!(h.companion().join("machines.json").is_file());
}

#[tokio::test]
async fn only_invalid_capabilities_are_recorded_as_none() {
    let h = harness().await;
    companion::ensure_identity(h.workspace.path()).unwrap();
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
    h.state
        .machine_registry
        .register(
            MachineInfo {
                machine_id: "mac-mini".into(),
                os: "macos".into(),
                hostname: "studio".into(),
                screen_width: 1440,
                screen_height: 900,
                last_seen: 1_700_000_000,
                instance_slug: None,
                platform: Some(cua_protocol::Platform::Macos),
                location: cua_protocol::MachineLocation::Desktop,
                permissions: None,
                capabilities: vec!["Has Space".into(), "UPPER".into()],
            },
            sender,
        )
        .await;
    let (status, body) = h
        .json(Method::GET, "/api/instances/companion/machines", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["machines"][0]["capabilities"],
        serde_json::json!([]),
        "nothing valid reported is not a legacy desktop"
    );
}

#[tokio::test]
async fn an_offline_computer_can_be_forgotten_over_the_api() {
    let h = harness().await;
    companion::ensure_identity(h.workspace.path()).unwrap();
    let mut receivers = Vec::new();
    for id in ["mac-mini", "laptop"] {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        receivers.push(receiver);
        h.state
            .machine_registry
            .register(
                MachineInfo {
                    machine_id: id.into(),
                    os: "macos".into(),
                    hostname: id.into(),
                    screen_width: 1440,
                    screen_height: 900,
                    last_seen: 1_700_000_000,
                    instance_slug: None,
                    platform: Some(cua_protocol::Platform::Macos),
                    location: cua_protocol::MachineLocation::Desktop,
                    permissions: None,
                    capabilities: Vec::new(),
                },
                sender,
            )
            .await;
    }
    h.state.machine_registry.unregister("laptop").await;
    let mut events = h.state.events.subscribe();

    // Connected computers stay; unknown ones and foreign slugs fail closed.
    let (status, body) = h
        .json(
            Method::DELETE,
            "/api/instances/companion/machines/mac-mini",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "machine_online");
    let (status, body) = h
        .json(
            Method::DELETE,
            "/api/instances/companion/machines/ghost",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["error"], "not_found");
    let (status, body) = h
        .json(Method::DELETE, "/api/instances/alice/machines/laptop", None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["error"], "unknown_companion");

    let (status, _) = h
        .send(
            Method::DELETE,
            "/api/instances/companion/machines/laptop",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = h
        .json(Method::GET, "/api/instances/companion/machines", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let ids: Vec<&str> = body["machines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["machine_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["mac-mini"]);
    let event = serde_json::to_value(events.try_recv().unwrap()).unwrap();
    assert_eq!(event["type"], "machine_forgotten");
    assert_eq!(event["instance_slug"], CANONICAL_SLUG);
    assert_eq!(event["machine_id"], "laptop");
    let (status, _) = h
        .send(
            Method::DELETE,
            "/api/instances/companion/machines/laptop",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "already gone");
}

#[tokio::test]
async fn machine_hello_and_bye_never_guess_between_connected_computers() {
    let h = harness().await;
    companion::ensure_identity(h.workspace.path()).unwrap();

    // Nobody connected: nothing to greet, nothing to report.
    let (status, _) = h
        .send(Method::POST, "/api/instances/companion/machine-hello", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = h
        .send(Method::POST, "/api/instances/companion/machine-bye", None)
        .await;
    assert_eq!(status, StatusCode::OK);

    let mut receivers = Vec::new();
    for id in ["mac-mini", "laptop"] {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        receivers.push(receiver);
        h.state
            .machine_registry
            .register(
                MachineInfo {
                    machine_id: id.into(),
                    os: "macos".into(),
                    hostname: id.into(),
                    screen_width: 1440,
                    screen_height: 900,
                    last_seen: 1_700_000_000,
                    instance_slug: None,
                    platform: Some(cua_protocol::Platform::Macos),
                    location: cua_protocol::MachineLocation::Desktop,
                    permissions: None,
                    capabilities: Vec::new(),
                },
                sender,
            )
            .await;
    }

    // Two online computers and no explicit target: ask, do not pick machines[0].
    let (status, body) = h
        .json(Method::POST, "/api/instances/companion/machine-hello", None)
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "ambiguous_machine");
    let mut offered: Vec<String> = body["machine_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    offered.sort();
    assert_eq!(offered, ["laptop", "mac-mini"]);

    // An explicit target is addressed; an unknown or offline one is refused.
    let (status, _) = h
        .send(
            Method::POST,
            "/api/instances/companion/machine-hello",
            Some(serde_json::json!({"machine_id": "laptop"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = h
        .json(
            Method::POST,
            "/api/instances/companion/machine-hello",
            Some(serde_json::json!({"machine_id": "ghost"})),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["error"], "not_found");
    h.state.machine_registry.unregister("laptop").await;
    let (status, body) = h
        .json(
            Method::POST,
            "/api/instances/companion/machine-hello",
            Some(serde_json::json!({"machine_id": "laptop"})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "machine_offline");

    // Bye names the computer the user chose, or every connected one; it never picks.
    let (status, _) = h
        .send(
            Method::POST,
            "/api/instances/companion/machine-bye",
            Some(serde_json::json!({"machine_id": "mac-mini"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = h
        .json(
            Method::POST,
            "/api/instances/companion/machine-bye",
            Some(serde_json::json!({"machine_id": "ghost"})),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    receivers.push(receiver);
    h.state
        .machine_registry
        .register(
            MachineInfo {
                machine_id: "laptop".into(),
                os: "macos".into(),
                hostname: "laptop".into(),
                screen_width: 1440,
                screen_height: 900,
                last_seen: 1_700_000_000,
                instance_slug: None,
                platform: Some(cua_protocol::Platform::Macos),
                location: cua_protocol::MachineLocation::Desktop,
                permissions: None,
                capabilities: Vec::new(),
            },
            sender,
        )
        .await;
    let (status, _) = h
        .send(Method::POST, "/api/instances/companion/machine-bye", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let history =
        crate::services::chat::load_messages(h.workspace.path(), CANONICAL_SLUG, "default")
            .unwrap();
    let system: Vec<String> = history
        .messages
        .iter()
        .filter(|m| m.content.contains("user left"))
        .map(|m| m.content.clone())
        .collect();
    assert_eq!(system.len(), 2, "{system:?}");
    assert!(system[0].contains("'mac-mini'") && !system[0].contains("'laptop'"));
    assert!(
        system[1].contains("'laptop'") && system[1].contains("'mac-mini'"),
        "{}",
        system[1]
    );
}

#[tokio::test]
async fn scheduled_tasks_and_machine_connects_route_through_the_proactive_loop() {
    use crate::domain::proactive::{RunStatus, SkipReason, Trigger};

    let h = harness().await;
    companion::ensure_identity(h.workspace.path()).unwrap();
    let ws = h.workspace.path();
    let dir = companion::companion_dir(ws);
    fs::write(dir.join("soul.md"), "soul").unwrap();

    // A due schedule becomes one Schedule run that reaches the chat.
    let scheduled_dir = dir.join("scheduled");
    fs::create_dir_all(&scheduled_dir).unwrap();
    let task = crate::services::tools::ScheduledTask {
        id: "task-1".into(),
        task: "water the plants".into(),
        deliver_at: 0,
        created_at: 0,
    };
    fs::write(
        scheduled_dir.join("task-1.json"),
        serde_json::to_string(&task).unwrap(),
    )
    .unwrap();
    crate::services::scheduler::check_and_trigger(&h.state).await;
    let runs = h.state.proactive.list(10);
    assert_eq!(runs.len(), 1);
    assert_eq!(
        runs[0].trigger,
        Trigger::Schedule {
            task_id: "task-1".into()
        }
    );
    assert_eq!(runs[0].reason, "water the plants");
    assert_eq!(runs[0].status, RunStatus::Completed);
    assert!(!scheduled_dir.join("task-1.json").exists());

    // Reconnect bursts: one run, then cooldown skips; no LLM makes the run fail retryably.
    crate::routes::machine_agents::on_machine_connected(&h.state, "mac-mini", None).await;
    crate::routes::machine_agents::on_machine_connected(&h.state, "mac-mini", None).await;
    let runs = h.state.proactive.list(10);
    let machine_runs: Vec<_> = runs
        .iter()
        .filter(|run| {
            run.trigger
                == Trigger::MachineConnected {
                    machine_id: "mac-mini".into(),
                }
        })
        .collect();
    assert_eq!(machine_runs.len(), 2);
    let statuses: Vec<&RunStatus> = machine_runs.iter().map(|run| &run.status).collect();
    assert!(
        statuses.iter().any(|status| matches!(
            status,
            RunStatus::Failed {
                retryable: true,
                ..
            }
        )),
        "{statuses:?}"
    );
    assert!(
        statuses.iter().any(|status| matches!(
            status,
            RunStatus::Skipped {
                reason: SkipReason::Cooldown { .. } | SkipReason::Duplicate { .. }
            }
        )),
        "{statuses:?}"
    );
}

#[tokio::test]
async fn memory_receipts_are_readable_only_through_the_canonical_companion() {
    use crate::domain::receipt::{Confidence, RecallReason, RecalledMemory, SourceStatus};
    use crate::services::memory_receipts;

    let h = harness().await;
    companion::ensure_identity(h.workspace.path()).unwrap();
    h.seed_obsolete("alice");
    let message = crate::domain::chat::ChatMessage {
        id: "msg_1".into(),
        role: crate::domain::chat::ChatRole::Assistant,
        content: "remembered".into(),
        created_at: "1".into(),
        kind: Default::default(),
        tool_name: None,
        mcp_app_html: None,
        mcp_app_input: None,
        model: None,
    };
    for slug in [CANONICAL_SLUG, "alice"] {
        let memories = vec![RecalledMemory {
            path: "memory/facts.md".into(),
            source: "memory/facts.md".into(),
            excerpt: format!("secret of {slug}"),
            reason: RecallReason::Keyword,
            linked_from: None,
            confidence: Confidence::Medium,
            retrieved_at: "2026-09-20T12:00:00Z".into(),
            source_status: SourceStatus::Present,
        }];
        memory_receipts::write_receipts(
            h.workspace.path(),
            slug,
            "default",
            std::slice::from_ref(&message),
            &memories,
        )
        .unwrap();
    }

    let (status, receipt) = h
        .json(
            Method::GET,
            &format!("/api/instances/{CANONICAL_SLUG}/default/receipts/msg_1"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["memories"][0]["excerpt"], "secret of companion");
    let (status, listed) = h
        .json(
            Method::GET,
            &format!("/api/instances/{CANONICAL_SLUG}/default/receipts"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed[0]["memories"][0]["excerpt"], "secret of companion");

    for uri in [
        "/api/instances/alice/default/receipts",
        "/api/instances/alice/default/receipts/msg_1",
        "/api/instances/Companion/default/receipts/msg_1",
    ] {
        let (status, value) = h.json(Method::GET, uri, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(value["error"], "unknown_companion", "{uri}");
    }
    // Path segments can never reach another directory's receipts.
    for uri in [
        "/api/instances/companion/default/receipts/..%2F..%2F..%2Falice%2Fchats%2Fdefault%2Freceipts%2Fmsg_1",
        "/api/instances/companion/..%2F..%2Falice%2Fchats%2Fdefault/receipts/msg_1",
        "/api/instances/companion/..%2F..%2Falice%2Fchats%2Fdefault/receipts",
    ] {
        let (status, bytes) = h.send(Method::GET, uri, None).await;
        let body = String::from_utf8_lossy(&bytes);
        assert!(
            !body.contains("secret of alice"),
            "{uri} leaked another directory: {body}"
        );
        assert!(
            status == StatusCode::NOT_FOUND || body == "[]",
            "{uri}: {status} {body}"
        );
    }
    h.assert_obsolete_untouched("alice");
}

#[tokio::test]
async fn continuity_api_lists_inspects_updates_completes_and_dismisses_records() {
    use crate::domain::continuity::{
        ContinuityState, ContinuityUpdate, Origin, Provenance, ProvenanceSource, ResourceRef,
    };
    use crate::services::continuity::ContinuityStore;

    let h = harness().await;
    companion::ensure_identity(h.workspace.path()).unwrap();
    let api = |suffix: &str| format!("/api/instances/{CANONICAL_SLUG}/{suffix}");
    let store = ContinuityStore::new(h.workspace.path(), CANONICAL_SLUG);
    let by = |note: &str| Provenance {
        source: ProvenanceSource::Chat,
        at: 1_767_603_600,
        note: note.into(),
    };
    let origin = || Origin {
        chat_id: "default".into(),
        message_id: Some("msg_1".into()),
    };

    // Nothing yet: an empty listing, no storage created by the read.
    let (status, listing) = h.json(Method::GET, &api("continuity"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing, serde_json::json!({"records": [], "errors": []}));
    assert!(
        !companion::companion_dir(h.workspace.path())
            .join("continuity")
            .exists()
    );

    // Records are started by explicit task activity (the tool); the API
    // lists, inspects, and changes them.
    let photos = store
        .create(
            "rename the trip photos",
            origin(),
            &ContinuityUpdate {
                machine_ids: vec!["mac-mini".into()],
                resources: vec![ResourceRef::Upload {
                    id: "upload_gone".into(),
                }],
                next_step: Some("list the folder".into()),
                ..Default::default()
            },
            by("asked in chat"),
            1_767_603_600,
        )
        .await
        .unwrap();
    let taxes = store
        .create(
            "file the taxes",
            origin(),
            &ContinuityUpdate::default(),
            by("asked in chat"),
            1_767_603_601,
        )
        .await
        .unwrap();
    store
        .complete(&taxes.id, by("done last week"), 1_767_603_602)
        .await
        .unwrap();

    let (status, listing) = h.json(Method::GET, &api("continuity"), None).await;
    assert_eq!(status, StatusCode::OK);
    let records = listing["records"].as_array().unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["id"], taxes.id, "most recently updated first");
    assert_eq!(listing["errors"], serde_json::json!([]));
    let (_, resumable) = h
        .json(Method::GET, &api("continuity?resumable=true"), None)
        .await;
    let resumable = resumable["records"].as_array().unwrap();
    assert_eq!(resumable.len(), 1);
    assert_eq!(resumable[0]["id"], photos.id);
    assert_eq!(resumable[0]["state"], "active");

    // Inspecting runs the reference check: the unknown computer and the
    // missing upload are explicit blockers with server provenance.
    let (status, one) = h
        .json(
            Method::GET,
            &api(&format!("continuity/{}", photos.id)),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(one["goal"], "rename the trip photos");
    assert_eq!(one["origin"]["chat_id"], "default");
    let blockers = one["blockers"].as_array().unwrap();
    assert_eq!(blockers.len(), 2, "{one}");
    assert_eq!(blockers[0]["kind"]["kind"], "machine_unavailable");
    assert_eq!(blockers[0]["kind"]["machine_id"], "mac-mini");
    assert_eq!(blockers[1]["kind"]["kind"], "resource_missing");
    assert_eq!(blockers[1]["kind"]["resource"]["kind"], "upload");
    assert_eq!(blockers[1]["provenance"]["source"], "server");
    assert_eq!(one["resources"][0]["resource"]["id"], "upload_gone");
    assert!(
        one["resources"][0].get("content").is_none() && one["resources"][0].get("bytes").is_none(),
        "resources are links, never contents"
    );
    let (status, _) = h
        .send(Method::GET, &api("continuity/task_missing"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Updating is explicit user activity with a note.
    let (status, updated) = h
        .json(
            Method::PUT,
            &api(&format!("continuity/{}", photos.id)),
            Some(serde_json::json!({
                "state": "waiting",
                "completed_step": "listed 212 files",
                "next_step": "rename IMG_* to trip-*",
                "note": "picked a naming scheme",
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["state"], "waiting");
    assert_eq!(updated["completed_steps"][0]["summary"], "listed 212 files");
    assert_eq!(updated["next_step"], "rename IMG_* to trip-*");
    let provenance = updated["provenance"].as_array().unwrap();
    assert_eq!(provenance.last().unwrap()["source"], "user");
    assert_eq!(provenance.last().unwrap()["note"], "picked a naming scheme");
    let (status, error) = h
        .json(
            Method::PUT,
            &api(&format!("continuity/{}", photos.id)),
            Some(serde_json::json!({"state": "active", "note": ""})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"], "continuity");
    let (status, _) = h
        .send(
            Method::PUT,
            &api("continuity/task_missing"),
            Some(serde_json::json!({"state": "active", "note": "x"})),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        store.get(&photos.id).unwrap().state,
        ContinuityState::Waiting,
        "rejected writes change nothing"
    );

    // Complete and dismiss close records; closed records are not resumable.
    let (status, done) = h
        .json(
            Method::POST,
            &api(&format!("continuity/{}/complete", photos.id)),
            Some(serde_json::json!({"note": "all renamed"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert_eq!(done["state"], "completed");
    assert_eq!(
        done["provenance"].as_array().unwrap().last().unwrap()["note"],
        "all renamed"
    );
    let (status, dismissed) = h
        .json(
            Method::POST,
            &api(&format!("continuity/{}/dismiss", taxes.id)),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{dismissed}");
    assert_eq!(dismissed["state"], "dismissed");
    assert_eq!(
        dismissed["provenance"].as_array().unwrap().last().unwrap()["source"],
        "user"
    );
    let (status, _) = h
        .send(Method::POST, &api("continuity/task_missing/complete"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, resumable) = h
        .json(Method::GET, &api("continuity?resumable=true"), None)
        .await;
    assert_eq!(resumable["records"], serde_json::json!([]));
    assert_eq!(store.list().len(), 2, "closed records stay inspectable");

    // Corrupt files are surfaced, not hidden or deleted.
    let dir = companion::companion_dir(h.workspace.path()).join("continuity");
    fs::write(dir.join("task_broken.json"), "{").unwrap();
    let (_, listing) = h.json(Method::GET, &api("continuity"), None).await;
    assert_eq!(listing["records"].as_array().unwrap().len(), 2);
    assert_eq!(listing["errors"][0]["file"], "task_broken.json");
    assert!(dir.join("task_broken.json").is_file());

    // Every record lives under the canonical companion; foreign slugs fail closed.
    assert!(dir.join(format!("{}.json", photos.id)).is_file());
    for (method, uri) in [
        (Method::GET, "/api/instances/alice/continuity".to_owned()),
        (
            Method::PUT,
            format!("/api/instances/alice/continuity/{}", photos.id),
        ),
        (
            Method::POST,
            format!("/api/instances/alice/continuity/{}/dismiss", photos.id),
        ),
    ] {
        let (status, value) = h
            .json(
                method,
                &uri,
                Some(serde_json::json!({"state": "active", "note": "x"})),
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(value["error"], "unknown_companion", "{uri}");
    }
    assert_eq!(h.instance_dirs(), vec![CANONICAL_SLUG]);
}
