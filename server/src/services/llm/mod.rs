mod agent_loop;
mod anthropic;
pub mod contract;
mod helpers;
mod openai;
mod types;

use std::path::Path;

use tokio::sync::broadcast;

use crate::config::Config;
use crate::domain::events::ServerEvent;
use crate::services::tool::ToolDyn;

// Re-export all public types and functions that were accessible from crate::services::llm::*
pub use anthropic::{MAX_INLINE_IMAGE_BASE64_BYTES, MAX_INLINE_IMAGE_BYTES};
#[allow(unused_imports)]
pub use helpers::DEFAULT_ONBOARDING_PROMPT;
pub use helpers::{
    build_multimodal_prompt, get_real_input_tokens, history_to_chat_messages, load_system_prompt,
    refresh_resource_messages,
};
pub use types::{ContentBlock, HistoryEntry, LlmBackend, Message, ToolChatResult};
#[allow(unused_imports)]
pub use types::{DocumentSource, ImageSource, ResourceProvenance};

use agent_loop::{agent_loop, collect_tool_defs, streaming_agent_loop};
use contract::{ExecutionScope, LlmError, LlmRequest, ProviderAdapter};
use helpers::retry_on_rate_limit;

use types::{ANTHROPIC_BASE_URL, OPENAI_BASE_URL};

/// What a preset's provider offers for its model id; OpenAI's answer varies by model.
pub fn provider_capabilities(
    provider: crate::config::LlmProvider,
    model: &str,
) -> contract::Capabilities {
    match provider {
        crate::config::LlmProvider::Anthropic => anthropic::CAPABILITIES,
        crate::config::LlmProvider::Openai => openai::capabilities_for(model),
    }
}

/// The model a key probe for `provider` should name: the chat preset when it
/// runs on that provider, else the provider's first preset, else the model
/// its default chat preset would use (a first key has no presets yet).
pub fn probe_model(
    config: &crate::config::LlmConfig,
    provider: crate::config::LlmProvider,
) -> String {
    config
        .chat_preset()
        .filter(|preset| preset.provider == provider)
        .or_else(|| {
            config
                .presets
                .iter()
                .find(|preset| preset.provider == provider)
        })
        .map(|preset| preset.model.clone())
        .or_else(|| {
            crate::config::default_presets(provider)
                .into_iter()
                .next()
                .map(|preset| preset.model)
        })
        .unwrap_or_default()
}

/// Why a preset cannot become a backend (#156).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresetError {
    /// No preset with that id exists.
    Unknown(String),
    /// The preset's provider has no API key.
    MissingKey(crate::config::LlmProvider),
}

impl std::fmt::Display for PresetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PresetError::Unknown(id) => write!(f, "model preset {id:?} does not exist"),
            PresetError::MissingKey(provider) => {
                write!(f, "no {} API key is configured", provider.label())
            }
        }
    }
}

impl LlmBackend {
    pub(crate) fn adapter(&self) -> Result<Box<dyn ProviderAdapter>, LlmError> {
        match self.provider {
            crate::config::LlmProvider::Anthropic => {
                Ok(Box::new(anthropic::AnthropicAdapter(self.clone())))
            }
            crate::config::LlmProvider::Openai => Ok(Box::new(openai::OpenaiAdapter(self.clone()))),
        }
    }

    /// Build the backend for one named preset, sharing the given HTTP client.
    pub fn for_preset(
        config: &Config,
        http: reqwest::Client,
        preset_id: &str,
    ) -> Result<Self, PresetError> {
        let preset = config
            .llm
            .preset(preset_id)
            .ok_or_else(|| PresetError::Unknown(preset_id.to_owned()))?;
        let api_key = config
            .llm
            .key_for(preset.provider)
            .ok_or(PresetError::MissingKey(preset.provider))?
            .to_owned();
        Ok(Self {
            preset: preset.id.clone(),
            http,
            api_key,
            model: preset.model.clone(),
            base_url: match preset.provider {
                crate::config::LlmProvider::Anthropic => ANTHROPIC_BASE_URL.to_string(),
                crate::config::LlmProvider::Openai => OPENAI_BASE_URL.to_string(),
            },
            provider: preset.provider,
        })
    }

    /// The backend for conversations that pin no preset: the Chat slot.
    pub fn from_config(config: &Config) -> Option<Self> {
        Self::for_preset(config, reqwest::Client::new(), &config.llm.chat_preset)
            .map_err(|error| log::warn!("[llm] chat preset unavailable: {error}"))
            .ok()
    }

    /// The backend for memory extraction, titles, check-ins, and reflection:
    /// the Background slot. Never the chat preset by accident.
    pub fn background(config: &Config) -> Option<Self> {
        Self::for_preset(
            config,
            reqwest::Client::new(),
            &config.llm.background_preset,
        )
        .map_err(|error| log::warn!("[llm] background preset unavailable: {error}"))
        .ok()
    }

    pub fn model_name(&self) -> &str {
        &self.model
    }

    /// A backend for checking `api_key` against `provider` with `model`,
    /// outside any preset; the key is not in config yet.
    pub fn probe(
        http: reqwest::Client,
        provider: crate::config::LlmProvider,
        model: &str,
        api_key: &str,
    ) -> Self {
        Self {
            preset: "probe".into(),
            http,
            api_key: api_key.to_owned(),
            model: model.to_owned(),
            base_url: match provider {
                crate::config::LlmProvider::Anthropic => ANTHROPIC_BASE_URL.to_string(),
                crate::config::LlmProvider::Openai => OPENAI_BASE_URL.to_string(),
            },
            provider,
        }
    }

