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
//! the query string, and parse failures never echo the body. There is no
//! failure counter on the public routes: the invite secret is 32 random
//! bytes, and a counter would only let a stranger lock the owner out.

use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, Request, State},
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

/// Refusals as typed JSON. The message is the error's own text, which never
/// carries a secret or a request body.
enum ApiError {
    Federation(FederationError),
    /// The body is not the JSON shape the route takes. No detail on purpose.
    InvalidBody,
    PayloadTooLarge,
}

impl From<FederationError> for ApiError {
    fn from(error: FederationError) -> Self {
        Self::Federation(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let error = match self {
            Self::InvalidBody => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": "invalid_body",
                        "message": "request body does not have the expected shape",
                    })),
                )
                    .into_response();
            }
            Self::PayloadTooLarge => {
                return (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    Json(json!({
                        "error": "payload_too_large",
                        "message": format!("request body exceeds {MAX_ENVELOPE_BYTES} bytes"),
                    })),
                )
                    .into_response();
            }
            Self::Federation(error) => error,
        };
        let (status, code) = match &error {
            FederationError::VersionTooOld { .. } => (StatusCode::BAD_REQUEST, "version_too_old"),
            FederationError::VersionUnsupported { .. } => {
                (StatusCode::BAD_REQUEST, "version_unsupported")
            }
            FederationError::Malformed(_) => (StatusCode::BAD_REQUEST, "malformed"),
            FederationError::InvalidPublicKey => (StatusCode::BAD_REQUEST, "invalid_public_key"),
            FederationError::InvalidOrigin(_) => (StatusCode::BAD_REQUEST, "invalid_origin"),
            FederationError::SignatureMismatch => (StatusCode::FORBIDDEN, "signature_mismatch"),
            FederationError::CompanionIdMismatch => {
                (StatusCode::FORBIDDEN, "companion_id_mismatch")
            }
            FederationError::SenderMismatch => (StatusCode::FORBIDDEN, "sender_mismatch"),
            FederationError::IssuerMismatch => (StatusCode::FORBIDDEN, "issuer_mismatch"),
            FederationError::RecipientMismatch => (StatusCode::FORBIDDEN, "recipient_mismatch"),
            FederationError::PairingMismatch => (StatusCode::FORBIDDEN, "pairing_mismatch"),
            FederationError::PeerRevoked => (StatusCode::FORBIDDEN, "peer_revoked"),
            FederationError::Expired { .. } => (StatusCode::FORBIDDEN, "expired"),
            FederationError::IssuedInFuture { .. } => (StatusCode::FORBIDDEN, "issued_in_future"),
            FederationError::InvalidLifetime { .. } => {
                (StatusCode::BAD_REQUEST, "invalid_lifetime")
            }
            FederationError::BodyHashMismatch => (StatusCode::FORBIDDEN, "body_hash_mismatch"),
            FederationError::Replayed => (StatusCode::FORBIDDEN, "replayed"),
            FederationError::KeyRetired => (StatusCode::FORBIDDEN, "key_retired"),
            FederationError::RotationMismatch => (StatusCode::FORBIDDEN, "rotation_mismatch"),
            FederationError::InviteInvalid => (StatusCode::UNAUTHORIZED, "invalid_invite"),
            FederationError::UnknownPeer => (StatusCode::NOT_FOUND, "unknown_peer"),
            FederationError::PeerNotPaired { .. } => (StatusCode::CONFLICT, "peer_not_paired"),
            FederationError::Transport(_) => (StatusCode::BAD_GATEWAY, "peer_unreachable"),
            FederationError::PeerRefused { .. } => (StatusCode::BAD_GATEWAY, "peer_refused"),
            FederationError::KeyMismatch
            | FederationError::SigningKeyMissing(_)
            | FederationError::IdentityDocumentMissing(_)
            | FederationError::InsecureKeyPermissions { .. }
            | FederationError::RandomnessUnavailable
            | FederationError::ReplayCapacity
            | FederationError::Io { .. } => {
                (StatusCode::SERVICE_UNAVAILABLE, "federation_unavailable")
            }
        };
        let mut body = json!({ "error": code, "message": error.to_string() });
        match &error {
            FederationError::PeerRefused { status, error } => {
                body["peer_status"] = json!(status);
                body["peer_error"] = json!(error);
            }
            FederationError::PeerNotPaired { state } => body["state"] = json!(state),
            _ => {}
        }
        (status, Json(body)).into_response()
    }
}

