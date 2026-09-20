use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{delete, get, post, put},
};
use serde::Deserialize;
use serde_json::json;

use crate::{
    app::state::AppState,
    config::{self, McpServerConfig},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/config/status", get(get_status))
        .route("/api/config/models", get(get_models).put(update_models))
        .route("/api/config/models/seed", post(seed_models))
        .route(
            "/api/config/embedding",
            get(get_embedding).put(update_embedding),
        )
        .route("/api/config/llm", put(update_llm_key))
        .route("/api/config/mcp", get(list_mcp_servers))
        .route("/api/config/mcp", post(add_mcp_server))
        .route("/api/config/mcp/suggested", get(suggested_mcp_servers))
        .route("/api/config/mcp/{name}", delete(remove_mcp_server))
        .route("/api/config/mcp/{name}/tools", put(update_mcp_tool_grants))
        .route("/api/config/github", get(get_github))
        .route("/api/config/github", put(update_github))
        .route("/api/config/server", get(get_server))
        .route("/api/config/server", put(update_server))
}

async fn get_status(State(state): State<AppState>) -> Json<serde_json::Value> {
    let config = state.config.read().await;
    // Which optional keys are configured
    let t = &config.llm.tokens;
    let keys: Vec<&str> = [
        ("anthropic", !t.anthropic.is_empty()),
        ("openai", !t.open_ai.is_empty()),
        ("elevenlabs", !t.elevenlabs.is_empty()),
        ("openrouter", !t.open_router.is_empty()),
    ]
    .iter()
    .filter(|(_, configured)| *configured)
    .map(|(name, _)| *name)
    .collect();

    Json(json!({
        "embedding": embedding_status(&state, &config),
        "llm_configured": config.llm.is_configured(),
        "setup_required": config.llm.setup_required(),
        "capabilities": config
            .llm
            .chat_preset()
            .map(|preset| {
                crate::services::llm::provider_capabilities(preset.provider, &preset.model)
            }),
        "chat_preset": config.llm.chat_preset,
        "background_preset": config.llm.background_preset,
        "model": config.llm.chat_model(),
        "configured_keys": keys,
        "host": config.host,
        "port": config.port,
        "public_url": config.public_url,
        "auth_token_set": !config.auth_token.is_empty(),
    }))
}

/// Report the active vector space separately from pending persisted settings.
fn embedding_status(state: &AppState, config: &config::Config) -> serde_json::Value {
    let mut status = state.vector_store.embedding_status();
    let needs_restart = state.vector_store.embedding_needs_restart(config);
    status["needs_restart"] = json!(needs_restart);
    status["pending"] = if needs_restart {
        config.embedding_status()
    } else {
        serde_json::Value::Null
    };
    status
}

async fn get_embedding(State(state): State<AppState>) -> Json<serde_json::Value> {
    let config = state.config.read().await;
    Json(embedding_status(&state, &config))
}

async fn update_embedding(
    State(state): State<AppState>,
    Json(settings): Json<config::EmbeddingConfig>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    settings
        .validate()
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_owned()))?;
    let mut cfg = state.config.write().await;
    let mut next = cfg.clone();
    next.embedding = settings;
    save_config_at(&next, &state.workspace_dir.join("config.toml"))?;
    *cfg = next;
    Ok(Json(embedding_status(&state, &cfg)))
}

// ---------------------------------------------------------------------------
// Model mode
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// LLM API keys
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct UpdateLlmKeyRequest {
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    openai: Option<String>,
    #[serde(default)]
    elevenlabs: Option<String>,
    #[serde(default)]
    openrouter: Option<String>,
}

/// How a key probe outcome answers the save: `None` saves the key, `Some`
/// rejects it. Only an authentication failure says the key is wrong; an
/// unreachable or failing provider says nothing about the key and asks the
/// person to retry instead of storing something unverified.
fn key_probe_rejection(
    provider: config::LlmProvider,
    outcome: Result<(), crate::services::llm::contract::LlmError>,
) -> Option<(StatusCode, String)> {
    use crate::services::llm::contract::LlmError;
    match outcome {
        Ok(()) => None,
        Err(LlmError::Authentication(_)) => {
            Some((StatusCode::UNAUTHORIZED, "invalid API key".into()))
        }
        Err(LlmError::Transport(error)) => Some((
            StatusCode::BAD_GATEWAY,
            format!("failed to reach {}: {error}", provider.label()),
        )),
        Err(_) => Some((
            StatusCode::BAD_GATEWAY,
            format!("{} API error — try again", provider.label()),
        )),
    }
}

