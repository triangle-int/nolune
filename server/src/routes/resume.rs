//! Resume my work API (#83), under the companion boundary. The ritual is
//! read and configured here, invoked by the user ("Resume my work"), told
//! that Nolune was opened, and answered (not now, snooze, never this one).
//! Every answer is a bounded suggestion or the reason there is none; the
//! suggestion leads to the record's handoff card, and only accepting that
//! card (its own routes) continues anything.

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::{
    app::state::AppState,
    domain::resume::{ResumeRitualPolicy, RitualTrigger},
    services::resume_ritual::{self, Held, PolicyEdit, ResumeOffer, ResumeStatus},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/instances/{instance_slug}/resume",
            get(get_resume).put(put_policy).post(invoke_resume),
        )
        .route("/api/instances/{instance_slug}/resume/opened", post(opened))
        .route("/api/instances/{instance_slug}/resume/refuse", post(refuse))
        .route("/api/instances/{instance_slug}/resume/snooze", post(snooze))
        .route(
            "/api/instances/{instance_slug}/resume/dismiss",
            post(dismiss),
        )
}

/// What a trigger produced: one suggestion, or why there is none.
#[derive(Serialize)]
struct Outcome {
    suggestion: Option<ResumeOffer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    held: Option<Held>,
}

impl From<Result<ResumeOffer, Held>> for Outcome {
    fn from(result: Result<ResumeOffer, Held>) -> Self {
        match result {
            Ok(offer) => Self {
                suggestion: Some(offer),
                held: None,
            },
            Err(held) => Self {
                suggestion: None,
                held: Some(held),
            },
        }
    }
}

#[derive(Deserialize)]
struct SnoozeBody {
    /// Unix seconds; `null` ends the snooze.
    until: Option<i64>,
}

#[derive(Deserialize)]
struct DismissBody {
    record_id: String,
}

type ApiError = (StatusCode, Json<serde_json::Value>);

fn storage_error(error: std::io::Error) -> ApiError {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "error": "resume", "message": error.to_string() })),
    )
}

async fn get_resume(
    State(state): State<AppState>,
    Path(_instance_slug): Path<String>,
) -> Json<ResumeStatus> {
    let now = chrono::Utc::now().timestamp();
    Json(resume_ritual::status(&state, now).await)
}

async fn put_policy(
    State(state): State<AppState>,
    Path(_instance_slug): Path<String>,
    Json(edit): Json<PolicyEdit>,
) -> Result<Json<ResumeRitualPolicy>, ApiError> {
    resume_ritual::set_policy(&state, &edit)
        .await
        .map(Json)
        .map_err(storage_error)
}

/// "Resume my work": an explicit request. Refused with `409` while the
/// ritual is off; otherwise one suggestion or why there is none.
async fn invoke_resume(
    State(state): State<AppState>,
    Path(_instance_slug): Path<String>,
) -> Result<Json<Outcome>, ApiError> {
    let now = chrono::Utc::now().timestamp();
    match resume_ritual::invoke(&state, RitualTrigger::Manual, now).await {
        Err(Held::Disabled) => Err((
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "resume_disabled",
                "message": Held::Disabled.to_string(),
            })),
        )),
        Err(Held::Storage { message }) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "resume", "message": message })),
        )),
        result => Ok(Json(Outcome::from(result))),
    }
}

/// The client opened Nolune or brought it back: a suggestion only after
/// the configured break, and nothing at all while the ritual is off.
async fn opened(
    State(state): State<AppState>,
    Path(_instance_slug): Path<String>,
) -> Json<Outcome> {
    let now = chrono::Utc::now().timestamp();
    Json(Outcome::from(resume_ritual::opened(&state, now).await))
}

async fn refuse(
    State(state): State<AppState>,
    Path(_instance_slug): Path<String>,
) -> Result<StatusCode, ApiError> {
    let now = chrono::Utc::now().timestamp();
    resume_ritual::refuse(&state, now)
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(storage_error)
}

async fn snooze(
    State(state): State<AppState>,
    Path(_instance_slug): Path<String>,
    Json(body): Json<SnoozeBody>,
) -> Result<Json<ResumeRitualPolicy>, ApiError> {
    let now = chrono::Utc::now().timestamp();
    if body.until.is_some_and(|until| until <= now) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "invalid",
                "message": "a snooze must end in the future",
            })),
        ));
    }
    resume_ritual::snooze(&state, body.until)
        .await
        .map(Json)
        .map_err(storage_error)
}

async fn dismiss(
    State(state): State<AppState>,
    Path(_instance_slug): Path<String>,
    Json(body): Json<DismissBody>,
) -> Result<Json<ResumeRitualPolicy>, ApiError> {
    if !crate::domain::continuity::is_valid_id(&body.record_id) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "invalid",
                "message": "record_id is not a record id",
            })),
        ));
    }
    resume_ritual::dismiss(&state, &body.record_id)
        .await
        .map(Json)
        .map_err(storage_error)
}
