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
        .route("/api/config/models/{id}/test", post(test_model_preset))
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
        "chat_provider": config.llm.chat_preset().map(|preset| preset.provider),
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
        Err(LlmError::Timeout) => Some((
            StatusCode::GATEWAY_TIMEOUT,
            format!("{} did not answer in time — try again", provider.label()),
        )),
        Err(_) => Some((
            StatusCode::BAD_GATEWAY,
            format!("{} API error — try again", provider.label()),
        )),
    }
}

/// Where a key probe or connection test goes and how long it waits for the
/// answer. The routes use each provider's own endpoint and
/// [`PROBE_TIMEOUT`](crate::services::llm::PROBE_TIMEOUT); tests point a
/// probe at a stub with a short deadline.
#[derive(Clone, Copy)]
struct Probe<'a> {
    base_url: Option<&'a str>,
    deadline: std::time::Duration,
}

impl Probe<'static> {
    /// The provider's own endpoint, with the live deadline.
    const LIVE: Self = Self {
        base_url: None,
        deadline: crate::services::llm::PROBE_TIMEOUT,
    };
}

#[cfg(test)]
impl<'a> Probe<'a> {
    /// A stub that answers at once; the deadline only guards a broken test.
    fn at(base_url: &'a str) -> Self {
        Self {
            base_url: Some(base_url),
            deadline: std::time::Duration::from_secs(5),
        }
    }

    /// A stub that never answers; the deadline is what the test measures.
    fn stalled(base_url: &'a str) -> Self {
        Self {
            base_url: Some(base_url),
            deadline: std::time::Duration::from_millis(200),
        }
    }
}

/// Checks a key with its provider before it is saved: a one-token
/// completion through the adapter, with the model the person's presets
/// name for that provider (#24, #25), within the probe's deadline.
async fn verify_provider_key(
    state: &AppState,
    provider: config::LlmProvider,
    key: &str,
    probe: Probe<'_>,
) -> Result<(), (StatusCode, String)> {
    let model = {
        let cfg = state.config.read().await;
        crate::services::llm::probe_model(&cfg.llm, provider)
    };
    let mut backend =
        crate::services::llm::LlmBackend::probe(state.http_client.clone(), provider, &model, key);
    if let Some(base_url) = probe.base_url {
        backend.base_url = base_url.to_owned();
    }
    key_probe_rejection(provider, backend.probe_key(probe.deadline).await).map_or(Ok(()), Err)
}

async fn update_llm_key(
    State(state): State<AppState>,
    Json(req): Json<UpdateLlmKeyRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    save_llm_keys(&state, req, Probe::LIVE).await
}