/// Checks a key with its provider before it is saved: a one-token
/// completion through the adapter, with the model the person's presets
/// name for that provider (#24, #25). `base_url` replaces the provider's
/// endpoint; tests point it at a stub.
async fn verify_provider_key(
    state: &AppState,
    provider: config::LlmProvider,
    key: &str,
    base_url: Option<&str>,
) -> Result<(), (StatusCode, String)> {
    let model = {
        let cfg = state.config.read().await;
        crate::services::llm::probe_model(&cfg.llm, provider)
    };
    let mut backend =
        crate::services::llm::LlmBackend::probe(state.http_client.clone(), provider, &model, key);
    if let Some(base_url) = base_url {
        backend.base_url = base_url.to_owned();
    }
    key_probe_rejection(provider, backend.probe_key().await).map_or(Ok(()), Err)
}

async fn update_llm_key(
    State(state): State<AppState>,
    Json(req): Json<UpdateLlmKeyRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    save_llm_keys(&state, req, None).await
}

/// Probes and saves the keys in `req`. `probe_base_url` is where the probes
/// go instead of each provider's own endpoint; the route passes `None`.
async fn save_llm_keys(
    state: &AppState,
    req: UpdateLlmKeyRequest,
    probe_base_url: Option<&str>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // A new provider key is checked before it is saved; clearing one is not.
    for (provider, key) in [
        (config::LlmProvider::Anthropic, &req.api_key),
        (config::LlmProvider::Openai, &req.openai),
    ] {
        if let Some(key) = key.as_deref().map(str::trim).filter(|key| !key.is_empty()) {
            verify_provider_key(state, provider, key, probe_base_url).await?;
        }
    }

    let mut changes = Vec::new();

    {
        let mut cfg = state.config.write().await;

        if let Some(key) = &req.api_key {
            cfg.llm.tokens.anthropic = key.trim().to_string();
            changes.push("anthropic");
        }
        if let Some(key) = &req.openai {
            cfg.llm.tokens.open_ai = key.trim().to_string();
            changes.push("openai");
        }
        if let Some(key) = &req.elevenlabs {
            cfg.llm.tokens.elevenlabs = key.trim().to_string();
            changes.push("elevenlabs");
        }
        if let Some(key) = &req.openrouter {
            cfg.llm.tokens.open_router = key.trim().to_string();
            changes.push("openrouter");
        }

        save_config_at(&cfg, &state.workspace_dir.join("config.toml"))?;
    }

    // Rebuild LLM backend after any key change.
    // Can't use reload_config() — it compares disk vs in-memory, but we
    // already updated in-memory above, so it sees no diff.
    if !changes.is_empty() {
        state.rebuild_llm().await;
        log::info!("LLM backends rebuilt after API key change");
    }

    let cfg = state.config.read().await;
    Ok(Json(json!({ "status": "ok", "updated": changes,
        "embedding": embedding_status(state, &cfg),
        "needs_restart": state.vector_store.embedding_needs_restart(&cfg),
    })))
}

// ---------------------------------------------------------------------------
// MCP server management
// ---------------------------------------------------------------------------

async fn list_mcp_servers(
    State(state): State<AppState>,
) -> Json<Vec<crate::services::mcp::ServerGrants>> {
    let config = state.config.read().await;
    Json(state.mcp_registry.grants(&config.mcp_servers).await)
}

/// The reviewed catalog users can add with one click (#97).
async fn suggested_mcp_servers(State(state): State<AppState>) -> Json<serde_json::Value> {
    let config = state.config.read().await;
    let installed: Vec<&str> = config.mcp_servers.iter().map(|s| s.name.as_str()).collect();
    let suggested: Vec<serde_json::Value> = crate::services::mcp::curated_servers()
        .iter()
        .map(|entry| {
            json!({
                "name": entry.name,
                "description": entry.description,
                "url": entry.url,
                "requires_key": entry.requires_key,
                "key_env": entry.key_env,
                "key_url": entry.key_url,
                "installed": installed.contains(&entry.name),
            })
        })
        .collect();
    Json(serde_json::Value::Array(suggested))
}

#[derive(Deserialize)]
struct AddMcpServerRequest {
    name: String,
    url: String,
    /// Required for anything outside the catalog.
    #[serde(default)]
    acknowledge_untrusted: bool,
}

