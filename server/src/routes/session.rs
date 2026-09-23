//! Device pairing and session management (#112).
//!
//! One pairing code pairs either kind of device:
//!
//! * `POST /api/session/pair` (public) redeems a code in a browser for a cookie.
//! * `POST /api/session/pair-device` (public) redeems a code in the desktop
//!   app for a bearer token.
//! * `POST /api/session/pairing` (authenticated) mints a code; the CLI calls
//!   it with the API token, a paired browser or desktop app from Settings.
//! * `GET /api/session`, `POST /api/session/logout` and the
//!   `/api/session/devices` routes let owners see and revoke paired devices.

use axum::{
    Extension, Json, Router,
    extract::{Path, Request, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use serde::Deserialize;
use serde_json::json;

use crate::{
    app::{
        auth::{self, AuthContext},
        state::AppState,
    },
    services::browser_sessions::{self, PairingError},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/session", get(current_session))
        .route("/api/session/logout", post(logout))
        .route("/api/session/pairing", post(create_pairing_code))
        .route(
            "/api/session/devices",
            get(list_devices).delete(revoke_all_devices),
        )
        .route("/api/session/devices/{id}", delete(revoke_device))
}

/// Mounted outside the auth middleware: a new device has no credential yet.
pub fn public_router() -> Router<AppState> {
    Router::new()
        .route("/api/session/pair", post(pair))
        .route("/api/session/pair-device", post(pair_device))
}

fn auth_kind(context: Option<&AuthContext>) -> &'static str {
    match context {
        None | Some(AuthContext::Disabled) => "disabled",
        Some(AuthContext::ApiToken) => "token",
        Some(AuthContext::BrowserSession { .. }) => "session",
        Some(AuthContext::DesktopSession { .. }) => "desktop",
    }
}

fn current_session_id(context: Option<&AuthContext>) -> Option<&str> {
    context.and_then(AuthContext::session_id)
}

async fn current_session(
    State(state): State<AppState>,
    context: Option<Extension<AuthContext>>,
) -> Json<serde_json::Value> {
    let context = context.map(|Extension(c)| c);
    let session = current_session_id(context.as_ref()).and_then(|id| {
        state
            .browser_sessions
            .list()
            .into_iter()
            .find(|s| s.id == id)
    });
    Json(json!({
        "auth": auth_kind(context.as_ref()),
        "session": session,
    }))
}

async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
    context: Option<Extension<AuthContext>>,
) -> Response {
    let context = context.map(|Extension(c)| c);
    if let Some(id) = current_session_id(context.as_ref()) {
        state.browser_sessions.revoke(id);
    }
    let secure = auth::secure_cookie(&headers, &*state.config.read().await);
    (
        StatusCode::OK,
        [(header::SET_COOKIE, auth::clear_session_cookie(secure))],
        Json(json!({ "status": "ok" })),
    )
        .into_response()
}

#[derive(Deserialize, Default)]
struct CreatePairingRequest {
    /// Free-form origin of the request when made with the API token, for the
    /// device list ("cli", "desktop"). Ignored for browser sessions.
    #[serde(default)]
    source: Option<String>,
}

fn sanitize_source(source: Option<String>) -> String {
    source
        .map(|s| {
            s.chars()
                .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
                .take(32)
                .collect::<String>()
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "api-token".to_string())
}

async fn create_pairing_code(
    State(state): State<AppState>,
    context: Option<Extension<AuthContext>>,
    request: Request,
) -> Result<Json<browser_sessions::PairingChallenge>, (StatusCode, Json<serde_json::Value>)> {
    let context = context.map(|Extension(c)| c);
    let (bound_host, created_by) = match context {
        Some(AuthContext::BrowserSession { id }) => {
            let host = auth::request_host(request.headers(), &request).ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "missing_host" })),
                )
            })?;
            (Some(host), format!("browser:{id}"))
        }
        // The desktop app reaches the server through its own relay, so the
        // host it sees says nothing about where the new device will connect.
        Some(AuthContext::DesktopSession { id }) => (None, format!("desktop:{id}")),
        Some(AuthContext::ApiToken) => {
            let body: CreatePairingRequest =
                match axum::body::to_bytes(request.into_body(), 4096).await {
                    Ok(bytes) if bytes.is_empty() => CreatePairingRequest::default(),
                    Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
                    Err(_) => CreatePairingRequest::default(),
                };
            (None, sanitize_source(body.source))
        }
        None | Some(AuthContext::Disabled) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "auth_disabled",
                    "message": "authentication is disabled; devices do not need pairing",
                })),
            ));
        }
    };
    Ok(Json(
        state
            .browser_sessions
            .create_challenge(bound_host, &created_by),
    ))
}

#[derive(Deserialize)]
struct PairRequest {
    code: String,
}