/// Probes and saves the keys in `req`; `probe` says where the probes go
/// and how long they wait.
async fn save_llm_keys(
    state: &AppState,
    req: UpdateLlmKeyRequest,
    probe: Probe<'_>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // A new provider key is checked before it is saved; clearing one is not.
    for (provider, key) in [
        (config::LlmProvider::Anthropic, &req.api_key),
        (config::LlmProvider::Openai, &req.openai),
        (config::LlmProvider::Openrouter, &req.openrouter),
    ] {
        if let Some(key) = key.as_deref().map(str::trim).filter(|key| !key.is_empty()) {
            verify_provider_key(state, provider, key, probe).await?;
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

/// Model presets and slots (#156), which providers can back them, and what
/// each preset's provider offers for its model, keyed by preset id (#28),
/// so the client can warn before a slot or a chat picks a model that
/// cannot see images, read documents or call tools.
fn models_json(config: &config::Config) -> serde_json::Value {
    let capabilities: serde_json::Map<String, serde_json::Value> = config
        .llm
        .presets
        .iter()
        .map(|preset| {
            (
                preset.id.clone(),
                json!(crate::services::llm::provider_capabilities(
                    preset.provider,
                    &preset.model
                )),
            )
        })
        .collect();
    json!({
        "presets": config.llm.presets,
        "chat_preset": config.llm.chat_preset,
        "background_preset": config.llm.background_preset,
        "keyed_providers": config.llm.keyed_providers(),
        "setup_required": config.llm.setup_required(),
        "capabilities": capabilities,
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

// ---------------------------------------------------------------------------
// Connection test (#28)
// ---------------------------------------------------------------------------

/// `POST /api/config/models/{id}/test`: one completion through the preset's
/// adapter, answered with a typed outcome. Nothing is saved and no chat
/// message is created; the route glue over `run_preset_test`.
async fn test_model_preset(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, ModelsError> {
    run_preset_test(&state, &id, Probe::LIVE).await
}

/// Runs the connection test for one preset; `probe` says where it goes and
/// how long it waits.
async fn run_preset_test(
    state: &AppState,
    id: &str,
    probe: Probe<'_>,
) -> Result<Json<serde_json::Value>, ModelsError> {
    use crate::services::llm::{LlmBackend, PresetError, contract::LlmError};
    let (preset, backend) = {
        let cfg = state.config.read().await;
        let Some(preset) = cfg.llm.preset(id.trim()) else {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({
                    "ok": false,
                    "error": "unknown_preset",
                    "preset": id,
                    "message": format!("model preset {id:?} does not exist"),
                })),
            ));
        };
        let backend = LlmBackend::for_preset(&cfg, state.http_client.clone(), &preset.id);
        (preset.clone(), backend)
    };
    let key = backend.as_ref().ok().map(|backend| backend.api_key.clone());
    let outcome = match backend {
        Ok(mut backend) => {
            if let Some(base_url) = probe.base_url {
                backend.base_url = base_url.to_owned();
            }
            backend.test_connection(probe.deadline).await
        }
        Err(error @ PresetError::MissingKey(_)) => Err(LlmError::SetupRequired(error.to_string())),
        Err(error @ PresetError::Unknown(_)) => Err(LlmError::InvalidResponse(error.to_string())),
    };
    // A provider's 401 text tends to quote the key it refused (OpenAI masks
    // it, which no pattern catches); the browser gets the typed sentence
    // alone and the text stays in the log, redacted by the adapter.
    if let Err(LlmError::Authentication(detail)) = &outcome {
        log::warn!(
            "[llm] {} rejected the API key for preset {:?}: {detail}",
            preset.provider.label(),
            preset.id
        );
    }
    let mut answer = test_outcome(&preset, outcome);
    // The adapters redact key-shaped text already; a provider that echoes
    // the key in its refusal still never reaches the browser with it.
    if let (Some(key), Err((_, Json(body)))) = (key.filter(|key| !key.is_empty()), &mut answer)
        && let Some(message) = body["message"].as_str()
    {
        body["message"] = json!(scrub_key_echo(message, &key));
    }
    answer
}

/// `message` without `key`: the key itself, and any run of text that
/// starts with its first four characters and ends with its last four, the
/// way OpenAI masks a rejected key (`sk-revie******-key`). A key shorter
/// than eight characters is matched exactly only.
fn scrub_key_echo(message: &str, key: &str) -> String {
    let key = key.trim();
    if key.is_empty() {
        return message.to_owned();
    }
    let scrubbed = message.replace(key, "[redacted]");
    let chars: Vec<char> = key.chars().collect();
    if chars.len() < 8 {
        return scrubbed;
    }
    let prefix: String = chars[..4].iter().collect();
    let suffix: String = chars[chars.len() - 4..].iter().collect();
    // Between the two ends anything but a separator, so a masked echo ends
    // at the punctuation that follows it and never swallows the sentence.
    let pattern = format!(
        r#"{}[^\s"'`<>,;()\[\]{{}}]*{}"#,
        regex::escape(&prefix),
        regex::escape(&suffix)
    );
    match regex::Regex::new(&pattern) {
        Ok(masked) => masked.replace_all(&scrubbed, "[redacted]").into_owned(),
        Err(_) => scrubbed,
    }
}

/// How a connection test outcome answers (#28): a real answer is `ok` with
/// the model and its usage; every failure the person can act on has its
/// own `error`, matched by variant (#24, #25), with a message that names
/// the provider. Provider messages arrive redacted from the adapters.
fn test_outcome(
    preset: &config::ModelPreset,
    outcome: Result<
        crate::services::llm::contract::Usage,
        crate::services::llm::contract::LlmError,
    >,
) -> Result<Json<serde_json::Value>, ModelsError> {
    use crate::services::llm::contract::LlmError;
    let provider = preset.provider.label();
    let model = preset.model.as_str();
    let (status, error, message, retry_after) = match outcome {
        Ok(usage) => {
            return Ok(Json(json!({
                "ok": true,
                "preset": preset.id,
                "provider": preset.provider,
                "model": preset.model,
                "usage": {
                    "input_tokens": usage.input_tokens,
                    "output_tokens": usage.output_tokens,
                },
                "capabilities": crate::services::llm::provider_capabilities(
                    preset.provider,
                    &preset.model
                ),
            })));
        }
        Err(LlmError::SetupRequired(message)) => (
            StatusCode::SERVICE_UNAVAILABLE,
            "setup_required",
            message,
            None,
        ),
        // The provider's text is logged by the caller, never answered: it
        // can quote the key it refused.
        Err(LlmError::Authentication(_)) => (
            StatusCode::UNAUTHORIZED,
            "authentication",
            format!("{provider} rejected the API key."),
            None,
        ),
        Err(LlmError::RateLimited {
            retry_after,
            message,
        }) => (
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            format!("{provider} accepted the key but is rate limiting: {message}"),
            retry_after.map(|wait| wait.as_secs()),
        ),
        Err(LlmError::Http {
            status: 404,
            message,
        }) => (
            StatusCode::NOT_FOUND,
            "model_not_found",
            format!("{provider} has no model {model:?}: {message}"),
            None,
        ),
        Err(LlmError::Http { status, message }) if status < 500 => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "provider_rejected",
            format!("{provider} rejected the request ({status}): {message}"),
            None,
        ),
        Err(LlmError::Http { status, message }) => (
            StatusCode::BAD_GATEWAY,
            "provider_unavailable",
            format!("{provider} answered {status}: {message}"),
            None,
        ),
        Err(LlmError::Transport(message)) => (
            StatusCode::BAD_GATEWAY,
            "unreachable",
            format!("failed to reach {provider}: {message}"),
            None,
        ),
        Err(LlmError::Timeout) => (
            StatusCode::GATEWAY_TIMEOUT,
            "timeout",
            format!("{provider} did not answer in time"),
            None,
        ),
        Err(LlmError::UnsupportedCapability(capability)) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "unsupported",
            format!("{provider} does not support {capability} for {model:?}"),
            None,
        ),
        Err(LlmError::ContextLength(message)) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "provider_rejected",
            format!("{provider} rejected the request: {message}"),
            None,
        ),
        Err(error @ (LlmError::InvalidResponse(_) | LlmError::Cancelled)) => (
            StatusCode::BAD_GATEWAY,
            "invalid_response",
            format!("{provider} answered with something unexpected: {error}"),
            None,
        ),
    };
    Err((
        status,
        Json(json!({
            "ok": false,
            "error": error,
            "preset": preset.id,
            "provider": preset.provider,
            "model": preset.model,
            "message": message,
            "retry_after_seconds": retry_after,
        })),
    ))
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

    /// The status names the Chat preset's provider next to its model (#28),
    /// so onboarding can say who did not answer even before the presets
    /// listing loads; without a Chat preset both are null.
    #[tokio::test]
    async fn status_names_the_chat_presets_provider() {
        let mut bare = config::Config::default();
        bare.llm.presets.clear();
        bare.llm.chat_preset.clear();
        let Json(bare) = get_status(State(AppState::new(bare).await)).await;
        assert!(bare["chat_provider"].is_null(), "{bare}");
        assert!(bare["model"].is_null(), "{bare}");
        let mut cfg = config::Config::default();
        cfg.llm.tokens.open_ai = "secret-openai-key".into();
        cfg.llm.seed_presets(config::LlmProvider::Openai);
        cfg.llm.chat_preset = "gpt".into();
        let Json(status) = get_status(State(AppState::new(cfg).await)).await;
        assert_eq!(status["llm_configured"], true, "{status}");
        assert_eq!(status["chat_provider"], "openai", "{status}");
        assert_eq!(status["chat_preset"], "gpt", "{status}");
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
    use std::time::Duration;

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
        // A provider that never answers says nothing about the key either.
        let (status, message) =
            key_probe_rejection(config::LlmProvider::Openrouter, Err(LlmError::Timeout)).unwrap();
        assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
        assert!(
            message.contains("OpenRouter") && message.contains("did not answer in time"),
            "{message}"
        );
    }

    /// A connection the provider accepts but never answers ends at the probe
    /// deadline instead of holding the save open, and stores nothing.
    #[tokio::test]
    async fn a_stalled_provider_ends_the_key_save_at_the_deadline_without_saving() {
        let workspace = tempfile::tempdir().unwrap();
        let mut cfg = config::Config::default();
        cfg.llm.tokens.open_ai = "openai-before".into();
        let state = AppState::new_in(cfg, workspace.path().to_owned()).await;
        let (url, task) = stalled_stub().await;
        let started = std::time::Instant::now();
        let (status, message) = save_llm_keys(
            &state,
            UpdateLlmKeyRequest {
                api_key: None,
                openai: Some("new-secret".into()),
                elevenlabs: None,
                openrouter: None,
            },
            Probe::stalled(&url),
        )
        .await
        .unwrap_err();
        task.abort();
        assert_eq!(status, StatusCode::GATEWAY_TIMEOUT, "{message}");
        assert!(message.contains("OpenAI"), "{message}");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the save waited {:?} for a provider that never answers",
            started.elapsed()
        );
        assert_eq!(
            state.config.read().await.llm.tokens.open_ai,
            "openai-before"
        );
        assert!(!workspace.path().join("config.toml").exists());
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
    pub(super) async fn provider_stub(
        status: u16,
        body: String,
    ) -> (String, tokio::task::JoinHandle<()>) {
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

    /// A provider that accepts every connection and never answers on it.
    pub(super) async fn stalled_stub() -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let mut held = Vec::new();
            loop {
                let (socket, _) = listener.accept().await.unwrap();
                held.push(socket);
            }
        });
        (url, task)
    }

    /// The save route probes a new key with its provider and stores nothing
    /// the provider rejects (#24, #25), for Anthropic, OpenAI and
    /// OpenRouter (#26) alike.
    #[tokio::test]
    async fn a_rejected_key_is_not_saved_and_an_accepted_one_is() {
        fn token(cfg: &config::Config, provider: config::LlmProvider) -> String {
            match provider {
                config::LlmProvider::Anthropic => cfg.llm.tokens.anthropic.clone(),
                config::LlmProvider::Openai => cfg.llm.tokens.open_ai.clone(),
                config::LlmProvider::Openrouter => cfg.llm.tokens.open_router.clone(),
                config::LlmProvider::Codex => unreachable!("no key (#27)"),
            }
        }
        for provider in [
            config::LlmProvider::Anthropic,
            config::LlmProvider::Openai,
            config::LlmProvider::Openrouter,
        ] {
            let name = match provider {
                config::LlmProvider::Anthropic => "anthropic",
                config::LlmProvider::Openai => "openai",
                config::LlmProvider::Openrouter => "openrouter",
                config::LlmProvider::Codex => unreachable!("no key (#27)"),
            };
            let workspace = tempfile::tempdir().unwrap();
            let mut cfg = config::Config::default();
            cfg.llm.tokens.anthropic = "anthropic-before".into();
            cfg.llm.tokens.open_ai = "openai-before".into();
            cfg.llm.tokens.open_router = "openrouter-before".into();
            let state = AppState::new_in(cfg, workspace.path().to_owned()).await;
            let request = || {
                let secret = Some("new-secret".to_owned());
                let (api_key, openai, openrouter) = match provider {
                    config::LlmProvider::Anthropic => (secret, None, None),
                    config::LlmProvider::Openai => (None, secret, None),
                    config::LlmProvider::Openrouter => (None, None, secret),
                    config::LlmProvider::Codex => unreachable!("no key (#27)"),
                };
                UpdateLlmKeyRequest {
                    api_key,
                    openai,
                    elevenlabs: None,
                    openrouter,
                }
            };
            let persisted = || std::fs::read_to_string(workspace.path().join("config.toml"));

            // The provider rejects the key: nothing is saved anywhere.
            let (url, task) = provider_stub(401, "invalid key".into()).await;
            let (status, message) = save_llm_keys(&state, request(), Probe::at(&url))
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
                config::LlmProvider::Openrouter => {
                    json!({"id":"gen-1","choices":[{"index":0,"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1}}).to_string()
                }
                config::LlmProvider::Codex => unreachable!("no key (#27)"),
            };
            let (url, task) = provider_stub(200, accepted).await;
            let Json(saved) = save_llm_keys(&state, request(), Probe::at(&url))
                .await
                .unwrap();
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
mod preset_test_tests {
    use super::*;
    use crate::services::llm::contract::{LlmError, Usage};
    use axum::body::to_bytes;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime};
    use tower::ServiceExt;

    /// Every file under `root` with its size and mtime, so a test can prove a
    /// call wrote nothing there.
    fn tree(root: &Path) -> Vec<(PathBuf, u64, SystemTime)> {
        fn visit(dir: &Path, out: &mut Vec<(PathBuf, u64, SystemTime)>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    visit(&path, out);
                } else if let Ok(meta) = path.metadata() {
                    out.push((path, meta.len(), meta.modified().unwrap()));
                }
            }
        }
        let mut out = Vec::new();
        visit(root, &mut out);
        out.sort();
        out
    }

    /// No conversation was created or touched: the test never goes through
    /// chat, so nothing under any `chats/` exists afterwards.
    fn assert_no_chat_files(root: &Path) {
        let chats: Vec<_> = tree(root)
            .into_iter()
            .filter(|(path, _, _)| path.components().any(|c| c.as_os_str() == "chats"))
            .map(|(path, _, _)| path)
            .collect();
        assert!(chats.is_empty(), "the connection test wrote {chats:?}");
    }

    /// The key `configured()` stores for `provider`, shaped like a real one
    /// so an echo of it, masked or not, looks the way a provider writes it.
    fn key_for(provider: config::LlmProvider) -> &'static str {
        match provider {
            config::LlmProvider::Anthropic => "sk-ant-test-anthropic-key-7f3a",
            config::LlmProvider::Openai => "sk-test-openai-key-9c1d",
            config::LlmProvider::Openrouter => "sk-or-test-openrouter-key-2b8e",
            config::LlmProvider::Codex => unreachable!("no key (#27)"),
        }
    }

    /// How OpenAI echoes a rejected key: its first eight and last four
    /// characters around a mask. Neither `redact_secrets` (which wants a
    /// long run of key characters) nor an exact match catches it.
    fn masked(key: &str) -> String {
        format!("{}******{}", &key[..8], &key[key.len() - 4..])
    }

    /// No part of `key` a person could recognise it by reaches `body`: not
    /// the key, not its first eight characters, not the masked echo.
    fn assert_no_key(body: &serde_json::Value, key: &str, label: &str) {
        let text = body.to_string();
        assert!(
            !text.contains(key),
            "{label}: the key reached the browser: {text}"
        );
        assert!(
            !text.contains(&key[..8]),
            "{label}: the key's prefix reached the browser: {text}"
        );
        assert!(
            !text.contains(&masked(key)),
            "{label}: the masked key reached the browser: {text}"
        );
    }

    /// All three providers keyed and seeded; the Chat slot is Anthropic's.
    fn configured() -> config::Config {
        let mut cfg = config::Config::default();
        cfg.llm.tokens.anthropic = key_for(config::LlmProvider::Anthropic).into();
        cfg.llm.tokens.open_ai = key_for(config::LlmProvider::Openai).into();
        cfg.llm.tokens.open_router = key_for(config::LlmProvider::Openrouter).into();
        for provider in [
            config::LlmProvider::Anthropic,
            config::LlmProvider::Openai,
            config::LlmProvider::Openrouter,
        ] {
            cfg.llm.seed_presets(provider);
        }
        cfg
    }

    async fn post_test(state: AppState, id: &str) -> (StatusCode, serde_json::Value) {
        let response = router()
            .with_state(state)
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri(format!("/api/config/models/{id}/test"))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    /// The outcome mapping is by variant (#24, #25): each failure the person
    /// can act on has its own `error`, and the message names the provider.
    #[test]
    fn test_outcomes_are_typed_by_variant() {
        let preset = config::ModelPreset {
            id: "gpt".into(),
            name: "GPT-5.4".into(),
            provider: config::LlmProvider::Openai,
            model: "gpt-5.4".into(),
        };
        let Json(ok) = test_outcome(
            &preset,
            Ok(Usage {
                input_tokens: 8,
                output_tokens: 1,
                ..Default::default()
            }),
        )
        .unwrap();
        assert_eq!(ok["ok"], true);
        assert_eq!(ok["preset"], "gpt");
        assert_eq!(ok["provider"], "openai");
        assert_eq!(ok["model"], "gpt-5.4");
        assert_eq!(ok["usage"]["input_tokens"], 8);
        assert_eq!(ok["usage"]["output_tokens"], 1);
        assert_eq!(ok["capabilities"]["documents"], false);
        assert_eq!(ok["capabilities"]["tools"], true);

        let cases: Vec<(LlmError, StatusCode, &str)> = vec![
            (
                LlmError::SetupRequired("no OpenAI API key is configured".into()),
                StatusCode::SERVICE_UNAVAILABLE,
                "setup_required",
            ),
            (
                LlmError::Authentication("Incorrect API key".into()),
                StatusCode::UNAUTHORIZED,
                "authentication",
            ),
            (
                LlmError::RateLimited {
                    retry_after: Some(Duration::from_secs(7)),
                    message: "slow down".into(),
                },
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
            ),
            (
                LlmError::Http {
                    status: 404,
                    message: "The model `gpt-5.4` does not exist".into(),
                },
                StatusCode::NOT_FOUND,
                "model_not_found",
            ),
            (
                LlmError::Http {
                    status: 402,
                    message: "Insufficient credits".into(),
                },
                StatusCode::UNPROCESSABLE_ENTITY,
                "provider_rejected",
            ),
            (
                LlmError::Http {
                    status: 503,
                    message: "down".into(),
                },
                StatusCode::BAD_GATEWAY,
                "provider_unavailable",
            ),
            (
                LlmError::Transport("connection refused".into()),
                StatusCode::BAD_GATEWAY,
                "unreachable",
            ),
            (LlmError::Timeout, StatusCode::GATEWAY_TIMEOUT, "timeout"),
            (
                LlmError::InvalidResponse("no choices".into()),
                StatusCode::BAD_GATEWAY,
                "invalid_response",
            ),
            (
                LlmError::UnsupportedCapability("tools"),
                StatusCode::UNPROCESSABLE_ENTITY,
                "unsupported",
            ),
        ];
        for (error, expected_status, expected_error) in cases {
            let label = format!("{error:?}");
            let (status, Json(body)) = test_outcome(&preset, Err(error)).unwrap_err();
            assert_eq!(status, expected_status, "{label}");
            assert_eq!(body["ok"], false, "{label}");
            assert_eq!(body["error"], expected_error, "{label}");
            assert_eq!(body["preset"], "gpt", "{label}");
            let message = body["message"].as_str().unwrap();
            assert!(message.contains("OpenAI"), "{label}: {message}");
            if expected_error == "authentication" {
                // The provider's own 401 text can echo the key; the answer
                // is the typed sentence alone, the body goes to the log.
                assert_eq!(message, "OpenAI rejected the API key.", "{label}");
            }
            if expected_error == "rate_limited" {
                assert_eq!(body["retry_after_seconds"], 7, "{label}");
            }
            if expected_error == "model_not_found" {
                assert!(message.contains("gpt-5.4"), "{label}: {message}");
            }
            if expected_error == "provider_rejected" {
                assert!(
                    message.contains("Insufficient credits"),
                    "{label}: {message}"
                );
            }
        }
    }

    /// A provider's refusal can quote the key it refused, whole or masked
    /// the way OpenAI does (`sk-revie******-key`); neither form survives.
    #[test]
    fn key_echoes_are_scrubbed_whole_and_masked() {
        let key = "sk-test-openai-key-9c1d";
        assert_eq!(
            scrub_key_echo("Incorrect API key provided: sk-test-openai-key-9c1d.", key),
            "Incorrect API key provided: [redacted]."
        );
        assert_eq!(
            scrub_key_echo(
                "Incorrect API key provided: sk-test-******9c1d. You can find your key at https://platform.openai.com.",
                key
            ),
            "Incorrect API key provided: [redacted]. You can find your key at https://platform.openai.com."
        );
        assert_eq!(
            scrub_key_echo(r#"{"message":"key sk-t…9c1d is not valid"}"#, key),
            r#"{"message":"key [redacted] is not valid"}"#
        );
        // Text that merely shares letters with the key is left alone.
        assert_eq!(
            scrub_key_echo("The model `gpt-5.4` does not exist", key),
            "The model `gpt-5.4` does not exist"
        );
        assert_eq!(
            scrub_key_echo("sk-test is a prefix, 9c1d a suffix", key),
            "sk-test is a prefix, 9c1d a suffix"
        );
        // A short key is only ever matched exactly.
        assert_eq!(
            scrub_key_echo("abc…xyz and abcdxyz", "abcdxyz"),
            "abc…xyz and [redacted]"
        );
        assert_eq!(scrub_key_echo("nothing here", ""), "nothing here");
    }

    /// Unknown presets and presets without a key are answered before any
    /// network, and the route neither writes config nor creates a chat.
    #[tokio::test]
    async fn the_route_reports_unknown_presets_and_missing_keys_without_a_network() {
        let workspace = tempfile::tempdir().unwrap();
        let mut cfg = configured();
        cfg.llm.tokens.open_ai.clear();
        let state = AppState::new_in(cfg, workspace.path().to_owned()).await;
        let before = tree(workspace.path());

        let (status, body) = post_test(state.clone(), "nope").await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"], "unknown_preset");

        let (status, body) = post_test(state.clone(), "gpt").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"], "setup_required");
        assert_eq!(body["preset"], "gpt");
        assert!(
            body["message"].as_str().unwrap().contains("OpenAI"),
            "{body}"
        );

        assert_eq!(tree(workspace.path()), before, "the route wrote a file");
        assert_no_chat_files(workspace.path());
        assert!(!workspace.path().join("config.toml").exists());
    }

    /// One completion per provider through its adapter, the outcome typed
    /// by what the provider said, and nothing on disk either way (#28).
    #[tokio::test]
    async fn the_connection_test_runs_one_completion_per_provider_and_writes_nothing() {
        for provider in [
            config::LlmProvider::Anthropic,
            config::LlmProvider::Openai,
            config::LlmProvider::Openrouter,
        ] {
            let workspace = tempfile::tempdir().unwrap();
            let state = AppState::new_in(configured(), workspace.path().to_owned()).await;
            let before = tree(workspace.path());
            let preset_id = config::default_presets(provider)[0].id.clone();
            let model = config::default_presets(provider)[0].model.clone();
            let name = match provider {
                config::LlmProvider::Anthropic => "anthropic",
                config::LlmProvider::Openai => "openai",
                config::LlmProvider::Openrouter => "openrouter",
                config::LlmProvider::Codex => unreachable!("no key (#27)"),
            };
            let answer = match provider {
                config::LlmProvider::Anthropic => {
                    r#"{"content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn","usage":{"input_tokens":8,"output_tokens":1}}"#.to_owned()
                }
                config::LlmProvider::Openai => {
                    json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"ok"}]}],"usage":{"input_tokens":8,"output_tokens":1}}).to_string()
                }
                config::LlmProvider::Openrouter => {
                    json!({"id":"gen-1","choices":[{"index":0,"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":8,"completion_tokens":1}}).to_string()
                }
                config::LlmProvider::Codex => unreachable!("no key (#27)"),
            };

            let (url, task) = super::llm_key_tests::provider_stub(200, answer).await;
            let Json(ok) = run_preset_test(&state, &preset_id, Probe::at(&url))
                .await
                .unwrap_or_else(|(status, Json(body))| panic!("{provider:?}: {status} {body}"));
            task.abort();
            assert_eq!(ok["ok"], true, "{provider:?}");
            assert_eq!(ok["preset"], preset_id, "{provider:?}");
            assert_eq!(ok["provider"], name, "{provider:?}");
            assert_eq!(ok["model"], model, "{provider:?}");
            assert_eq!(ok["usage"]["input_tokens"], 8, "{provider:?}");
            assert_eq!(ok["usage"]["output_tokens"], 1, "{provider:?}");
            assert_eq!(
                ok["capabilities"]["documents"],
                provider == config::LlmProvider::Anthropic,
                "{provider:?}"
            );
            assert_no_key(&ok, key_for(provider), name);

            // A provider that echoes the key in its refusal, whole or masked
            // the way OpenAI writes it: the answer names the refusal, never
            // the key, and a 401 carries the typed sentence alone.
            let key = key_for(provider);
            let echoed = format!("rejected: key {key} is not allowed");
            let masked_401 = format!("Incorrect API key provided: {}.", masked(key));
            let masked_400 = format!("bad request for key {}", masked(key));
            for (status, body, expected_status, expected_error) in [
                (401, "nope", StatusCode::UNAUTHORIZED, "authentication"),
                (
                    401,
                    masked_401.as_str(),
                    StatusCode::UNAUTHORIZED,
                    "authentication",
                ),
                (
                    404,
                    "no such model",
                    StatusCode::NOT_FOUND,
                    "model_not_found",
                ),
                (
                    429,
                    "slow down",
                    StatusCode::TOO_MANY_REQUESTS,
                    "rate_limited",
                ),
                (503, "down", StatusCode::BAD_GATEWAY, "provider_unavailable"),
                (
                    400,
                    echoed.as_str(),
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "provider_rejected",
                ),
                (
                    400,
                    masked_400.as_str(),
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "provider_rejected",
                ),
            ] {
                let (url, task) = super::llm_key_tests::provider_stub(status, body.into()).await;
                let (got, Json(body)) = run_preset_test(&state, &preset_id, Probe::at(&url))
                    .await
                    .unwrap_err();
                task.abort();
                let label = format!("{provider:?} {status} {expected_error}");
                assert_eq!(got, expected_status, "{label}: {body}");
                assert_eq!(body["ok"], false, "{label}");
                assert_eq!(body["error"], expected_error, "{label}");
                assert_no_key(&body, key, &label);
                let message = body["message"].as_str().unwrap();
                if expected_error == "authentication" {
                    assert!(
                        message.ends_with("rejected the API key."),
                        "{label}: {message}"
                    );
                } else if status == 400 {
                    // The adapter's own redaction may get there first.
                    assert!(
                        message.to_lowercase().contains("[redacted]"),
                        "{label}: {message}"
                    );
                }
            }

            let (got, Json(body)) =
                run_preset_test(&state, &preset_id, Probe::at("http://127.0.0.1:1"))
                    .await
                    .unwrap_err();
            assert_eq!(got, StatusCode::BAD_GATEWAY, "{provider:?}: {body}");
            assert_eq!(body["error"], "unreachable", "{provider:?}");

            // A provider that accepts the connection and never answers ends
            // at the deadline as `timeout`, instead of holding the Test
            // button forever.
            let (url, task) = super::llm_key_tests::stalled_stub().await;
            let started = std::time::Instant::now();
            let (got, Json(body)) = run_preset_test(&state, &preset_id, Probe::stalled(&url))
                .await
                .unwrap_err();
            task.abort();
            assert_eq!(got, StatusCode::GATEWAY_TIMEOUT, "{provider:?}: {body}");
            assert_eq!(body["error"], "timeout", "{provider:?}");
            assert!(
                body["message"]
                    .as_str()
                    .unwrap()
                    .contains("did not answer in time"),
                "{provider:?}: {body}"
            );
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "{provider:?}: the test waited {:?}",
                started.elapsed()
            );

            assert_eq!(
                tree(workspace.path()),
                before,
                "{provider:?}: the connection test wrote a file"
            );
            assert_no_chat_files(workspace.path());
            assert!(!workspace.path().join("config.toml").exists());
        }
    }

    /// `GET /api/config/models` says what each preset's provider offers for
    /// its model, keyed by preset id, and still carries no key.
    #[tokio::test]
    async fn models_listing_carries_each_presets_capabilities_and_no_secrets() {
        let workspace = tempfile::tempdir().unwrap();
        let state = AppState::new_in(configured(), workspace.path().to_owned()).await;
        let Json(models) = get_models(State(state)).await;
        let capabilities = models["capabilities"].as_object().unwrap();
        for preset in models["presets"].as_array().unwrap() {
            let id = preset["id"].as_str().unwrap();
            assert!(capabilities.contains_key(id), "no capabilities for {id}");
        }
        assert_eq!(capabilities["sonnet"]["documents"], true);
        assert_eq!(capabilities["sonnet"]["vision"], true);
        assert_eq!(capabilities["gpt"]["documents"], false);
        assert_eq!(capabilities["gpt"]["tools"], true);
        assert_eq!(capabilities["gpt"]["reasoning_controls"], true);
        assert_eq!(capabilities["openrouter-sonnet"]["documents"], false);
        for provider in [
            config::LlmProvider::Anthropic,
            config::LlmProvider::Openai,
            config::LlmProvider::Openrouter,
        ] {
            assert_no_key(&models, key_for(provider), "models listing");
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