async fn add_mcp_server(
    State(state): State<AppState>,
    Json(request): Json<AddMcpServerRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    fn reject(status: StatusCode, message: &str) -> (StatusCode, Json<serde_json::Value>) {
        (
            status,
            Json(json!({ "error": "invalid_request", "message": message })),
        )
    }

    let name = request.name.trim().to_string();
    let url = request.url.trim().to_string();
    if name.is_empty() || url.is_empty() {
        return Err(reject(StatusCode::BAD_REQUEST, "name and url are required"));
    }
    let curated = crate::services::mcp::is_curated(&name, &url);
    if !curated && !request.acknowledge_untrusted {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "untrusted_extension_requires_acknowledgement",
                "message": "this server is not in the reviewed catalog; add it from Advanced setup and acknowledge that its tools act with your data",
            })),
        ));
    }

    {
        let mut config = state.config.write().await;
        if config.mcp_servers.iter().any(|s| s.name == name) {
            return Err(reject(
                StatusCode::CONFLICT,
                &format!("MCP server '{name}' already exists"),
            ));
        }
        config.mcp_servers.push(McpServerConfig {
            name: name.clone(),
            url: Some(url.clone()),
            command: None,
            args: Default::default(),
            headers: Default::default(),
            trust: if curated {
                config::McpTrust::Curated
            } else {
                config::McpTrust::Custom
            },
            enabled_tools: Vec::new(),
        });
        save_config_at(&config, &state.workspace_dir.join("config.toml"))
            .map_err(|(status, message)| reject(status, &message))?;
    }

    // Connect, then grant: curated servers get every discovered tool,
    // custom servers get none until the user enables them.
    let configs = state.config.read().await.mcp_servers.clone();
    state.mcp_registry.reconnect(&configs).await;
    let discovered = state.mcp_registry.discovered_tools(&name).await;
    if curated && let Some(discovered) = &discovered {
        let mut config = state.config.write().await;
        if let Some(server) = config.mcp_servers.iter_mut().find(|s| s.name == name) {
            server.enabled_tools = discovered.iter().map(|(n, _)| n.clone()).collect();
        }
        save_config_at(&config, &state.workspace_dir.join("config.toml"))
            .map_err(|(status, message)| reject(status, &message))?;
        let configs = config.mcp_servers.clone();
        drop(config);
        state.mcp_registry.reconnect(&configs).await;
    }
    let tool_count = discovered.map(|d| d.len()).unwrap_or(0);

    Ok(Json(json!({
        "status": "ok",
        "name": name,
        "trust": if curated { "curated" } else { "custom" },
        "tool_count": tool_count,
    })))
}

#[derive(Deserialize)]
struct UpdateMcpGrantsRequest {
    enabled: Vec<String>,
}

/// Exact capability grant for one server (#97).
async fn update_mcp_tool_grants(
    State(state): State<AppState>,
    axum::extract::Path(name): axum::extract::Path<String>,
    Json(request): Json<UpdateMcpGrantsRequest>,
) -> Result<Json<crate::services::mcp::ServerGrants>, (StatusCode, String)> {
    if !state
        .config
        .read()
        .await
        .mcp_servers
        .iter()
        .any(|s| s.name == name)
    {
        return Err((
            StatusCode::NOT_FOUND,
            format!("MCP server '{name}' not found"),
        ));
    }
    let Some(discovered) = state.mcp_registry.discovered_tools(&name).await else {
        return Err((
            StatusCode::CONFLICT,
            format!("MCP server '{name}' is not connected; tools can be granted once it is"),
        ));
    };
    let known: Vec<&str> = discovered.iter().map(|(n, _)| n.as_str()).collect();
    if let Some(unknown) = request
        .enabled
        .iter()
        .find(|n| !known.contains(&n.as_str()))
    {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("'{unknown}' is not a tool of '{name}'"),
        ));
    }
    let mut enabled = request.enabled;
    enabled.sort();
    enabled.dedup();
    let grants = {
        let mut config = state.config.write().await;
        let server = config
            .mcp_servers
            .iter_mut()
            .find(|s| s.name == name)
            .expect("checked above");
        server.enabled_tools = enabled;
        save_config_at(&config, &state.workspace_dir.join("config.toml"))?;
        let configs = config.mcp_servers.clone();
        drop(config);
        state.mcp_registry.reconnect(&configs).await;
        let config = state.config.read().await;
        state.mcp_registry.grants(&config.mcp_servers).await
    };
    grants
        .into_iter()
        .find(|g| g.name == name)
        .map(Json)
        .ok_or((StatusCode::INTERNAL_SERVER_ERROR, "grant vanished".into()))
}

