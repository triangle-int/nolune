//! Companion federation pairing (#108).
//!
//! Owner side, behind the API auth middleware:
//!
//! * `POST /api/federation/invites` mints a one-time invite (shown once),
//!   as its fields and as the one-line `invite` form the owner hands over.
//! * `DELETE /api/federation/invites/{id}` withdraws an outstanding invite.
//! * `POST /api/federation/accept` redeems an invite another owner handed
//!   over: the three fields, or the one line as `{ "invite": "…" }`.
//! * `GET /api/federation/peers` lists this companion's identity, invites, and peers.
//! * `POST /api/federation/peers/{companion_id}/confirm` pairs a pending peer.
//! * `POST /api/federation/peers/{companion_id}/revoke` withdraws trust.
//! * `POST /api/federation/rotate` replaces this companion's key, withdraws
//!   invites and pending pairings, and tells every paired peer.
//! * `GET /api/federation/policy` lists the owner's federation policy and
//!   the defaults that apply where it says nothing (#109).
//! * `GET /api/federation/receipts` lists the audit receipts, newest first.
//! * `POST /api/federation/peers/{companion_id}/ping` pings a paired peer
//!   through its policy and records the answer on this side.
//! * `GET /api/federation/approvals` lists the requests that asked the
//!   owner (#109, PR 3); `POST …/approvals/{id}/approve` and `…/deny` take
//!   `{ "scope": "once" | "until", "expires_at" | "class" }`; `DELETE
//!   …/approvals/{id}` withdraws an entry whatever its status.
//! * `POST /api/federation/peers/{companion_id}/rules` writes one rule
//!   (`intent`, `disclosure`, `access`, optional `expires_at`), replacing
//!   the rule for that pair; `POST …/rules/revoke` takes `{ "intent",
//!   "disclosure" }` and revokes one capability. Revoking the peer drops
//!   every rule and pending approval it had. Every owner decision is an
//!   audit receipt. Paths name a companion id or an entry id and nothing
//!   else; everything the owner chooses travels in a body.
//!
//! Peer side, public, verified by signature only. Every verified envelope
//! is judged by the owner's policy and recorded before it is dispatched
//! (#109): a refusal is `403` with a `policy_denied`, `approval_required`,
//! or `deferred` code, or `429 rate_limited`, each carrying the decision in
//! its wire shape: a rate limit says how long the peer's own window has
//! left, a deferral says nothing about when the owner's quiet hours end.
//!
//! * `POST /federation/v1/pair` redeems an invite with a signed pair request.
//! * `POST /federation/v1/pair/confirm` and `/revoke` carry signed notices.
//! * `POST /federation/v1/ping` takes a transport envelope from a paired
//!   peer and answers with one (#108, PR 3).
//! * `POST /federation/v1/rotate` takes a key rotation notice signed by the
//!   retiring key and answers with an ack for the new identity.
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
use serde::Deserialize;
use serde_json::json;

use crate::{
    app::state::AppState,
    domain::{
        federation::{AcceptInvite, FederationError, SignedEnvelope, TransportEnvelope},
        federation_policy::{
            ApprovalScope, Decision, DecisionReason, DisclosureClass, IntentClass, ReceiptSide,
            RuleRequest, Verdict,
        },
    },
    services::federation::{
        envelope, identity, invite_token,
        pairing::{
            CONFIRM_PATH, MAX_ENVELOPE_BYTES, PAIR_PATH, PING_PATH, REVOKE_PATH, ROTATE_PATH,
        },
    },
};

/// The one-line form of an invite, as `nolune federation accept` and the
/// Companions section send it back.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InviteLine {
    invite: String,
}

/// Which rule to revoke: one intent at one disclosure class, matched
/// against the closed classes.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleKey {
    intent: IntentClass,
    disclosure: DisclosureClass,
}

/// What the accept route takes: the one line, or the three fields.
#[derive(Deserialize)]
#[serde(untagged)]
enum AcceptRequest {
    Line(InviteLine),
    Fields(AcceptInvite),
}

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
        .route("/api/federation/rotate", post(rotate_identity))
        .route("/api/federation/policy", get(show_policy))
        .route("/api/federation/receipts", get(list_receipts))
        .route("/api/federation/peers/{companion_id}/ping", post(ping_peer))
        .route("/api/federation/approvals", get(list_approvals))
        .route("/api/federation/approvals/{id}", delete(withdraw_approval))
        .route(
            "/api/federation/approvals/{id}/approve",
            post(approve_request),
        )
        .route("/api/federation/approvals/{id}/deny", post(deny_request))
        .route("/api/federation/peers/{companion_id}/rules", post(set_rule))
        .route(
            "/api/federation/peers/{companion_id}/rules/revoke",
            post(revoke_rule),
        )
}

