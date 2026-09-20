//! Companion federation pairing (#108).
//!
//! Owner side, behind the API auth middleware:
//!
//! * `POST /api/federation/invites` mints a one-time invite (shown once).
//! * `DELETE /api/federation/invites/{id}` withdraws an outstanding invite.
//! * `POST /api/federation/accept` redeems an invite another owner handed over.
//! * `GET /api/federation/peers` lists this companion's identity, invites, and peers.
//! * `POST /api/federation/peers/{companion_id}/confirm` pairs a pending peer.
//! * `POST /api/federation/peers/{companion_id}/revoke` withdraws trust.
//!
//! Peer side, public, verified by signature only:
//!
//! * `POST /federation/v1/pair` redeems an invite with a signed pair request.
//! * `POST /federation/v1/pair/confirm` and `/revoke` carry signed notices.
//!
//! Every body is JSON and read whole under a size cap; nothing is taken from
//! the query string, and parse failures never echo the body.

use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use serde_json::json;

use crate::{
    app::state::AppState,
    domain::federation::{AcceptInvite, FederationError, SignedEnvelope},
    services::federation::{
        identity,
        pairing::{CONFIRM_PATH, MAX_ENVELOPE_BYTES, PAIR_PATH, REVOKE_PATH},
        peers,
    },
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/federation/invites", post(create_invite))
        .route("/api/federation/invites/{id}", delete(cancel_invite))
        .route("/api/federation/accept", post(accept_invite))
        .route("/api/federation/peers", get(list_peers))
        .route(
            "/api/federation/peers/{companion_id}/confirm",
            post(confirm_peer),
        )
        .route(
            "/api/federation/peers/{companion_id}/revoke",
            post(revoke_peer),
        )
}

/// Mounted outside the auth middleware: a peer has no owner credential and
/// must never need one. Every handler verifies a signature instead.
pub fn public_router() -> Router<AppState> {
    Router::new()
        .route(PAIR_PATH, post(pair))
        .route(CONFIRM_PATH, post(confirm_notice))
        .route(REVOKE_PATH, post(revoke_notice))
}

/// Federation refusals as typed JSON. The message is the error's own text,
/// which never carries a secret.
struct ApiError(FederationError);

impl From<FederationError> for ApiError {
    fn from(error: FederationError) -> Self {
        Self(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let _ = &self.0;
        todo!("PR 2: routes")
    }
}

async fn create_invite(State(state): State<AppState>) -> Result<Response, ApiError> {
    let _ = state;
    todo!("PR 2: routes")
}

async fn cancel_invite(State(state): State<AppState>, Path(id): Path<String>) -> StatusCode {
    let _ = (state, id);
    todo!("PR 2: routes")
}

async fn accept_invite(State(state): State<AppState>, body: Bytes) -> Result<Response, ApiError> {
    let _ = (state, body, MAX_ENVELOPE_BYTES);
    let _: Option<AcceptInvite> = None;
    todo!("PR 2: routes")
}

async fn list_peers(State(state): State<AppState>) -> Result<Response, ApiError> {
    let _ = state;
    todo!("PR 2: routes")
}

async fn confirm_peer(
    State(state): State<AppState>,
    Path(companion_id): Path<String>,
) -> Result<Response, ApiError> {
    let _ = (state, companion_id);
    todo!("PR 2: routes")
}

async fn revoke_peer(
    State(state): State<AppState>,
    Path(companion_id): Path<String>,
) -> Result<Response, ApiError> {
    let _ = (state, companion_id);
    todo!("PR 2: routes")
}

async fn pair(State(state): State<AppState>, body: Bytes) -> Result<Response, ApiError> {
    let _ = (state, body);
    let _: Option<SignedEnvelope> = None;
    let _ = (identity::parse_envelope, peers::FAILURE_WINDOW_SECS);
    let _ = (header::RETRY_AFTER, Json(json!({})));
    todo!("PR 2: routes")
}

async fn confirm_notice(State(state): State<AppState>, body: Bytes) -> Result<Response, ApiError> {
    let _ = (state, body);
    todo!("PR 2: routes")
}

async fn revoke_notice(State(state): State<AppState>, body: Bytes) -> Result<Response, ApiError> {
    let _ = (state, body);
    todo!("PR 2: routes")
}