async fn remove_mcp_server(
    State(state): State<AppState>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    {
        let mut config = state.config.write().await;
        let before = config.mcp_servers.len();
        config.mcp_servers.retain(|s| s.name != name);
        if config.mcp_servers.len() == before {
            return Err((
                StatusCode::NOT_FOUND,
                format!("MCP server '{name}' not found"),
            ));
        }
        save_config_at(&config, &state.workspace_dir.join("config.toml"))?;
    }

    // Reconnect
    let configs = state.config.read().await.mcp_servers.clone();
    state.mcp_registry.reconnect(&configs).await;

    Ok(Json(json!({ "status": "ok" })))
}

// ---------------------------------------------------------------------------
// GitHub integration
// ---------------------------------------------------------------------------

async fn get_github(State(state): State<AppState>) -> Json<serde_json::Value> {
    let config = state.config.read().await;
    let has_token = !config.github.token.is_empty();
    Json(json!({
        "configured": has_token,
    }))
}

#[derive(Deserialize)]
struct UpdateGithubRequest {
    token: String,
}

async fn update_github(
    State(state): State<AppState>,
    Json(request): Json<UpdateGithubRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let token = request.token.trim().to_string();

    {
        let mut config = state.config.write().await;
        config.github.token = token.clone();
        save_config(&config)?;
    }

    let configured = !token.is_empty();
    Ok(Json(json!({
        "status": "ok",
        "configured": configured,
    })))
}

// ---------------------------------------------------------------------------
// Server settings (host, port, auth_token)
// ---------------------------------------------------------------------------

async fn get_server(State(state): State<AppState>) -> Json<serde_json::Value> {
    let config = state.config.read().await;
    Json(json!({
        "host": config.host,
        "port": config.port,
        "auth_token_set": !config.auth_token.is_empty(),
    }))
}

#[derive(Deserialize)]
struct UpdateServerRequest {
    host: Option<String>,
    port: Option<u16>,
    auth_token: Option<String>,
}

async fn update_server(
    State(state): State<AppState>,
    Json(request): Json<UpdateServerRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mut needs_restart = false;

    {
        let mut config = state.config.write().await;
        let mut candidate = config.clone();
        if let Some(host) = &request.host {
            if candidate.host != *host {
                candidate.host = host.trim().to_string();
                needs_restart = true;
            }
        }
        if let Some(port) = request.port {
            if port > 0 && candidate.port != port {
                candidate.port = port;
                needs_restart = true;
            }
        }
        if let Some(token) = &request.auth_token {
            candidate.auth_token = token.trim().to_string();
        }
        save_config_at(&candidate, &state.workspace_dir.join("config.toml"))?;
        if config.auth_token != candidate.auth_token {
            state.resources.replace(&candidate.auth_token);
            // Rotating or clearing the API token is independent of paired
            // browsers: their sessions keep working until revoked (#112).
            log::info!(
                "[config] API token {}; {} paired browser session(s) unaffected",
                if candidate.auth_token.is_empty() {
                    "cleared"
                } else {
                    "rotated"
                },
                state.browser_sessions.list().len()
            );
        }
        *config = candidate;
    }

    Ok(Json(json!({
        "status": "ok",
        "needs_restart": needs_restart,
    })))
}

/// Write the current config back to disk.
/// Model presets and slots (#156), plus which providers can back them.
fn models_json(config: &config::Config) -> serde_json::Value {
    json!({
        "presets": config.llm.presets,
        "chat_preset": config.llm.chat_preset,
        "background_preset": config.llm.background_preset,
        "keyed_providers": config.llm.keyed_providers(),
        "setup_required": config.llm.setup_required(),
    })
}

type ModelsError = (StatusCode, Json<serde_json::Value>);

fn models_error(code: StatusCode, error: &str, message: String) -> ModelsError {
    (code, Json(json!({ "error": error, "message": message })))
}

async fn get_models(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(models_json(&*state.config.read().await))
}

#[derive(Deserialize)]
struct UpdateModelsRequest {
    presets: Vec<config::ModelPreset>,
    chat_preset: String,
    background_preset: String,
}

