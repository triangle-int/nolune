use axum::response::IntoResponse;
use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post, put},
};
use serde::{Deserialize, Serialize};
use std::fs;

use crate::{
    app::state::AppState,
    domain::{
        correction::{CorrectionLedger, Keep},
        memory::MemoryEntry,
        receipt::MemoryReceipt,
    },
    services::{
        chat, memory,
        memory_corrections::{self, CorrectionError, CorrectionOutcome, FlagUpdate},
        memory_receipts, profile_archive, tools,
    },
};

/// Retired control-token resource namespace; always denies access.
pub fn public_memory_router() -> Router<AppState> {
    Router::new().route(
        "/public/memory/{instance_slug}/{*path}",
        get(serve_memory_file_public),
    )
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/instances/{instance_slug}/mood", get(get_mood))
        .route(
            "/api/instances/{instance_slug}/companion-name",
            get(get_companion_name),
        )
        .route(
            "/api/instances/{instance_slug}/companion-name",
            put(set_companion_name),
        )
        .route("/api/instances/{instance_slug}/timezone", get(get_timezone))
        .route("/api/instances/{instance_slug}/timezone", put(set_timezone))
        .route("/api/instances/{instance_slug}/secret", post(submit_secret))
        .route(
            "/api/instances/{instance_slug}/secret/{secret_id}",
            delete(cancel_secret),
        )
        .route(
            "/api/instances/{instance_slug}/context-stats",
            get(get_context_stats),
        )
        .route(
            "/api/instances/{instance_slug}/{chat_id}/context-stats",
            get(get_context_stats_chat),
        )
        .route(
            "/api/instances/{instance_slug}/rhythm",
            get(get_rhythm_tracking).put(set_rhythm_tracking),
        )
        .route("/api/instances/{instance_slug}/memory", get(list_memory))
        .route(
            "/api/instances/{instance_slug}/memory/search",
            get(search_memory),
        )
        .route(
            "/api/instances/{instance_slug}/memory/graph",
            get(get_memory_graph),
        )
        .route(
            "/api/instances/{instance_slug}/memory/{*path}",
            get(read_memory_file)
                .delete(delete_memory_file)
                .put(correct_memory_file)
                .patch(set_memory_flags),
        )
        .route(
            "/api/instances/{instance_slug}/memory-corrections",
            get(list_memory_corrections),
        )
        .route(
            "/api/instances/{instance_slug}/memory-corrections/{conflict_id}/resolve",
            post(resolve_memory_correction),
        )
        .route(
            "/api/instances/{instance_slug}/{chat_id}/receipts",
            get(list_memory_receipts),
        )
        .route(
            "/api/instances/{instance_slug}/{chat_id}/receipts/{message_id}",
            get(get_memory_receipt),
        )
        .route(
            "/api/instances/{instance_slug}/email",
            get(get_email_config),
        )
        .route(
            "/api/instances/{instance_slug}/email",
            put(set_email_config),
        )
        .route(
            "/api/instances/{instance_slug}/email",
            delete(delete_email_config),
        )
        .route("/api/instances/{instance_slug}/voice", get(get_voice_id))
        .route("/api/instances/{instance_slug}/voice", put(set_voice_id))
        .route(
            "/api/instances/{instance_slug}/voice-mode",
            get(get_voice_enabled),
        )
        .route(
            "/api/instances/{instance_slug}/voice-mode",
            put(set_voice_enabled),
        )
        .route("/api/instances/{instance_slug}/skin", get(get_skin))
        .route("/api/instances/{instance_slug}/skin", put(set_skin))
        .route(
            "/api/instances/{instance_slug}/scheduled",
            get(list_scheduled),
        )
        .route(
            "/api/instances/{instance_slug}/scheduled/{message_id}",
            delete(cancel_scheduled),
        )
        .route(
            "/api/instances/{instance_slug}/export",
            get(export_instance),
        )
        .route(
            "/api/instances/{instance_slug}/import",
            post(import_instance),
        )
}

#[derive(Serialize)]
struct MoodResponse {
    mood: String,
}

async fn get_mood(
    State(state): State<AppState>,
    Path(instance_slug): Path<String>,
) -> Json<MoodResponse> {
    let instance_dir = state.workspace_dir.join("instances").join(&instance_slug);
    let mood_state = tools::load_mood_state(&instance_dir);
    Json(MoodResponse {
        mood: mood_state.companion_mood,
    })
}

#[derive(Serialize)]
struct CompanionNameResponse {
    name: String,
}

#[derive(Deserialize)]
struct SetCompanionNameRequest {
    name: String,
}

async fn get_companion_name(
    State(state): State<AppState>,
    Path(instance_slug): Path<String>,
) -> Json<CompanionNameResponse> {
    let instance_dir = state.workspace_dir.join("instances").join(&instance_slug);
    let name = read_identity_name(&instance_dir).unwrap_or_default();
    Json(CompanionNameResponse { name })
}