    /// Checks the key with a one-token completion through the adapter, the
    /// probe the key routes use for both providers (#28 builds on it).
    /// `Ok` means the provider accepted the key: a completion, a rate limit,
    /// or any other answer that required authentication first.
    /// `Err(Authentication)` means it rejected the key. Transport and server
    /// errors are returned as they are: the key is unknown, not wrong.
    pub async fn probe_key(&self) -> Result<(), LlmError> {
        let messages = [Message::user("hi")];
        let mut request = LlmRequest::new(ExecutionScope::Subagent, &[], &messages, &[]);
        // The smallest completion each API accepts.
        request.max_tokens = match self.provider {
            crate::config::LlmProvider::Anthropic => 1,
            crate::config::LlmProvider::Openai => 16,
        };
        match self.adapter()?.complete(request).await {
            Ok(_) => Ok(()),
            // Past authentication, whatever the provider then objected to.
            Err(
                LlmError::RateLimited { .. }
                | LlmError::ContextLength(_)
                | LlmError::InvalidResponse(_),
            ) => Ok(()),
            Err(LlmError::Http { status, .. }) if status < 500 => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Simple chat without tools. Returns (text, tokens_used).
    /// A subagent one-shot: its cache entries need only outlive the run.
    pub async fn chat(
        &self,
        system_prompt: &str,
        prompt: &str,
        history: Vec<Message>,
    ) -> anyhow::Result<(String, u64)> {
        let backend = self.clone();
        let system = system_prompt.to_string();
        let prompt = prompt.to_string();
        retry_on_rate_limit(|| {
            let backend = backend.clone();
            let system = system.clone();
            let prompt = prompt.clone();
            let history = history.clone();
            async move {
                let mut messages = history;
                messages.push(Message::user(&prompt));
                let adapter = backend.adapter()?;
                let system = [system.as_str()];
                let response = adapter
                    .complete(LlmRequest::new(
                        ExecutionScope::Subagent,
                        &system,
                        &messages,
                        &[],
                    ))
                    .await?;
                Ok((response.text, response.tokens_used))
            }
        })
        .await
    }

    /// Chat with structured JSON output.
    pub async fn chat_json(
        &self,
        system_prompt: &str,
        prompt: &str,
        schema: serde_json::Value,
    ) -> anyhow::Result<(String, u64)> {
        retry_on_rate_limit(|| async {
            let messages = [Message::user(prompt)];
            let system = [system_prompt];
            let mut request = LlmRequest::new(ExecutionScope::Subagent, &system, &messages, &[]);
            request.json_schema = Some(&schema);
            let response = self.adapter()?.complete(request).await?;
            Ok((response.text, response.tokens_used))
        })
        .await
    }

    /// Streaming chat with tools: the conversation loop, so its prompt cache
    /// outlives the pause between a person's turns (#137).
    pub async fn chat_with_tools_streaming(
        &self,
        system_prompt: &[&str],
        prompt: Message,
        history: Vec<Message>,
        tools: Vec<Box<dyn ToolDyn>>,
        events: broadcast::Sender<ServerEvent>,
        instance_slug: &str,
        chat_id: &str,
        workspace_dir: &Path,
        mcp_snapshot: Option<super::mcp::McpAppSnapshot>,
        sent_files: super::tools::SentFiles,
    ) -> anyhow::Result<ToolChatResult> {
        log::info!("chat_with_tools_streaming: {} tools", tools.len());

        let tool_defs = collect_tool_defs(&tools).await;
        let mut messages = history;
        if let Message::User { content } = prompt {
            messages.push(Message::User { content });
        }

        let result = streaming_agent_loop(
            self,
            ExecutionScope::Conversation,
            system_prompt,
            &tool_defs,
            &tools,
            &mut messages,
            &events,
            instance_slug,
            chat_id,
            workspace_dir,
            mcp_snapshot.as_ref(),
            &sent_files,
        )
        .await;

        match result {
            Ok((text, message_id, tokens_used)) => Ok(ToolChatResult {
                text,
                rig_history: Some(messages),
                message_id,
                tokens_used,
            }),
            Err(e) => Err(e),
        }
    }

    /// Simplified tool call (no streaming). Used by heartbeat.
    #[allow(dead_code)]
    pub async fn chat_with_tools_only(
        &self,
        system_prompt: &str,
        prompt: &str,
        history: Vec<Message>,
        tools: Vec<Box<dyn ToolDyn>>,
    ) -> anyhow::Result<(String, u64)> {
        if tools.is_empty() {
            return self.chat(system_prompt, prompt, history).await;
        }
        let system_blocks: &[&str] = &[system_prompt];

        let tool_defs = collect_tool_defs(&tools).await;
        let mut messages = history;
        messages.push(Message::user(prompt));

        agent_loop(
            self,
            ExecutionScope::Subagent,
            system_blocks,
            &tool_defs,
            &tools,
            &mut messages,
        )
        .await
    }

    /// Like `chat_with_tools_only` but returns the full message trace. The
    /// companion routines run here, in subagent scope.
    pub async fn chat_with_tools_traced(
        &self,
        system_prompt: &str,
        prompt: &str,
        history: Vec<Message>,
        tools: Vec<Box<dyn ToolDyn>>,
    ) -> anyhow::Result<(String, u64, Vec<Message>)> {
        if tools.is_empty() {
            let (text, tokens) = self.chat(system_prompt, prompt, history).await?;
            return Ok((text, tokens, vec![]));
        }
        let system_blocks: &[&str] = &[system_prompt];
        let tool_defs = collect_tool_defs(&tools).await;
        let mut messages = history;
        messages.push(Message::user(prompt));
        let (text, tokens) = agent_loop(
            self,
            ExecutionScope::Subagent,
            system_blocks,
            &tool_defs,
            &tools,
            &mut messages,
        )
        .await?;
        Ok((text, tokens, messages))
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn provider_error_outputs_never_echo_control_tokens() {
        const SECRET: &str = "issue116-provider-error-secret";
        crate::services::tools::register_control_secret(SECRET);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let app = axum::Router::new()
            .fallback(|| async { (axum::http::StatusCode::BAD_REQUEST, SECRET) });
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let http = reqwest::Client::new();
        let a = anthropic::anthropic_complete(
            &http,
            "provider-key",
            "model",
            &[],
            &[],
            &[],
            100,
            contract::ExecutionScope::Subagent,
            &base,
            None,
        )
        .await
        .err()
        .unwrap();
        let o = openai::openai_complete(
            &http,
            "provider-key",
            "model",
            &[],
            &[],
            &[],
            100,
            None,
            &base,
            None,
        )
        .await
        .err()
        .unwrap();
        let count = anthropic::anthropic_count_tokens(
            &http,
            "provider-key",
            "model",
            &[],
            &[],
            &[],
            contract::ExecutionScope::Subagent,
            &base,
        )
        .await
        .err()
        .unwrap();
        task.abort();
        assert!(!a.to_string().contains(SECRET));
        assert!(!o.to_string().contains(SECRET));
        assert!(!count.to_string().contains(SECRET));
    }

    #[test]
    fn provider_payloads_redact_seeded_control_tokens() {
        let secret = "issue116-provider-secret";
        crate::services::tools::register_control_secret(secret);
        let messages = vec![types::Message::user(format!(
            "saved historical URL https://example.test/file?token={secret}"
        ))];
        let anthropic = anthropic::build_anthropic_request(
            "model",
            &[secret],
            &[],
            &messages,
            100,
            contract::ExecutionScope::Subagent,
            false,
            "provider-key",
        );
        assert!(!anthropic.to_string().contains(secret));
        let (instructions, input) = openai::messages_to_openai(&[secret], &messages);
        assert!(!instructions.contains(secret));
        assert!(!serde_json::to_string(&input).unwrap().contains(secret));
    }

    use super::*;
    use crate::config::LlmProvider;
    use crate::services::tool::ToolDefinition;
    use types::Message;

    // ── Model selection per provider ─────────────────────────────────────

    // ── Backend construction ─────────────────────────────────────────────

    /// Model id of a seeded preset, by the tier name the old profiles used.
    fn seed(provider: LlmProvider, tier: &str) -> String {
        let id = match (provider, tier) {
            (LlmProvider::Anthropic, "heavy") => "opus",
            (LlmProvider::Anthropic, "fast") => "sonnet",
            (LlmProvider::Anthropic, _) => "haiku",
            (LlmProvider::Openai, "cheap") => "gpt-mini",
            (LlmProvider::Openai, _) => "gpt",
        };
        crate::config::default_presets(provider)
            .into_iter()
            .find(|preset| preset.id == id)
            .unwrap()
            .model
    }

    fn keyed_config(provider: LlmProvider) -> Config {
        let mut config = Config::default();
        config.llm.seed_presets(provider);
        config.llm.tokens.anthropic = "test-key".into();
        config.llm.tokens.open_ai = "test-key".into();
        config
    }

    fn make_backend(provider: LlmProvider) -> LlmBackend {
        let id = match provider {
            LlmProvider::Anthropic => "sonnet",
            LlmProvider::Openai => "gpt",
        };
        LlmBackend::for_preset(&keyed_config(provider), reqwest::Client::new(), id).unwrap()
    }

    #[test]
    fn seeded_presets_name_real_models_per_provider() {
        for preset in crate::config::default_presets(LlmProvider::Anthropic) {
            assert!(preset.model.starts_with("claude-"), "{preset:?}");
        }
        for preset in crate::config::default_presets(LlmProvider::Openai) {
            assert!(preset.model.starts_with("gpt-"), "{preset:?}");
        }
    }

    #[test]
    fn backend_api_points_to_anthropic() {
        let b = make_backend(LlmProvider::Anthropic);
        assert_eq!(b.base_url, "https://api.anthropic.com");
        assert!(b.model.starts_with("claude-"));
        assert_eq!(b.preset, "sonnet");
    }

    #[test]
    fn backend_openai_points_to_openai() {
        let b = make_backend(LlmProvider::Openai);
        assert_eq!(b.base_url, "https://api.openai.com");
        assert!(b.model.starts_with("gpt-"));
    }

    // ── Presets (#156) ───────────────────────────────────────────────────

    #[test]
    fn for_preset_reports_unknown_ids_and_missing_keys() {
        let mut config = keyed_config(LlmProvider::Anthropic);
        config.llm.seed_presets(LlmProvider::Openai);
        config.llm.tokens.open_ai.clear();
        let http = reqwest::Client::new();
        assert_eq!(
            LlmBackend::for_preset(&config, http.clone(), "nope").err(),
            Some(PresetError::Unknown("nope".into()))
        );
        assert_eq!(
            LlmBackend::for_preset(&config, http.clone(), "gpt").err(),
            Some(PresetError::MissingKey(LlmProvider::Openai))
        );
        let opus = LlmBackend::for_preset(&config, http, "opus").unwrap();
        assert_eq!(opus.model, "claude-opus-4-6");
        assert_eq!(opus.provider, LlmProvider::Anthropic);
        assert_eq!(opus.api_key, "test-key");
    }

    #[test]
    fn probe_model_prefers_the_chat_preset_of_that_provider() {
        let mut config = keyed_config(LlmProvider::Anthropic);
        assert_eq!(
            probe_model(&config.llm, LlmProvider::Anthropic),
            "claude-sonnet-4-6"
        );
        // A first key: no preset for the provider yet, so its default chat model.
        assert_eq!(probe_model(&config.llm, LlmProvider::Openai), "gpt-5.4");
        // A preset for the provider that is not the chat slot.
        config.llm.presets.push(crate::config::ModelPreset {
            id: "custom".into(),
            name: "Custom".into(),
            provider: LlmProvider::Openai,
            model: "gpt-custom".into(),
        });
        assert_eq!(probe_model(&config.llm, LlmProvider::Openai), "gpt-custom");
        // The chat slot wins over other presets of the same provider.
        config.llm.chat_preset = "opus".into();
        assert_eq!(
            probe_model(&config.llm, LlmProvider::Anthropic),
            "claude-opus-4-6"
        );
    }

    #[test]
    fn chat_and_background_builders_follow_their_slots_independently() {
        let mut config = keyed_config(LlmProvider::Anthropic);
        config.llm.seed_presets(LlmProvider::Openai);
        config.llm.chat_preset = "gpt".into();
        config.llm.background_preset = "haiku".into();
        let chat = LlmBackend::from_config(&config).unwrap();
        assert_eq!(chat.provider, LlmProvider::Openai);
        assert_eq!(chat.model, "gpt-5.4");
        let background = LlmBackend::background(&config).unwrap();
        assert_eq!(background.provider, LlmProvider::Anthropic);
        assert_eq!(background.model, "claude-haiku-4-5-20251001");

        // A dangling background slot yields no backend rather than the chat one.
        config.llm.background_preset = "gone".into();
        assert!(LlmBackend::background(&config).is_none());
        assert!(LlmBackend::from_config(&config).is_some());
    }

    // ── OpenAI Responses API message conversion ────────────────────────

    #[test]
    fn openai_messages_separate_instructions() {
        let msgs = vec![Message::user("hello")];
        let (instructions, input) = openai::messages_to_openai(&["You are helpful."], &msgs);
        assert_eq!(instructions, "You are helpful.");
        assert_eq!(input[0]["type"], "message");
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[0]["content"], "hello");
    }

    #[test]
    fn openai_messages_skip_empty_system() {
        let msgs = vec![Message::user("hi")];
        let (instructions, input) = openai::messages_to_openai(&[], &msgs);
        assert!(instructions.is_empty());
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["role"], "user");
    }

    #[test]
    fn openai_messages_join_multiple_system_blocks() {
        let msgs = vec![Message::user("test")];
        let (instructions, _input) = openai::messages_to_openai(&["block1", "block2"], &msgs);
        assert_eq!(instructions, "block1\n\nblock2");
    }

    #[test]
    fn openai_messages_convert_tool_use_to_function_call() {
        let msgs = vec![Message::Assistant {
            content: vec![
                types::ContentBlock::Text {
                    text: "Let me search.".into(),
                },
                types::ContentBlock::ToolCall {
                    id: "call_1".into(),
                    name: "web_search".into(),
                    arguments: serde_json::json!({"query": "rust"}),
                },
            ],
        }];
        let (_instructions, input) = openai::messages_to_openai(&[], &msgs);
        // Text becomes a message item
        assert_eq!(input[0]["type"], "message");
        assert_eq!(input[0]["role"], "assistant");
        assert_eq!(input[0]["content"], "Let me search.");
        // Tool use becomes a function_call item
        assert_eq!(input[1]["type"], "function_call");
        assert_eq!(input[1]["call_id"], "call_1");
        assert_eq!(input[1]["name"], "web_search");
    }

    #[test]
    fn openai_messages_convert_tool_result() {
        let msgs = vec![Message::User {
            content: vec![types::ContentBlock::ToolOutput {
                call_id: "call_1".into(),
                content: types::ToolOutputContent::Text("result text".into()),
            }],
        }];
        let (_instructions, input) = openai::messages_to_openai(&[], &msgs);
        assert_eq!(input[0]["type"], "function_call_output");
        assert_eq!(input[0]["call_id"], "call_1");
        assert_eq!(input[0]["output"], "result text");
    }

    // ── OpenAI Responses API tool conversion ────────────────────────────

    #[test]
    fn openai_tools_use_function_format() {
        let defs = vec![ToolDefinition {
            name: "get_weather".into(),
            description: "Get weather for a city".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": { "city": { "type": "string" } }
            }),
        }];
        let oai = openai::tools_to_openai(&defs, false);
        assert_eq!(oai.len(), 1);
        assert_eq!(oai[0]["type"], "function");
        assert_eq!(oai[0]["name"], "get_weather");
        assert_eq!(oai[0]["description"], "Get weather for a city");
        assert!(oai[0]["parameters"]["properties"]["city"].is_object());
    }

    #[test]
    fn openai_streaming_tools_include_web_search() {
        let defs = vec![ToolDefinition {
            name: "my_tool".into(),
            description: "test".into(),
            parameters: serde_json::json!({"type": "object"}),
        }];
        let oai = openai::tools_to_openai(&defs, true);
        assert_eq!(oai.len(), 2); // my_tool + web_search
        assert_eq!(oai[1]["type"], "web_search");
    }

    // ── Anthropic request building ───────────────────────────────────────

    #[test]
    fn anthropic_request_uses_max_tokens() {
        let msgs = vec![Message::user("hi")];
        let req = anthropic::build_anthropic_request(
            "claude-sonnet-4-6",
            &["system prompt"],
            &[],
            &msgs,
            4096,
            contract::ExecutionScope::Subagent,
            false,
            "key",
        );
        assert_eq!(req["max_tokens"], 4096);
        // Anthropic should NOT have max_completion_tokens
        assert!(req.get("max_completion_tokens").is_none());
    }

    #[test]
    fn anthropic_request_has_system_blocks_with_cache_control() {
        let msgs = vec![Message::user("hi")];
        let req = anthropic::build_anthropic_request(
            "claude-sonnet-4-6",
            &["block1", "block2"],
            &[],
            &msgs,
            4096,
            contract::ExecutionScope::Subagent,
            false,
            "key",
        );
        let system = req["system"].as_array().unwrap();
        assert_eq!(system.len(), 2);
        for block in system {
            assert_eq!(block["type"], "text");
            assert_eq!(block["cache_control"]["type"], "ephemeral");
        }
        assert_eq!(system[0]["text"], "block1");
        assert_eq!(system[1]["text"], "block2");
    }

    #[test]
    fn anthropic_request_skips_empty_system_blocks() {
        let msgs = vec![Message::user("hi")];
        let req = anthropic::build_anthropic_request(
            "claude-sonnet-4-6",
            &["", "actual content", ""],
            &[],
            &msgs,
            4096,
            contract::ExecutionScope::Subagent,
            false,
            "key",
        );
        let system = req["system"].as_array().unwrap();
        assert_eq!(system.len(), 1);
        assert_eq!(system[0]["text"], "actual content");
    }

    #[test]
    fn anthropic_request_tools_use_input_schema() {
        let tools = vec![ToolDefinition {
            name: "search".into(),
            description: "Search the web".into(),
            parameters: serde_json::json!({"type": "object"}),
        }];
        let msgs = vec![Message::user("hi")];
        let req = anthropic::build_anthropic_request(
            "claude-sonnet-4-6",
            &["sys"],
            &tools,
            &msgs,
            4096,
            contract::ExecutionScope::Subagent,
            false,
            "key",
        );
        let t = &req["tools"][0];
        assert_eq!(t["name"], "search");
        assert_eq!(t["input_schema"]["type"], "object");
        // Last tool gets cache_control
        assert_eq!(t["cache_control"]["type"], "ephemeral");
    }

    // ── Prompt-cache TTL per execution scope (#137) ──────────────────────

    /// Every `cache_control` object anywhere in a request body.
    fn cache_controls(value: &serde_json::Value) -> Vec<&serde_json::Value> {
        fn walk<'a>(value: &'a serde_json::Value, out: &mut Vec<&'a serde_json::Value>) {
            match value {
                serde_json::Value::Object(map) => {
                    for (key, child) in map {
                        if key == "cache_control" {
                            out.push(child);
                        } else {
                            walk(child, out);
                        }
                    }
                }
                serde_json::Value::Array(items) => items.iter().for_each(|item| walk(item, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        walk(value, &mut out);
        out
    }

    /// Builds a streaming chat request with two system blocks and two tools,
    /// then checks each breakpoint the adapter places: both system blocks,
    /// the last tool, and the top-level field. Nothing else may carry one,
    /// and none may omit the ttl (the API would silently default to 5m).
    fn assert_every_breakpoint_uses(scope: contract::ExecutionScope, ttl: &str) {
        let tool = |name: &str| ToolDefinition {
            name: name.into(),
            description: "test".into(),
            parameters: serde_json::json!({"type": "object"}),
        };
        let tools = vec![tool("first"), tool("last")];
        let msgs = vec![Message::user("hi")];
        let req = anthropic::build_anthropic_request(
            "claude-sonnet-4-6",
            &["block1", "block2"],
            &tools,
            &msgs,
            4096,
            scope,
            true,
            "key",
        );
        let expected = serde_json::json!({"type": "ephemeral", "ttl": ttl});
        assert_eq!(req["cache_control"], expected, "top-level breakpoint");
        for block in req["system"].as_array().unwrap() {
            assert_eq!(
                block["cache_control"], expected,
                "system block {}",
                block["text"]
            );
        }
        let tools = req["tools"].as_array().unwrap();
        assert!(
            tools[0].get("cache_control").is_none(),
            "only the last tool is a breakpoint"
        );
        assert_eq!(tools[1]["cache_control"], expected, "last tool breakpoint");
        let all = cache_controls(&req);
        assert_eq!(
            all.len(),
            4,
            "two system blocks, last tool, top level: {all:?}"
        );
        assert!(
            all.iter().all(|control| **control == expected),
            "every breakpoint must be {expected}: {all:?}"
        );
    }

    #[test]
    fn execution_scopes_map_to_distinct_cache_ttls() {
        use contract::ExecutionScope;
        assert_eq!(ExecutionScope::Conversation.cache_ttl(), "1h");
        assert_eq!(ExecutionScope::Subagent.cache_ttl(), "5m");
        assert_ne!(
            ExecutionScope::Conversation.cache_ttl(),
            ExecutionScope::Subagent.cache_ttl(),
            "conversation and subagent runs must not share one cache TTL"
        );
    }

    #[test]
    fn anthropic_conversation_requests_cache_for_one_hour() {
        assert_every_breakpoint_uses(contract::ExecutionScope::Conversation, "1h");
    }

    #[test]
    fn anthropic_subagent_requests_cache_for_five_minutes() {
        assert_every_breakpoint_uses(contract::ExecutionScope::Subagent, "5m");
    }

    #[test]
    fn anthropic_streaming_adds_server_tools() {
        let tools = vec![ToolDefinition {
            name: "my_tool".into(),
            description: "test".into(),
            parameters: serde_json::json!({"type": "object"}),
        }];
        let msgs = vec![Message::user("hi")];
        let req = anthropic::build_anthropic_request(
            "claude-sonnet-4-6",
            &["sys"],
            &tools,
            &msgs,
            4096,
            contract::ExecutionScope::Subagent,
            true,
            "key",
        );
        let all_tools = req["tools"].as_array().unwrap();
        // my_tool + web_search + web_fetch
        assert_eq!(all_tools.len(), 3);
        let names: Vec<&str> = all_tools
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"my_tool"));
        assert!(names.contains(&"web_search"));
        assert!(names.contains(&"web_fetch"));
    }

    #[test]
    fn anthropic_non_streaming_no_server_tools() {
        let tools = vec![ToolDefinition {
            name: "my_tool".into(),
            description: "test".into(),
            parameters: serde_json::json!({"type": "object"}),
        }];
        let msgs = vec![Message::user("hi")];
        let req = anthropic::build_anthropic_request(
            "claude-sonnet-4-6",
            &["sys"],
            &tools,
            &msgs,
            4096,
            contract::ExecutionScope::Subagent,
            false,
            "key",
        );
        let all_tools = req["tools"].as_array().unwrap();
        assert_eq!(all_tools.len(), 1);
        assert_eq!(all_tools[0]["name"], "my_tool");
    }

    #[test]
    fn anthropic_request_merges_consecutive_same_role() {
        // Two consecutive user messages should get merged
        let msgs = vec![Message::user("first"), Message::user("second")];
        let req = anthropic::build_anthropic_request(
            "claude-sonnet-4-6",
            &["sys"],
            &[],
            &msgs,
            4096,
            contract::ExecutionScope::Subagent,
            false,
            "key",
        );
        let api_msgs = req["messages"].as_array().unwrap();
        assert_eq!(
            api_msgs.len(),
            1,
            "consecutive same-role messages should merge"
        );
        let content = api_msgs[0]["content"].as_array().unwrap();
        assert_eq!(
            content.len(),
            2,
            "merged message should have 2 content blocks"
        );
    }

    // ── Anthropic headers ────────────────────────────────────────────────

    #[test]
    fn anthropic_headers_include_required_fields() {
        let h = anthropic::anthropic_headers("test-api-key").unwrap();
        assert_eq!(h.get("x-api-key").unwrap(), "test-api-key");
        assert!(h.get("anthropic-version").is_some());
        assert!(h.get("anthropic-beta").is_some());
        assert_eq!(h.get("content-type").unwrap(), "application/json");
    }

    // ── Cross-provider consistency ───────────────────────────────────────

    #[test]
    fn all_providers_have_distinct_model_tiers() {
        for provider in [LlmProvider::Anthropic, LlmProvider::Openai] {
            let heavy = seed(provider, "heavy");
            let cheap = seed(provider, "cheap");
            assert_ne!(heavy, cheap, "{provider:?}: heavy and cheap should differ");
        }
    }

    #[test]
    fn providers_use_official_urls() {
        let api = make_backend(LlmProvider::Anthropic);
        assert_eq!(api.base_url, ANTHROPIC_BASE_URL);
        let oai = make_backend(LlmProvider::Openai);
        assert_eq!(oai.base_url, OPENAI_BASE_URL);
    }

    // ── Content block helpers ────────────────────────────────────────────

    #[test]
    fn content_block_text_helper() {
        let block = types::ContentBlock::text("hello");
        match block {
            types::ContentBlock::Text { text } => assert_eq!(text, "hello"),
            _ => panic!("expected Text block"),
        }
    }

    #[test]
    fn tool_result_unwraps_json_string_quoting() {
        // serde_json::to_string wraps strings in quotes: "foo" -> "\"foo\""
        let block = types::ContentBlock::tool_output("id1".into(), "\"hello world\"".into(), false);
        match block {
            types::ContentBlock::ToolOutput { content, .. } => {
                assert_eq!(content.as_str(), Some("hello world"));
            }
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn tool_result_passes_content_block_arrays_directly() {
        let json_blocks = r#"[{"type":"text","text":"result"}]"#;
        let block = types::ContentBlock::tool_output("id1".into(), json_blocks.into(), false);
        match block {
            types::ContentBlock::ToolOutput { content, .. } => {
                assert!(matches!(content, types::ToolOutputContent::Blocks(_)));
                assert_eq!(serde_json::to_value(content).unwrap()[0]["type"], "text");
            }
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn tool_result_preserves_ordinary_json_arrays_as_text() {
        for json in [
            r#"[{"title":"result"}]"#,
            r#"[{"type":"expense","amount":1}]"#,
            r#"[{"type":"text"}]"#,
            r#"[{"type":"text","text":"ok"},{"title":"result"}]"#,
            r#"[{"type":"text","text":"memo","amount":1}]"#,
            r#"[{"type":"image","source":{"type":"url","url":"https://example.com/a.png","tracking":"x"}}]"#,
            r#"[]"#,
        ] {
            let block = types::ContentBlock::tool_output("id1".into(), json.into(), false);
            match block {
                types::ContentBlock::ToolOutput { content, .. } => {
                    assert!(
                        matches!(content, types::ToolOutputContent::Text(ref value) if value == json)
                    );
                }
                _ => panic!("expected ToolOutput"),
            }
        }
    }

    #[test]
    fn persisted_tool_output_does_not_drop_domain_fields() {
        let json = r#"[{"type":"text","text":"memo","amount":1}]"#;
        let content: types::ToolOutputContent = serde_json::from_str(json).unwrap();
        assert!(matches!(content, types::ToolOutputContent::Legacy(_)));
        assert_eq!(
            serde_json::to_value(content).unwrap(),
            serde_json::from_str::<serde_json::Value>(json).unwrap()
        );
    }

    #[test]
    fn untrusted_tool_output_cannot_persist_guessed_resource_provenance() {
        let json = r#"[{"type":"image","source":{"type":"url","url":"https://attacker.invalid/resources/model-provider/files/moon/guessed?cap=forged"},"resource_provenance":{"kind":"uploaded_file","version":1,"slug":"moon","id":"guessed"}}]"#;
        let block = types::ContentBlock::tool_output("malicious".into(), json.into(), false);
        let types::ContentBlock::ToolOutput {
            content: types::ToolOutputContent::Blocks(blocks),
            ..
        } = block
        else {
            panic!("useful multimodal block should remain structured");
        };
        assert!(matches!(
            &blocks[0],
            types::ContentBlock::Image {
                resource_provenance: None,
                ..
            }
        ));
    }

    #[test]
    fn trusted_tool_output_preserves_resource_provenance() {
        let json = r#"[{"type":"image","source":{"type":"url","url":"https://self.test/resources/model-provider/files/moon/upload-1?cap=signed"},"resource_provenance":{"kind":"uploaded_file","version":1,"slug":"moon","id":"upload-1"}}]"#;
        let block = types::ContentBlock::tool_output("trusted".into(), json.into(), true);
        let types::ContentBlock::ToolOutput {
            content: types::ToolOutputContent::Blocks(blocks),
            ..
        } = block
        else {
            panic!("expected structured blocks");
        };
        assert!(matches!(
            &blocks[0],
            types::ContentBlock::Image {
                resource_provenance: Some(types::ResourceProvenance::UploadedFile { id, .. }),
                ..
            } if id == "upload-1"
        ));
    }

    // ═════════════════════════════════════════════════════════════════════
    // Network integration tests — hit real APIs
    // Skipped when the corresponding env var is missing.
    // Run with: ANTHROPIC_API_KEY=... OPENAI_API_KEY=... cargo test -- --ignored
    // ═════════════════════════════════════════════════════════════════════

    fn anthropic_backend(model: &str) -> Option<LlmBackend> {
        let key = std::env::var("ANTHROPIC_API_KEY")
            .ok()
            .filter(|k| !k.is_empty())?;
        Some(LlmBackend {
            preset: "network".into(),
            http: reqwest::Client::new(),
            api_key: key,
            model: model.to_string(),
            base_url: ANTHROPIC_BASE_URL.to_string(),
            provider: LlmProvider::Anthropic,
        })
    }

    fn openai_backend(model: &str) -> Option<LlmBackend> {
        let key = std::env::var("OPENAI_API_KEY")
            .ok()
            .filter(|k| !k.is_empty())?;
        Some(LlmBackend {
            preset: "network".into(),
            http: reqwest::Client::new(),
            api_key: key,
            model: model.to_string(),
            base_url: OPENAI_BASE_URL.to_string(),
            provider: LlmProvider::Openai,
        })
    }

    // ── Anthropic (direct API) ───────────────────────────────────────

    #[tokio::test]
    #[ignore] // requires ANTHROPIC_API_KEY
    async fn network_anthropic_haiku_chat() {
        let Some(b) = anthropic_backend(&seed(LlmProvider::Anthropic, "cheap")) else {
            eprintln!("SKIP: ANTHROPIC_API_KEY not set");
            return;
        };
        let (text, tokens) = b
            .chat("Reply with exactly one word: hello", "say it", vec![])
            .await
            .unwrap();
        assert!(!text.is_empty(), "expected non-empty response");
        assert!(tokens > 0, "expected token usage > 0");
    }

    #[tokio::test]
    #[ignore]
    async fn network_anthropic_sonnet_chat() {
        let Some(b) = anthropic_backend(&seed(LlmProvider::Anthropic, "fast")) else {
            eprintln!("SKIP: ANTHROPIC_API_KEY not set");
            return;
        };
        let (text, tokens) = b
            .chat("Reply with exactly one word: pong", "ping", vec![])
            .await
            .unwrap();
        assert!(!text.is_empty());
        assert!(tokens > 0);
    }

    #[tokio::test]
    #[ignore]
    async fn network_anthropic_chat_with_history() {
        let Some(b) = anthropic_backend(&seed(LlmProvider::Anthropic, "cheap")) else {
            eprintln!("SKIP: ANTHROPIC_API_KEY not set");
            return;
        };
        let history = vec![
            Message::user("My name is TestBot."),
            Message::assistant("Nice to meet you, TestBot!"),
        ];
        let (text, _) = b
            .chat(
                "You remember names. Reply with the user's name only.",
                "What's my name?",
                history,
            )
            .await
            .unwrap();
        let lower = text.to_lowercase();
        assert!(
            lower.contains("testbot"),
            "expected model to recall name, got: {text}"
        );
    }

    #[tokio::test]
    #[ignore]
    async fn network_anthropic_json_output() {
        let Some(b) = anthropic_backend(&seed(LlmProvider::Anthropic, "cheap")) else {
            eprintln!("SKIP: ANTHROPIC_API_KEY not set");
            return;
        };
        let schema = serde_json::json!({
            "type": "object",
            "properties": { "color": { "type": "string" } },
            "required": ["color"]
        });
        let (text, _) = b
            .chat_json(
                "Return JSON with a color field.",
                "What color is the sky?",
                schema,
            )
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&text).expect("should be valid JSON");
        assert!(
            parsed["color"].is_string(),
            "expected color field, got: {text}"
        );
    }

    // ── OpenAI (direct API) ──────────────────────────────────────────

    #[tokio::test]
    #[ignore] // requires OPENAI_API_KEY
    async fn network_openai_mini_chat() {
        let Some(b) = openai_backend(&seed(LlmProvider::Openai, "cheap")) else {
            eprintln!("SKIP: OPENAI_API_KEY not set");
            return;
        };
        let (text, tokens) = b
            .chat("Reply with exactly one word: hello", "say it", vec![])
            .await
            .unwrap();
        assert!(!text.is_empty(), "expected non-empty response");
        assert!(tokens > 0, "expected token usage > 0");
    }

    #[tokio::test]
    #[ignore]
    async fn network_openai_heavy_chat() {
        let Some(b) = openai_backend(&seed(LlmProvider::Openai, "heavy")) else {
            eprintln!("SKIP: OPENAI_API_KEY not set");
            return;
        };
        let (text, tokens) = b
            .chat("Reply with exactly one word: pong", "ping", vec![])
            .await
            .unwrap();
        assert!(!text.is_empty());
        assert!(tokens > 0);
    }

    #[tokio::test]
    #[ignore]
    async fn network_openai_chat_with_history() {
        let Some(b) = openai_backend(&seed(LlmProvider::Openai, "cheap")) else {
            eprintln!("SKIP: OPENAI_API_KEY not set");
            return;
        };
        let history = vec![
            Message::user("My name is TestBot."),
            Message::assistant("Nice to meet you, TestBot!"),
        ];
        let (text, _) = b
            .chat(
                "You remember names. Reply with the user's name only.",
                "What's my name?",
                history,
            )
            .await
            .unwrap();
        let lower = text.to_lowercase();
        assert!(
            lower.contains("testbot"),
            "expected model to recall name, got: {text}"
        );
    }

    #[tokio::test]
    #[ignore]
    async fn network_openai_json_output() {
        let Some(b) = openai_backend(&seed(LlmProvider::Openai, "cheap")) else {
            eprintln!("SKIP: OPENAI_API_KEY not set");
            return;
        };
        let schema = serde_json::json!({
            "type": "object",
            "properties": { "color": { "type": "string" } },
            "required": ["color"]
        });
        let (text, _) = b
            .chat_json(
                "Return JSON with a color field.",
                "What color is the sky?",
                schema,
            )
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&text).expect("should be valid JSON");
        assert!(
            parsed["color"].is_string(),
            "expected color field, got: {text}"
        );
    }

    #[tokio::test]
    #[ignore]
    async fn network_openai_max_completion_tokens_accepted() {
        // Regression test: gpt-5.x rejects max_tokens, requires max_completion_tokens
        let Some(b) = openai_backend(&seed(LlmProvider::Openai, "heavy")) else {
            eprintln!("SKIP: OPENAI_API_KEY not set");
            return;
        };
        let result = b.chat("Reply with one word.", "hi", vec![]).await;
        assert!(
            result.is_ok(),
            "gpt-5.x should accept max_completion_tokens: {}",
            result.unwrap_err()
        );
    }

    // ── Cross-provider: same prompt, both formats ─���───────���──────────

    #[tokio::test]
    #[ignore] // requires both ANTHROPIC_API_KEY and OPENAI_API_KEY
    async fn network_cross_provider_same_prompt() {
        let anthropic = anthropic_backend(&seed(LlmProvider::Anthropic, "cheap"));
        let openai = openai_backend(&seed(LlmProvider::Openai, "cheap"));
        if anthropic.is_none() || openai.is_none() {
            eprintln!("SKIP: need both ANTHROPIC_API_KEY and OPENAI_API_KEY");
            return;
        }
        let prompt = "What is 2+2? Reply with just the number.";
        let (a_text, _) = anthropic
            .unwrap()
            .chat("Answer math questions.", prompt, vec![])
            .await
            .unwrap();
        let (o_text, _) = openai
            .unwrap()
            .chat("Answer math questions.", prompt, vec![])
            .await
            .unwrap();
        assert!(
            a_text.contains('4'),
            "anthropic should answer 4, got: {a_text}"
        );
        assert!(
            o_text.contains('4'),
            "openai should answer 4, got: {o_text}"
        );
    }
}