/// Mounted outside the auth middleware: a peer has no owner credential and
/// must never need one. Every handler verifies a signature instead.
pub fn public_router() -> Router<AppState> {
    Router::new()
        .route(PAIR_PATH, post(pair))
        .route(CONFIRM_PATH, post(confirm_notice))
        .route(REVOKE_PATH, post(revoke_notice))
        .route(PING_PATH, post(ping))
        .route(ROTATE_PATH, post(rotation_notice))
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
            FederationError::PolicyRefused(decision) => (
                if decision.reason == DecisionReason::RateLimited {
                    StatusCode::TOO_MANY_REQUESTS
                } else {
                    StatusCode::FORBIDDEN
                },
                policy_error_code(decision),
            ),
            FederationError::InviteInvalid => (StatusCode::UNAUTHORIZED, "invalid_invite"),
            FederationError::UnknownPeer => (StatusCode::NOT_FOUND, "unknown_peer"),
            FederationError::UnknownApproval => (StatusCode::NOT_FOUND, "unknown_approval"),
            FederationError::UnknownRule => (StatusCode::NOT_FOUND, "unknown_rule"),
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
        // A policy refusal crosses in its wire shape: the receipt on this
        // side keeps the whole decision, the peer gets what it may know.
        let error = match error {
            FederationError::PolicyRefused(decision) => {
                FederationError::PolicyRefused(decision.over_the_wire())
            }
            other => other,
        };
        let mut body = json!({ "error": code, "message": error.to_string() });
        match &error {
            FederationError::PeerRefused { status, error } => {
                body["peer_status"] = json!(status);
                body["peer_error"] = json!(error);
            }
            FederationError::PeerNotPaired { state } => body["state"] = json!(state),
            FederationError::PolicyRefused(decision) => {
                body["decision"] = json!(decision);
                if let Some(secs) = decision.retry_after_secs {
                    return (
                        status,
                        [(header::RETRY_AFTER, secs.to_string())],
                        Json(body),
                    )
                        .into_response();
                }
            }
            _ => {}
        }
        (status, Json(body)).into_response()
    }
}

