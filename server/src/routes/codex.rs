//! The codex login routes (#27): whether the pinned binary is there and who
//! codex is logged in as, a login (the ChatGPT managed flow where this host
//! has a browser, a device code for a headless server) and a logout. The
//! answers carry an account's label and what a person needs to finish a
//! login; never a token, which codex keeps to itself.

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::json;

use crate::{
    app::state::AppState,
    services::llm::codex::{
        AppServerError,
        auth::{LoginChoice, LoginStatus, Status},
    },
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/config/codex/status", get(get_status))
        .route("/api/config/codex/login", post(start_login))
        .route("/api/config/codex/logout", post(logout))
}

type ApiError = (StatusCode, Json<serde_json::Value>);

/// Every failure has a name a client can act on and the app-server's own
/// words: the binary that is missing, another release or one that cannot
/// run is setup (`503`); an app-server that did not start, died, refused or
/// answered out of protocol is unavailable right now (`502`).
fn api_error(error: AppServerError) -> ApiError {
    let _ = error;
    todo!("27d: api_error")
}

async fn get_status(State(state): State<AppState>) -> Json<Status> {
    let _ = state;
    todo!("27d: get_status")
}

#[derive(Default, Deserialize)]
struct LoginBody {
    #[serde(default)]
    method: LoginChoice,
}

/// Start a login. The body is optional: `{"method": "auto" | "browser" |
/// "device_code"}`, `auto` by default, which is the browser flow where this
/// host has a display and a device code where it is headless.
async fn start_login(
    State(state): State<AppState>,
    body: Option<Json<LoginBody>>,
) -> Result<Json<LoginStatus>, ApiError> {
    let _ = (state, body);
    todo!("27d: start_login")
}

