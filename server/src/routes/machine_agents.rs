use axum::{
    Json, Router,
    extract::{
        Path, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;

use crate::app::state::AppState;
use crate::domain::machine::{KnownMachine, validate_machine_id};
use crate::services::machine_registry::{ActionResult, MachineError, MachineInfo};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/agents/ws/machine", get(upgrade))
        .route(
            "/api/instances/{instance_slug}/machine-hello",
            post(machine_hello),
        )
        .route(
            "/api/instances/{instance_slug}/machine-bye",
            post(machine_bye),
        )
        .route(
            "/api/instances/{instance_slug}/machines",
            get(list_machines),
        )
        .route(
            "/api/instances/{instance_slug}/machines/{machine_id}",
            axum::routing::put(rename_machine).delete(forget_machine),
        )
}

/// Store refusals as typed JSON so the client can explain them.
struct ApiError(MachineError);

impl From<MachineError> for ApiError {
    fn from(error: MachineError) -> Self {
        Self(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self.0 {
            MachineError::NotFound(_) => StatusCode::NOT_FOUND,
            MachineError::Invalid(_) => StatusCode::BAD_REQUEST,
            MachineError::Online(_) => StatusCode::CONFLICT,
            MachineError::Unsupported(_) => StatusCode::SERVICE_UNAVAILABLE,
            MachineError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (
            status,
            Json(serde_json::json!({
                "error": self.0.code(),
                "message": self.0.to_string(),
            })),
        )
            .into_response()
    }
}

/// Known computers (#80): every desktop that ever attached to the one
/// companion, online ones first, for the Computers page and Settings › Connections.
async fn list_machines(
    State(state): State<AppState>,
    Path(_instance_slug): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let machines = state.machine_registry.known().await?;
    Ok(Json(serde_json::json!({ "machines": machines })))
}

#[derive(Deserialize)]
struct RenameBody {
    /// Blank or null shows the hostname again.
    #[serde(default)]
    display_name: Option<String>,
}

/// The user's name for a computer, kept on the server so every client shows it.
async fn rename_machine(
    State(state): State<AppState>,
    Path((_instance_slug, machine_id)): Path<(String, String)>,
    Json(body): Json<RenameBody>,
) -> Result<Json<KnownMachine>, ApiError> {
    let machine = state
        .machine_registry
        .rename(&machine_id, body.display_name.as_deref())
        .await?;
    Ok(Json(machine))
}

/// Forget an offline computer: its record and name are dropped and every
/// client hears `machine_forgotten`. A connected one answers `409 machine_online`.
async fn forget_machine(
    State(state): State<AppState>,
    Path((_instance_slug, machine_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    state.machine_registry.forget(&machine_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Which computer a hello or bye is about. Absent means "the connected one"
/// when exactly one is; several connected computers are never guessed between.
#[derive(Deserialize, Default)]
struct MachineTarget {
    #[serde(default)]
    machine_id: Option<String>,
}

/// Why a hello or bye could not be addressed to one computer.
enum TargetRefusal {
    /// The named computer was never seen.
    NotFound(String),
    /// The named computer is known but not connected.
    Offline(String),
    /// Several are connected and none was named.
    Ambiguous(Vec<String>),
    Store(MachineError),
}

impl IntoResponse for TargetRefusal {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            Self::NotFound(id) => (
                StatusCode::NOT_FOUND,
                serde_json::json!({"error": "not_found", "message": format!("unknown machine {id}")}),
            ),
            Self::Offline(id) => (
                StatusCode::CONFLICT,
                serde_json::json!({"error": "machine_offline", "message": format!("machine {id} is not connected")}),
            ),
            Self::Ambiguous(ids) => (
                StatusCode::CONFLICT,
                serde_json::json!({
                    "error": "ambiguous_machine",
                    "message": "several computers are connected; choose one",
                    "machine_ids": ids,
                }),
            ),
            Self::Store(error) => return ApiError(error).into_response(),
        };
        (status, Json(body)).into_response()
    }
}

/// The connected computers a hello or bye addresses: the named one, or the
/// only one. `Ok(vec![])` means nobody is connected and there is nothing to do.
async fn resolve_targets(
    state: &AppState,
    requested: Option<&str>,
    all_when_unnamed: bool,
) -> Result<Vec<KnownMachine>, TargetRefusal> {
    let known = state
        .machine_registry
        .known()
        .await
        .map_err(TargetRefusal::Store)?;
    if let Some(id) = requested {
        return match known.into_iter().find(|machine| machine.machine_id == id) {
            Some(machine) if machine.online => Ok(vec![machine]),
            Some(_) => Err(TargetRefusal::Offline(id.to_owned())),
            None => Err(TargetRefusal::NotFound(id.to_owned())),
        };
    }
    let online: Vec<KnownMachine> = known.into_iter().filter(|machine| machine.online).collect();
    if online.len() > 1 && !all_when_unnamed {
        return Err(TargetRefusal::Ambiguous(
            online
                .into_iter()
                .map(|machine| machine.machine_id)
                .collect(),
        ));
    }
    Ok(online)
}

/// The user opened the companion in a browser: run the connection check-in
/// for the computer they named, or the only connected one. With several
/// connected and none named the caller must choose; nothing is guessed.
async fn machine_hello(
    State(state): State<AppState>,
    Path(instance_slug): Path<String>,
    body: Option<Json<MachineTarget>>,
) -> Result<StatusCode, TargetRefusal> {
    let requested = body.and_then(|Json(target)| target.machine_id);
    let targets = resolve_targets(&state, requested.as_deref(), false).await?;
    let Some(machine) = targets.into_iter().next() else {
        return Ok(StatusCode::OK);
    };

    tokio::spawn({
        let bg_state = state.clone();
        let slug = instance_slug.clone();
        async move {
            on_machine_connected(&bg_state, &machine.machine_id, Some(&slug)).await;
        }
    });

    Ok(StatusCode::OK)
}

/// Called when the user leaves the companion in a browser: logs which
/// connected computers stay attached, the named one or all of them.
async fn machine_bye(
    State(state): State<AppState>,
    Path(instance_slug): Path<String>,
    body: Option<Json<MachineTarget>>,
) -> Result<StatusCode, TargetRefusal> {
    let requested = body.and_then(|Json(target)| target.machine_id);
    let targets = resolve_targets(&state, requested.as_deref(), true).await?;
    if targets.is_empty() {
        return Ok(StatusCode::OK);
    }
    let names: Vec<String> = targets
        .iter()
        .map(|machine| format!("'{}'", machine.machine_id))
        .collect();
    let noun = if names.len() == 1 {
        "desktop"
    } else {
        "desktops"
    };
    let _ = crate::services::chat::save_system_message(
        &state.workspace_dir,
        &instance_slug,
        "default",
        &format!(
            "[system] user left this instance. {noun} {} still connected to server.",
            names.join(", ")
        ),
    );

    Ok(StatusCode::OK)
}

async fn upgrade(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(move |socket| handle_agent(socket, state))
}

/// What a desktop sends first on its socket. `machine_id` is the desktop's
/// persisted stable id (#80); older desktops send their hostname.
#[derive(Deserialize)]
struct Registration {
    machine_id: String,
    os: String,
    hostname: String,
    screen_width: u32,
    screen_height: u32,
    #[serde(default)]
    instance_slug: Option<String>,
    /// Desktop permission state, in the protocol's shape; absent from older desktops.
    #[serde(default)]
    permissions: Option<cua_protocol::PermissionState>,
    /// Action names the agent executes; absent from older desktops (legacy set).
    #[serde(default)]
    capabilities: Vec<String>,
}

impl Registration {
    /// The registry's view of this registration, seen at `now`. Labels and
    /// capabilities are handed over as reported; the registry bounds and
    /// normalizes them in one place.
    fn into_info(self, now: i64) -> MachineInfo {
        MachineInfo {
            platform: crate::domain::machine::platform_from_os(&self.os),
            machine_id: self.machine_id,
            os: self.os,
            hostname: self.hostname,
            screen_width: self.screen_width,
            screen_height: self.screen_height,
            last_seen: now,
            instance_slug: self.instance_slug,
            location: cua_protocol::MachineLocation::Desktop,
            permissions: self.permissions,
            capabilities: self.capabilities,
        }
    }
}

/// Message types from the agent.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AgentMessage {
    /// Agent registers itself on connect.
    Register(Registration),
    /// Agent sends back the result of a toolcall.
    ActionResult {
        request_id: String,
        #[serde(flatten)]
        result: ActionResult,
    },
    /// Heartbeat/ping from agent.
    Heartbeat { machine_id: String },
}

async fn handle_agent(mut socket: WebSocket, state: AppState) {
    // The agent must send a Register message first.
    let (machine_id, connection, mut agent_rx) =
        match wait_for_registration(&mut socket, &state).await {
            Some(v) => v,
            None => return,
        };

    log::info!("[machine-ws] agent '{machine_id}' connected");

    let mut ping_interval = tokio::time::interval(std::time::Duration::from_secs(15));
    ping_interval.tick().await; // skip first immediate tick

    loop {
        tokio::select! {
            // Forward toolcalls to the agent
            toolcall_msg = agent_rx.recv() => {
                match toolcall_msg {
                    Some(msg) => {
                        log::info!("[machine-ws] sending toolcall to '{machine_id}'");
                        if socket.send(Message::Text(msg.into())).await.is_err() {
                            log::warn!("[machine-ws] failed to send to '{machine_id}', disconnecting");
                            break;
                        }
                    }
                    None => break, // channel closed
                }
            }
            // Receive messages from the agent
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(msg) = serde_json::from_str::<AgentMessage>(&text) {
                            match msg {
                                AgentMessage::ActionResult { request_id, result } => {
                                    log::info!("[machine-ws] result from '{machine_id}' for {}", &request_id[..8.min(request_id.len())]);
                                    state.machine_registry.complete(&request_id, result).await;
                                }
                                AgentMessage::Heartbeat { machine_id: mid } => {
                                    state.machine_registry.heartbeat(&mid).await;
                                }
                                AgentMessage::Register(_) => {
                                    // Already registered, ignore duplicate
                                }
                            }
                        } else {
                            log::warn!("[machine-ws] unparseable message from '{machine_id}': {}", &text[..100.min(text.len())]);
                        }
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        if socket.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Pong(_))) => {
                        // Pong received — connection alive
                        state.machine_registry.heartbeat(&machine_id).await;
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        log::warn!("[machine-ws] error from '{machine_id}': {e}");
                        break;
                    }
                }
            }
            // Periodic ping to detect dead connections
            _ = ping_interval.tick() => {
                if socket.send(Message::Ping(vec![].into())).await.is_err() {
                    log::warn!("[machine-ws] ping failed for '{machine_id}', disconnecting");
                    break;
                }
            }
        }
    }

    log::info!("[machine-ws] agent '{machine_id}' disconnected");
    state
        .machine_registry
        .unregister_connection(&machine_id, connection)
        .await;
}

