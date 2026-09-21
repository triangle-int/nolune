use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
};
use std::io::ErrorKind;
use tokio_util::sync::CancellationToken;

use crate::{
    app::state::AppState,
    config,
    domain::{
        chat::{AgentLoopExit, ChatMessage, ChatRequest, ChatResponse, ChatRole, ChatSummary},
        events::ServerEvent,
    },
    services::chat,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/chat", post(post_chat))
        .route("/api/chat/{instance_slug}/chats", get(list_chats))
        .route(
            "/api/chat/{instance_slug}/messages",
            get(get_messages_default),
        )
        .route(
            "/api/chat/{instance_slug}/{chat_id}/messages",
            get(get_messages),
        )
        .route("/api/chat/{instance_slug}/{chat_id}/stop", post(stop_agent))
        .route(
            "/api/chat/{instance_slug}/{chat_id}/context",
            delete(clear_context),
        )
        // Legacy routes (use default chat_id)
        .route("/api/chat/{instance_slug}/stop", post(stop_agent_default))
        .route(
            "/api/chat/{instance_slug}/{chat_id}/preset",
            get(get_chat_preset).put(update_chat_preset),
        )
        .route(
            "/api/chat/{instance_slug}/context",
            delete(clear_context_default),
        )
}

/// The `agent_tasks` key of one conversation's agent loop.
pub(crate) fn task_key(slug: &str, chat_id: &str) -> String {
    format!("{slug}/{chat_id}")
}

async fn post_chat(
    State(state): State<AppState>,
    Json(request): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, axum::response::Response> {
    // The body names the companion; admit it exactly like a path parameter,
    // holding the import gate (#74) until the message is saved and the
    // agent loop is registered, so an import never interleaves with either.
    let _writer = state
        .vector_store
        .media_store()
        .import_gate()
        .writer()
        .await;
    crate::app::companion_boundary::admit(
        &state.workspace_dir,
        &request.instance_slug,
        &axum::http::Method::POST,
    )
    .map_err(IntoResponse::into_response)?;
    super::require_provider(&state)
        .await
        .map_err(IntoResponse::into_response)?;
    let instance_slug = request.instance_slug.clone();
    let chat_id = request.chat_id.clone();
    let content = request.content.trim().to_string();
    let voice_mode = request.voice_mode;
    // The computer the user chose (#80) travels with the run that this
    // message starts; a running loop keeps the target it started with. It
    // is checked like a registered id before it reaches the prompt or the
    // log, and the refusal names the rule rather than echoing it.
    crate::services::tools::TargetSelection::check_request(request.machine_id.as_deref())
        .map_err(|reason| (StatusCode::BAD_REQUEST, reason).into_response())?;
    let machine_target = request.machine_id.clone();

    if content.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "content required").into_response());
    }

    // Save user message immediately
    let user_message =
        chat::save_user_message(&state.workspace_dir, &instance_slug, &chat_id, &content)
            .map_err(|error| map_chat_error(error).into_response())?;

    // Broadcast user message
    let _ = state.events.send(ServerEvent::ChatMessageCreated {
        instance_slug: instance_slug.clone(),
        chat_id: chat_id.clone(),
        message: user_message.clone(),
    });

    let key = task_key(&instance_slug, &chat_id);

    // If an agent is already running for this chat, don't start another one.
    // The running agent will pick up the new message on its next turn
    // (it re-reads messages from disk each iteration).
    let already_running = {
        let tasks = state.agent_tasks.lock().await;
        tasks.contains_key(&key)
    };

    if !already_running {
        let cancel = CancellationToken::new();
        {
            let mut tasks = state.agent_tasks.lock().await;
            // Double-check after re-acquiring lock
            if !tasks.contains_key(&key) {
                tasks.insert(key.clone(), cancel.clone());
            }
        }

        let bg_state = state.clone();
        let bg_chat_id = chat_id.clone();
        tokio::spawn(async move {
            run_agent_loop(
                bg_state,
                instance_slug,
                bg_chat_id,
                cancel,
                voice_mode,
                machine_target,
            )
            .await;
        });
    }

    // Return immediately with the user message
    Ok(Json(ChatResponse {
        instance_slug: request.instance_slug,
        chat_id: request.chat_id,
        messages: vec![user_message],
        agent_running: true,
    }))
}

