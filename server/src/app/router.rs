use std::path::PathBuf;

use axum::{
    Json, Router,
    http::StatusCode,
    middleware,
    response::{IntoResponse, Response},
    routing::any,
};
use tower_http::services::{ServeDir, ServeFile};

use crate::{app::state::AppState, routes};

use super::{auth::auth_middleware, companion_boundary::companion_boundary};

fn api_router(state: &AppState) -> Router<AppState> {
    Router::new()
        .merge(routes::meta::router())
        .merge(routes::companion::router())
        .merge(routes::activity::router())
        .merge(routes::commitments::router())
        .merge(routes::continuity::router())
        .merge(routes::handoff::router())
        .merge(routes::resume::router())
        .merge(routes::resources::issuance_router())
        .merge(routes::instances::router())
        .merge(routes::chat::router())
        .merge(routes::drops::router())
        .merge(routes::config::router())
        .merge(routes::codex::router())
        .merge(routes::soul::router())
        .merge(routes::uploads::router())
        .merge(routes::skills::router())
        .merge(routes::ws::router())
        .merge(routes::update::router())
        .merge(routes::tts::router())
        .merge(routes::memory_import::router())
        .merge(routes::machine_agents::router())
        .merge(routes::session::router())
        .merge(routes::federation::router())
        // Removed or unknown API paths answer 404 JSON instead of the SPA shell.
        .route("/api/{*rest}", any(api_not_found))
        // Inner: admit only the canonical companion once the caller is authenticated.
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            companion_boundary,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
}

async fn api_not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "error": "not_found",
            "message": "no such API route",
        })),
    )
        .into_response()
}

pub fn build_router(state: AppState, static_dir: Option<PathBuf>) -> Router {
    // API routes — protected by auth middleware
    let api = api_router(&state);

    // Health is public; resource handlers verify capabilities independently of API auth.
    let health = routes::health::router();
    // Public routes still address a companion by slug and fail closed on foreign ones.
    let public_files = routes::uploads::public_router().route_layer(
        middleware::from_fn_with_state(state.clone(), companion_boundary),
    );
    let public_memory = routes::instances::public_memory_router().route_layer(
        middleware::from_fn_with_state(state.clone(), companion_boundary),
    );
    // Browser pairing has no credential yet and no companion slug (#112).
    let pairing = routes::session::public_router();
    // Peer-side federation routes are verified by signature only (#108).
    let federation = routes::federation::public_router();

    let app = Router::new()
        .merge(health)
        .merge(routes::resources::router())
        .merge(public_files)
        .merge(public_memory)
        .merge(pairing)
        .merge(federation)
        .merge(api)
        .with_state(state);

    // Serve static client files as fallback (SPA routing)
    // Priority: external static_dir > embedded assets
    if let Some(dir) = static_dir {
        let index = dir.join("index.html");
        let serve = ServeDir::new(dir).not_found_service(ServeFile::new(index));
        app.fallback_service(serve)
    } else {
        app.fallback_service(super::embedded_static::EmbeddedStaticService)
    }
}

#[cfg(test)]
#[path = "../../test-support/router_security.rs"]
mod tests;

#[cfg(test)]
#[path = "../../test-support/companion_boundary.rs"]
mod companion_boundary_tests;

#[cfg(test)]
#[path = "../../test-support/session_security.rs"]
mod session_tests;

#[cfg(test)]
#[path = "../../test-support/federation_pairing.rs"]
mod federation_tests;

#[cfg(test)]
#[path = "../../test-support/federation_transport.rs"]
mod federation_transport_tests;

#[cfg(test)]
#[path = "../../test-support/federation_policy.rs"]
mod federation_policy_tests;

#[cfg(test)]
#[path = "../../test-support/federation_inbound.rs"]
mod federation_inbound_tests;

#[cfg(test)]
#[path = "../../test-support/federation_outbox.rs"]
mod federation_outbox_tests;

#[cfg(test)]
#[path = "../../test-support/federation_scheduling.rs"]
mod federation_scheduling_tests;

#[cfg(test)]
#[path = "../../test-support/handoff_cards.rs"]
mod handoff_tests;

#[cfg(test)]
#[path = "../../test-support/resume_ritual.rs"]
mod resume_ritual_tests;

#[cfg(test)]
#[path = "../../test-support/cua_desktop_frames.rs"]
mod cua_desktop_frames_tests;