/// Wait for the agent to send a Register message.
/// Returns the machine ID, its connection number, and the receiver used to
/// forward tool calls. A registration with an unusable machine id is refused.
async fn wait_for_registration(
    socket: &mut WebSocket,
    state: &AppState,
) -> Option<(String, u64, tokio::sync::mpsc::UnboundedReceiver<String>)> {
    // Give agent 10s to register
    let deadline = tokio::time::sleep(std::time::Duration::from_secs(10));
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            _ = &mut deadline => {
                log::warn!("[machine-ws] agent timed out waiting for registration");
                return None;
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(AgentMessage::Register(registration)) =
                            serde_json::from_str::<AgentMessage>(&text)
                        {
                            if let Err(error) = validate_machine_id(&registration.machine_id) {
                                log::warn!("[machine-ws] registration refused: {error}");
                                let refusal = serde_json::json!({"type": "error", "error": "invalid_machine_id", "message": error});
                                let _ = socket.send(Message::Text(refusal.to_string().into())).await;
                                return None;
                            }
                            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                            let machine_id = registration.machine_id.clone();
                            let info = registration.into_info(chrono::Utc::now().timestamp());
                            let connection = state.machine_registry.register(info, tx).await;

                            // Send ack
                            let ack = serde_json::json!({"type": "registered", "machine_id": machine_id});
                            let _ = socket.send(Message::Text(serde_json::to_string(&ack).unwrap().into())).await;

                            return Some((machine_id, connection, rx));
                        }
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        let _ = socket.send(Message::Pong(payload)).await;
                    }
                    Some(Ok(Message::Close(_))) | None => return None,
                    _ => {}
                }
            }
        }
    }
}