async fn logout(State(state): State<AppState>) -> Result<Json<Status>, ApiError> {
    let _ = state;
    todo!("27d: logout")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::Config,
        services::llm::codex::{CODEX_ENV, CODEX_VERSION, auth::Auth, fake},
    };
    use axum::body::{Body, to_bytes};
    use std::{
        collections::BTreeSet,
        time::{Duration, Instant},
    };
    use tower::ServiceExt;

    /// A state over a tempdir whose codex login speaks to `auth`; the real
    /// home and the real binary are never consulted.
    async fn state_with(auth: Auth) -> (tempfile::TempDir, AppState) {
        let workspace = tempfile::tempdir().unwrap();
        let mut state = AppState::new_in(Config::default(), workspace.path().to_owned()).await;
        state.codex_auth = auth;
        (workspace, state)
    }

    async fn call(
        state: &AppState,
        method: &str,
        uri: &str,
        body: Option<serde_json::Value>,
    ) -> (StatusCode, serde_json::Value) {
        let request = axum::http::Request::builder().method(method).uri(uri);
        let request = match body {
            Some(body) => request
                .header("content-type", "application/json")
                .body(Body::from(body.to_string())),
            None => request.body(Body::empty()),
        }
        .unwrap();
        let response = router()
            .with_state(state.clone())
            .oneshot(request)
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let body = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or_else(|_| {
                serde_json::Value::String(String::from_utf8_lossy(&bytes).into_owned())
            })
        };
        (status, body)
    }

    fn keys(value: &serde_json::Value, out: &mut BTreeSet<String>) {
        match value {
            serde_json::Value::Object(map) => {
                for (key, value) in map {
                    out.insert(key.clone());
                    keys(value, out);
                }
            }
            serde_json::Value::Array(items) => items.iter().for_each(|item| keys(item, out)),
            _ => {}
        }
    }

    /// No member of a body, at any depth, is one a token could hide behind.
    fn assert_no_token_bearing_member(body: &serde_json::Value, label: &str) {
        let mut found = BTreeSet::new();
        keys(body, &mut found);
        for key in found {
            let lowered = key.to_lowercase();
            for banned in ["token", "secret", "apikey", "api_key", "password", "cookie"] {
                assert!(!lowered.contains(banned), "{label} carries {key}: {body}");
            }
        }
    }

    /// Poll the status until `done` says so, or fail after ten seconds.
    async fn status_when(
        state: &AppState,
        what: &str,
        done: impl Fn(&serde_json::Value) -> bool,
    ) -> serde_json::Value {
        let give_up = Instant::now() + Duration::from_secs(10);
        loop {
            let (code, body) = call(state, "GET", "/api/config/codex/status", None).await;
            assert_eq!(code, StatusCode::OK, "{body}");
            if done(&body) {
                return body;
            }
            assert!(Instant::now() < give_up, "waiting for {what}: {body}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    #[tokio::test]
    async fn status_reports_a_missing_binary_as_setup_not_as_a_failure() {
        let empty = tempfile::tempdir().unwrap();
        let auth = Auth::with_lookup(None, Some(empty.path().as_os_str().to_owned()));
        let (_workspace, state) = state_with(auth).await;
        let (code, body) = call(&state, "GET", "/api/config/codex/status", None).await;
        assert_eq!(code, StatusCode::OK, "{body}");
        assert_eq!(body["binary"]["state"], "not_installed");
        assert_eq!(body["binary"]["pinned_version"], CODEX_VERSION);
        assert_eq!(body["installed"], false);
        assert_eq!(body["compatible"], false);
        assert_eq!(body["logged_in"], false);
        assert_eq!(body["account"], serde_json::Value::Null);
        assert_eq!(body["login"], serde_json::Value::Null);
        assert!(
            body["binary"]["message"]
                .as_str()
                .unwrap()
                .contains(CODEX_ENV),
            "{body}"
        );
        assert_no_token_bearing_member(&body, "status");
    }

    #[tokio::test]
    async fn login_and_logout_refuse_a_missing_or_mismatched_binary_with_a_typed_error() {
        let empty = tempfile::tempdir().unwrap();
        let auth = Auth::with_lookup(None, Some(empty.path().as_os_str().to_owned()));
        let (_workspace, state) = state_with(auth).await;
        for (method, uri) in [
            ("POST", "/api/config/codex/login"),
            ("POST", "/api/config/codex/logout"),
        ] {
            let (code, body) = call(&state, method, uri, None).await;
            assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE, "{uri}: {body}");
            assert_eq!(body["error"], "codex_not_installed", "{uri}: {body}");
            let message = body["message"].as_str().unwrap();
            assert!(message.contains(CODEX_ENV), "{uri}: actionable: {message}");
        }

        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("codex");
        std::fs::write(&binary, "#!/bin/sh\necho 'codex-cli 9.0.0'\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let auth = Auth::with_lookup(Some(binary.as_os_str().to_owned()), None);
        let (_workspace, state) = state_with(auth).await;
        let (code, body) = call(
            &state,
            "POST",
            "/api/config/codex/login",
            Some(json!({"method": "device_code"})),
        )
        .await;
        assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE, "{body}");
        assert_eq!(body["error"], "codex_incompatible");
        let message = body["message"].as_str().unwrap();
        assert!(message.contains("9.0.0"), "{message}");
        assert!(message.contains(CODEX_VERSION), "{message}");
        assert!(message.contains(binary.to_str().unwrap()), "{message}");
        let (code, body) = call(&state, "GET", "/api/config/codex/status", None).await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(body["binary"]["state"], "incompatible");
        assert_eq!(body["binary"]["version"], "9.0.0");
        assert_eq!(body["installed"], true);
        assert_eq!(body["compatible"], false);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_device_code_login_answers_the_url_the_code_and_a_poll_handle_only() {
        let auth = Auth::with_launch(fake::launch_with_state(
            None,
            Some(json!({"session": "none"})),
        ));
        let (_workspace, state) = state_with(auth.clone()).await;
        let (code, before) = call(&state, "GET", "/api/config/codex/status", None).await;
        assert_eq!(code, StatusCode::OK, "{before}");
        assert_eq!(before["binary"]["state"], "ready");
        assert_eq!(before["binary"]["version"], CODEX_VERSION);
        assert_eq!(before["compatible"], true);
        assert_eq!(before["logged_in"], false);

        let (code, login) = call(
            &state,
            "POST",
            "/api/config/codex/login",
            Some(json!({"method": "device_code"})),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{login}");
        assert_eq!(login["method"], "device_code");
        assert_eq!(login["state"], "pending");
        assert_eq!(
            login["verification_url"],
            "https://auth.openai.com/codex/device"
        );
        assert_eq!(login["user_code"], "NLNE-FXTR");
        assert!(login["id"].as_str().is_some_and(|id| !id.is_empty()));
        let members: BTreeSet<_> = login.as_object().unwrap().keys().cloned().collect();
        assert_eq!(
            members,
            ["id", "method", "state", "verification_url", "user_code"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            "only what a person needs and the handle to poll with"
        );
        assert_no_token_bearing_member(&login, "login");

        let after = status_when(&state, "the login to complete", |s| {
            s["login"]["state"] != "pending"
        })
        .await;
        assert_eq!(after["login"]["id"], login["id"]);
        assert_eq!(after["login"]["state"], "completed", "{after}");
        assert_eq!(after["logged_in"], true);
        assert_eq!(after["account"]["kind"], "chatgpt");
        assert_eq!(after["account"]["email"], "companion@example.test");
        assert_eq!(after["account"]["plan"], "plus");
        assert_eq!(
            after["login"].get("user_code"),
            None,
            "the code is gone once it was used"
        );
        assert_no_token_bearing_member(&after, "status");
        auth.shutdown().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_browser_login_answers_the_auth_url_and_an_empty_body_picks_a_method() {
        let auth = Auth::with_launch(fake::launch_with_state(
            None,
            Some(json!({"session": "none"})),
        ));
        let (_workspace, state) = state_with(auth.clone()).await;
        let (code, login) = call(
            &state,
            "POST",
            "/api/config/codex/login",
            Some(json!({"method": "browser"})),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{login}");
        assert_eq!(login["method"], "browser");
        assert!(
            login["auth_url"]
                .as_str()
                .is_some_and(|url| url.starts_with("https://auth.openai.com/")),
            "{login}"
        );
        assert_eq!(login.get("user_code"), None);
        assert_eq!(login.get("verification_url"), None);
        status_when(&state, "the login to complete", |s| {
            s["login"]["state"] != "pending"
        })
        .await;

        // No body at all: `auto`, which is one of the two flows.
        let (code, login) = call(&state, "POST", "/api/config/codex/login", None).await;
        assert_eq!(code, StatusCode::OK, "{login}");
        assert!(
            login["method"] == "browser" || login["method"] == "device_code",
            "{login}"
        );
        assert_eq!(login["state"], "pending");

        // A method that does not exist is refused before anything starts.
        let (code, body) = call(
            &state,
            "POST",
            "/api/config/codex/login",
            Some(json!({"method": "carrier_pigeon"})),
        )
        .await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        auth.shutdown().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn logout_answers_the_status_without_the_account_or_the_login() {
        let auth = Auth::with_launch(fake::launch_with_state(
            None,
            Some(json!({"session": "none"})),
        ));
        let (_workspace, state) = state_with(auth.clone()).await;
        call(
            &state,
            "POST",
            "/api/config/codex/login",
            Some(json!({"method": "device_code"})),
        )
        .await;
        status_when(&state, "the login to complete", |s| s["logged_in"] == true).await;
        let (code, body) = call(&state, "POST", "/api/config/codex/logout", None).await;
        assert_eq!(code, StatusCode::OK, "{body}");
        assert_eq!(body["logged_in"], false);
        assert_eq!(body["account"], serde_json::Value::Null);
        assert_eq!(body["login"], serde_json::Value::Null);
        assert_eq!(body["compatible"], true);
        assert_no_token_bearing_member(&body, "logout");
        let (_, again) = call(&state, "GET", "/api/config/codex/status", None).await;
        assert_eq!(again["logged_in"], false);
        auth.shutdown().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn an_app_server_that_is_gone_is_unavailable_not_setup() {
        let auth = Auth::with_launch(fake::launch(None));
        let (_workspace, state) = state_with(auth.clone()).await;
        auth.shutdown().await;
        let (code, body) = call(&state, "POST", "/api/config/codex/logout", None).await;
        assert_eq!(code, StatusCode::BAD_GATEWAY, "{body}");
        assert_eq!(body["error"], "codex_unavailable");
        let (code, body) = call(&state, "GET", "/api/config/codex/status", None).await;
        assert_eq!(code, StatusCode::OK, "{body}");
        assert_eq!(body["binary"]["state"], "ready");
        assert_eq!(body["logged_in"], false);
        assert!(body["error"].as_str().is_some(), "{body}");
    }
}
