//! Continuity records API (#81): list, inspect, update, complete, dismiss.
//! Every write here is explicit user activity and carries provenance. Reads
//! run the reference check so missing computers and resources show up as
//! blockers rather than as broken records. Every handle for the companion's
//! continuity directory shares one lock, so a read here never overwrites a
//! write from the chat tool or another request.

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::{
    app::state::AppState,
    domain::{
        companion::CANONICAL_SLUG,
        continuity::{
            ContinuityError, ContinuityRecord, ContinuityUpdate, Provenance, ProvenanceSource,
        },
    },
    services::continuity::{ContinuityStore, RecordError},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/instances/{instance_slug}/continuity",
            get(list_records),
        )
        .route(
            "/api/instances/{instance_slug}/continuity/{record_id}",
            get(get_record).put(update_record),
        )
        .route(
            "/api/instances/{instance_slug}/continuity/{record_id}/complete",
            post(complete_record),
        )
        .route(
            "/api/instances/{instance_slug}/continuity/{record_id}/dismiss",
            post(dismiss_record),
        )
}

fn store(state: &AppState) -> ContinuityStore {
    ContinuityStore::new(&state.workspace_dir, CANONICAL_SLUG)
}

#[derive(Deserialize)]
struct ListQuery {
    /// Only records the user can pick up again.
    #[serde(default)]
    resumable: bool,
}

#[derive(Serialize)]
struct Listing {
    records: Vec<ContinuityRecord>,
    /// Files under `continuity/` that could not be read. Surfaced, never deleted.
    errors: Vec<RecordError>,
}

#[derive(Deserialize)]
struct UpdateBody {
    #[serde(flatten)]
    update: ContinuityUpdate,
    /// Why; recorded as provenance.
    #[serde(default)]
    note: String,
}

#[derive(Deserialize, Default)]
struct NoteBody {
    #[serde(default)]
    note: String,
}

type ApiError = (StatusCode, Json<serde_json::Value>);

fn api_error(error: ContinuityError) -> ApiError {
    let status = match error {
        ContinuityError::NotFound => StatusCode::NOT_FOUND,
        ContinuityError::Invalid(_) => StatusCode::BAD_REQUEST,
        ContinuityError::TooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
        ContinuityError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (
        status,
        Json(serde_json::json!({
            "error": "continuity",
            "message": error.to_string(),
        })),
    )
}

fn by_user(note: &str, fallback: &str, now: i64) -> Provenance {
    let note = note.trim();
    Provenance {
        source: ProvenanceSource::User,
        at: now,
        note: if note.is_empty() { fallback } else { note }.to_owned(),
    }
}

async fn list_records(
    State(state): State<AppState>,
    Path(_instance_slug): Path<String>,
    Query(query): Query<ListQuery>,
) -> Json<Listing> {
    let now = chrono::Utc::now().timestamp();
    let (records, errors) = store(&state)
        .list_validated(&state.machine_registry, now, query.resumable)
        .await;
    Json(Listing { records, errors })
}

async fn get_record(
    State(state): State<AppState>,
    Path((_instance_slug, record_id)): Path<(String, String)>,
) -> Result<Json<ContinuityRecord>, ApiError> {
    let now = chrono::Utc::now().timestamp();
    store(&state)
        .validate_references(&record_id, &state.machine_registry, now)
        .await
        .map(Json)
        .ok_or_else(|| api_error(ContinuityError::NotFound))
}

async fn update_record(
    State(state): State<AppState>,
    Path((_instance_slug, record_id)): Path<(String, String)>,
    Json(body): Json<UpdateBody>,
) -> Result<Json<ContinuityRecord>, ApiError> {
    let now = chrono::Utc::now().timestamp();
    let provenance = Provenance {
        source: ProvenanceSource::User,
        at: now,
        note: body.note,
    };
    store(&state)
        .update(&record_id, &body.update, provenance, now)
        .await
        .map(Json)
        .map_err(api_error)
}

async fn complete_record(
    State(state): State<AppState>,
    Path((_instance_slug, record_id)): Path<(String, String)>,
    body: Option<Json<NoteBody>>,
) -> Result<Json<ContinuityRecord>, ApiError> {
    let now = chrono::Utc::now().timestamp();
    let note = body.map(|Json(body)| body.note).unwrap_or_default();
    store(&state)
        .complete(&record_id, by_user(&note, "marked done", now), now)
        .await
        .map(Json)
        .map_err(api_error)
}

async fn dismiss_record(
    State(state): State<AppState>,
    Path((_instance_slug, record_id)): Path<(String, String)>,
    body: Option<Json<NoteBody>>,
) -> Result<Json<ContinuityRecord>, ApiError> {
    let now = chrono::Utc::now().timestamp();
    let note = body.map(|Json(body)| body.note).unwrap_or_default();
    store(&state)
        .dismiss(&record_id, by_user(&note, "dismissed", now), now)
        .await
        .map(Json)
        .map_err(api_error)
}
