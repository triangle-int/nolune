use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{Mutex, RwLock, broadcast};
use tokio_util::sync::CancellationToken;

use crate::{
    config::{self, Config},
    domain::events::ServerEvent,
    services::browser_sessions::BrowserSessionStore,
    services::llm::LlmBackend,
    services::machine_registry::MachineRegistry,
    services::mcp::McpRegistry,
    services::vector::VectorStore,
};

/// A pending secret request waiting for user input.
pub struct PendingSecret {
    #[allow(dead_code)]
    pub target: String,
    pub responder: tokio::sync::oneshot::Sender<String>,
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<Config>>,
    pub(crate) resources: crate::services::resource_access::ResourceAccess,
    pub workspace_dir: PathBuf,
    pub events: broadcast::Sender<ServerEvent>,
    /// Backend for conversations that pin no preset: the Chat slot (#156).
    pub llm: Arc<RwLock<Option<LlmBackend>>>,
    /// Backend for memory extraction, titles, check-ins, and reflection: the Background slot.
    pub background_llm: Arc<RwLock<Option<LlmBackend>>>,
    /// Active agent tasks per instance slug — cancellation tokens.
    pub agent_tasks: Arc<Mutex<HashMap<String, CancellationToken>>>,
    /// Pending secret requests awaiting user input.
    pub pending_secrets: Arc<Mutex<HashMap<String, PendingSecret>>>,
    /// Connected MCP servers and their tools.
    pub mcp_registry: McpRegistry,
    /// Shared HTTP client for provider and integration calls.
    pub http_client: reqwest::Client,
    /// Versioned local vector store for semantic memory search.
    pub vector_store: Arc<VectorStore>,
    /// Connected Tauri agent machines (for computer use) and every machine
    /// that ever registered, persisted under the companion directory (#80).
    pub machine_registry: MachineRegistry,
    /// The one proactive companion loop (#92): every self-started run is admitted here.
    pub proactive: crate::services::proactive::ProactiveLoop,
    /// Commitments the companion follows through on (#85); the record is the source of truth.
    pub commitments: crate::services::commitments::CommitmentStore,
    /// Paired browsers and pending pairing codes (#112). In memory until
    /// `attach_storage` is called by the server entrypoint.
    pub browser_sessions: Arc<BrowserSessionStore>,
    /// Federation identity and peers (#108); the keystore under `workspace_dir` opens on first use.
    pub federation: Arc<crate::services::federation::pairing::FederationState>,
}

// No hardcoded MCP servers — users add them via Settings UI or config.toml.
// Suggested MCP servers are listed in the client's Extensions settings.

impl AppState {
    /// The state for the workspace the process was started for. `new_in` is the
    /// explicit form; this wrapper only supplies the resolved root (#107).
    pub async fn new(config: Config) -> Self {
        Self::new_in(config, config::workspace_root()).await
    }

    /// Open every store under `workspace_dir`. Nothing here consults the environment, so
    /// two profiles in one test process, or one profile on a shared host, stay apart.
    pub(crate) async fn new_in(config: Config, workspace_dir: PathBuf) -> Self {
        let (events, _) = broadcast::channel(4096);
        let llm = LlmBackend::from_config(&config);
        let background_llm = LlmBackend::background(&config);

        // Connect to configured MCP servers
        let mcp_connections = crate::services::mcp::connect_all(&config.mcp_servers).await;
        let mcp_registry = McpRegistry::new(mcp_connections, config.mcp_servers.clone());
        let mcp_tool_count = mcp_registry.tool_count().await;
        if mcp_tool_count > 0 {
            log::info!("MCP: {} tools from external servers", mcp_tool_count);
        }

        let http_client = reqwest::Client::new();
        let federation = crate::services::federation::pairing::FederationState::new(
            &workspace_dir,
            http_client.clone(),
        );

        // Open the local derived vector index.
        let vector_store = VectorStore::connect_with_config(&workspace_dir, &config).await;

        let proactive = crate::services::proactive::ProactiveLoop::new(
            &workspace_dir,
            crate::domain::companion::CANONICAL_SLUG,
        )
        .with_events(events.clone());
        let commitments = crate::services::commitments::CommitmentStore::new(
            &workspace_dir,
            crate::domain::companion::CANONICAL_SLUG,
        )
        .with_events(events.clone());
        let machine_registry =
            MachineRegistry::open(&workspace_dir, crate::domain::companion::CANONICAL_SLUG)
                .with_events(events.clone());

        Self {
            resources: crate::services::resource_access::ResourceAccess::new(&config.auth_token),
            config: Arc::new(RwLock::new(config)),
            workspace_dir,
            events,
            llm: Arc::new(RwLock::new(llm)),
            background_llm: Arc::new(RwLock::new(background_llm)),
            agent_tasks: Arc::new(Mutex::new(HashMap::new())),
            pending_secrets: Arc::new(Mutex::new(HashMap::new())),
            mcp_registry,
            http_client,
            vector_store: Arc::new(vector_store),
            machine_registry,
            proactive,
            commitments,
            browser_sessions: Arc::new(BrowserSessionStore::new()),
            federation: Arc::new(federation),
        }
    }

    /// Rebuild both backends from the in-memory config (#156): the Chat slot
    /// and the Background slot. Callers that changed config in memory use
    /// this instead of `reload_config`, which diffs against disk.
    pub async fn rebuild_llm(&self) {
        let config = self.config.read().await;
        let chat = LlmBackend::from_config(&config);
        let background = LlmBackend::background(&config);
        drop(config);
        *self.llm.write().await = chat;
        *self.background_llm.write().await = background;
    }

    /// Reload config from disk and rebuild LLM if credentials or model selection changed.
    pub async fn reload_config(&self) {
        let new_config = match config::load_config() {
            Ok(c) => c,
            Err(e) => {
                log::warn!("failed to reload config: {e}");
                return;
            }
        };

        let (llm_changed, mcp_changed) = {
            let mut old = self.config.write().await;
            let llm = old.llm.tokens != new_config.llm.tokens
                || old.llm.presets != new_config.llm.presets
                || old.llm.chat_preset != new_config.llm.chat_preset
                || old.llm.background_preset != new_config.llm.background_preset;
            let mcp = old.mcp_servers.len() != new_config.mcp_servers.len()
                || old
                    .mcp_servers
                    .iter()
                    .zip(new_config.mcp_servers.iter())
                    .any(|(a, b)| a.name != b.name || a.url != b.url);
            if old.auth_token != new_config.auth_token {
                self.resources.replace(&new_config.auth_token);
            }
            *old = new_config.clone();
            (llm, mcp)
        };

        if llm_changed {
            self.rebuild_llm().await;
            log::info!("config reloaded: LLM rebuilt");
        }

        if mcp_changed {
            self.mcp_registry.reconnect(&new_config.mcp_servers).await;
            log::info!(
                "config reloaded: MCP servers reconnected ({} tools)",
                self.mcp_registry.tool_count().await
            );
        }

        if self.vector_store.embedding_needs_restart(&new_config) {
            log::info!(
                "embedding settings changed: restart required; active index remains unchanged"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn new_in_pins_the_workspace_and_opens_its_stores_there() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("profiles").join("molinka");

        let state = AppState::new_in(Config::default(), root.clone()).await;

        assert_eq!(state.workspace_dir, root);
        assert!(
            root.is_dir(),
            "the vector store must open under the given root, not the process default"
        );
        assert!(
            !tmp.path().join(".nolune").exists(),
            "nothing may fall back to a home-relative root"
        );
    }
}