/// Replace presets and both slots atomically; nothing is saved unless the
/// whole shape validates.
async fn update_models(
    State(state): State<AppState>,
    Json(request): Json<UpdateModelsRequest>,
) -> Result<Json<serde_json::Value>, ModelsError> {
    {
        let mut cfg = state.config.write().await;
        let mut next = cfg.llm.clone();
        next.presets = request
            .presets
            .into_iter()
            .map(|preset| config::ModelPreset {
                id: preset.id.trim().to_owned(),
                name: preset.name.trim().to_owned(),
                provider: preset.provider,
                model: preset.model.trim().to_owned(),
            })
            .collect();
        next.chat_preset = request.chat_preset.trim().to_owned();
        next.background_preset = request.background_preset.trim().to_owned();
        next.validate_presets()
            .map_err(|message| models_error(StatusCode::BAD_REQUEST, "invalid_presets", message))?;
        cfg.llm = next;
        save_config_at(&cfg, &state.workspace_dir.join("config.toml"))
            .map_err(|(code, message)| models_error(code, "save_failed", message))?;
    }
    state.rebuild_llm().await;
    Ok(Json(models_json(&*state.config.read().await)))
}

#[derive(Deserialize)]
struct SeedModelsRequest {
    provider: String,
}

/// Add a provider's default presets and fill empty slots. Idempotent.
async fn seed_models(
    State(state): State<AppState>,
    Json(request): Json<SeedModelsRequest>,
) -> Result<Json<serde_json::Value>, ModelsError> {
    let Some(provider) = config::LlmProvider::parse(request.provider.trim()) else {
        return Err(models_error(
            StatusCode::BAD_REQUEST,
            "unknown_provider",
            format!("unknown provider: {}", request.provider),
        ));
    };
    let added = {
        let mut cfg = state.config.write().await;
        let added = cfg.llm.seed_presets(provider);
        save_config_at(&cfg, &state.workspace_dir.join("config.toml"))
            .map_err(|(code, message)| models_error(code, "save_failed", message))?;
        added
    };
    state.rebuild_llm().await;
    let mut body = models_json(&*state.config.read().await);
    body["added"] = json!(added);
    Ok(Json(body))
}

fn save_config(config: &config::Config) -> Result<(), (StatusCode, String)> {
    save_config_at(config, &config::config_path())
}

fn save_config_at(
    config: &config::Config,
    config_path: &std::path::Path,
) -> Result<(), (StatusCode, String)> {
    let original = match std::fs::read_to_string(&config_path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to read config: {e}"),
            ));
        }
    };
    let raw = config::serialize_config_preserving_keys(config, &original).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to serialize config: {e}"),
        )
    })?;
    std::fs::write(&config_path, &raw).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to write config: {e}"),
        )
    })?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Provider switching
// ---------------------------------------------------------------------------

#[cfg(test)]
mod embedding_status_tests {
    use super::*;
    use tower::ServiceExt;

    #[tokio::test]
    async fn embedding_update_validates_persists_and_requires_restart() {
        let workspace = tempfile::tempdir().unwrap();
        let mut state = AppState::new(config::Config::default()).await;
        state.workspace_dir = workspace.path().to_owned();
        let mut settings = config::EmbeddingConfig::default();
        settings.model = "updated-model".into();
        let Json(saved) = update_embedding(State(state.clone()), Json(settings.clone()))
            .await
            .unwrap();
        assert_eq!(saved["needs_restart"], true);
        let persisted: config::Config =
            toml::from_str(&std::fs::read_to_string(workspace.path().join("config.toml")).unwrap())
                .unwrap();
        assert_eq!(persisted.embedding, settings);
        settings.base_url = "https://user:secret@invalid.test/v1".into();
        let (code, error) = update_embedding(State(state.clone()), Json(settings))
            .await
            .unwrap_err();
        assert_eq!(code, StatusCode::BAD_REQUEST);
        assert!(!error.contains("secret"));
        assert_eq!(state.config.read().await.embedding.model, "updated-model");
    }

    #[tokio::test]
    async fn embedding_status_distinguishes_active_and_pending_config() {
        let state = AppState::new(config::Config::default()).await;
        {
            let mut cfg = state.config.write().await;
            cfg.embedding.model = "pending-model".into();
            cfg.llm.tokens.open_ai = "new-secret".into();
        }
        let Json(status) = get_status(State(state)).await;
        assert_eq!(status["embedding"]["model"], "text-embedding-3-small");
        assert_eq!(status["embedding"]["needs_restart"], true);
        assert_eq!(status["embedding"]["pending"]["model"], "pending-model");
        assert_eq!(status["embedding"]["status"], "unavailable");
        assert!(!status.to_string().contains("new-secret"));
    }

