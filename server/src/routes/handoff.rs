//! Handoff cards API (#82), under the companion boundary. Reads build the
//! card and the pre-continuation preview; the three writes record the
//! user's decision. "Continue here" and "Continue on…" are the same
//! acceptance with the stable id of the chosen computer; the client only
//! differs in how it picked it. Nothing here drives a computer: an accepted
//! continuation is handed to the task's conversation, whose tools do the
//! work after the user's explicit acceptance.

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::Deserialize;

use crate::{
    app::state::AppState,
    domain::handoff::{ContinuationPreview, HandoffCard},
    services::handoff::{self, Accepted, HandoffError, Listing},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/instances/{instance_slug}/handoffs",
            get(list_handoffs),
        )
        .route(
            "/api/instances/{instance_slug}/continuity/{record_id}/handoff",
            get(get_handoff),
        )
        .route(
            "/api/instances/{instance_slug}/continuity/{record_id}/handoff/preview",
            get(preview_handoff),
        )
        .route(
            "/api/instances/{instance_slug}/continuity/{record_id}/handoff/accept",
            post(accept_handoff),
        )
        .route(
            "/api/instances/{instance_slug}/continuity/{record_id}/handoff/keep",
            post(keep_handoff),
        )
        .route(
            "/api/instances/{instance_slug}/continuity/{record_id}/handoff/dismiss",
            post(dismiss_handoff),
        )
}

#[derive(Deserialize)]
struct MachineQuery {
    machine_id: String,
}

#[derive(Deserialize)]
struct AcceptBody {
    /// The stable id of the computer to continue on.
    machine_id: String,
}

type ApiError = (StatusCode, Json<serde_json::Value>);

/// A refusal lists every check so the client can show the reasons; the
/// message is the same reasons as one sentence.
fn api_error(error: HandoffError) -> ApiError {
    let message = error.to_string();
    match error {
        HandoffError::NotFound => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "not_found", "message": message })),
        ),
        HandoffError::NotReady(checks) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "handoff_not_ready",
                "message": message,
                "checks": checks,
            })),
        ),
        HandoffError::Invalid(_) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "invalid", "message": message })),
        ),
        HandoffError::Storage(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "handoff", "message": message })),
        ),
    }
}

async fn list_handoffs(
    State(state): State<AppState>,
    Path(_instance_slug): Path<String>,
) -> Json<Listing> {
    let now = chrono::Utc::now().timestamp();
    Json(handoff::list(&state, now).await)
}

async fn get_handoff(
    State(state): State<AppState>,
    Path((_instance_slug, record_id)): Path<(String, String)>,
) -> Result<Json<HandoffCard>, ApiError> {
    let now = chrono::Utc::now().timestamp();
    handoff::card(&state, &record_id, now)
        .await
        .map(Json)
        .map_err(api_error)
}

async fn preview_handoff(
    State(state): State<AppState>,
    Path((_instance_slug, record_id)): Path<(String, String)>,
    Query(query): Query<MachineQuery>,
) -> Result<Json<ContinuationPreview>, ApiError> {
    let now = chrono::Utc::now().timestamp();
    handoff::preview(&state, &record_id, &query.machine_id, now)
        .await
        .map(Json)
        .map_err(api_error)
}

async fn accept_handoff(
    State(state): State<AppState>,
    Path((_instance_slug, record_id)): Path<(String, String)>,
    Json(body): Json<AcceptBody>,
) -> Result<Json<Accepted>, ApiError> {
    let now = chrono::Utc::now().timestamp();
    handoff::accept(&state, &record_id, &body.machine_id, now)
        .await
        .map(Json)
        .map_err(api_error)
}

async fn keep_handoff(
    State(state): State<AppState>,
    Path((_instance_slug, record_id)): Path<(String, String)>,
) -> Result<Json<HandoffCard>, ApiError> {
    let now = chrono::Utc::now().timestamp();
    handoff::keep(&state, &record_id, now)
        .await
        .map(Json)
        .map_err(api_error)
}

async fn dismiss_handoff(
    State(state): State<AppState>,
    Path((_instance_slug, record_id)): Path<(String, String)>,
) -> Result<Json<HandoffCard>, ApiError> {
    let now = chrono::Utc::now().timestamp();
    handoff::dismiss(&state, &record_id, now)
        .await
        .map(Json)
        .map_err(api_error)
}
