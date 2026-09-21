//! Request-admission boundary for the one canonical companion (#103).
//!
//! Applied as a route layer to every router that carries an `instance_slug`
//! path parameter. Foreign slugs fail closed with `404 unknown_companion`
//! before any handler runs, so no request can create, read, or delete a
//! second companion. Canonical reads never create storage; canonical writes
//! create-or-open the companion through the identity marker, and an
//! unsupported marker refuses both with `503 companion_format_unsupported`.
//!
//! A mutating request also holds the process-wide import gate (#74) shared
//! from before it is admitted until its response is built, so a companion
//! import never interleaves with it: the request either finishes before the
//! import replaces the tree or runs against the imported tree afterwards.
//! The import route itself is the one exception; it takes the exclusive
//! side of the gate inside its handler.

use std::{collections::HashMap, path::Path};

use axum::{
    Json,
    extract::{MatchedPath, Path as PathParams, Request, State, rejection::PathRejection},
    http::{Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::{
    app::state::AppState,
    domain::companion::{CANONICAL_SLUG, IdentityError, is_canonical},
    services::companion,
};

/// Path parameter name shared by every companion-scoped route.
pub const SLUG_PARAM: &str = "instance_slug";

/// Why a request was not admitted to the companion.
#[derive(Debug)]
pub enum CompanionRejection {
    /// The slug names something other than the one companion this server owns.
    UnknownCompanion,
    /// The canonical companion exists but its storage cannot be trusted.
    Identity(IdentityError),
}

impl From<IdentityError> for CompanionRejection {
    fn from(error: IdentityError) -> Self {
        Self::Identity(error)
    }
}

impl IntoResponse for CompanionRejection {
    fn into_response(self) -> Response {
        match self {
            Self::UnknownCompanion => (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": "unknown_companion",
                    "message": format!("this server owns one companion: {CANONICAL_SLUG}"),
                })),
            )
                .into_response(),
            Self::Identity(IdentityError::UnsupportedFormat(message)) => (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": "companion_format_unsupported",
                    "message": message,
                })),
            )
                .into_response(),
            Self::Identity(IdentityError::Io(message)) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "companion_storage_error",
                    "message": message,
                })),
            )
                .into_response(),
        }
    }
}

/// Admit a request addressed to `slug`.
///
/// Reads (`GET`, `HEAD`, `OPTIONS`) and `DELETE` never create storage. Any
/// other method creates-or-opens the canonical companion first.
pub fn admit(workspace_dir: &Path, slug: &str, method: &Method) -> Result<(), CompanionRejection> {
    if !is_canonical(slug) {
        return Err(CompanionRejection::UnknownCompanion);
    }
    let read_only = matches!(
        *method,
        Method::GET | Method::HEAD | Method::OPTIONS | Method::DELETE
    );
    if read_only {
        companion::read_identity(workspace_dir)?;
    } else {
        companion::ensure_identity(workspace_dir)?;
    }
    Ok(())
}

/// Methods that never write the companion tree and hold no gate.
fn is_read_only(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

pub async fn companion_boundary(
    State(state): State<AppState>,
    params: Result<PathParams<HashMap<String, String>>, PathRejection>,
    request: Request,
    next: Next,
) -> Response {
    let Ok(PathParams(params)) = params else {
        return next.run(request).await;
    };
    let Some(slug) = params.get(SLUG_PARAM) else {
        return next.run(request).await;
    };
    // Held until the response is built; `DELETE` writes too, only `admit`
    // treats it as read-only because it never creates the companion.
    let is_import = request
        .extensions()
        .get::<MatchedPath>()
        .is_some_and(|path| path.as_str() == crate::routes::instances::IMPORT_ROUTE);
    let _writer = if is_read_only(request.method()) || is_import {
        None
    } else {
        Some(
            state
                .vector_store
                .media_store()
                .import_gate()
                .writer()
                .await,
        )
    };
    if let Err(rejection) = admit(&state.workspace_dir, slug, request.method()) {
        return rejection.into_response();
    }
    next.run(request).await
}
