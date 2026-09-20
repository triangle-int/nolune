//! Activity receipts and policy for the one proactive loop (#92).

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::Deserialize;

use crate::{
    app::state::AppState,
    domain::proactive::{ProactivePolicy, ProactiveRun, Trigger},
    services::proactive::Admission,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/instances/{instance_slug}/activity",
            get(list_activity),
        )
        .route(
            "/api/instances/{instance_slug}/activity/{run_id}",
            get(get_activity),
        )
        .route(
            "/api/instances/{instance_slug}/activity/{run_id}/cancel",
            post(cancel_activity),
        )
        .route(
            "/api/instances/{instance_slug}/activity/{run_id}/retry",
            post(retry_activity),
        )
        .route(
            "/api/instances/{instance_slug}/proactive",
            get(get_policy).put(set_policy),
        )
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    50
}

async fn list_activity(
    State(state): State<AppState>,
    Path(_instance_slug): Path<String>,
    Query(query): Query<ListQuery>,
) -> Json<Vec<ProactiveRun>> {
    Json(state.proactive.list(query.limit.clamp(1, 500)))
}

async fn get_activity(
    State(state): State<AppState>,
    Path((_instance_slug, run_id)): Path<(String, String)>,
) -> Result<Json<ProactiveRun>, StatusCode> {
    state
        .proactive
        .get(&run_id)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

async fn cancel_activity(
    State(state): State<AppState>,
    Path((_instance_slug, run_id)): Path<(String, String)>,
) -> StatusCode {
    if state.proactive.get(&run_id).is_none() {
        return StatusCode::NOT_FOUND;
    }
    if state.proactive.cancel(&run_id) {
        StatusCode::OK
    } else {
        StatusCode::CONFLICT
    }
}

async fn retry_activity(
    State(state): State<AppState>,
    Path((_instance_slug, run_id)): Path<(String, String)>,
) -> Result<(StatusCode, Json<ProactiveRun>), (StatusCode, String)> {
    let Some(previous) = state.proactive.get(&run_id) else {
        return Err((StatusCode::NOT_FOUND, format!("unknown run {run_id}")));
    };
    let now = chrono::Utc::now().timestamp();
    if matches!(previous.trigger, Trigger::Commitment { .. }) {
        // A commitment check is executed by its evaluator (#85), which admits
        // the linked attempt and runs it; the route never leaves a run behind.
        return match crate::services::commitment_evaluator::retry(&state, &run_id, now).await {
            Ok(run) => Ok((StatusCode::ACCEPTED, Json(run))),
            Err(message) => Err((StatusCode::CONFLICT, message)),
        };
    }
    match state.proactive.retry(&run_id, now) {
        Ok(Admission::Admitted(handle)) => {
            // The retry is admitted as a pending run; the owning trigger's worker
            // (#93 narrows this to the companion check-in) executes it. Until
            // then it is visible and cancellable like any other run.
            let run = state
                .proactive
                .get(handle.id())
                .ok_or((StatusCode::INTERNAL_SERVER_ERROR, "run vanished".into()))?;
            std::mem::forget(handle);
            Ok((StatusCode::ACCEPTED, Json(run)))
        }
        Ok(Admission::Skipped(run)) => Ok((StatusCode::ACCEPTED, Json(run))),
        Err(message) => Err((StatusCode::CONFLICT, message)),
    }
}

async fn get_policy(
    State(state): State<AppState>,
    Path(_instance_slug): Path<String>,
) -> Json<ProactivePolicy> {
    Json(state.proactive.policy())
}

async fn set_policy(
    State(state): State<AppState>,
    Path(_instance_slug): Path<String>,
    Json(policy): Json<ProactivePolicy>,
) -> Result<StatusCode, (StatusCode, String)> {
    if let Some(quiet) = policy.quiet_hours
        && (quiet.start_hour > 23 || quiet.end_hour > 23)
    {
        return Err((StatusCode::BAD_REQUEST, "quiet hours must be 0-23".into()));
    }
    for (label, hours) in [
        ("check_in_interval_hours", policy.check_in_interval_hours),
        (
            "reflection_interval_hours",
            policy.reflection_interval_hours,
        ),
    ] {
        if !hours.is_finite() || !(0.25..=24.0 * 30.0).contains(&hours) {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("{label} must be between 0.25 and 720 hours"),
            ));
        }
    }
    if policy.retention_max == 0 || policy.retention_days == 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            "retention must keep at least one record for one day".into(),
        ));
    }
    state
        .proactive
        .set_policy(&policy)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(StatusCode::OK)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::commitment::Deadline;
    use crate::domain::companion::CANONICAL_SLUG;
    use crate::domain::proactive::{RunStatus, Target, Trigger};
    use crate::services::commitment_evaluator::CommitmentEvaluator;
    use crate::services::commitments::{CommitmentStore, NewCommitment};
    use crate::services::proactive::ProactiveLoop;
    use axum::{
        body::{Body, to_bytes},
        http::{Method, Request},
    };
    use tower::ServiceExt;

    async fn state(workspace: &std::path::Path) -> AppState {
        let mut state = AppState::new(crate::config::Config::default()).await;
        state.workspace_dir = workspace.to_path_buf();
        state.proactive = ProactiveLoop::new(workspace, CANONICAL_SLUG);
        state.commitments = CommitmentStore::new(workspace, CANONICAL_SLUG);
        state
    }

    async fn post(state: &AppState, uri: &str) -> (StatusCode, serde_json::Value) {
        let request = Request::builder()
            .method(Method::POST)
            .uri(uri)
            .body(Body::empty())
            .unwrap();
        let response = router()
            .with_state(state.clone())
            .oneshot(request)
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let value = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| serde_json::Value::String(String::from_utf8_lossy(&bytes).into()));
        (status, value)
    }

    #[tokio::test]
    async fn a_commitment_retry_goes_through_the_evaluator_and_never_orphans_a_run() {
        let workspace = tempfile::tempdir().unwrap();
        let state = state(workspace.path()).await;
        let instance_dir = workspace.path().join("instances").join(CANONICAL_SLUG);
        std::fs::create_dir_all(&instance_dir).unwrap();
        std::fs::write(instance_dir.join("soul.md"), "i am little moon").unwrap();
        let now = chrono::Utc::now().timestamp();
        let call = state
            .commitments
            .create(
                NewCommitment {
                    promise: "call the dentist".into(),
                    deadline: Some(Deadline::At { at: now - 60 }),
                    ..Default::default()
                },
                now - 120,
            )
            .unwrap();
        let evaluator =
            CommitmentEvaluator::new(state.commitments.clone(), state.proactive.clone());
        let mut runs = evaluator.admit_due(now);
        assert_eq!(runs.len(), 1);
        let failed = evaluator.finish(runs.remove(0), Err("provider offline".into()), now);
        let trigger = Trigger::Commitment {
            commitment_id: call.id.clone(),
        };
        let retry_uri = |id: &str| format!("/api/instances/{CANONICAL_SLUG}/activity/{id}/retry");

        // Without a background model the check cannot run: the retry is
        // refused, and nothing is admitted or parked under the dedupe key.
        let (status, body) = post(&state, &retry_uri(&failed.id)).await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert!(
            body.as_str()
                .is_some_and(|s| s.contains("background model")),
            "{body}"
        );
        assert_eq!(state.proactive.running(&trigger), None);
        assert_eq!(
            state.proactive.list(usize::MAX).len(),
            1,
            "no run was admitted"
        );
        assert_eq!(
            state.commitments.get(&call.id, now).unwrap().next_check,
            Some(now + crate::services::commitment_evaluator::RETRY_BACKOFF_SECS),
            "the record is untouched"
        );

        // The evaluator's own backoff look still happens.
        let later = now + crate::services::commitment_evaluator::RETRY_BACKOFF_SECS;
        let mut runs = evaluator.admit_due(later);
        assert_eq!(runs.len(), 1);
        let done = evaluator.finish(
            runs.remove(0),
            Ok(crate::domain::proactive::RunOutcome::default()),
            later + 1,
        );
        assert_eq!(done.status, RunStatus::Completed);

        // Other triggers keep the loop's plain retry, and unknown runs are 404.
        let Admission::Admitted(heartbeat) = state.proactive.begin_at(
            Trigger::Heartbeat {
                agent: "companion".into(),
            },
            "check-in",
            Target::Companion,
            now,
        ) else {
            panic!("admitted");
        };
        let heartbeat = heartbeat.fail_at("provider offline", true, now + 1);
        let (status, body) = post(&state, &retry_uri(&heartbeat.id)).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        assert_eq!(body["retry_of"], heartbeat.id);
        assert_eq!(body["attempt"], 2);
        let (status, _) = post(&state, &retry_uri("run_missing")).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}