/// The error code a policy refusal answers with, by verdict; the requesting
/// side maps it back to a decision for its own receipt.
pub(crate) fn policy_error_code(decision: &Decision) -> &'static str {
    match decision.verdict {
        Verdict::Allow => "policy_denied",
        Verdict::Ask => "approval_required",
        Verdict::Defer => "deferred",
        Verdict::Deny if decision.reason == DecisionReason::RateLimited => "rate_limited",
        Verdict::Deny => "policy_denied",
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

/// Parses an owner request body as `T`; any shape complaint is the fixed
/// `invalid_body`, never the parser's text.
fn parse_json<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, ApiError> {
    serde_json::from_slice(bytes).map_err(|_| ApiError::InvalidBody)
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

/// Parses a transport envelope the same way.
fn parse_transport(bytes: &[u8]) -> Result<TransportEnvelope, ApiError> {
    let text = std::str::from_utf8(bytes).map_err(|_| ApiError::InvalidBody)?;
    envelope::parse(text).map_err(|error| match error {
        FederationError::Malformed(_) => ApiError::Federation(FederationError::Malformed(
            "request body is not a transport envelope".into(),
        )),
        other => ApiError::Federation(other),
    })
}

async fn create_invite(State(state): State<AppState>) -> Result<Response, ApiError> {
    let origin = state.config.read().await.public_url.clone();
    let invite = state.federation.create_invite(&origin)?;
    // The one response that carries the secret also carries the one line
    // that packs it with the origin and the issuer document.
    let line = invite_token::encode_invite(&invite.origin, &invite.secret, &invite.issuer);
    let mut issued = serde_json::to_value(&invite).expect("invites serialize");
    issued["invite"] = json!(line);
    Ok((StatusCode::CREATED, Json(issued)).into_response())
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
    let accept = match serde_json::from_slice(&bytes).map_err(|_| ApiError::InvalidBody)? {
        AcceptRequest::Line(line) => invite_token::decode_invite(&line.invite)?,
        AcceptRequest::Fields(accept) => accept,
    };
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
    // A revoked peer keeps nothing: its rules and whatever it asked the
    // owner go with the trust (#109).
    state
        .federation_gate
        .forget_peer(&state.federation, &companion_id, ReceiptSide::Owner)?;
    Ok(Json(json!({ "peer": peer.summary(), "notified": notified })).into_response())
}

async fn rotate_identity(State(state): State<AppState>) -> Result<Response, ApiError> {
    Ok(Json(state.federation.rotate_identity().await?).into_response())
}

async fn show_policy(State(state): State<AppState>) -> Result<Response, ApiError> {
    Ok(Json(state.federation_gate.policy()?).into_response())
}

async fn list_receipts(State(state): State<AppState>) -> Result<Response, ApiError> {
    Ok(Json(json!({ "receipts": state.federation_gate.receipts()? })).into_response())
}

/// The peer's decision comes back as `200 { decision }` whether it allowed
/// the ping or refused it by policy; only an unreachable peer or a refusal
/// that was not a decision is an error.
async fn ping_peer(
    State(state): State<AppState>,
    Path(companion_id): Path<String>,
) -> Result<Response, ApiError> {
    let decision = state
        .federation_gate
        .send_ping(&state.federation, &companion_id)
        .await?;
    Ok(Json(json!({ "decision": decision })).into_response())
}

async fn list_approvals(State(state): State<AppState>) -> Result<Response, ApiError> {
    Ok(Json(json!({ "approvals": state.federation_gate.approvals()? })).into_response())
}

async fn approve_request(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Result<Response, ApiError> {
    let scope: ApprovalScope = parse_json(&read_body(request).await?)?;
    let outcome = state
        .federation_gate
        .approve(&state.federation, &id, scope)?;
    Ok(Json(outcome).into_response())
}

async fn deny_request(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Result<Response, ApiError> {
    let scope: ApprovalScope = parse_json(&read_body(request).await?)?;
    let outcome = state.federation_gate.deny(&state.federation, &id, scope)?;
    Ok(Json(outcome).into_response())
}

async fn withdraw_approval(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state
        .federation_gate
        .withdraw_approval(&state.federation, &id)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn set_rule(
    State(state): State<AppState>,
    Path(companion_id): Path<String>,
    request: Request,
) -> Result<Response, ApiError> {
    let rule: RuleRequest = parse_json(&read_body(request).await?)?;
    let policy = state
        .federation_gate
        .set_rule(&state.federation, &companion_id, rule)?;
    Ok(Json(json!({ "policy": policy })).into_response())
}

async fn revoke_rule(
    State(state): State<AppState>,
    Path(companion_id): Path<String>,
    request: Request,
) -> Result<Response, ApiError> {
    let key: RuleKey = parse_json(&read_body(request).await?)?;
    let policy = state.federation_gate.revoke_rule(
        &state.federation,
        &companion_id,
        key.intent,
        key.disclosure,
    )?;
    Ok(Json(json!({ "policy": policy })).into_response())
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
    let ack = state.federation.receive_revoke(&envelope)?;
    // The peer withdrew the pairing: what it asked the owner and what the
    // owner had granted it go with it, and the withdrawal is recorded.
    state.federation_gate.forget_peer(
        &state.federation,
        &envelope.sender,
        ReceiptSide::Answering,
    )?;
    Ok(Json(ack).into_response())
}

async fn ping(State(state): State<AppState>, request: Request) -> Result<Response, ApiError> {
    let envelope = parse_transport(&read_body(request).await?)?;
    let pong = state
        .federation_gate
        .receive_ping(&state.federation, &envelope)?;
    Ok(Json(pong).into_response())
}

async fn rotation_notice(
    State(state): State<AppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let envelope = parse_transport(&read_body(request).await?)?;
    let ack = state
        .federation_gate
        .receive_rotation(&state.federation, &envelope)?;
    Ok(Json(ack).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn refusal(decision: Decision) -> (StatusCode, Option<String>, serde_json::Value) {
        let response =
            ApiError::Federation(FederationError::PolicyRefused(decision)).into_response();
        let status = response.status();
        let retry_after = response
            .headers()
            .get(header::RETRY_AFTER)
            .map(|value| value.to_str().unwrap().to_owned());
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        (status, retry_after, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn a_deferred_refusal_carries_no_quiet_hours_end_time() {
        // The engine's decision says when the owner's quiet hours end; the
        // peer is told only that it was deferred, in the body, the message,
        // and the headers.
        let deferred = Decision {
            retry_after_secs: Some(7200),
            ..Decision::deferred(1_800_007_200)
        };
        let (status, retry_after, body) = refusal(deferred).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(retry_after, None, "no Retry-After for a deferral");
        assert_eq!(body["error"], "deferred");
        assert_eq!(
            body["decision"],
            json!({"verdict": "defer", "reason": "quiet_hours"})
        );
        let text = body.to_string();
        assert!(
            !text.contains("1800007200") && !text.contains("7200") && !text.contains("until"),
            "{text}"
        );

        // A rate-limited peer still learns how long its own window has left.
        let (status, retry_after, body) = refusal(Decision::rate_limited(45)).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(retry_after.as_deref(), Some("45"));
        assert_eq!(body["error"], "rate_limited");
        assert_eq!(body["decision"]["retry_after_secs"], 45);
    }
}
