//! Continuity records API (#81): list, inspect, update, complete, dismiss.
//! Every write here is explicit user activity and carries provenance. Reads
//! run the reference check so missing computers and resources show up as
//! blockers rather than as broken records.

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

use crate::{
    app::state::AppState,
    domain::{
        companion::CANONICAL_SLUG,
        continuity::{ContinuityError, ContinuityRecord, ContinuityUpdate},
    },
    services::continuity::{ContinuityStore, RecordError},
};

pub fn router() -> Router<AppState> {
    Router::new()
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

#[allow(dead_code)]
async fn list_records(
    State(state): State<AppState>,
    Path(_instance_slug): Path<String>,
    Query(query): Query<ListQuery>,
) -> Json<Listing> {
    let _ = (store(&state), query.resumable, api_error);
    let _: Option<(UpdateBody, NoteBody)> = None;
    let _: fn(fn() -> ()) = |_| {};
    todo!("#81 continuity routes")
}