async fn pair(State(state): State<AppState>, request: Request) -> Response {
    let (secure, enabled) = {
        let config = state.config.read().await;
        (
            auth::secure_cookie(request.headers(), &config),
            !config.auth_token.is_empty(),
        )
    };
    if !enabled {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "auth_disabled" })),
        )
            .into_response();
    }
    let Some(host) = auth::request_host(request.headers(), &request) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "missing_host" })),
        )
            .into_response();
    };
    if !auth::same_origin(request.headers(), &host) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "cross_origin" })),
        )
            .into_response();
    }
    let label = request
        .headers()
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(browser_sessions::describe_user_agent)
        .unwrap_or_else(|| "Browser".to_string());
    let body: PairRequest = match axum::body::to_bytes(request.into_body(), 4096).await {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(body) => body,
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "invalid_body" })),
                )
                    .into_response();
            }
        },
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid_body" })),
            )
                .into_response();
        }
    };

    match state
        .browser_sessions
        .confirm_challenge(&body.code, &host, &label)
    {
        Ok(issued) => (
            StatusCode::OK,
            [(
                header::SET_COOKIE,
                auth::session_cookie(&issued.cookie_value, issued.max_age_secs, secure),
            )],
            Json(json!({ "session": issued.summary })),
        )
            .into_response(),
        Err(error) => pairing_error_response(error),
    }
}

#[derive(Deserialize)]
struct PairDeviceRequest {
    code: String,
    /// How the app names itself in the devices list, e.g. "Nolune Desktop on macOS".
    #[serde(default)]
    label: String,
}

fn pairing_error_response(error: PairingError) -> Response {
    match error {
        PairingError::Invalid => (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "invalid_code" })),
        )
            .into_response(),
        PairingError::RateLimited => (
            StatusCode::TOO_MANY_REQUESTS,
            [(
                header::RETRY_AFTER,
                browser_sessions::FAILURE_WINDOW_SECS.to_string(),
            )],
            Json(json!({ "error": "rate_limited" })),
        )
            .into_response(),
    }
}

/// The desktop app redeems a pairing code for a bearer token it keeps in the
/// OS credential store. Browsers are refused: a token readable by page script
/// is exactly what the cookie flow exists to avoid.
async fn pair_device(State(state): State<AppState>, request: Request) -> Response {
    if state.config.read().await.auth_token.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "auth_disabled" })),
        )
            .into_response();
    }
    let from_browser = request.headers().contains_key(header::ORIGIN)
        || request.headers().contains_key("sec-fetch-site");
    if from_browser {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "browser_not_allowed" })),
        )
            .into_response();
    }
    let Some(host) = auth::request_host(request.headers(), &request) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "missing_host" })),
        )
            .into_response();
    };
    let body: PairDeviceRequest = match axum::body::to_bytes(request.into_body(), 4096)
        .await
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
    {
        Some(body) => body,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid_body" })),
            )
                .into_response();
        }
    };
    match state
        .browser_sessions
        .confirm_device_challenge(&body.code, &host, &body.label)
    {
        Ok(issued) => (
            StatusCode::OK,
            [(header::CACHE_CONTROL, "no-store")],
            Json(json!({ "token": issued.token, "session": issued.summary })),
        )
            .into_response(),
        Err(error) => pairing_error_response(error),
    }
}

async fn list_devices(
    State(state): State<AppState>,
    context: Option<Extension<AuthContext>>,
) -> Json<serde_json::Value> {
    let context = context.map(|Extension(c)| c);
    let current = current_session_id(context.as_ref()).map(str::to_string);
    let devices: Vec<serde_json::Value> = state
        .browser_sessions
        .list()
        .into_iter()
        .map(|summary| {
            let current = current.as_deref() == Some(summary.id.as_str());
            let mut value = serde_json::to_value(summary).unwrap_or_default();
            value["current"] = json!(current);
            value
        })
        .collect();
    Json(json!({
        "auth": auth_kind(context.as_ref()),
        "devices": devices,
    }))
}

async fn revoke_device(
    State(state): State<AppState>,
    headers: HeaderMap,
    context: Option<Extension<AuthContext>>,
    Path(id): Path<String>,
) -> Response {
    if !state.browser_sessions.revoke(&id) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let context = context.map(|Extension(c)| c);
    if current_session_id(context.as_ref()) == Some(id.as_str()) {
        let secure = auth::secure_cookie(&headers, &*state.config.read().await);
        return (
            StatusCode::NO_CONTENT,
            [(header::SET_COOKIE, auth::clear_session_cookie(secure))],
        )
            .into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

async fn revoke_all_devices(
    State(state): State<AppState>,
    headers: HeaderMap,
    context: Option<Extension<AuthContext>>,
) -> Response {
    let revoked = state.browser_sessions.revoke_all();
    let context = context.map(|Extension(c)| c);
    let body = Json(json!({ "revoked": revoked }));
    if current_session_id(context.as_ref()).is_some() {
        let secure = auth::secure_cookie(&headers, &*state.config.read().await);
        return (
            StatusCode::OK,
            [(header::SET_COOKIE, auth::clear_session_cookie(secure))],
            body,
        )
            .into_response();
    }
    (StatusCode::OK, body).into_response()
}
