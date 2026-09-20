//! Inspect, edit, snooze, complete, and cancel commitments (#85).

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;

use crate::{
    app::state::AppState,
    domain::commitment::{Commitment, CompletionEvidence},
    services::commitments::{CommitmentError, CommitmentPatch, ListFilter, NewCommitment},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/instances/{instance_slug}/commitments",
            get(list_commitments).post(create_commitment),
        )
        .route(
            "/api/instances/{instance_slug}/commitments/{commitment_id}",
            get(get_commitment).patch(update_commitment),
        )
        .route(
            "/api/instances/{instance_slug}/commitments/{commitment_id}/snooze",
            post(snooze_commitment),
        )
        .route(
            "/api/instances/{instance_slug}/commitments/{commitment_id}/complete",
            post(complete_commitment),
        )
        .route(
            "/api/instances/{instance_slug}/commitments/{commitment_id}/cancel",
            post(cancel_commitment),
        )
}

/// Store refusals as typed JSON so the client can explain them.
struct ApiError(CommitmentError);

impl From<CommitmentError> for ApiError {
    fn from(error: CommitmentError) -> Self {
        Self(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self.0 {
            CommitmentError::NotFound(_) => StatusCode::NOT_FOUND,
            CommitmentError::Invalid(_) => StatusCode::BAD_REQUEST,
            CommitmentError::Closed { .. } => StatusCode::CONFLICT,
            CommitmentError::EvidenceRequired => StatusCode::UNPROCESSABLE_ENTITY,
            CommitmentError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
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

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default)]
    status: ListFilter,
}

async fn list_commitments(
    State(state): State<AppState>,
    Path(_instance_slug): Path<String>,
    Query(query): Query<ListQuery>,
) -> Json<Vec<Commitment>> {
    Json(state.commitments.list(query.status))
}

async fn create_commitment(
    State(state): State<AppState>,
    Path(_instance_slug): Path<String>,
    Json(new): Json<NewCommitment>,
) -> Result<(StatusCode, Json<Commitment>), ApiError> {
    let commitment = state.commitments.create(new, now())?;
    Ok((StatusCode::CREATED, Json(commitment)))
}

async fn get_commitment(
    State(state): State<AppState>,
    Path((_instance_slug, commitment_id)): Path<(String, String)>,
) -> Result<Json<Commitment>, ApiError> {
    state
        .commitments
        .get(&commitment_id)
        .map(Json)
        .ok_or(ApiError(CommitmentError::NotFound(commitment_id)))
}

async fn update_commitment(
    State(state): State<AppState>,
    Path((_instance_slug, commitment_id)): Path<(String, String)>,
    Json(patch): Json<CommitmentPatch>,
) -> Result<Json<Commitment>, ApiError> {
    Ok(Json(state.commitments.update(
        &commitment_id,
        patch,
        now(),
    )?))
}

#[derive(Deserialize)]
struct SnoozeBody {
    until: i64,
}

async fn snooze_commitment(
    State(state): State<AppState>,
    Path((_instance_slug, commitment_id)): Path<(String, String)>,
    Json(body): Json<SnoozeBody>,
) -> Result<Json<Commitment>, ApiError> {
    Ok(Json(state.commitments.snooze(
        &commitment_id,
        body.until,
        now(),
    )?))
}

async fn complete_commitment(
    State(state): State<AppState>,
    Path((_instance_slug, commitment_id)): Path<(String, String)>,
    Json(evidence): Json<CompletionEvidence>,
) -> Result<Json<Commitment>, ApiError> {
    Ok(Json(state.commitments.complete(
        &commitment_id,
        evidence,
        now(),
    )?))
}

async fn cancel_commitment(
    State(state): State<AppState>,
    Path((_instance_slug, commitment_id)): Path<(String, String)>,
) -> Result<Json<Commitment>, ApiError> {
    Ok(Json(state.commitments.cancel(&commitment_id, now())?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::companion::CANONICAL_SLUG;
    use crate::services::commitments::CommitmentStore;
    use axum::{
        body::{Body, to_bytes},
        http::{Method, Request},
    };
    use tower::ServiceExt;

    async fn state(workspace: &std::path::Path) -> AppState {
        let mut state = AppState::new(crate::config::Config::default()).await;
        state.workspace_dir = workspace.to_path_buf();
        state.commitments = CommitmentStore::new(workspace, CANONICAL_SLUG);
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
                request = request.header("content-type", "application/json");
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
        // Framework rejections (bad JSON shape) are plain text; keep them readable.
        let value = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| serde_json::Value::String(String::from_utf8_lossy(&bytes).into()));
        (status, value)
    }

    #[tokio::test]
    async fn user_can_inspect_edit_snooze_complete_and_cancel_commitments() {
        let workspace = tempfile::tempdir().unwrap();
        let state = state(workspace.path()).await;
        let base = format!("/api/instances/{CANONICAL_SLUG}/commitments");
        let far = chrono::Utc::now().timestamp() + 86_400;

        let (status, created) = call(
            &state,
            Method::POST,
            &base,
            Some(serde_json::json!({
                "promise": "send the photos from Saturday",
                "deadline": {"kind": "at", "at": far},
                "provenance": {"kind": "chat", "chat_id": "default"},
            })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{created}");
        assert_eq!(created["status"], "active");
        assert_eq!(created["owner"], "companion");
        assert_eq!(created["next_check"], far);
        let id = created["id"].as_str().unwrap().to_owned();

        let (status, list) = call(&state, Method::GET, &base, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(list.as_array().unwrap().len(), 1);
        let (status, one) = call(&state, Method::GET, &format!("{base}/{id}"), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(one["id"], id);

        let (status, edited) = call(
            &state,
            Method::PATCH,
            &format!("{base}/{id}"),
            Some(serde_json::json!({
                "promise": "send the photos from Saturday to Ana",
                "waiting_on": {"kind": "user_reply"},
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{edited}");
        assert_eq!(edited["promise"], "send the photos from Saturday to Ana");
        assert_eq!(edited["status"], "waiting");

        let (status, snoozed) = call(
            &state,
            Method::POST,
            &format!("{base}/{id}/snooze"),
            Some(serde_json::json!({"until": far + 3600})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{snoozed}");
        assert_eq!(snoozed["snoozed_until"], far + 3600);
        assert_eq!(snoozed["snooze_count"], 1);

        let (status, refused) = call(
            &state,
            Method::POST,
            &format!("{base}/{id}/complete"),
            Some(serde_json::json!({})),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(refused["error"], "evidence_required");
        assert!(
            refused["message"]
                .as_str()
                .unwrap()
                .contains("confirmation")
        );

        let (status, done) = call(
            &state,
            Method::POST,
            &format!("{base}/{id}/complete"),
            Some(serde_json::json!({"confirmed_by_user": true})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{done}");
        assert_eq!(done["status"], "completed");
        assert_eq!(done["completion"]["confirmed_by_user"], true);

        let (status, closed) =
            call(&state, Method::POST, &format!("{base}/{id}/cancel"), None).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(closed["error"], "closed");

        let (status, open) = call(&state, Method::GET, &base, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(open.as_array().unwrap().len(), 0, "closed by default");
        let (_, all) = call(&state, Method::GET, &format!("{base}?status=all"), None).await;
        assert_eq!(all.as_array().unwrap().len(), 1);

        let (status, other) = call(
            &state,
            Method::POST,
            &base,
            Some(serde_json::json!({"promise": "book the table", "owner": "user"})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let other_id = other["id"].as_str().unwrap().to_owned();
        let (status, dismissed) = call(
            &state,
            Method::POST,
            &format!("{base}/{other_id}/cancel"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(dismissed["status"], "dismissed");
    }

    #[tokio::test]
    async fn refusals_are_typed_and_never_touch_storage() {
        let workspace = tempfile::tempdir().unwrap();
        let state = state(workspace.path()).await;
        let base = format!("/api/instances/{CANONICAL_SLUG}/commitments");

        let (status, missing) =
            call(&state, Method::GET, &format!("{base}/cmt_missing"), None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(missing["error"], "not_found");
        let (status, _) = call(
            &state,
            Method::POST,
            &format!("{base}/cmt_missing/snooze"),
            Some(serde_json::json!({"until": 1})),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, invalid) = call(
            &state,
            Method::POST,
            &base,
            Some(serde_json::json!({"promise": "   "})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(invalid["error"], "invalid");

        let (status, _) = call(
            &state,
            Method::POST,
            &base,
            Some(serde_json::json!({"promise": "x", "deadline": {"kind": "soon"}})),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "axum rejects the shape"
        );

        assert!(
            !workspace
                .path()
                .join("instances")
                .join(CANONICAL_SLUG)
                .join("commitments")
                .exists(),
            "nothing was written"
        );
    }
}