    #[tokio::test]
    async fn status_api_exposes_embedding_settings_without_tokens() {
        let mut cfg = config::Config::default();
        cfg.llm.tokens.open_ai = "secret-openai-key".into();
        let state = AppState::new(cfg).await;
        let Json(status) = get_status(State(state)).await;
        assert_eq!(status["embedding"]["provider"], "openai");
        assert_eq!(status["embedding"]["dimensions"], 768);
        assert_eq!(status["embedding"]["fallback"], "bm25");
        assert_eq!(status["embedding"]["update_semantics"], "full_replacement");
        assert!(!status.to_string().contains("secret-"));
    }

    #[tokio::test]
    async fn embedding_api_rejects_unknown_fields() {
        let state = AppState::new(config::Config::default()).await;
        let response = router()
            .with_state(state)
            .oneshot(
                axum::http::Request::builder()
                    .method("PUT")
                    .uri("/api/config/embedding")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::json!({
                            "version": 1,
                            "enabled": true,
                            "provider": "openai",
                            "model": "text-embedding-3-small",
                            "dimensions": 768,
                            "base_url": "https://api.openai.com/v1",
                            "unexpected": true
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    fn browser_file_grant(state: &AppState) -> String {
        state
            .resources
            .url(
                "",
                "moon",
                crate::services::resource_capability::CapabilityResource::uploaded_file("id")
                    .unwrap(),
                crate::services::resource_capability::CapabilityAudience::Browser,
            )
            .unwrap()
    }

    fn verifies_browser_file(state: &AppState, url: &str) -> bool {
        state
            .resources
            .verify(
                "moon",
                crate::services::resource_capability::CapabilityResource::uploaded_file("id")
                    .unwrap(),
                crate::services::resource_capability::CapabilityAudience::Browser,
                &url.parse().unwrap(),
                "GET",
            )
            .is_ok()
    }

    #[tokio::test]
    async fn server_update_save_failure_publishes_neither_config_nor_auth() {
        let mut cfg = config::Config::default();
        cfg.auth_token = "old-token".into();
        let mut state = AppState::new(cfg).await;
        let workspace = tempfile::tempdir().unwrap();
        state.workspace_dir = workspace.path().join("missing-parent");
        let old_grant = browser_file_grant(&state);
        let result = update_server(
            State(state.clone()),
            Json(UpdateServerRequest {
                host: None,
                port: None,
                auth_token: Some("new-token".into()),
            }),
        )
        .await;
        assert!(result.is_err());
        assert_eq!(state.config.read().await.auth_token, "old-token");
        assert!(verifies_browser_file(&state, &old_grant));
    }

    #[tokio::test]
    async fn same_token_update_preserves_grants_but_changed_token_revokes() {
        let mut cfg = config::Config::default();
        cfg.auth_token = "old-token".into();
        let mut state = AppState::new(cfg).await;
        let workspace = tempfile::tempdir().unwrap();
        state.workspace_dir = workspace.path().to_owned();
        let old_grant = browser_file_grant(&state);
        let _response = update_server(
            State(state.clone()),
            Json(UpdateServerRequest {
                host: None,
                port: None,
                auth_token: Some("old-token".into()),
            }),
        )
        .await
        .unwrap();
        assert!(verifies_browser_file(&state, &old_grant));
        let _response = update_server(
            State(state.clone()),
            Json(UpdateServerRequest {
                host: None,
                port: None,
                auth_token: Some("new-token".into()),
            }),
        )
        .await
        .unwrap();
        assert!(!verifies_browser_file(&state, &old_grant));
        assert_eq!(state.config.read().await.auth_token, "new-token");
    }

    #[tokio::test]
    async fn auth_request_waits_for_atomic_config_and_resource_transition() {
        let mut cfg = config::Config::default();
        cfg.auth_token = "old-token".into();
        let state = AppState::new(cfg).await;
        let app = crate::app::router::build_router(state.clone(), None);
        let mut guard = state.config.write().await;
        let request = tokio::spawn(async move {
            app.oneshot(
                axum::http::Request::builder()
                    .uri("/api/meta")
                    .header("authorization", "Bearer new-token")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
        });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert!(!request.is_finished());
        state.resources.replace("new-token");
        guard.auth_token = "new-token".into();
        drop(guard);
        assert_eq!(request.await.unwrap().status(), StatusCode::OK);
    }
}

#[cfg(test)]
mod llm_key_tests {
    use super::*;
    use crate::services::llm::contract::LlmError;

    #[test]
    fn key_probe_rejects_only_bad_keys_and_unanswered_probes() {
        let openai = config::LlmProvider::Openai;
        assert_eq!(key_probe_rejection(openai, Ok(())), None);
        assert_eq!(
            key_probe_rejection(
                openai,
                Err(LlmError::Authentication("Incorrect API key".into()))
            ),
            Some((StatusCode::UNAUTHORIZED, "invalid API key".into()))
        );
        let (status, message) = key_probe_rejection(
            openai,
            Err(LlmError::Transport("connection refused".into())),
        )
        .unwrap();
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert!(message.contains("OpenAI"), "{message}");
        let (status, message) = key_probe_rejection(
            config::LlmProvider::Anthropic,
            Err(LlmError::Http {
                status: 503,
                message: "down".into(),
            }),
        )
        .unwrap();
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert!(
            message.contains("Anthropic") && message.contains("try again"),
            "{message}"
        );
    }

    /// Keys are saved into the workspace the state was opened for (#107),
    /// and an empty key clears without probing anything.
    #[tokio::test]
    async fn clearing_a_key_saves_into_the_state_workspace_without_a_probe() {
        let workspace = tempfile::tempdir().unwrap();
        let mut cfg = config::Config::default();
        cfg.llm.tokens.open_ai = "old-secret".into();
        let state = AppState::new_in(cfg, workspace.path().to_owned()).await;
        let Json(saved) = update_llm_key(
            State(state.clone()),
            Json(UpdateLlmKeyRequest {
                api_key: None,
                openai: Some("  ".into()),
                elevenlabs: None,
                openrouter: None,
            }),
        )
        .await
        .unwrap();
        assert_eq!(saved["updated"], json!(["openai"]));
        assert!(state.config.read().await.llm.tokens.open_ai.is_empty());
        let persisted = std::fs::read_to_string(workspace.path().join("config.toml")).unwrap();
        assert!(!persisted.contains("old-secret"), "{persisted}");
    }

    /// A provider that answers every request with `status` and `body`.
    async fn provider_stub(status: u16, body: String) -> (String, tokio::task::JoinHandle<()>) {
        let app = axum::Router::new().fallback(axum::routing::post(move || {
            let body = body.clone();
            async move { (StatusCode::from_u16(status).unwrap(), body) }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (url, task)
    }

    /// The save route probes a new key with its provider and stores nothing
    /// the provider rejects (#24, #25), for Anthropic and OpenAI alike.
    #[tokio::test]
    async fn a_rejected_key_is_not_saved_and_an_accepted_one_is() {
        fn token(cfg: &config::Config, provider: config::LlmProvider) -> String {
            match provider {
                config::LlmProvider::Anthropic => cfg.llm.tokens.anthropic.clone(),
                config::LlmProvider::Openai => cfg.llm.tokens.open_ai.clone(),
            }
        }
        for provider in [config::LlmProvider::Anthropic, config::LlmProvider::Openai] {
            let name = match provider {
                config::LlmProvider::Anthropic => "anthropic",
                config::LlmProvider::Openai => "openai",
            };
            let workspace = tempfile::tempdir().unwrap();
            let mut cfg = config::Config::default();
            cfg.llm.tokens.anthropic = "anthropic-before".into();
            cfg.llm.tokens.open_ai = "openai-before".into();
            let state = AppState::new_in(cfg, workspace.path().to_owned()).await;
            let request = || {
                let (api_key, openai) = match provider {
                    config::LlmProvider::Anthropic => (Some("new-secret".to_owned()), None),
                    config::LlmProvider::Openai => (None, Some("new-secret".to_owned())),
                };
                UpdateLlmKeyRequest {
                    api_key,
                    openai,
                    elevenlabs: None,
                    openrouter: None,
                }
            };
            let persisted = || std::fs::read_to_string(workspace.path().join("config.toml"));

            // The provider rejects the key: nothing is saved anywhere.
            let (url, task) = provider_stub(401, "invalid key".into()).await;
            let (status, message) = save_llm_keys(&state, request(), Some(&url))
                .await
                .unwrap_err();
            task.abort();
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{provider:?}: {message}");
            assert_eq!(message, "invalid API key");
            assert_eq!(
                token(&*state.config.read().await, provider),
                format!("{name}-before"),
                "{provider:?}: a rejected key replaced the stored one"
            );
            if let Ok(text) = persisted() {
                assert!(
                    !text.contains("new-secret"),
                    "{provider:?}: a rejected key reached config.toml"
                );
            }

            // The provider accepts it: the key is saved to the workspace.
            let accepted = match provider {
                config::LlmProvider::Anthropic => {
                    r#"{"content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1}}"#.to_owned()
                }
                config::LlmProvider::Openai => {
                    json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"ok"}]}],"usage":{"input_tokens":1,"output_tokens":1}}).to_string()
                }
            };
            let (url, task) = provider_stub(200, accepted).await;
            let Json(saved) = save_llm_keys(&state, request(), Some(&url)).await.unwrap();
            task.abort();
            assert_eq!(saved["updated"], json!([name]));
            assert_eq!(
                token(&*state.config.read().await, provider),
                "new-secret",
                "{provider:?}"
            );
            assert!(
                persisted().unwrap().contains("new-secret"),
                "{provider:?}: the accepted key was not written"
            );
        }
    }
}

#[cfg(test)]
mod extension_tests {
    use super::*;

    async fn state_with_workspace() -> (tempfile::TempDir, AppState) {
        let workspace = tempfile::tempdir().unwrap();
        let mut state = AppState::new(config::Config::default()).await;
        state.workspace_dir = workspace.path().to_owned();
        (workspace, state)
    }

    #[tokio::test]
    async fn custom_servers_need_an_explicit_acknowledgement_and_start_with_no_grant() {
        let (workspace, state) = state_with_workspace().await;
        let (status, Json(denied)) = add_mcp_server(
            State(state.clone()),
            Json(AddMcpServerRequest {
                name: "mine".into(),
                url: "https://127.0.0.1:9/mcp".into(),
                acknowledge_untrusted: false,
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(
            denied["error"],
            "untrusted_extension_requires_acknowledgement"
        );
        assert!(state.config.read().await.mcp_servers.is_empty());

        let Json(added) = add_mcp_server(
            State(state.clone()),
            Json(AddMcpServerRequest {
                name: "mine".into(),
                url: "https://127.0.0.1:9/mcp".into(),
                acknowledge_untrusted: true,
            }),
        )
        .await
        .unwrap();
        assert_eq!(added["trust"], "custom");
        let persisted: config::Config =
            toml::from_str(&std::fs::read_to_string(workspace.path().join("config.toml")).unwrap())
                .unwrap();
        assert_eq!(persisted.mcp_servers[0].trust, config::McpTrust::Custom);
        assert!(persisted.mcp_servers[0].enabled_tools.is_empty());

        // Not connected (the URL is unreachable): grants must wait.
        let (code, _) = update_mcp_tool_grants(
            State(state.clone()),
            axum::extract::Path("mine".into()),
            Json(UpdateMcpGrantsRequest {
                enabled: vec!["anything".into()],
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(code, StatusCode::CONFLICT);
        let (code, _) = update_mcp_tool_grants(
            State(state.clone()),
            axum::extract::Path("missing".into()),
            Json(UpdateMcpGrantsRequest { enabled: vec![] }),
        )
        .await
        .unwrap_err();
        assert_eq!(code, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn catalog_servers_need_no_acknowledgement_and_listing_never_leaks_headers() {
        let (_workspace, state) = state_with_workspace().await;
        {
            let mut cfg = state.config.write().await;
            cfg.mcp_servers.push(McpServerConfig {
                name: "old".into(),
                url: Some("https://old.example/mcp".into()),
                command: None,
                args: Vec::new(),
                headers: [(
                    "Authorization".to_owned(),
                    "Bearer secret-header".to_owned(),
                )]
                .into_iter()
                .collect(),
                trust: config::McpTrust::Custom,
                enabled_tools: vec!["fetch".into()],
            });
        }
        let Json(added) = add_mcp_server(
            State(state.clone()),
            Json(AddMcpServerRequest {
                name: "brave-search".into(),
                url: "https://mcp.bravesearch.com/sse".into(),
                acknowledge_untrusted: false,
            }),
        )
        .await
        .unwrap();
        assert_eq!(added["trust"], "curated");

        let Json(listed) = list_mcp_servers(State(state.clone())).await;
        let json = serde_json::to_string(&listed).unwrap();
        assert!(!json.contains("secret-header"));
        assert!(!json.contains("headers"));
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].trust, config::McpTrust::Custom);
        assert_eq!(listed[1].trust, config::McpTrust::Curated);
        assert!(
            !listed[1].connected,
            "the catalog URL is not reachable from tests"
        );

        let Json(suggested) = suggested_mcp_servers(State(state.clone())).await;
        let brave = suggested
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == "brave-search")
            .unwrap();
        assert_eq!(brave["installed"], true);
    }
}