/// When a desktop machine connects, the companion check-in runs once with a
/// connection task, as one run in the proactive loop (#92, #93).
pub(crate) async fn on_machine_connected(
    state: &AppState,
    machine_id: &str,
    bound_slug: Option<&str>,
) {
    use crate::domain::companion::{CANONICAL_SLUG, is_canonical};
    use crate::domain::proactive::{Target, Trigger};
    use crate::services::companion_routine::{self, Routine};
    use crate::services::proactive::{Admission, outcome_from_trace};

    if let Some(slug) = bound_slug
        && !is_canonical(slug)
    {
        log::warn!(
            "[machine-connect] '{machine_id}' asked for foreign companion {slug:?}; this server owns only {CANONICAL_SLUG}"
        );
        return;
    }
    let instance_dir = crate::services::companion::companion_dir(&state.workspace_dir);
    if !instance_dir.join("soul.md").exists() {
        log::warn!(
            "[machine-connect] '{machine_id}' connected before the companion was onboarded; nothing to notify"
        );
        return;
    }

    // A commitment waiting on this computer stops waiting and is checked on
    // the next evaluator tick (#85).
    let observed = state.commitments.observe_event(
        &crate::services::commitment_evaluator::machine_connected_event(machine_id),
        chrono::Utc::now().timestamp(),
    );
    if !observed.is_empty() {
        log::info!(
            "[machine-connect] '{machine_id}' resolves {} waiting commitment(s)",
            observed.len()
        );
    }

    // Waiting work that names this computer may be offered for resumption (#83).
    crate::services::resume_ritual::on_machine_connected(state, machine_id).await;

    // Reconnect bursts are deduplicated and rate-limited by the loop.
    let handle = match state.proactive.begin(
        Trigger::MachineConnected {
            machine_id: machine_id.to_owned(),
        },
        "desktop connected",
        Target::Machine {
            machine_id: machine_id.to_owned(),
        },
    ) {
        Admission::Admitted(handle) => handle,
        Admission::Skipped(run) => {
            log::info!(
                "[machine-connect] '{machine_id}' skipped ({:?})",
                run.status
            );
            return;
        }
    };

    let slug = CANONICAL_SLUG;
    let msg = format!("[system] desktop '{machine_id}' connected.");
    if let Err(e) =
        crate::services::chat::save_system_message(&state.workspace_dir, slug, "default", &msg)
    {
        log::error!("[machine-connect] failed to save system message: {e}");
    }

    let llm_guard = state.background_llm.read().await;
    let Some(llm) = llm_guard.as_ref() else {
        handle.fail("background model preset not configured", true);
        return;
    };
    let task = format!(
        "the user's desktop computer '{machine_id}' just connected.\n\
         USE reach_out NOW to let the user know their computer is connected. \
         keep it brief and friendly."
    );
    let ws = state.workspace_dir.clone();
    let events = state.events.clone();
    let vs = state.vector_store.clone();
    let llm_c = llm.clone();
    let resources = state.resources.clone();
    let proactive = state.proactive.clone();
    tokio::spawn(async move {
        let run_id = handle.id().to_owned();
        match companion_routine::run(
            &ws,
            slug,
            &instance_dir,
            &llm_c,
            &events,
            &vs,
            &resources,
            Routine::CheckIn,
            Some(&task),
            "machine_connected",
            (&proactive, run_id.as_str()),
        )
        .await
        {
            Ok(r) => {
                log::info!(
                    "[machine-connect] companion reach_out done ({} tokens)",
                    r.tokens
                );
                handle.complete(outcome_from_trace(&r.trace, r.tokens));
            }
            Err(e) => {
                log::error!("[machine-connect] companion failed: {e}");
                let err_msg =
                    format!("[system] failed to notify companion about desktop connection: {e}");
                if let Ok(message) =
                    crate::services::chat::save_system_message(&ws, slug, "default", &err_msg)
                {
                    let _ = events.send(crate::domain::events::ServerEvent::ChatMessageCreated {
                        instance_slug: slug.to_string(),
                        chat_id: "default".to_string(),
                        message,
                    });
                }
                handle.fail(&e.to_string(), true);
            }
        }
    });
}