/// Reads the whole body under the envelope size cap.
async fn read_body(request: Request) -> Result<Bytes, ApiError> {
    let declared = request
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok());
    if declared.is_some_and(|length| length > MAX_ENVELOPE_BYTES) {
        return Err(ApiError::PayloadTooLarge);
    }
    axum::body::to_bytes(request.into_body(), MAX_ENVELOPE_BYTES)
        .await
        .map_err(|_| ApiError::PayloadTooLarge)
}

/// Parses a signed envelope, keeping the version-first refusal and replacing
/// any shape complaint with one that does not quote the body.
fn parse_envelope(bytes: &[u8]) -> Result<SignedEnvelope, ApiError> {
    let text = std::str::from_utf8(bytes).map_err(|_| ApiError::InvalidBody)?;
    identity::parse_envelope(text).map_err(|error| match error {
        FederationError::Malformed(_) => ApiError::Federation(FederationError::Malformed(
            "request body is not a signed envelope".into(),
        )),
        other => ApiError::Federation(other),
    })
}

async fn create_invite(State(state): State<AppState>) -> Result<Response, ApiError> {
    let origin = state.config.read().await.public_url.clone();
    let invite = state.federation.create_invite(&origin)?;
    Ok((StatusCode::CREATED, Json(invite)).into_response())
}

async fn cancel_invite(State(state): State<AppState>, Path(id): Path<String>) -> StatusCode {
    if state.federation.cancel_invite(&id) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

async fn accept_invite(
    State(state): State<AppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let bytes = read_body(request).await?;
    let accept: AcceptInvite = serde_json::from_slice(&bytes).map_err(|_| ApiError::InvalidBody)?;
    let own_origin = state.config.read().await.public_url.clone();
    let peer = state.federation.accept_invite(accept, &own_origin).await?;
    Ok(Json(json!({ "peer": peer.summary() })).into_response())
}

async fn list_peers(State(state): State<AppState>) -> Result<Response, ApiError> {
    Ok(Json(state.federation.overview()?).into_response())
}

async fn confirm_peer(
    State(state): State<AppState>,
    Path(companion_id): Path<String>,
) -> Result<Response, ApiError> {
    let (peer, notified) = state.federation.confirm_peer(&companion_id).await?;
    Ok(Json(json!({ "peer": peer.summary(), "notified": notified })).into_response())
}

async fn revoke_peer(
    State(state): State<AppState>,
    Path(companion_id): Path<String>,
) -> Result<Response, ApiError> {
    let (peer, notified) = state.federation.revoke_peer(&companion_id).await?;
    Ok(Json(json!({ "peer": peer.summary(), "notified": notified })).into_response())
}

async fn pair(State(state): State<AppState>, request: Request) -> Result<Response, ApiError> {
    let envelope = parse_envelope(&read_body(request).await?)?;
    Ok(Json(state.federation.receive_pair_request(&envelope)?).into_response())
}

async fn confirm_notice(
    State(state): State<AppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let envelope = parse_envelope(&read_body(request).await?)?;
    Ok(Json(state.federation.receive_confirm(&envelope)?).into_response())
}

async fn revoke_notice(
    State(state): State<AppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let envelope = parse_envelope(&read_body(request).await?)?;
    Ok(Json(state.federation.receive_revoke(&envelope)?).into_response())
}