async fn set_companion_name(
    State(state): State<AppState>,
    Path(instance_slug): Path<String>,
    Json(req): Json<SetCompanionNameRequest>,
) -> StatusCode {
    let instance_dir = state.workspace_dir.join("instances").join(&instance_slug);
    let state_path = instance_dir.join("project_state.json");

    let mut project_state: serde_json::Value = fs::read_to_string(&state_path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    if project_state.get("identity").is_none() {
        project_state["identity"] = serde_json::json!({});
    }
    project_state["identity"]["name"] = serde_json::Value::String(req.name);

    fs::create_dir_all(&instance_dir).ok();
    match serde_json::to_string_pretty(&project_state) {
        Ok(body) => {
            if fs::write(&state_path, body).is_ok() {
                StatusCode::OK
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn read_identity_name(instance_dir: &std::path::Path) -> Option<String> {
    let raw = fs::read_to_string(instance_dir.join("project_state.json")).ok()?;
    let state: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let name = state.get("identity")?.get("name")?.as_str()?;
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

// ---------------------------------------------------------------------------
// Timezone
// ---------------------------------------------------------------------------

async fn get_timezone(
    State(state): State<AppState>,
    Path(instance_slug): Path<String>,
) -> Json<serde_json::Value> {
    let instance_dir = state.workspace_dir.join("instances").join(&instance_slug);
    let tz = read_timezone(&instance_dir).unwrap_or_default();
    Json(serde_json::json!({ "timezone": tz }))
}

#[derive(Deserialize)]
struct SetTimezoneRequest {
    timezone: String,
}

async fn set_timezone(
    State(state): State<AppState>,
    Path(instance_slug): Path<String>,
    Json(req): Json<SetTimezoneRequest>,
) -> StatusCode {
    // Validate timezone string
    if !req.timezone.is_empty() {
        if req.timezone.parse::<chrono_tz::Tz>().is_err() {
            return StatusCode::BAD_REQUEST;
        }
    }

    let instance_dir = state.workspace_dir.join("instances").join(&instance_slug);
    let state_path = instance_dir.join("project_state.json");

    let mut project_state: serde_json::Value = fs::read_to_string(&state_path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    project_state["timezone"] = serde_json::Value::String(req.timezone);

    fs::create_dir_all(&instance_dir).ok();
    match serde_json::to_string_pretty(&project_state) {
        Ok(body) => {
            if fs::write(&state_path, body).is_ok() {
                StatusCode::OK
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// Format current time in the instance's configured timezone (or UTC).
pub fn format_instance_now(instance_dir: &std::path::Path) -> String {
    let now = chrono::Utc::now();
    if let Some(tz_str) = read_timezone(instance_dir) {
        if let Ok(tz) = tz_str.parse::<chrono_tz::Tz>() {
            return now
                .with_timezone(&tz)
                .format("%A, %B %-d, %Y %H:%M %Z")
                .to_string();
        }
    }
    now.format("%A, %B %-d, %Y %H:%M UTC").to_string()
}

pub fn read_timezone(instance_dir: &std::path::Path) -> Option<String> {
    let raw = fs::read_to_string(instance_dir.join("project_state.json")).ok()?;
    let state: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let tz = state.get("timezone")?.as_str()?;
    if tz.is_empty() {
        None
    } else {
        Some(tz.to_string())
    }
}

// ---------------------------------------------------------------------------
// Voice ID (ElevenLabs)
// ---------------------------------------------------------------------------

async fn get_voice_id(
    State(state): State<AppState>,
    Path(instance_slug): Path<String>,
) -> Json<serde_json::Value> {
    let inst = crate::config::InstanceConfig::load(&state.workspace_dir, &instance_slug);
    Json(serde_json::json!({ "voice_id": inst.elevenlabs_voice_id }))
}

#[derive(Deserialize)]
struct SetVoiceIdRequest {
    voice_id: String,
}

async fn set_voice_id(
    State(state): State<AppState>,
    Path(instance_slug): Path<String>,
    Json(req): Json<SetVoiceIdRequest>,
) -> StatusCode {
    let mut inst = crate::config::InstanceConfig::load(&state.workspace_dir, &instance_slug);
    inst.elevenlabs_voice_id = req.voice_id;
    match inst.save(&state.workspace_dir, &instance_slug) {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

// ---------------------------------------------------------------------------
// Voice enabled
// ---------------------------------------------------------------------------

async fn get_voice_enabled(
    State(state): State<AppState>,
    Path(instance_slug): Path<String>,
) -> Json<serde_json::Value> {
    let inst = crate::config::InstanceConfig::load(&state.workspace_dir, &instance_slug);
    Json(serde_json::json!({ "voice_enabled": inst.voice_enabled }))
}

#[derive(Deserialize)]
struct SetVoiceEnabledRequest {
    voice_enabled: bool,
}

async fn set_voice_enabled(
    State(state): State<AppState>,
    Path(instance_slug): Path<String>,
    Json(req): Json<SetVoiceEnabledRequest>,
) -> StatusCode {
    let mut inst = crate::config::InstanceConfig::load(&state.workspace_dir, &instance_slug);
    inst.voice_enabled = req.voice_enabled;
    match inst.save(&state.workspace_dir, &instance_slug) {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

// ---------------------------------------------------------------------------
// Skin
// ---------------------------------------------------------------------------

async fn get_skin(
    State(state): State<AppState>,
    Path(instance_slug): Path<String>,
) -> Json<serde_json::Value> {
    let inst = crate::config::InstanceConfig::load(&state.workspace_dir, &instance_slug);
    Json(serde_json::json!({ "skin": inst.skin }))
}

#[derive(Deserialize)]
struct SetSkinRequest {
    skin: String,
}

async fn set_skin(
    State(state): State<AppState>,
    Path(instance_slug): Path<String>,
    Json(req): Json<SetSkinRequest>,
) -> StatusCode {
    let mut inst = crate::config::InstanceConfig::load(&state.workspace_dir, &instance_slug);
    inst.skin = req.skin;
    match inst.save(&state.workspace_dir, &instance_slug) {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

// ---------------------------------------------------------------------------
// Secret submission endpoint
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct SubmitSecretRequest {
    id: String,
    value: String,
}

async fn submit_secret(
    State(state): State<AppState>,
    Path(_instance_slug): Path<String>,
    Json(req): Json<SubmitSecretRequest>,
) -> StatusCode {
    let mut secrets = state.pending_secrets.lock().await;
    match secrets.remove(&req.id) {
        Some(pending) => {
            let _ = pending.responder.send(req.value);
            StatusCode::OK
        }
        None => StatusCode::NOT_FOUND,
    }
}

async fn cancel_secret(
    State(state): State<AppState>,
    Path((_instance_slug, secret_id)): Path<(String, String)>,
) -> StatusCode {
    let mut secrets = state.pending_secrets.lock().await;
    match secrets.remove(&secret_id) {
        Some(_pending) => {
            // Dropping the PendingSecret drops the oneshot Sender,
            // which causes the tool's rx.await to return Err → "cancelled"
            StatusCode::OK
        }
        None => StatusCode::NOT_FOUND,
    }
}

// ---------------------------------------------------------------------------
// Context stats endpoint
// ---------------------------------------------------------------------------

async fn get_context_stats(
    State(state): State<AppState>,
    Path(instance_slug): Path<String>,
) -> Json<chat::ContextStats> {
    let wd = state.workspace_dir.clone();
    let slug = instance_slug.clone();
    let public_url = state.config.read().await.public_url.clone();
    let resources = state.resources.clone();
    let http_client = state.http_client.clone();
    let stats = tokio::spawn(async move {
        chat::compute_context_stats_async(
            wd,
            slug,
            "default".to_string(),
            public_url,
            resources,
            http_client,
        )
        .await
    })
    .await
    .unwrap_or_else(|_| {
        chat::compute_context_stats(&state.workspace_dir, &instance_slug, "default")
    });
    Json(stats)
}

async fn get_context_stats_chat(
    State(state): State<AppState>,
    Path((instance_slug, chat_id)): Path<(String, String)>,
) -> Json<chat::ContextStats> {
    let wd = state.workspace_dir.clone();
    let slug = instance_slug.clone();
    let cid = chat_id.clone();
    let public_url = state.config.read().await.public_url.clone();
    let resources = state.resources.clone();
    let http_client = state.http_client.clone();
    let stats = tokio::spawn(async move {
        chat::compute_context_stats_async(wd, slug, cid, public_url, resources, http_client).await
    })
    .await
    .unwrap_or_else(|_| {
        chat::compute_context_stats(&state.workspace_dir, &instance_slug, &chat_id)
    });
    Json(stats)
}

// ---------------------------------------------------------------------------
// Stats / Analytics
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Interaction rhythm (#95): the one retained behavioral aggregate, with opt-out.
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct RhythmTrackingResponse {
    enabled: bool,
}

async fn get_rhythm_tracking(
    State(state): State<AppState>,
    Path(instance_slug): Path<String>,
) -> Json<RhythmTrackingResponse> {
    Json(RhythmTrackingResponse {
        enabled: crate::services::rhythm::tracking_enabled(&state.workspace_dir, &instance_slug),
    })
}

/// Turning tracking off also deletes the aggregate; nothing is retained.
async fn set_rhythm_tracking(
    State(state): State<AppState>,
    Path(instance_slug): Path<String>,
    Json(req): Json<RhythmTrackingResponse>,
) -> StatusCode {
    let mut inst = crate::config::InstanceConfig::load(&state.workspace_dir, &instance_slug);
    inst.rhythm_tracking = req.enabled;
    if inst.save(&state.workspace_dir, &instance_slug).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    if !req.enabled {
        let instance_dir = state.workspace_dir.join("instances").join(&instance_slug);
        if crate::services::rhythm::clear_rhythm(&instance_dir).is_err() {
            return StatusCode::INTERNAL_SERVER_ERROR;
        }
    }
    StatusCode::OK
}

async fn list_memory(
    State(state): State<AppState>,
    Path(instance_slug): Path<String>,
) -> Json<Vec<MemoryEntry>> {
    Json(memory::scan_library(
        &state.vector_store.media_store(),
        &instance_slug,
    ))
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
    #[serde(default = "default_search_limit")]
    limit: usize,
}

fn default_search_limit() -> usize {
    10
}

async fn search_memory(
    State(state): State<AppState>,
    Path(instance_slug): Path<String>,
    axum::extract::Query(params): axum::extract::Query<SearchQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let results = state
        .vector_store
        .search_text(&instance_slug, &params.q, params.limit)
        .await;

    let media = state.vector_store.media_store();

    let json: Vec<serde_json::Value> = results
        .into_iter()
        .map(|r| {
            let is_media = r.source_type.starts_with("media_");

            let text = if r.content_preview.is_empty() && !is_media {
                media
                    .read_memory_text(&instance_slug, &r.path)
                    .unwrap_or_default()
            } else {
                r.content_preview
            };

            let mut obj = serde_json::json!({
                "path": r.path,
                "text": text,
                "score": r.score,
                "source_type": r.source_type,
            });

            // For media results, include a URL to the file
            if is_media {
                if let Some(upload_id) = &r.upload_id {
                    use crate::services::resource_capability::{
                        CapabilityAudience, CapabilityResource,
                    };
                    let resource = if upload_id == &r.path || upload_id.contains('/') {
                        CapabilityResource::memory(upload_id)
                    } else {
                        CapabilityResource::uploaded_file(upload_id)
                    };
                    let url = resource
                        .and_then(|resource| {
                            state.resources.url(
                                "",
                                &instance_slug,
                                resource,
                                CapabilityAudience::Browser,
                            )
                        })
                        .unwrap_or_default();
                    obj["media_url"] = serde_json::Value::String(url);
                }
            }

            obj
        })
        .collect();

    Ok(Json(serde_json::Value::Array(json)))
}

async fn get_memory_graph(
    State(state): State<AppState>,
    Path(instance_slug): Path<String>,
) -> Json<crate::domain::memory::MemoryGraph> {
    Json(memory::load_graph(
        &state.vector_store.media_store(),
        &instance_slug,
    ))
}

async fn serve_memory_file_public() -> StatusCode {
    StatusCode::UNAUTHORIZED
}

async fn read_memory_file(
    State(state): State<AppState>,
    Path((instance_slug, file_path)): Path<(String, String)>,
) -> Result<axum::response::Response<axum::body::Body>, StatusCode> {
    serve_memory_file_inner(&state, &instance_slug, &file_path).await
}

pub(super) async fn serve_memory_file_inner(
    state: &AppState,
    instance_slug: &str,
    file_path: &str,
) -> Result<axum::response::Response<axum::body::Body>, StatusCode> {
    const MAX_MEMORY_FILE_BYTES: usize = 64 * 1024 * 1024;
    let media = state.vector_store.media_store();
    let slug = instance_slug.to_owned();
    let path = file_path.to_owned();
    let bytes = tokio::task::spawn_blocking(move || {
        media.read_memory_file(&slug, &path, MAX_MEMORY_FILE_BYTES)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .map_err(|_| StatusCode::NOT_FOUND)?;

    let extension = std::path::Path::new(file_path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    let content_type = match extension.as_deref() {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("mov") => "video/quicktime",
        Some("ogg") => "audio/ogg",
        Some("m4a") => "audio/mp4",
        Some("flac") => "audio/flac",
        Some("aac") => "audio/aac",
        Some("avif") => "image/avif",
        Some("bmp") => "image/bmp",
        Some("ico") => "image/x-icon",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("pdf") => "application/pdf",
        Some("md" | "txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    };

    let is_media = content_type.starts_with("image/")
        || content_type.starts_with("video/")
        || content_type.starts_with("audio/");
    let filename = std::path::Path::new(file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");
    let disposition = if is_media { "inline" } else { "attachment" };

    axum::response::Response::builder()
        .header(axum::http::header::CONTENT_TYPE, content_type)
        .header(
            axum::http::header::CONTENT_DISPOSITION,
            format!("{disposition}; filename=\"{filename}\""),
        )
        .header(axum::http::header::CACHE_CONTROL, "private, no-store")
        .body(axum::body::Body::from(bytes))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn delete_memory_file(
    State(state): State<AppState>,
    Path((instance_slug, file_path)): Path<(String, String)>,
) -> StatusCode {
    if let Err(error) = state
        .vector_store
        .delete_memory(&instance_slug, &file_path)
        .await
    {
        log::warn!("[delete_memory] cleanup failed for {file_path}: {error}");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    StatusCode::OK
}

// ---------------------------------------------------------------------------
// Memory corrections (#84) — correct, pin, exclude, and the conflict ledger
// ---------------------------------------------------------------------------

type CorrectionApiError = (StatusCode, Json<serde_json::Value>);

fn correction_error(path: &str, error: CorrectionError) -> CorrectionApiError {
    let status = match error {
        CorrectionError::NotFound => StatusCode::NOT_FOUND,
        CorrectionError::Invalid(_) => StatusCode::UNPROCESSABLE_ENTITY,
        CorrectionError::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        CorrectionError::LedgerFull => StatusCode::INSUFFICIENT_STORAGE,
        CorrectionError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    if status == StatusCode::INTERNAL_SERVER_ERROR {
        log::warn!("[memory_corrections] {path}: {error}");
    }
    (
        status,
        Json(serde_json::json!({
            "error": "memory_correction",
            "message": error.to_string(),
            "path": path,
        })),
    )
}

#[derive(Deserialize)]
struct CorrectionBody {
    /// What the memory should say; replaces the body, keeps the flags.
    content: String,
}

/// PUT /api/instances/{slug}/memory/{*path} — the user's own statement.
/// `200 applied` / `200 unchanged`, or `409 needs_resolution` listing both
/// statements when an earlier correction is still in force and differs.
async fn correct_memory_file(
    State(state): State<AppState>,
    Path((instance_slug, file_path)): Path<(String, String)>,
    Json(body): Json<CorrectionBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), CorrectionApiError> {
    let outcome = memory_corrections::correct(
        &state.vector_store,
        &instance_slug,
        &file_path,
        &body.content,
    )
    .await
    .map_err(|error| correction_error(&file_path, error))?;
    Ok(match outcome {
        CorrectionOutcome::Applied(entry) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "applied",
                "path": file_path,
                "correction": entry,
            })),
        ),
        CorrectionOutcome::Unchanged => (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "unchanged", "path": file_path })),
        ),
        CorrectionOutcome::NeedsResolution(conflict) => {
            let mut value = serde_json::to_value(conflict).unwrap_or_default();
            value["status"] = serde_json::Value::String("needs_resolution".into());
            (StatusCode::CONFLICT, Json(value))
        }
    })
}

/// PATCH /api/instances/{slug}/memory/{*path} — `pinned` and/or
/// `exclude_from_proactive`; a flag left out is unchanged.
async fn set_memory_flags(
    State(state): State<AppState>,
    Path((instance_slug, file_path)): Path<(String, String)>,
    Json(update): Json<FlagUpdate>,
) -> Result<Json<serde_json::Value>, CorrectionApiError> {
    let flags =
        memory_corrections::set_flags(&state.vector_store, &instance_slug, &file_path, update)
            .await
            .map_err(|error| correction_error(&file_path, error))?;
    let mut value = serde_json::to_value(flags).unwrap_or_default();
    value["path"] = serde_json::Value::String(file_path);
    Ok(Json(value))
}

/// GET /api/instances/{slug}/memory-corrections — the whole ledger.
async fn list_memory_corrections(
    State(state): State<AppState>,
    Path(instance_slug): Path<String>,
) -> Result<Json<CorrectionLedger>, CorrectionApiError> {
    let media = state.vector_store.media_store();
    tokio::task::spawn_blocking(move || memory_corrections::load_ledger(&media, &instance_slug))
        .await
        .map_err(|error| correction_error("", CorrectionError::Io(error.to_string())))?
        .map(Json)
        .map_err(|error| correction_error("", error))
}

#[derive(Deserialize)]
struct ResolveBody {
    keep: Keep,
}

/// POST /api/instances/{slug}/memory-corrections/{id}/resolve — settle a
/// `needs_resolution` entry by keeping `current` or `proposed`.
async fn resolve_memory_correction(
    State(state): State<AppState>,
    Path((instance_slug, conflict_id)): Path<(String, String)>,
    Json(body): Json<ResolveBody>,
) -> Result<Json<serde_json::Value>, CorrectionApiError> {
    let resolution =
        memory_corrections::resolve(&state.vector_store, &instance_slug, &conflict_id, body.keep)
            .await
            .map_err(|error| correction_error(&conflict_id, error))?;
    let mut value = serde_json::to_value(resolution).unwrap_or_default();
    value["status"] = serde_json::Value::String("resolved".into());
    Ok(Json(value))
}

// ---------------------------------------------------------------------------
// Memory recall receipts (#84) — read-only provenance per assistant message
// ---------------------------------------------------------------------------

/// GET /api/instances/{slug}/{chat_id}/receipts — every receipt of one chat.
async fn list_memory_receipts(
    State(state): State<AppState>,
    Path((instance_slug, chat_id)): Path<(String, String)>,
) -> Result<Json<Vec<MemoryReceipt>>, StatusCode> {
    let media = state.vector_store.media_store();
    let workspace = state.workspace_dir.clone();
    tokio::task::spawn_blocking(move || {
        memory_receipts::list_receipts(&workspace, &media, &instance_slug, &chat_id)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .map(Json)
    .map_err(|error| {
        log::warn!("[receipts] listing failed: {error}");
        StatusCode::INTERNAL_SERVER_ERROR
    })
}

/// GET /api/instances/{slug}/{chat_id}/receipts/{message_id} — one receipt,
/// with deleted sources reported as missing; 404 only when no receipt exists.
async fn get_memory_receipt(
    State(state): State<AppState>,
    Path((instance_slug, chat_id, message_id)): Path<(String, String, String)>,
) -> Result<Json<MemoryReceipt>, StatusCode> {
    let media = state.vector_store.media_store();
    let workspace = state.workspace_dir.clone();
    tokio::task::spawn_blocking(move || {
        memory_receipts::read_receipt(&workspace, &media, &instance_slug, &chat_id, &message_id)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .map_err(|error| {
        log::warn!("[receipts] read failed: {error}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .map(Json)
    .ok_or(StatusCode::NOT_FOUND)
}

// ---------------------------------------------------------------------------
// Email config (per-instance SMTP/IMAP)
// ---------------------------------------------------------------------------

async fn get_email_config(
    State(state): State<AppState>,
    Path(instance_slug): Path<String>,
) -> Json<serde_json::Value> {
    let accounts = crate::config::EmailAccounts::load(&state.workspace_dir, &instance_slug);
    let items: Vec<serde_json::Value> = accounts
        .iter()
        .map(|cfg| {
            serde_json::json!({
                "smtp_host": cfg.smtp_host,
                "smtp_port": cfg.smtp_port,
                "smtp_user": cfg.smtp_user,
                "smtp_from": cfg.smtp_from,
                "imap_host": cfg.imap_host,
                "imap_port": cfg.imap_port,
                "imap_user": cfg.imap_user,
                // Never expose passwords
            })
        })
        .collect();
    Json(serde_json::json!({ "accounts": items }))
}

async fn set_email_config(
    State(state): State<AppState>,
    Path(instance_slug): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> StatusCode {
    // Accept either { accounts: [...] } or a single account object (legacy)
    let accounts: Vec<crate::config::EmailConfig> =
        if let Some(arr) = body.get("accounts").and_then(|v| v.as_array()) {
            match serde_json::from_value::<Vec<crate::config::EmailConfig>>(
                serde_json::Value::Array(arr.clone()),
            ) {
                Ok(a) => a,
                Err(_) => return StatusCode::BAD_REQUEST,
            }
        } else {
            match serde_json::from_value::<crate::config::EmailConfig>(body) {
                Ok(single) => vec![single],
                Err(_) => return StatusCode::BAD_REQUEST,
            }
        };

    // Reject accounts with empty passwords — they won't work and are likely
    // Google OAuth accounts mistakenly added as SMTP/IMAP
    for acct in &accounts {
        if acct.smtp_password.is_empty() || acct.imap_password.is_empty() {
            return StatusCode::BAD_REQUEST;
        }
    }

    match crate::config::EmailAccounts::save(&accounts, &state.workspace_dir, &instance_slug) {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn delete_email_config(
    State(state): State<AppState>,
    Path(instance_slug): Path<String>,
) -> StatusCode {
    let path = state
        .workspace_dir
        .join("instances")
        .join(&instance_slug)
        .join("email.toml");
    if path.exists() {
        match fs::remove_file(&path) {
            Ok(_) => StatusCode::NO_CONTENT,
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    } else {
        StatusCode::NO_CONTENT
    }
}

// ---------------------------------------------------------------------------
// Scheduled messages
// ---------------------------------------------------------------------------

async fn list_scheduled(
    State(state): State<AppState>,
    Path(instance_slug): Path<String>,
) -> Json<Vec<serde_json::Value>> {
    let dir = state
        .workspace_dir
        .join("instances")
        .join(&instance_slug)
        .join("scheduled");
    let mut items = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(raw) = std::fs::read_to_string(&path) {
                if let Ok(task) = serde_json::from_str::<tools::ScheduledTask>(&raw) {
                    items.push(serde_json::json!({
                        "id": task.id,
                        "task": task.task,
                        "deliver_at": task.deliver_at,
                        "created_at": task.created_at,
                    }));
                }
            }
        }
    }
    items.sort_by_key(|v| v["deliver_at"].as_i64().unwrap_or(0));
    Json(items)
}

async fn cancel_scheduled(
    State(state): State<AppState>,
    Path((instance_slug, message_id)): Path<(String, String)>,
) -> StatusCode {
    let file = state
        .workspace_dir
        .join("instances")
        .join(&instance_slug)
        .join("scheduled")
        .join(format!("{message_id}.json"));
    if file.exists() {
        match std::fs::remove_file(&file) {
            Ok(_) => StatusCode::OK,
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    } else {
        StatusCode::NOT_FOUND
    }
}

// ---------------------------------------------------------------------------
// Export / Import
// ---------------------------------------------------------------------------

/// GET /api/instances/{slug}/export → tar.gz download of the companion directory.
/// The archive is written in-process by `services::profile_archive` (#74) on a
/// blocking thread and streamed to the client as it is produced.
async fn export_instance(
    Path(instance_slug): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    use futures::StreamExt;

    let instance_dir = state.workspace_dir.join("instances").join(&instance_slug);
    if !instance_dir.is_dir() {
        return (StatusCode::NOT_FOUND, "instance not found").into_response();
    }
    let source = match profile_archive::open_companion_dir(&instance_dir) {
        Ok(dir) => dir,
        Err(e) => {
            log::error!("[export] failed to open companion directory: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "export failed").into_response();
        }
    };

    // The first chunk (or the first error) decides the status code. A failure
    // after that aborts the body; the writer poisons its sink first, so the
    // bytes already delivered lack the tar and gzip trailers and no reader
    // accepts them as a complete backup.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<std::io::Result<axum::body::Bytes>>(16);
    tokio::task::spawn_blocking(move || {
        let mut sink = ArchiveChunks { tx: tx.clone() };
        if let Err(error) = profile_archive::write_archive(&source, &mut sink) {
            log::error!("[export] failed to write archive: {error}");
            let _ = tx.blocking_send(Err(std::io::Error::other(error.to_string())));
        }
    });
    let first = match rx.recv().await {
        Some(Ok(chunk)) => chunk,
        Some(Err(_)) | None => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "export failed").into_response();
        }
    };
    let rest = futures::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|chunk| (chunk, rx))
    });
    let body = Body::from_stream(
        futures::stream::once(async move { Ok::<_, std::io::Error>(first) }).chain(rest),
    );

    let headers = [
        (
            axum::http::header::CONTENT_TYPE,
            profile_archive::ARCHIVE_CONTENT_TYPE,
        ),
        (
            axum::http::header::CONTENT_DISPOSITION,
            &format!(
                "attachment; filename=\"{}\"",
                profile_archive::ARCHIVE_FILE_NAME
            ),
        ),
    ];
    (headers, body).into_response()
}

/// `Write` sink that hands archive chunks from the blocking writer to the
/// response stream and stops the writer once the client has gone away.
struct ArchiveChunks {
    tx: tokio::sync::mpsc::Sender<std::io::Result<axum::body::Bytes>>,
}

impl std::io::Write for ArchiveChunks {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        self.tx
            .blocking_send(Ok(axum::body::Bytes::copy_from_slice(buf)))
            .map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::BrokenPipe, "export client went away")
            })?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// POST /api/instances/{slug}/import is disabled until a capability-safe
/// importer exists for the stabilized storage format.
async fn import_instance(Path(_instance_slug): Path<String>) -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        "profile import is temporarily unavailable during storage format stabilization",
    )
        .into_response()
}

#[cfg(test)]
mod media_tests {
    use super::*;
    use crate::services::embedding::tests::{MockServer, response};

    #[tokio::test]
    async fn import_endpoint_is_disabled_without_filesystem_or_index_mutation() {
        use axum::{
            body::{Body, to_bytes},
            http::Request,
        };
        use tower::ServiceExt;

        let workspace = tempfile::tempdir().unwrap();
        let store = std::sync::Arc::new(
            crate::services::vector::VectorStore::connect(workspace.path()).await,
        );
        let mut vector = vec![0.; 768];
        vector[0] = 1.;
        store
            .upsert_text_memory("existing", "note.md", vec![("sentinel".into(), vector)])
            .await
            .unwrap();
        let mut state = AppState::new(crate::config::Config::default()).await;
        state.workspace_dir = workspace.path().to_owned();
        state.vector_store = store.clone();
        let app = router().with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/instances/new/import")
                    .body(Body::from(b"not an archive".as_slice()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        assert!(
            String::from_utf8_lossy(&body).contains("storage format stabilization"),
            "{}",
            String::from_utf8_lossy(&body)
        );
        assert!(!workspace.path().join("instances/new").exists());
        assert_eq!(store.list_all("existing", 10).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn export_streams_an_in_process_archive_that_round_trips() {
        use axum::{
            body::{Body, to_bytes},
            http::Request,
        };
        use cap_std::{ambient_authority, fs::Dir};
        use tower::ServiceExt;

        let workspace = tempfile::tempdir().unwrap();
        let companion = workspace.path().join("instances/companion");
        std::fs::create_dir_all(companion.join("memory/notes")).unwrap();
        std::fs::write(
            companion.join("companion.json"),
            serde_json::to_vec(&crate::domain::companion::CompanionIdentity::canonical()).unwrap(),
        )
        .unwrap();
        std::fs::write(companion.join("soul.md"), b"# soul\n").unwrap();
        std::fs::write(companion.join("memory/notes/tea.md"), b"oolong").unwrap();
        let mut state = AppState::new(crate::config::Config::default()).await;
        state.workspace_dir = workspace.path().to_owned();
        let app = router().with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/instances/companion/export")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[axum::http::header::CONTENT_TYPE],
            "application/gzip"
        );
        assert_eq!(
            response.headers()[axum::http::header::CONTENT_DISPOSITION],
            "attachment; filename=\"companion.tar.gz\""
        );
        let archive = to_bytes(response.into_body(), 16 * 1024 * 1024)
            .await
            .unwrap();

        let staging = workspace.path().join("staging");
        std::fs::create_dir(&staging).unwrap();
        let staging_dir = Dir::open_ambient_dir(&staging, ambient_authority()).unwrap();
        let summary = profile_archive::extract_into(archive.as_ref(), &staging_dir).unwrap();
        assert_eq!(summary.files, 3);
        assert_eq!(
            std::fs::read(staging.join("memory/notes/tea.md")).unwrap(),
            b"oolong"
        );
        assert_eq!(std::fs::read(staging.join("soul.md")).unwrap(), b"# soul\n");
    }

    #[tokio::test]
    async fn export_fails_before_streaming_when_the_marker_is_missing() {
        use axum::{
            body::{Body, to_bytes},
            http::Request,
        };
        use tower::ServiceExt;

        let workspace = tempfile::tempdir().unwrap();
        let companion = workspace.path().join("instances/companion");
        std::fs::create_dir_all(&companion).unwrap();
        std::fs::write(companion.join("soul.md"), b"# soul\n").unwrap();
        let mut state = AppState::new(crate::config::Config::default()).await;
        state.workspace_dir = workspace.path().to_owned();
        let app = router().with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/instances/companion/export")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        assert_eq!(&body[..], b"export failed");
    }

    /// A failure after the first chunk has left cannot change the status any
    /// more, so the body is aborted instead; the bytes delivered up to then
    /// must not form an archive the reader accepts, or a client that keeps
    /// the partial download holds a backup that silently lacks files.
    #[cfg(unix)]
    #[tokio::test]
    async fn export_aborts_without_a_complete_archive_when_a_file_is_unreadable() {
        use axum::{body::Body, http::Request};
        use cap_std::{ambient_authority, fs::Dir};
        use futures::StreamExt;
        use std::os::unix::fs::PermissionsExt;
        use tower::ServiceExt;

        let workspace = tempfile::tempdir().unwrap();
        let companion = workspace.path().join("instances/companion");
        std::fs::create_dir_all(&companion).unwrap();
        std::fs::write(
            companion.join("companion.json"),
            serde_json::to_vec(&crate::domain::companion::CompanionIdentity::canonical()).unwrap(),
        )
        .unwrap();
        // Incompressible bytes so real chunks stream before the failure.
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        let noise: Vec<u8> = (0..256 * 1024)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 56) as u8
            })
            .collect();
        std::fs::write(companion.join("a.md"), &noise).unwrap();
        let unreadable = companion.join("b_unreadable.md");
        std::fs::write(&unreadable, b"secret").unwrap();
        std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::File::open(&unreadable).is_ok() {
            // Running as root: permissions cannot make the read fail.
            return;
        }
        std::fs::write(companion.join("c.md"), b"after the failure").unwrap();
        let mut state = AppState::new(crate::config::Config::default()).await;
        state.workspace_dir = workspace.path().to_owned();
        let app = router().with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/instances/companion/export")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let mut chunks = response.into_body().into_data_stream();
        let mut delivered = Vec::new();
        let mut aborted = false;
        while let Some(chunk) = chunks.next().await {
            match chunk {
                Ok(bytes) => delivered.extend_from_slice(&bytes),
                Err(_) => {
                    aborted = true;
                    break;
                }
            }
        }
        assert!(aborted, "the body must end in an error, not a clean EOF");
        assert!(
            !delivered.is_empty(),
            "the failure must come after the first chunk"
        );

        let staging = workspace.path().join("staging");
        std::fs::create_dir(&staging).unwrap();
        let staging_dir = Dir::open_ambient_dir(&staging, ambient_authority()).unwrap();
        let error = profile_archive::extract_into(delivered.as_slice(), &staging_dir).unwrap_err();
        assert!(
            matches!(error, profile_archive::ArchiveError::Malformed(_)),
            "the delivered prefix must be refused as truncated, got: {error}"
        );
        assert!(!staging.join("c.md").exists());
    }

    #[tokio::test]
    async fn automatic_backfill_preserves_committed_index_during_provider_failure() {
        let mock = MockServer::new(vec![
            (200, response(vec![1., 0., 0.])),
            (503, serde_json::json!({"error":"offline"})),
        ])
        .await;
        let ws = tempfile::tempdir().unwrap();
        let dir = ws.path().join("instances/one/memory");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("photo.png"), [0xff]).unwrap();
        crate::services::media_text::write(&dir, "photo.png", "sky").unwrap();
        let mut state = AppState::new(crate::config::Config::default()).await;
        state.workspace_dir = ws.path().to_owned();
        state.vector_store = std::sync::Arc::new(
            crate::services::vector::VectorStore::connect_with_config(ws.path(), &mock.config)
                .await,
        );
        state
            .vector_store
            .backfill_text_memories(ws.path(), "one")
            .await
            .unwrap();
        // A second recovery pass hits the failing provider; the committed index survives.
        let _ = state
            .vector_store
            .backfill_text_memories(ws.path(), "one")
            .await;
        assert_eq!(
            state.vector_store.list_all("one", 10).await.unwrap().len(),
            1
        );
    }
    #[tokio::test]
    async fn delete_endpoint_reconciles_missing_source_and_stale_derived_state() {
        let ws = tempfile::tempdir().unwrap();
        let store =
            std::sync::Arc::new(crate::services::vector::VectorStore::connect(ws.path()).await);
        let mut vector = vec![0.; 768];
        vector[0] = 1.;
        store
            .upsert_text_memory("one", "photo.png", vec![("stale".into(), vector)])
            .await
            .unwrap();
        let mut state = AppState::new(crate::config::Config::default()).await;
        state.workspace_dir = ws.path().to_owned();
        state.vector_store = store.clone();

        assert_eq!(
            delete_memory_file(State(state), Path(("one".into(), "photo.png".into())),).await,
            StatusCode::OK
        );
        assert!(store.list_all("one", 10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn serving_uses_case_insensitive_media_mime_detection() {
        let ws = tempfile::tempdir().unwrap();
        let memory = ws.path().join("instances/one/memory");
        std::fs::create_dir_all(&memory).unwrap();
        std::fs::write(memory.join("PHOTO.PNG"), b"png").unwrap();
        let mut state = AppState::new(crate::config::Config::default()).await;
        state.workspace_dir = ws.path().to_owned();
        state.vector_store =
            std::sync::Arc::new(crate::services::vector::VectorStore::connect(ws.path()).await);

        let response = serve_memory_file_inner(&state, "one", "PHOTO.PNG")
            .await
            .unwrap();
        assert_eq!(
            response.headers()[axum::http::header::CONTENT_TYPE],
            "image/png"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn http_memory_symlink_never_serves_outside_bytes() {
        use std::os::unix::fs::symlink;

        let ws = tempfile::tempdir().unwrap();
        let memory = ws.path().join("instances/one/memory");
        std::fs::create_dir_all(&memory).unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(outside.path(), b"outside sentinel").unwrap();
        symlink(outside.path(), memory.join("leak.txt")).unwrap();
        let mut state = AppState::new(crate::config::Config::default()).await;
        state.workspace_dir = ws.path().to_owned();
        state.vector_store =
            std::sync::Arc::new(crate::services::vector::VectorStore::connect(ws.path()).await);

        assert_eq!(
            serve_memory_file_inner(&state, "one", "leak.txt")
                .await
                .unwrap_err(),
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn media_search_metadata_resolves_root_and_nested_files_through_public_endpoint() {
        use axum::{
            body::{Body, to_bytes},
            http::Request,
        };
        use tower::ServiceExt;
        let mock = MockServer::new(vec![(200, response(vec![1., 0., 0.])); 6]).await;
        let ws = tempfile::tempdir().unwrap();
        let dir = ws.path().join("instances/one/memory");
        std::fs::create_dir_all(dir.join("documents")).unwrap();
        for path in [
            "photo.png",
            "documents/report #1.pdf",
            "clip.mp4",
            "voice.mp3",
        ] {
            std::fs::write(dir.join(path), [0xff, 0x81]).unwrap();
            crate::services::media_text::write(&dir, path, "Orion description").unwrap();
        }
        let mut cfg = crate::config::Config::default();
        cfg.auth_token = "test-token".into();
        cfg.public_url = "https://memory.example".into();
        let mut state = AppState::new(cfg).await;
        state.workspace_dir = ws.path().to_owned();
        state.vector_store = std::sync::Arc::new(
            crate::services::vector::VectorStore::connect_with_config(ws.path(), &mock.config)
                .await,
        );
        state
            .vector_store
            .backfill_text_memories(ws.path(), "one")
            .await
            .unwrap();
        let app = router()
            .merge(public_memory_router())
            .merge(crate::routes::resources::router())
            .with_state(state.clone());
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/instances/one/memory/search?q=Orion")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let results: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 100_000).await.unwrap())
                .unwrap();
        assert_eq!(results.as_array().unwrap().len(), 4);
        for result in results.as_array().unwrap() {
            assert!(
                result["source_type"]
                    .as_str()
                    .unwrap()
                    .starts_with("media_")
            );
            let url = result["media_url"].as_str().unwrap();
            assert!(url.starts_with("/resources/browser/memory/one/"), "{url}");
            let response = app
                .clone()
                .oneshot(Request::builder().uri(url).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{url}");
            assert_eq!(
                to_bytes(response.into_body(), 100).await.unwrap().as_ref(),
                [0xff, 0x81]
            );
        }
        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/public/memory/one/photo.png")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
        state.config.write().await.public_url.clear();
        let Json(local) = search_memory(
            State(state.clone()),
            Path("one".into()),
            axum::extract::Query(SearchQuery {
                q: "Orion".into(),
                limit: 10,
            }),
        )
        .await
        .unwrap();
        assert!(local.as_array().unwrap().iter().all(|r| {
            r["media_url"]
                .as_str()
                .unwrap()
                .starts_with("/resources/browser/memory/one/")
        }));
        assert_eq!(
            delete_memory_file(
                State(state.clone()),
                Path(("one".into(), "photo.png".into()))
            )
            .await,
            StatusCode::OK
        );
        assert!(!dir.join("photo.png").exists());
        assert!(!dir.join("photo.png.md").exists());
        assert_eq!(
            state.vector_store.list_all("one", 10).await.unwrap().len(),
            3
        );
    }
}

#[cfg(test)]
mod receipt_tests {
    use super::*;
    use crate::domain::receipt::{Confidence, RecallReason, RecalledMemory, SourceStatus};
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;

    async fn state_with_receipt(ws: &std::path::Path) -> AppState {
        let memory = ws.join("instances/one/memory");
        std::fs::create_dir_all(&memory).unwrap();
        std::fs::write(memory.join("note.md"), "Orion nebula").unwrap();
        let message = crate::domain::chat::ChatMessage {
            id: "msg_1".into(),
            role: crate::domain::chat::ChatRole::Assistant,
            content: "hi".into(),
            created_at: "1".into(),
            kind: Default::default(),
            tool_name: None,
            mcp_app_html: None,
            mcp_app_input: None,
            model: None,
        };
        let memories = vec![
            RecalledMemory {
                path: "note.md".into(),
                source: "note.md".into(),
                excerpt: "Orion nebula".into(),
                reason: RecallReason::Semantic,
                linked_from: None,
                confidence: Confidence::High,
                retrieved_at: "2026-09-20T12:00:00Z".into(),
                source_status: SourceStatus::Present,
            },
            RecalledMemory {
                path: "gone.md".into(),
                source: "gone.md".into(),
                excerpt: "deleted later".into(),
                reason: RecallReason::Keyword,
                linked_from: None,
                confidence: Confidence::Low,
                retrieved_at: "2026-09-20T12:00:00Z".into(),
                source_status: SourceStatus::Present,
            },
        ];
        memory_receipts::write_receipts(ws, "one", "default", &[message], &memories).unwrap();
        let mut state = AppState::new(crate::config::Config::default()).await;
        state.workspace_dir = ws.to_owned();
        state.vector_store =
            std::sync::Arc::new(crate::services::vector::VectorStore::connect(ws).await);
        state
    }

    async fn get_json(state: &AppState, uri: &str) -> (StatusCode, serde_json::Value) {
        let response = router()
            .with_state(state.clone())
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, value)
    }

    #[tokio::test]
    async fn receipt_routes_serve_one_chat_and_report_missing_sources() {
        let ws = tempfile::tempdir().unwrap();
        let state = state_with_receipt(ws.path()).await;

        let (status, receipt) = get_json(&state, "/api/instances/one/default/receipts/msg_1").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(receipt["message_id"], "msg_1");
        assert_eq!(receipt["chat_id"], "default");
        assert_eq!(receipt["memories"][0]["path"], "note.md");
        assert_eq!(receipt["memories"][0]["reason"], "semantic");
        assert_eq!(receipt["memories"][0]["confidence"], "high");
        assert_eq!(receipt["memories"][0]["source_status"], "present");
        assert!(receipt["memories"][0].get("score").is_none());
        assert_eq!(receipt["memories"][1]["path"], "gone.md");
        assert_eq!(
            receipt["memories"][1]["source_status"], "missing",
            "a deleted source never fails the receipt"
        );

        let (status, listed) = get_json(&state, "/api/instances/one/default/receipts").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(listed.as_array().map(Vec::len), Some(1));
        assert_eq!(listed[0]["message_id"], "msg_1");

        for uri in [
            "/api/instances/one/default/receipts/msg_2",
            "/api/instances/one/other/receipts/msg_1",
            "/api/instances/one/default/receipts/..%2Fmsg_1",
        ] {
            let (status, _) = get_json(&state, uri).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        }
        let (status, listed) = get_json(&state, "/api/instances/one/other/receipts").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(listed, serde_json::json!([]));
    }
}

#[cfg(test)]
mod correction_tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::{Method, Request},
    };
    use tower::ServiceExt;

    const STAMPED: &str = "---\ncreated: 2026-01-01\nupdated: 2026-01-01\n---\nlikes tea\n";

    async fn state(ws: &std::path::Path) -> AppState {
        let memory = ws.join("instances/one/memory/about");
        std::fs::create_dir_all(&memory).unwrap();
        std::fs::write(memory.join("tea.md"), STAMPED).unwrap();
        std::fs::write(ws.join("instances/one/memory/photo.png"), [0xff]).unwrap();
        let mut state = AppState::new(crate::config::Config::default()).await;
        state.workspace_dir = ws.to_owned();
        state.vector_store =
            std::sync::Arc::new(crate::services::vector::VectorStore::connect(ws).await);
        state
    }

    async fn call(
        state: &AppState,
        method: Method,
        uri: &str,
        body: Option<serde_json::Value>,
    ) -> (StatusCode, serde_json::Value) {
        let mut request = Request::builder().method(method).uri(uri);
        let body = match body {
            Some(json) => {
                request = request.header(axum::http::header::CONTENT_TYPE, "application/json");
                Body::from(serde_json::to_vec(&json).unwrap())
            }
            None => Body::empty(),
        };
        let response = router()
            .with_state(state.clone())
            .oneshot(request.body(body).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, value)
    }

    fn body_of(ws: &std::path::Path) -> String {
        let raw = std::fs::read_to_string(ws.join("instances/one/memory/about/tea.md")).unwrap();
        memory::parse_frontmatter(&raw).1.to_owned()
    }

    #[tokio::test]
    async fn correction_routes_apply_park_conflicts_and_resolve() {
        let ws = tempfile::tempdir().unwrap();
        let state = state(ws.path()).await;
        let uri = "/api/instances/one/memory/about/tea.md";
        let correct = |content: &str| serde_json::json!({ "content": content });

        let (status, value) = call(&state, Method::PUT, uri, Some(correct("drinks oolong"))).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        assert_eq!(value["status"], "applied");
        assert_eq!(value["path"], "about/tea.md");
        assert_eq!(value["correction"]["statement"], "drinks oolong");
        assert_eq!(value["correction"]["previous"], "likes tea");
        assert_eq!(value["correction"]["status"], "applied");
        let first_id = value["correction"]["id"].as_str().unwrap().to_owned();
        assert_eq!(body_of(ws.path()), "drinks oolong");

        let (status, value) = call(&state, Method::PUT, uri, Some(correct("drinks oolong"))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["status"], "unchanged");

        // The second statement is parked with both statements listed.
        let (status, value) = call(&state, Method::PUT, uri, Some(correct("drinks matcha"))).await;
        assert_eq!(status, StatusCode::CONFLICT, "{value}");
        assert_eq!(value["status"], "needs_resolution");
        assert_eq!(value["path"], "about/tea.md");
        assert_eq!(value["current"]["id"], first_id);
        assert_eq!(value["current"]["statement"], "drinks oolong");
        assert_eq!(value["proposed"]["statement"], "drinks matcha");
        let conflict_id = value["conflict_id"].as_str().unwrap().to_owned();
        assert_eq!(value["proposed"]["id"], conflict_id);
        assert_eq!(body_of(ws.path()), "drinks oolong");

        let (status, ledger) = call(
            &state,
            Method::GET,
            "/api/instances/one/memory-corrections",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(ledger["version"], 1);
        assert_eq!(ledger["entries"].as_array().map(Vec::len), Some(2));
        assert_eq!(ledger["entries"][1]["status"], "needs_resolution");
        assert_eq!(ledger["entries"][1]["conflicts_with"], first_id);

        // Resolve in favour of the proposed statement.
        let (status, value) = call(
            &state,
            Method::POST,
            &format!("/api/instances/one/memory-corrections/{conflict_id}/resolve"),
            Some(serde_json::json!({ "keep": "proposed" })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{value}");
        assert_eq!(value["status"], "resolved");
        assert_eq!(value["kept"], "proposed");
        assert_eq!(value["entry"]["id"], conflict_id);
        assert_eq!(value["entry"]["status"], "applied");
        assert_eq!(body_of(ws.path()), "drinks matcha");
        let (status, _) = call(
            &state,
            Method::POST,
            &format!("/api/instances/one/memory-corrections/{conflict_id}/resolve"),
            Some(serde_json::json!({ "keep": "current" })),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "settled conflicts are gone");
        let (status, value) = call(
            &state,
            Method::POST,
            &format!("/api/instances/one/memory-corrections/{conflict_id}/resolve"),
            Some(serde_json::json!({ "keep": "both" })),
        )
        .await;
        assert!(status.is_client_error(), "{status} {value}");

        // Refusals keep the file as it is.
        let (status, value) = call(
            &state,
            Method::PUT,
            "/api/instances/one/memory/about/missing.md",
            Some(correct("x")),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(value["error"], "memory_correction");
        let (status, _) = call(&state, Method::PUT, uri, Some(correct("   "))).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        let (status, _) = call(
            &state,
            Method::PUT,
            uri,
            Some(correct(&"x".repeat(64 * 1024 + 1))),
        )
        .await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(body_of(ws.path()), "drinks matcha");
    }

    #[tokio::test]
    async fn flag_routes_rewrite_the_file_and_show_in_the_listing() {
        let ws = tempfile::tempdir().unwrap();
        let state = state(ws.path()).await;
        let uri = "/api/instances/one/memory/about/tea.md";

        let (status, value) = call(
            &state,
            Method::PATCH,
            uri,
            Some(serde_json::json!({ "pinned": true })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{value}");
        assert_eq!(
            value,
            serde_json::json!({ "path": "about/tea.md", "pinned": true, "exclude_from_proactive": false })
        );
        let (status, value) = call(
            &state,
            Method::PATCH,
            uri,
            Some(serde_json::json!({ "exclude_from_proactive": true })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{value}");
        assert_eq!(value["pinned"], true);
        assert_eq!(value["exclude_from_proactive"], true);
        let raw =
            std::fs::read_to_string(ws.path().join("instances/one/memory/about/tea.md")).unwrap();
        assert_eq!(
            raw,
            "---\ncreated: 2026-01-01\nupdated: 2026-01-01\npinned: true\nexclude_from_proactive: true\n---\nlikes tea\n"
        );

        // Still listed and searchable from the library, flags included.
        let (status, listed) = call(&state, Method::GET, "/api/instances/one/memory", None).await;
        assert_eq!(status, StatusCode::OK);
        let entry = listed
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["path"] == "about/tea.md")
            .unwrap();
        assert_eq!(entry["pinned"], true);
        assert_eq!(entry["exclude_from_proactive"], true);
        let (status, found) = call(
            &state,
            Method::GET,
            "/api/instances/one/memory/search?q=tea",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(found[0]["path"], "about/tea.md");

        // Media memories carry no flags; unknown paths are 404.
        let (status, value) = call(
            &state,
            Method::PATCH,
            "/api/instances/one/memory/photo.png",
            Some(serde_json::json!({ "pinned": true })),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{value}");
        let (status, _) = call(
            &state,
            Method::PATCH,
            "/api/instances/one/memory/about/missing.md",
            Some(serde_json::json!({ "pinned": true })),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // Forgetting still works on a flagged memory (memory_forget path).
        assert_eq!(
            delete_memory_file(
                State(state.clone()),
                Path(("one".into(), "about/tea.md".into()))
            )
            .await,
            StatusCode::OK
        );
        assert!(!ws.path().join("instances/one/memory/about/tea.md").exists());
    }
}