/// Agent loop: keeps calling the LLM until it responds without tool use or is cancelled.
/// New user messages are automatically picked up because each turn re-reads from disk.
/// `machine_target` is the computer the user chose for this run (#80): a
/// known machine's stable id or `server-home`; `None` leaves the desktop
/// tools to the only connected computer and refuses several.
pub async fn run_agent_loop(
    state: AppState,
    instance_slug: String,
    chat_id: String,
    cancel: CancellationToken,
    voice_mode: bool,
    mut machine_target: Option<String>,
) -> AgentLoopExit {
    let key = task_key(&instance_slug, &chat_id);
    let _ = state.events.send(ServerEvent::AgentRunning {
        instance_slug: instance_slug.clone(),
        chat_id: chat_id.clone(),
    });

    // Persist marker so we can detect interrupted agents across restarts
    chat::set_agent_running(&state.workspace_dir, &instance_slug, &chat_id);

    // Background TTS: subscribe to events and voice ALL assistant messages
    // TTS pipeline: forwarder task → mpsc channel → synthesizer task.
    // Forwarder stops when cancel fires. Synthesizer drains remaining
    // messages in the channel before exiting (no abort, no message loss).
    let tts_cancel = cancel.clone();
    let (tts_fwd_handle, tts_synth_handle) = if voice_mode {
        let (tts_tx, mut tts_rx) = tokio::sync::mpsc::unbounded_channel::<ChatMessage>();
        let mut rx = state.events.subscribe();
        let fwd_slug = instance_slug.clone();
        let fwd_chat = chat_id.clone();
        let fwd_cancel = tts_cancel.clone();

        // Forwarder: drains broadcast into mpsc. Stops when cancel fires,
        // dropping tts_tx so the synthesizer's recv() returns None.
        let fwd = tokio::spawn(async move {
            while !fwd_cancel.is_cancelled() {
                let event = match rx.recv().await {
                    Ok(e) => e,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        log::warn!("[tts] broadcast lagged by {n} events");
                        continue;
                    }
                    Err(_) => break,
                };
                if let ServerEvent::ChatMessageCreated {
                    instance_slug: ref slug,
                    chat_id: ref cid,
                    ref message,
                } = event
                {
                    if slug != &fwd_slug || cid != &fwd_chat {
                        continue;
                    }
                    if message.role != ChatRole::Assistant {
                        continue;
                    }
                    if message.content.trim().is_empty() {
                        continue;
                    }
                    if message.kind == crate::domain::chat::MessageKind::ToolCall
                        || message.kind == crate::domain::chat::MessageKind::ToolOutput
                    {
                        continue;
                    }
                    let _ = tts_tx.send(message.clone());
                }
            }
            // tts_tx is dropped here → synthesizer's recv() will return None
            // after processing remaining queued messages
        });

        let tts_state = state.clone();
        let tts_slug = instance_slug.clone();
        let tts_chat = chat_id.clone();

        // Synthesizer: processes all messages until channel is drained
        let synth = tokio::spawn(async move {
            let api_key = {
                let cfg = tts_state.config.read().await;
                cfg.llm.tokens.elevenlabs.clone()
            };
            if api_key.is_empty() {
                return;
            }

            while let Some(message) = tts_rx.recv().await {
                let voice_id =
                    crate::routes::tts::resolve_voice_id(&tts_state.workspace_dir, &tts_slug);
                let idir = tts_state.workspace_dir.join("instances").join(&tts_slug);
                let mood = crate::services::tools::companion::load_mood_state(&idir).companion_mood;
                match crate::routes::tts::synthesize_bytes(
                    &tts_state.http_client,
                    &api_key,
                    &voice_id,
                    &message.content,
                    &mood,
                )
                .await
                {
                    Ok(audio_bytes) => {
                        use base64::Engine;
                        let audio_base64 =
                            base64::engine::general_purpose::STANDARD.encode(&audio_bytes);
                        let _ = tts_state.events.send(ServerEvent::ChatAudioReady {
                            instance_slug: tts_slug.clone(),
                            chat_id: tts_chat.clone(),
                            audio_base64,
                            message_ids: vec![message.id.clone()],
                        });
                    }
                    Err(e) => log::warn!("[tts] failed for {}: {e}", message.id),
                }
            }
        });

        (Some(fwd), Some(synth))
    } else {
        (None, None)
    };

    const MAX_ITERATIONS: usize = 5;
    let mut iteration = 0;
    // Why the loop stops, recorded for whoever waits on this conversation.
    let mut exit = AgentLoopExit::Finished;

    loop {
        if cancel.is_cancelled() {
            log::info!("[agent] {instance_slug}/{chat_id} — cancelled by user");
            exit = AgentLoopExit::Cancelled;
            break;
        }

        if iteration >= MAX_ITERATIONS {
            log::info!("[agent] {instance_slug}/{chat_id} — reached max iterations");
            break;
        }

        iteration += 1;

        // A request queued on this conversation since the last turn (a
        // handoff accepted while it ran, #82) names the computer this turn
        // acts on; otherwise the loop keeps the target it started with.
        machine_target = next_turn_target(&state, &key, machine_target).await;

        let config_path = config::config_path();

        // Resolve the model for this turn (#156): the chat's pinned preset,
        // else the Chat slot. Background work always uses the Background slot.
        let pinned = chat::get_chat_preset(&state.workspace_dir, &instance_slug, &chat_id)
            .ok()
            .flatten();
        let (effective_llm, background_llm, public_url) = {
            let cfg = state.config.read().await;
            let public_url = cfg.public_url.clone();
            let pinned_llm = pinned.as_deref().and_then(|id| {
                match crate::services::llm::LlmBackend::for_preset(
                    &cfg,
                    state.http_client.clone(),
                    id,
                ) {
                    Ok(backend) => Some(backend),
                    Err(error) => {
                        log::warn!(
                            "[agent] {instance_slug}/{chat_id} — pinned preset {id:?} unavailable ({error}); using the Chat slot"
                        );
                        None
                    }
                }
            });
            drop(cfg);
            let effective = match pinned_llm {
                Some(backend) => Some(backend),
                None => state.llm.read().await.clone(),
            };
            (
                effective,
                state.background_llm.read().await.clone(),
                public_url,
            )
        };
        let Some(effective_llm) = effective_llm else {
            log::warn!("[agent] {instance_slug}/{chat_id} — no LLM configured");
            exit = AgentLoopExit::NoModel;
            break;
        };
        if background_llm.is_none() {
            log::warn!(
                "[agent] {instance_slug}/{chat_id} — background preset unavailable; memory extraction is skipped this turn"
            );
        }

        // Timeout must exceed stream item timeout (480s) to allow long tools to complete.
        const TURN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);

        log::info!(
            "[agent] {instance_slug}/{chat_id} — preset={} model={}",
            effective_llm.preset,
            effective_llm.model
        );

        let turn_fut = chat::run_single_turn(
            &state.workspace_dir,
            &config_path,
            &instance_slug,
            &chat_id,
            &effective_llm,
            background_llm.as_ref(),
            state.events.clone(),
            state.pending_secrets.clone(),
            &state.mcp_registry,
            voice_mode,
            state.vector_store.clone(),
            state.agent_tasks.clone(),
            state.machine_registry.clone(),
            machine_target.as_deref(),
            &public_url,
            &state.resources,
        );

        let result = tokio::select! {
            r = tokio::time::timeout(TURN_TIMEOUT, turn_fut) => {
                match r {
                    Ok(inner) => inner,
                    Err(_) => {
                        log::warn!("[agent] {instance_slug}/{chat_id} — turn timed out after {}s", TURN_TIMEOUT.as_secs());
                        Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "turn timed out — the model or a tool took too long"))
                    }
                }
            }
            _ = cancel.cancelled() => {
                log::info!("[agent] {instance_slug}/{chat_id} — cancelled during turn");
                exit = AgentLoopExit::Cancelled;
                break;
            }
        };

        // Reload config in case LLM changed it
        state.reload_config().await;

        match result {
            Ok(turn) => {
                for msg in &turn.messages {
                    let _ = state.events.send(ServerEvent::ChatMessageCreated {
                        instance_slug: instance_slug.clone(),
                        chat_id: chat_id.clone(),
                        message: msg.clone(),
                    });
                    // TTS handled by background subscriber task
                }

                // Check if a new user message arrived while agent was processing
                if has_pending_user_message(&state, &instance_slug, &chat_id).await {
                    log::info!(
                        "[agent] {instance_slug}/{chat_id} — new user message arrived, continuing"
                    );
                    continue;
                }
                break;
            }
            Err(e) => {
                use crate::services::llm::contract::LlmError;
                let msg = e.to_string();
                log::warn!("[agent] {instance_slug}/{chat_id} — error: {msg}");

                // Provider failures arrive typed; the strings below are the
                // turn timeout and the missing-backend cases, not the API.
                let llm_error = e.get_ref().and_then(|e| e.downcast_ref::<LlmError>());
                let error_label = match llm_error {
                    Some(LlmError::SetupRequired(_)) => msg.as_str(),
                    Some(LlmError::RateLimited { .. }) => "rate limited — try again in a moment",
                    Some(LlmError::Timeout) => "request timed out",
                    Some(LlmError::Authentication(_)) => {
                        "not authenticated — check the API key in Settings → Provider"
                    }
                    Some(LlmError::ContextLength(_)) => {
                        "this conversation no longer fits the model's context — clear context or start a new chat"
                    }
                    _ if msg.contains("timed out") => "request timed out",
                    _ if msg.contains("no LLM") || msg.contains("not configured") => {
                        "no API key configured — add one in Settings"
                    }
                    _ => "something went wrong",
                };
                let error_msg = chat::save_system_message(
                    &state.workspace_dir,
                    &instance_slug,
                    &chat_id,
                    &format!("[system] {error_label}"),
                );
                if let Ok(m) = error_msg {
                    let _ = state.events.send(ServerEvent::ChatMessageCreated {
                        instance_slug: instance_slug.clone(),
                        chat_id: chat_id.clone(),
                        message: m,
                    });
                }
                exit = AgentLoopExit::Failed {
                    error: error_label.to_owned(),
                };
                break;
            }
        }
    }

    // Auto-generate title if missing
    if let Ok(response) = chat::load_messages(&state.workspace_dir, &instance_slug, &chat_id) {
        let needs_title = chat::get_chat_title(&state.workspace_dir, &instance_slug, &chat_id)
            .map(|t| t.is_empty())
            .unwrap_or(true);

        if needs_title && !response.messages.is_empty() {
            let llm_guard = state.background_llm.read().await;
            if let Some(llm) = llm_guard.as_ref() {
                let snippet: String = response
                    .messages
                    .iter()
                    .take(6)
                    .map(|m| {
                        format!(
                            "{}: {}",
                            if m.role == ChatRole::User {
                                "user"
                            } else {
                                "assistant"
                            },
                            m.content.chars().take(200).collect::<String>()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                let prompt = format!(
                    "Generate a very short title (3-6 words, no quotes) for this conversation:\n\n{snippet}"
                );

                if let Ok((title, _)) = llm.chat("You generate short chat titles. Respond with only the title, nothing else.", &prompt, vec![]).await {
                    let title = title.trim().trim_matches('"').to_string();
                    let _ = chat::update_chat_title(&state.workspace_dir, &instance_slug, &chat_id, &title);
                }
            }
        }
    }

    // Clean up
    chat::clear_agent_running(&state.workspace_dir, &instance_slug, &chat_id);

    // The reason is on record before the key is released, so a follower
    // that sees the conversation idle can read it; a target queued on this
    // loop that it never took is released with the key.
    state.release_agent(&key, exit.clone()).await;

    // Final snapshot — client gets complete state before agent_stopped
    send_snapshot(&state, &instance_slug, &chat_id, false);

    let _ = state.events.send(ServerEvent::AgentStopped {
        instance_slug: instance_slug.clone(),
        chat_id: chat_id.clone(),
    });

    // Stop the TTS forwarder (drops sender → synthesizer drains remaining queue)
    if let Some(fwd) = tts_fwd_handle {
        fwd.abort(); // forwarder can be killed immediately
    }
    // Wait for synthesizer to finish processing queued messages (up to 30s)
    if let Some(synth) = tts_synth_handle {
        let _ = tokio::time::timeout(std::time::Duration::from_secs(30), synth).await;
    }
    exit
}

/// Send a full chat state snapshot so all clients converge to the same state.
/// The computer the next turn of the conversation `key` acts on (#80): what
/// a request queued on it since the last turn asked for, else what the loop
/// started with (`current`). A queued target is taken once and carried by
/// the loop from then on.
pub(crate) async fn next_turn_target(
    state: &AppState,
    key: &str,
    current: Option<String>,
) -> Option<String> {
    match state.take_queued_target(key).await {
        Some(queued) => {
            log::info!(
                "[agent] {key} — a queued request re-targets this conversation to {queued:?}"
            );
            Some(queued)
        }
        None => current,
    }
}

fn send_snapshot(state: &AppState, instance_slug: &str, chat_id: &str, agent_running: bool) {
    match chat::load_messages(&state.workspace_dir, instance_slug, chat_id) {
        Ok(resp) => {
            let _ = state.events.send(ServerEvent::ChatSnapshot {
                instance_slug: instance_slug.to_string(),
                chat_id: chat_id.to_string(),
                messages: resp.messages,
                agent_running,
            });
        }
        Err(e) => log::warn!("[snapshot] failed to load messages: {e}"),
    }
}

/// Check if the last message in the chat is from the user (meaning they sent something
/// while the agent was processing and we should do another turn).
#[derive(serde::Deserialize)]
struct ChatPresetRequest {
    /// Preset id to pin, or null to follow the Chat slot.
    preset: Option<String>,
}

async fn chat_preset_json(
    state: &AppState,
    instance_slug: &str,
    chat_id: &str,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let pinned = chat::get_chat_preset(&state.workspace_dir, instance_slug, chat_id)
        .map_err(map_chat_error)?;
    let cfg = state.config.read().await;
    let effective = pinned
        .as_deref()
        .filter(|id| cfg.llm.preset(id).is_some())
        .map(str::to_owned)
        .unwrap_or_else(|| cfg.llm.chat_preset.clone());
    Ok(serde_json::json!({
        "preset": pinned,
        "effective_preset": effective,
        "default_preset": cfg.llm.chat_preset,
    }))
}

/// Per-conversation model preset (#156).
async fn get_chat_preset(
    State(state): State<AppState>,
    Path((instance_slug, chat_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    Ok(Json(
        chat_preset_json(&state, &instance_slug, &chat_id).await?,
    ))
}

async fn update_chat_preset(
    State(state): State<AppState>,
    Path((instance_slug, chat_id)): Path<(String, String)>,
    Json(request): Json<ChatPresetRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let preset = request
        .preset
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty());
    if let Some(id) = preset {
        let cfg = state.config.read().await;
        if cfg.llm.preset(id).is_none() {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("unknown model preset {id:?}"),
            ));
        }
    }
    chat::set_chat_preset(&state.workspace_dir, &instance_slug, &chat_id, preset)
        .map_err(map_chat_error)?;
    Ok(Json(
        chat_preset_json(&state, &instance_slug, &chat_id).await?,
    ))
}

async fn has_pending_user_message(state: &AppState, instance_slug: &str, chat_id: &str) -> bool {
    match chat::load_messages(&state.workspace_dir, instance_slug, chat_id) {
        Ok(response) => response
            .messages
            .last()
            .is_some_and(|m| m.role == ChatRole::User),
        Err(_) => false,
    }
}

async fn list_chats(
    State(state): State<AppState>,
    Path(instance_slug): Path<String>,
) -> Result<Json<Vec<ChatSummary>>, (StatusCode, String)> {
    let chats = chat::list_chats(&state.workspace_dir, &instance_slug).map_err(map_chat_error)?;
    Ok(Json(chats))
}

async fn get_messages_default(
    State(state): State<AppState>,
    Path(instance_slug): Path<String>,
) -> Result<Json<ChatResponse>, super::ProviderRequestError> {
    super::require_provider(&state).await?;
    let mut response = chat::load_messages(&state.workspace_dir, &instance_slug, "default")
        .map_err(map_chat_error)?;
    response.agent_running = is_agent_running(&state, &instance_slug, "default").await;
    Ok(Json(response))
}

async fn get_messages(
    State(state): State<AppState>,
    Path((instance_slug, chat_id)): Path<(String, String)>,
) -> Result<Json<ChatResponse>, super::ProviderRequestError> {
    super::require_provider(&state).await?;
    let mut response = chat::load_messages(&state.workspace_dir, &instance_slug, &chat_id)
        .map_err(map_chat_error)?;
    response.agent_running = is_agent_running(&state, &instance_slug, &chat_id).await;
    Ok(Json(response))
}

async fn is_agent_running(state: &AppState, instance_slug: &str, chat_id: &str) -> bool {
    let key = task_key(instance_slug, chat_id);
    let tasks = state.agent_tasks.lock().await;
    tasks.contains_key(&key)
}

async fn stop_agent(
    State(state): State<AppState>,
    Path((instance_slug, chat_id)): Path<(String, String)>,
) -> StatusCode {
    let key = task_key(&instance_slug, &chat_id);
    let mut tasks = state.agent_tasks.lock().await;
    if let Some(token) = tasks.remove(&key) {
        token.cancel();
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
}

async fn stop_agent_default(
    State(state): State<AppState>,
    Path(instance_slug): Path<String>,
) -> StatusCode {
    let key = task_key(&instance_slug, "default");
    let mut tasks = state.agent_tasks.lock().await;
    if let Some(token) = tasks.remove(&key) {
        token.cancel();
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
}

async fn clear_context(
    State(state): State<AppState>,
    Path((instance_slug, chat_id)): Path<(String, String)>,
) -> StatusCode {
    chat::clear_context(&state.workspace_dir, &instance_slug, &chat_id);
    StatusCode::OK
}

async fn clear_context_default(
    State(state): State<AppState>,
    Path(instance_slug): Path<String>,
) -> StatusCode {
    chat::clear_context(&state.workspace_dir, &instance_slug, "default");
    StatusCode::OK
}

fn map_chat_error(error: std::io::Error) -> (StatusCode, String) {
    let status = match error.kind() {
        ErrorKind::InvalidInput => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };

    (status, error.to_string())
}

#[cfg(test)]
mod queued_target_tests {
    //! #80: a request queued on a running conversation (a handoff accepted
    //! while it ran, #82) names the computer its next turn acts on.
    use super::*;
    use crate::domain::companion::CANONICAL_SLUG;

    const STUDIO: &str = "4f3c1c2e-9b5e-4d2b-8f0a-1c2d3e4f5a6b";
    const LAPTOP: &str = "9a8b7c6d-5e4f-4a3b-9c2d-1e0f9a8b7c6d";

    async fn state() -> (tempfile::TempDir, AppState) {
        let tmp = tempfile::tempdir().unwrap();
        let state = AppState::new_in(config::Config::default(), tmp.path().join("workspace")).await;
        (tmp, state)
    }

    #[tokio::test]
    async fn the_next_turn_takes_the_queued_computer_once_and_keeps_it_after() {
        let (_tmp, state) = state().await;
        let key = task_key(CANONICAL_SLUG, "chat_1");
        // Nothing queued: the loop keeps what it started with.
        assert_eq!(next_turn_target(&state, &key, None).await, None);
        assert_eq!(
            next_turn_target(&state, &key, Some(STUDIO.into()))
                .await
                .as_deref(),
            Some(STUDIO)
        );
        // Queued: the next turn acts there, whatever the loop started with.
        state
            .agent_targets
            .lock()
            .await
            .insert(key.clone(), LAPTOP.to_owned());
        assert_eq!(
            next_turn_target(&state, &key, Some(STUDIO.into()))
                .await
                .as_deref(),
            Some(LAPTOP)
        );
        // Taken once; the loop carries it from there.
        assert_eq!(state.take_queued_target(&key).await, None);
        assert_eq!(
            next_turn_target(&state, &key, Some(LAPTOP.into()))
                .await
                .as_deref(),
            Some(LAPTOP)
        );
        // Another conversation's queue is not this one's.
        state
            .agent_targets
            .lock()
            .await
            .insert(task_key(CANONICAL_SLUG, "chat_2"), LAPTOP.to_owned());
        assert_eq!(next_turn_target(&state, &key, None).await, None);
    }

    #[tokio::test]
    async fn the_loop_takes_the_queued_computer_at_the_start_of_a_turn_and_releases_it_with_the_key()
     {
        let (_tmp, state) = state().await;
        let key = task_key(CANONICAL_SLUG, "chat_1");
        state
            .agent_tasks
            .lock()
            .await
            .insert(key.clone(), CancellationToken::new());
        state
            .agent_targets
            .lock()
            .await
            .insert(key.clone(), LAPTOP.to_owned());
        // No model is configured: the turn stops before a provider is
        // reached, right after the loop settled the turn's target.
        let exit = run_agent_loop(
            state.clone(),
            CANONICAL_SLUG.to_owned(),
            "chat_1".to_owned(),
            CancellationToken::new(),
            false,
            Some(STUDIO.to_owned()),
        )
        .await;
        assert_eq!(exit, AgentLoopExit::NoModel);
        assert_eq!(
            state.take_queued_target(&key).await,
            None,
            "the loop took the queued computer for its turn"
        );
        assert!(
            !state.agent_tasks.lock().await.contains_key(&key),
            "the loop released the conversation"
        );

        // A target queued for a loop that stopped with it untaken does not
        // reach the next loop: what that loop is started with wins.
        state
            .agent_tasks
            .lock()
            .await
            .insert(key.clone(), CancellationToken::new());
        state
            .agent_targets
            .lock()
            .await
            .insert(key.clone(), LAPTOP.to_owned());
        state.release_agent(&key, AgentLoopExit::Finished).await;
        assert_eq!(state.take_queued_target(&key).await, None);
        assert_eq!(
            state.agent_exits.lock().await.get(&key),
            Some(&AgentLoopExit::Finished)
        );
    }
}

#[cfg(test)]
mod request_target_tests {
    //! #80: the request's `machine_id` is checked the way registration
    //! checks one before it reaches the prompt, the log or a refusal.
    use super::*;
    use crate::{
        config::{Config, LlmProvider, ModelPreset},
        domain::{companion::CANONICAL_SLUG, machine::MAX_MACHINE_ID_BYTES},
        services::companion,
    };
    use axum::{
        body::Body,
        http::{Method, Request, header},
    };
    use tower::ServiceExt;

    const TOKEN: &str = "issue-80-request-token";

    /// A companion with a chat model configured, so a well-formed message
    /// would start a run; nothing here reaches a provider.
    async fn state() -> (tempfile::TempDir, AppState) {
        let tmp = tempfile::tempdir().unwrap();
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
        let state = AppState::new_in(config, tmp.path().to_owned()).await;
        companion::ensure_identity(tmp.path()).unwrap();
        (tmp, state)
    }

    async fn post_chat(state: &AppState, body: serde_json::Value) -> (StatusCode, String) {
        let response = crate::app::router::build_router(state.clone(), None)
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/chat")
                    .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    #[tokio::test]
    async fn a_malformed_machine_id_is_refused_before_anything_is_saved_or_started() {
        let (tmp, state) = state().await;
        let key = task_key(CANONICAL_SLUG, "chat_1");
        let too_long = "a".repeat(MAX_MACHINE_ID_BYTES + 1);
        for bad in [
            too_long.as_str(),
            "studio mac",
            "studio\nmac",
            "<b>studio</b>",
            "studio\u{7f}",
        ] {
            let (status, body) = post_chat(
                &state,
                serde_json::json!({
                    "instance_slug": CANONICAL_SLUG,
                    "chat_id": "chat_1",
                    "content": "hi",
                    "machine_id": bad,
                }),
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{bad:?}: {body}");
            assert!(
                body.contains("machine id"),
                "{bad:?}: the refusal says what is wrong: {body}"
            );
            assert!(
                !body.contains(bad),
                "{bad:?}: the refusal does not echo the id: {body}"
            );
        }
        assert!(
            !state.agent_tasks.lock().await.contains_key(&key),
            "no run was started"
        );
        let saved = chat::load_messages(tmp.path(), CANONICAL_SLUG, "chat_1").unwrap();
        assert!(saved.messages.is_empty(), "no message was saved: {saved:?}");
    }

    #[test]
    fn the_request_target_is_checked_like_a_registration() {
        use crate::services::tools::TargetSelection;
        // Nothing chosen, the home, a server-local id and a stable id pass.
        for ok in [
            None,
            Some(""),
            Some("  "),
            Some("server-home"),
            Some("server-local:studio"),
            Some("4f3c1c2e-9b5e-4d2b-8f0a-1c2d3e4f5a6b"),
            Some("Studio_Mac.local"),
        ] {
            assert_eq!(TargetSelection::check_request(ok), Ok(()), "{ok:?}");
        }
        let too_long = "a".repeat(MAX_MACHINE_ID_BYTES + 1);
        for bad in [too_long.as_str(), "studio mac", "studio\nmac", "<b>x</b>"] {
            let error = TargetSelection::check_request(Some(bad)).unwrap_err();
            assert!(error.contains("machine id"), "{bad:?}: {error}");
            assert!(!error.contains(bad), "{bad:?}: {error}");
        }
    }
}