#[cfg(test)]
mod registration_tests {
    use super::*;
    use crate::domain::machine::LEGACY_DESKTOP_CAPABILITIES;
    use crate::services::machine_registry::MachineRegistry;

    const T0: i64 = 1_767_603_600;
    const STABLE_ID: &str = "4f3c1c2e-9b5e-4d2b-8f0a-1c2d3e4f5a6b";

    fn registration(extra: serde_json::Value) -> Registration {
        let mut frame = serde_json::json!({
            "type": "register",
            "machine_id": STABLE_ID,
            "os": "macos",
            "hostname": "studio",
            "screen_width": 1440,
            "screen_height": 900,
        });
        frame
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let AgentMessage::Register(registration) =
            serde_json::from_str(&frame.to_string()).unwrap()
        else {
            panic!("not a registration");
        };
        registration
    }

    /// Review finding: the route normalized capabilities and the registry did
    /// it again, so a report with only invalid names became the legacy set.
    #[tokio::test]
    async fn capabilities_are_normalized_once_so_only_invalid_names_record_none() {
        let info =
            registration(serde_json::json!({"capabilities": ["Has Space", "UPPER"]})).into_info(T0);
        assert_eq!(
            info.capabilities,
            vec!["Has Space".to_owned(), "UPPER".to_owned()],
            "the route hands the report over untouched; the registry is the one normalizer"
        );
        assert_eq!(info.last_seen, T0);
        assert_eq!(info.platform, Some(cua_protocol::Platform::Macos));
        assert_eq!(info.location, cua_protocol::MachineLocation::Desktop);

        let registry = MachineRegistry::new();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        registry.register(info, tx).await;
        let known = registry.get_known(STABLE_ID, T0).await.unwrap();
        assert_eq!(
            known.capabilities,
            Vec::<String>::new(),
            "nothing valid reported: it reports nothing, not the legacy set"
        );

        // A desktop that predates capability reporting still gets the legacy set.
        let legacy = registration(serde_json::json!({})).into_info(T0);
        assert!(legacy.capabilities.is_empty());
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        registry.register(legacy, tx).await;
        assert_eq!(
            registry
                .get_known(STABLE_ID, T0)
                .await
                .unwrap()
                .capabilities,
            LEGACY_DESKTOP_CAPABILITIES
                .iter()
                .map(|s| (*s).to_owned())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn labels_are_handed_over_untouched_and_bounded_by_the_registry() {
        let info = registration(serde_json::json!({
            "os": "o".repeat(5_000),
            "hostname": "h".repeat(5_000),
            "permissions": {"accessibility": "granted", "screen_capture": "denied"},
        }))
        .into_info(T0);
        assert_eq!(info.os.len(), 5_000);
        assert_eq!(info.hostname.len(), 5_000);
        assert_eq!(
            info.permissions.as_ref().unwrap().screen_capture,
            cua_protocol::Permission::Denied
        );
    }
}
