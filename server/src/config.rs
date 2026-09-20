use std::{
    env, fs, io,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub auth_token: String,
    #[serde(default)]
    pub static_dir: String,
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub embedding: EmbeddingConfig,
    #[serde(default = "default_registry_url")]
    pub registry_url: String,
    #[serde(default)]
    pub public_url: String,
    #[serde(default)]
    pub mcp_servers: Vec<McpServerConfig>,
    #[serde(default)]
    pub github: GithubConfig,
}

/// Independent text embedding configuration. Credentials stay in llm.tokens.OPEN_AI.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct EmbeddingConfig {
    pub version: u32,
    pub enabled: bool,
    /// `openai` or an explicitly configured `openai_compatible` backend.
    pub provider: String,
    pub model: String,
    pub dimensions: u32,
    pub base_url: String,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            version: 1,
            enabled: true,
            provider: "openai".into(),
            model: "text-embedding-3-small".into(),
            dimensions: 768,
            base_url: "https://api.openai.com/v1".into(),
        }
    }
}

impl EmbeddingConfig {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.version != 1 {
            return Err("unsupported embedding config version");
        }
        if !matches!(self.provider.as_str(), "openai" | "openai_compatible") {
            return Err("embedding provider is unconfigured or unsupported");
        }
        if self.model.trim().is_empty() || self.model.len() > 1024 {
            return Err("invalid embedding model");
        }
        if self.dimensions == 0 || self.dimensions > 16384 {
            return Err("invalid embedding dimensions");
        }
        let url = self.endpoint()?;
        if self.provider == "openai" {
            if self.base_url != "https://api.openai.com/v1" {
                return Err("OpenAI embeddings require https://api.openai.com/v1 exactly");
            }
        } else {
            let host = url.host_str().unwrap_or_default();
            let ip_literal = host
                .strip_prefix('[')
                .and_then(|host| host.strip_suffix(']'))
                .unwrap_or(host);
            let loopback = host.eq_ignore_ascii_case("localhost")
                || ip_literal
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|address| address.is_loopback());
            if !loopback {
                return Err("OpenAI-compatible embeddings require a loopback endpoint");
            }
        }
        Ok(())
    }

    fn endpoint(&self) -> Result<reqwest::Url, &'static str> {
        let url = reqwest::Url::parse(&self.base_url).map_err(|_| "invalid embedding base URL")?;
        if !matches!(url.scheme(), "https" | "http")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("embedding base URL must not contain credentials, query, or fragment");
        }
        Ok(url)
    }

    pub fn unavailable_reason(&self, key: &str) -> Option<&'static str> {
        if !self.enabled {
            return Some("embeddings disabled");
        }
        if let Err(reason) = self.validate() {
            return Some(reason);
        }
        if self.provider == "openai" && key.trim().is_empty() {
            return Some("OpenAI embedding API key is missing");
        }
        None
    }

    /// Never return credentials accidentally placed in a URL.
    pub fn safe_status(&self, key: &str) -> serde_json::Value {
        let reason = self.unavailable_reason(key);
        serde_json::json!({
            "version": self.version, "enabled": self.enabled,
            "provider": self.provider, "model": self.model, "dimensions": self.dimensions,
            "base_url": self.endpoint().ok().map(|url| url.to_string()),
            "authentication": if self.provider == "openai" { "OpenAI bearer key" } else { "none" },
            "configured": reason.is_none(), "reason": reason,
            "status": if reason.is_some() { "unavailable" } else { "unverified" },
            "fallback": "bm25", "changes_require_restart": true,
            "update_semantics": "full_replacement",
        })
    }
}

impl Config {
    pub fn embedding_status(&self) -> serde_json::Value {
        self.embedding.safe_status(&self.llm.tokens.open_ai)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct GithubConfig {
    #[serde(default)]
    pub token: String,
}

/// A single SMTP/IMAP email account.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct EmailConfig {
    #[serde(default)]
    pub smtp_host: String,
    #[serde(default = "default_smtp_port")]
    pub smtp_port: u16,
    #[serde(default)]
    pub smtp_user: String,
    #[serde(default)]
    pub smtp_password: String,
    #[serde(default)]
    pub smtp_from: String,
    #[serde(default)]
    pub imap_host: String,
    #[serde(default = "default_imap_port")]
    pub imap_port: u16,
    #[serde(default)]
    pub imap_user: String,
    #[serde(default)]
    pub imap_password: String,
}

fn default_smtp_port() -> u16 {
    587
}
fn default_imap_port() -> u16 {
    993
}

/// Per-instance configuration stored at `instances/{slug}/instance.toml`.
/// Holds settings that are specific to one user/instance, such as GitHub token.
/// Takes precedence over global `config.toml` for the same fields.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InstanceConfig {
    #[serde(default)]
    pub github: GithubConfig,
    /// ElevenLabs voice ID override for this instance.
    #[serde(default)]
    pub elevenlabs_voice_id: String,
    /// Whether voice mode (TTS) is enabled. Default: false.
    #[serde(default)]
    pub voice_enabled: bool,
    /// Visual skin for this instance. Default: "moon" (Little Moon).
    #[serde(default = "default_skin")]
    pub skin: String,
    /// Whether the companion keeps a bounded interaction-rhythm aggregate
    /// (peak hours, response pace) to time proactive behavior. Default: true.
    #[serde(default = "default_true")]
    pub rhythm_tracking: bool,
}

fn default_true() -> bool {
    true
}

fn default_skin() -> String {
    "moon".to_string()
}

impl Default for InstanceConfig {
    fn default() -> Self {
        Self {
            github: GithubConfig::default(),
            elevenlabs_voice_id: String::new(),
            voice_enabled: false,
            skin: default_skin(),
            rhythm_tracking: true,
        }
    }
}

impl InstanceConfig {
    /// Load per-instance config from `instances/{slug}/instance.toml`.
    /// Returns default (empty) config if the file doesn't exist.
    pub fn load(workspace_dir: &Path, instance_slug: &str) -> Self {
        let path = workspace_dir
            .join("instances")
            .join(instance_slug)
            .join("instance.toml");
        let raw = match fs::read_to_string(&path) {
            Ok(r) => r,
            Err(_) => return Self::default(),
        };
        toml::from_str::<InstanceConfig>(&raw).unwrap_or_default()
    }

    /// Save per-instance config to `instances/{slug}/instance.toml`.
    pub fn save(&self, workspace_dir: &Path, instance_slug: &str) -> anyhow::Result<()> {
        let dir = workspace_dir.join("instances").join(instance_slug);
        fs::create_dir_all(&dir)?;
        let raw = toml::to_string_pretty(self)?;
        fs::write(dir.join("instance.toml"), raw)?;
        Ok(())
    }

    /// Return the effective GitHub token: instance-level if set, otherwise fall back to global.
    #[allow(dead_code)]
    pub fn effective_github_token<'a>(&'a self, global: &'a Config) -> Option<&'a str> {
        if !self.github.token.is_empty() {
            return Some(&self.github.token);
        }
        if !global.github.token.is_empty() {
            return Some(&global.github.token);
        }
        None
    }
}

/// Wrapper for `instances/{slug}/email.toml` — supports multiple accounts.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct EmailAccounts {
    #[serde(default)]
    pub accounts: Vec<EmailConfig>,
}

impl EmailAccounts {
    /// Load email accounts from `instances/{slug}/email.toml`.
    /// Supports both legacy flat format (single account) and `[[accounts]]` array.
    pub fn load(workspace_dir: &Path, instance_slug: &str) -> Vec<EmailConfig> {
        let path = workspace_dir
            .join("instances")
            .join(instance_slug)
            .join("email.toml");
        let raw = match fs::read_to_string(&path) {
            Ok(r) => r,
            Err(_) => return vec![],
        };

        // Try new format first: [[accounts]]
        if let Ok(wrapper) = toml::from_str::<EmailAccounts>(&raw) {
            if !wrapper.accounts.is_empty() {
                return wrapper
                    .accounts
                    .into_iter()
                    .filter(|c| !c.smtp_host.is_empty() || !c.imap_host.is_empty())
                    .collect();
            }
        }

        // Fall back to legacy flat format (single account)
        if let Ok(cfg) = toml::from_str::<EmailConfig>(&raw) {
            if !cfg.smtp_host.is_empty() || !cfg.imap_host.is_empty() {
                return vec![cfg];
            }
        }

        vec![]
    }

    /// Save email accounts to `instances/{slug}/email.toml`.
    pub fn save(
        accounts: &[EmailConfig],
        workspace_dir: &Path,
        instance_slug: &str,
    ) -> anyhow::Result<()> {
        let dir = workspace_dir.join("instances").join(instance_slug);
        fs::create_dir_all(&dir)?;
        let wrapper = EmailAccounts {
            accounts: accounts.to_vec(),
        };
        let raw = toml::to_string_pretty(&wrapper)?;
        fs::write(dir.join("email.toml"), raw)?;
        Ok(())
    }
}

/// Who reviewed an MCP server (#97). Legacy entries without the field are custom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum McpTrust {
    /// Installed from the reviewed catalog; every discovered tool is granted.
    Curated,
    /// Added through the advanced path; no tool is granted until enabled.
    #[default]
    Custom,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpServerConfig {
    /// Human-readable name for this MCP server.
    pub name: String,
    /// URL for HTTP/SSE transport (e.g. "https://mcp.excalidraw.com/mcp").
    pub url: Option<String>,
    /// Command for stdio transport (e.g. "npx" or "node").
    pub command: Option<String>,
    /// Arguments for the stdio command.
    #[serde(default)]
    pub args: Vec<String>,
    /// HTTP headers to send with requests / env vars for stdio servers.
    /// Only this server's connection ever sees them.
    #[serde(default)]
    pub headers: std::collections::HashMap<String, String>,
    /// Review status (#97).
    #[serde(default)]
    pub trust: McpTrust,
    /// Exact tool grant: raw tool names ordinary chats may use. Empty = none.
    #[serde(default)]
    pub enabled_tools: Vec<String>,
}

impl McpServerConfig {
    pub fn allows_tool(&self, raw_tool_name: &str) -> bool {
        self.enabled_tools.iter().any(|name| name == raw_tool_name)
    }
}

/// A user-defined model choice (#156): which provider and model to call.
/// Presets replace the retired cheap/fast/heavy tiers. Users name them, and
/// the two slots on [`LlmConfig`] say which preset does which job.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ModelPreset {
    pub id: String,
    pub name: String,
    pub provider: LlmProvider,
    pub model: String,
}

impl ModelPreset {
    fn seeded(id: &str, name: &str, provider: LlmProvider, model: &str) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            provider,
            model: model.into(),
        }
    }
}

/// Presets seeded when a provider is first set up. Users can rename, edit,
/// or delete them like any other preset.
pub fn default_presets(provider: LlmProvider) -> Vec<ModelPreset> {
    match provider {
        LlmProvider::Anthropic => vec![
            ModelPreset::seeded("sonnet", "Claude Sonnet", provider, "claude-sonnet-4-6"),
            ModelPreset::seeded("opus", "Claude Opus", provider, "claude-opus-4-6"),
            ModelPreset::seeded(
                "haiku",
                "Claude Haiku",
                provider,
                "claude-haiku-4-5-20251001",
            ),
        ],
        LlmProvider::Openai => vec![
            ModelPreset::seeded("gpt", "GPT-5.4", provider, "gpt-5.4"),
            ModelPreset::seeded("gpt-mini", "GPT-5.4 mini", provider, "gpt-5.4-mini"),
        ],
    }
}

/// Which seeded preset fills each slot for a provider: `(chat, background)`.
fn default_slots(provider: LlmProvider) -> (&'static str, &'static str) {
    match provider {
        LlmProvider::Anthropic => ("sonnet", "haiku"),
        LlmProvider::Openai => ("gpt", "gpt-mini"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmProvider {
    /// Direct Anthropic API (requires API key). Format: Anthropic Messages.
    Anthropic,
    /// OpenAI API (requires API key). Format: OpenAI Responses.
    Openai,
}

impl LlmProvider {
    pub fn label(self) -> &'static str {
        match self {
            LlmProvider::Anthropic => "Anthropic",
            LlmProvider::Openai => "OpenAI",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "api" | "anthropic" | "claude_cli" | "cli" => Some(Self::Anthropic),
            "openai" => Some(Self::Openai),
            _ => None,
        }
    }
}

// Read legacy names, but always serialize the canonical provider name.
impl<'de> serde::Deserialize<'de> for LlmProvider {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::parse(&s)
            .ok_or_else(|| serde::de::Error::unknown_variant(&s, &["anthropic", "openai"]))
    }
}

impl Default for LlmProvider {
    fn default() -> Self {
        LlmProvider::Anthropic
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(from = "RawLlmConfig")]
pub struct LlmConfig {
    #[serde(default)]
    pub tokens: LlmTokens,
    /// User-defined model presets (#156), `[[llm.presets]]` in config.toml.
    #[serde(default)]
    pub presets: Vec<ModelPreset>,
    /// Preset id for conversations, unless a chat pins its own.
    #[serde(default)]
    pub chat_preset: String,
    /// Preset id for memory extraction, chat titles, check-ins, and reflection.
    #[serde(default)]
    pub background_preset: String,
    #[serde(flatten)]
    pub extra: std::collections::BTreeMap<String, toml::Value>,
    /// The `provider` a config from before presets (#157) named. Read on
    /// load so `seed_for_keys` puts that provider in the slots; never
    /// written back.
    #[serde(skip)]
    retired_provider: Option<LlmProvider>,
}

/// `[llm]` keys from the tiered-model era. Dropped on load and on save.
pub const RETIRED_LLM_KEYS: &[&str] = &[
    "provider",
    "model_mode",
    "profiles",
    "model",
    "heavy_multiplier",
];

#[derive(Deserialize)]
struct RawLlmConfig {
    #[serde(default)]
    tokens: LlmTokens,
    #[serde(default)]
    presets: Vec<ModelPreset>,
    #[serde(default)]
    chat_preset: String,
    #[serde(default)]
    background_preset: String,
    #[serde(flatten)]
    extra: std::collections::BTreeMap<String, toml::Value>,
}

impl From<RawLlmConfig> for LlmConfig {
    fn from(mut raw: RawLlmConfig) -> Self {
        let retired_provider = raw
            .extra
            .get("provider")
            .and_then(toml::Value::as_str)
            .and_then(LlmProvider::parse);
        for key in RETIRED_LLM_KEYS {
            raw.extra.remove(*key);
        }
        Self {
            tokens: raw.tokens,
            presets: raw.presets,
            chat_preset: raw.chat_preset,
            background_preset: raw.background_preset,
            extra: raw.extra,
            retired_provider,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct LlmTokens {
    #[serde(default, rename = "OPEN_AI", alias = "open_ai", alias = "openai")]
    pub open_ai: String,
    #[serde(default, rename = "ANTHROPIC", alias = "anthropic")]
    pub anthropic: String,
    #[serde(
        default,
        rename = "BRAVE_SEARCH",
        alias = "brave_search",
        alias = "brave"
    )]
    pub brave_search: String,
    #[serde(
        default,
        rename = "OPENROUTER",
        alias = "open_router",
        alias = "openrouter"
    )]
    pub open_router: String,
    #[serde(default, rename = "ELEVENLABS", alias = "elevenlabs")]
    pub elevenlabs: String,
}

/// Token keys that no longer exist. They are dropped from saved config and
/// reported at load time. `GOOGLE_AI`/`gemini` powered built-in video
/// analysis, retired in #91.
pub const RETIRED_TOKEN_KEYS: [&str; 3] = ["GOOGLE_AI", "google_ai", "gemini"];

fn default_host() -> String {
    "0.0.0.0".into()
}

fn default_port() -> u16 {
    26559
}

fn default_registry_url() -> String {
    "https://raw.githubusercontent.com/triangle-int/bolly-skills/main/registry.json".into()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            auth_token: String::new(),
            static_dir: String::new(),
            public_url: String::new(),
            llm: LlmConfig::default(),
            embedding: EmbeddingConfig::default(),
            registry_url: default_registry_url(),
            mcp_servers: Vec::new(),
            github: GithubConfig::default(),
        }
    }
}

impl LlmConfig {
    pub fn preset(&self, id: &str) -> Option<&ModelPreset> {
        self.presets.iter().find(|preset| preset.id == id)
    }

    /// The preset conversations use unless a chat pins its own.
    pub fn chat_preset(&self) -> Option<&ModelPreset> {
        self.preset(&self.chat_preset)
    }

    /// The preset for everything the companion does off-screen. It never
    /// falls back to the chat preset: pointing both slots at one preset is
    /// the user's explicit choice, not a default.
    pub fn background_preset(&self) -> Option<&ModelPreset> {
        self.preset(&self.background_preset)
    }

    /// The API key for a provider, or None when it is not configured.
    pub fn key_for(&self, provider: LlmProvider) -> Option<&str> {
        let key = match provider {
            LlmProvider::Anthropic => &self.tokens.anthropic,
            LlmProvider::Openai => &self.tokens.open_ai,
        };
        (!key.is_empty()).then_some(key.as_str())
    }

    pub fn has_key(&self, provider: LlmProvider) -> bool {
        self.key_for(provider).is_some()
    }

    /// Providers that have an API key, in preset-provider order.
    pub fn keyed_providers(&self) -> Vec<LlmProvider> {
        [LlmProvider::Anthropic, LlmProvider::Openai]
            .into_iter()
            .filter(|provider| self.has_key(*provider))
            .collect()
    }

    pub fn setup_required(&self) -> Option<String> {
        if self.presets.is_empty() {
            return Some("Add a model preset and an API key for its provider.".into());
        }
        let Some(chat) = self.chat_preset() else {
            return Some("Choose a model preset for chat.".into());
        };
        if !self.has_key(chat.provider) {
            return Some(format!(
                "Configure an API key for {}.",
                chat.provider.label()
            ));
        }
        None
    }

    /// Whether conversations can run: the chat preset exists and its provider has a key.
    pub fn is_configured(&self) -> bool {
        self.chat_preset()
            .is_some_and(|preset| self.has_key(preset.provider))
    }

    /// The model conversations use by default, for status surfaces.
    pub fn chat_model(&self) -> Option<&str> {
        self.chat_preset().map(|preset| preset.model.as_str())
    }

    /// Add the provider's default presets that are missing and fill empty or
    /// dangling slots. Returns how many presets were added.
    pub fn seed_presets(&mut self, provider: LlmProvider) -> usize {
        let mut added = 0;
        for preset in default_presets(provider) {
            if self.preset(&preset.id).is_none() {
                self.presets.push(preset);
                added += 1;
            }
        }
        let (chat, background) = default_slots(provider);
        if self.chat_preset().is_none() {
            self.chat_preset = chat.into();
        }
        if self.background_preset().is_none() {
            self.background_preset = background.into();
        }
        added
    }

    /// Configs written before presets existed (#157) carry a key and no
    /// `[[llm.presets]]`. Seeding the keyed providers' defaults on load keeps
    /// those installs working without a click; presets a user has written
    /// or edited are never touched. The provider such a config named fills
    /// the slots when it has a key, so an OpenAI user stays on OpenAI (#25).
    /// Returns how many presets were added.
    pub fn seed_for_keys(&mut self) -> usize {
        if !self.presets.is_empty() {
            return 0;
        }
        let mut providers = self.keyed_providers();
        if let Some(named) = self
            .retired_provider
            .and_then(|named| providers.iter().position(|p| *p == named))
        {
            let named = providers.remove(named);
            providers.insert(0, named);
        }
        providers
            .into_iter()
            .map(|provider| self.seed_presets(provider))
            .sum()
    }

    /// Reject shapes the UI must never save: empty or duplicate ids, empty
    /// names or models, and slots that point nowhere or at a provider with
    /// no key.
    pub fn validate_presets(&self) -> Result<(), String> {
        let mut seen = std::collections::HashSet::new();
        for preset in &self.presets {
            let id = preset.id.trim();
            if id.is_empty()
                || !id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                return Err(format!(
                    "preset id {:?} must use letters, digits, '-' or '_'",
                    preset.id
                ));
            }
            if !seen.insert(id) {
                return Err(format!("duplicate preset id {id:?}"));
            }
            if preset.name.trim().is_empty() {
                return Err(format!("preset {id:?} needs a name"));
            }
            if preset.model.trim().is_empty() {
                return Err(format!("preset {id:?} needs a model"));
            }
        }
        if self.presets.is_empty() {
            return Ok(());
        }
        for (slot, id) in [
            ("chat_preset", &self.chat_preset),
            ("background_preset", &self.background_preset),
        ] {
            let Some(preset) = self.preset(id) else {
                return Err(format!("{slot} points at unknown preset {id:?}"));
            };
            if !self.has_key(preset.provider) {
                return Err(format!(
                    "{slot} uses {} but no {} API key is configured",
                    preset.name,
                    preset.provider.label()
                ));
            }
        }
        Ok(())
    }

    /// List of service names that have API keys set.
    pub fn configured_providers(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if !self.tokens.anthropic.is_empty() {
            out.push("anthropic");
        }
        if !self.tokens.open_ai.is_empty() {
            out.push("openai");
        }
        if !self.tokens.open_router.is_empty() {
            out.push("openrouter");
        }
        if !self.tokens.brave_search.is_empty() {
            out.push("brave_search");
        }
        out
    }
}

impl Default for LlmConfig {
    fn default() -> Self {
        let mut config = Self {
            tokens: LlmTokens::default(),
            presets: Vec::new(),
            chat_preset: String::new(),
            background_preset: String::new(),
            extra: Default::default(),
            retired_provider: None,
        };
        config.seed_presets(LlmProvider::default());
        config
    }
}

impl Default for LlmTokens {
    fn default() -> Self {
        Self {
            open_ai: String::new(),
            anthropic: String::new(),
            brave_search: String::new(),
            open_router: String::new(),
            elevenlabs: String::new(),
        }
    }
}

/// The profile every existing install lives in; its root is `~/.nolune` (or `NOLUNE_HOME`).
pub const DEFAULT_PROFILE: &str = "default";

/// Where named profiles keep their roots: a sibling of `~/.nolune`, never inside it, so
/// `nolune uninstall --yes` on one profile cannot reach another (#107).
pub const PROFILES_DIR: &str = ".nolune-profiles";

/// The isolated deployment a command addresses (#107). The name is local deployment
/// metadata only: never a companion identity, federation address, or trust anchor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    pub root: PathBuf,
}

impl Profile {
    pub fn is_default(&self) -> bool {
        self.name == DEFAULT_PROFILE
    }
}

/// Profile names are short lower-case slugs: `^[a-z0-9][a-z0-9-]{0,31}$`.
pub fn validate_profile_name(name: &str) -> Result<(), String> {
    let mut chars = name.chars();
    let valid = match chars.next() {
        Some(first) if first.is_ascii_lowercase() || first.is_ascii_digit() => {
            name.len() <= 32
                && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(format!(
            "invalid profile name {name:?}: use 1-32 lower-case letters, digits, or dashes, \
             starting with a letter or digit"
        ))
    }
}

/// `~/.nolune-profiles`, the directory every named profile root lives in.
pub fn profiles_dir(home_dir: &Path) -> PathBuf {
    home_dir.join(PROFILES_DIR)
}

/// `~/.nolune` for the default profile, `~/.nolune-profiles/<name>` for any other.
pub fn profile_root(home_dir: &Path, name: &str) -> PathBuf {
    if name == DEFAULT_PROFILE {
        home_dir.join(".nolune")
    } else {
        profiles_dir(home_dir).join(name)
    }
}

/// Resolve the profile a command addresses from `--profile` and `NOLUNE_HOME`.
pub fn resolve_profile(name: Option<&str>) -> Result<Profile, String> {
    let home_dir = dirs::home_dir().ok_or("cannot resolve the home directory")?;
    resolve_profile_in(
        &home_dir,
        env::var_os("NOLUNE_HOME").map(PathBuf::from),
        name,
    )
}

/// No name, or `default`, is today's workspace: `NOLUNE_HOME` when set, else `~/.nolune`.
/// A named profile always lives at its sibling root; a `NOLUNE_HOME` that points anywhere
/// else is refused rather than silently picking one of the two deployments.
fn resolve_profile_in(
    home_dir: &Path,
    env_home: Option<PathBuf>,
    name: Option<&str>,
) -> Result<Profile, String> {
    let name = name.unwrap_or(DEFAULT_PROFILE);
    validate_profile_name(name)?;
    let root = profile_root(home_dir, name);
    let root = match env_home {
        Some(env_home) if name == DEFAULT_PROFILE => env_home,
        Some(env_home) if env_home != root => {
            return Err(format!(
                "--profile {name} lives at {} but NOLUNE_HOME is {}; unset NOLUNE_HOME or drop --profile",
                root.display(),
                env_home.display()
            ));
        }
        _ => root,
    };
    Ok(Profile {
        name: name.to_owned(),
        root,
    })
}

static SELECTED_WORKSPACE: OnceLock<PathBuf> = OnceLock::new();

/// Pin the workspace for this process once (#107): the entrypoint resolves the profile,
/// then every `workspace_root()` and `config_path()` reader addresses it. Later calls
/// are ignored so a running server can never switch roots.
pub fn select_workspace(root: PathBuf) {
    let _ = SELECTED_WORKSPACE.set(root);
}

pub fn workspace_root() -> PathBuf {
    if let Some(root) = SELECTED_WORKSPACE.get() {
        return root.clone();
    }
    if let Some(path) = env::var_os("NOLUNE_HOME") {
        return PathBuf::from(path);
    }

    dirs::home_dir()
        .expect("failed to resolve home directory")
        .join(".nolune")
}

pub fn config_path() -> PathBuf {
    workspace_root().join("config.toml")
}

fn ensure_config_exists(path: &Path) -> io::Result<()> {
    if path.exists() {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let default_config =
        toml::to_string_pretty(&Config::default()).expect("default config should serialize");
    fs::write(path, default_config)
}

fn ensure_workspace_layout(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    fs::create_dir_all(path.join("instances"))?;
    fs::create_dir_all(path.join("skills"))?;
    Ok(())
}

pub fn load_config() -> anyhow::Result<Config> {
    let path = config_path();
    ensure_config_exists(&path)?;
    let raw = fs::read_to_string(&path)?;
    let mut config: Config = toml::from_str(&raw)?;
    ensure_workspace_layout(&workspace_root())?;

    let document: toml::Value = toml::from_str(&raw)?;
    let mut obsolete = Vec::new();
    for key in ["landing_url", "plan", "landing_auth_token"] {
        if document.get(key).is_some() {
            obsolete.push(format!("config.{key}"));
        }
    }
    if let Some(llm) = document.get("llm") {
        for key in RETIRED_LLM_KEYS {
            if llm.get(key).is_some() {
                obsolete.push(format!(
                    "config.llm.{key} (model presets replaced model modes, #156)"
                ));
            }
        }
    }
    if let Some(tokens) = document.get("llm").and_then(|value| value.get("tokens")) {
        for key in RETIRED_TOKEN_KEYS {
            if tokens.get(key).is_some() {
                obsolete.push(format!(
                    "config.llm.tokens.{key} (built-in video analysis was retired)"
                ));
            }
        }
    }
    for key in ["LANDING_URL", "FLY_APP_NAME", "FLY_MACHINE_ID"] {
        if env::var_os(key).is_some() {
            obsolete.push(format!("env.{key}"));
        }
    }
    if !obsolete.is_empty() {
        log::warn!(
            "ignoring obsolete managed control-plane settings ({}); Nolune now runs only as a self-hosted server",
            obsolete.join(", ")
        );
    }

    // Generic runtime override for local, remote, or container deployments.
    if let Ok(token) = env::var("NOLUNE_AUTH_TOKEN") {
        if !token.is_empty() {
            config.auth_token = token;
        }
    }

    if let Ok(url) = env::var("NOLUNE_PUBLIC_URL") {
        if !url.is_empty() {
            config.public_url = url;
        }
    }

    // Conventional container/platform port override.
    if let Ok(port) = env::var("PORT") {
        if let Ok(p) = port.parse::<u16>() {
            config.port = p;
        }
    }

    // Provider credential overrides.
    if let Ok(key) = env::var("ANTHROPIC_API_KEY") {
        if !key.is_empty() {
            config.llm.tokens.anthropic = key;
        }
    }
    if let Ok(key) = env::var("OPENAI_API_KEY") {
        if !key.is_empty() {
            config.llm.tokens.open_ai = key;
        }
    }
    if let Ok(key) = env::var("OPENROUTER_API_KEY") {
        if !key.is_empty() {
            config.llm.tokens.open_router = key;
        }
    }
    if let Ok(key) = env::var("BRAVE_SEARCH_API_KEY") {
        if !key.is_empty() {
            config.llm.tokens.brave_search = key;
        }
    }
    if let Ok(key) = env::var("ELEVENLABS_API_KEY") {
        if !key.is_empty() {
            config.llm.tokens.elevenlabs = key;
        }
    }
    // GitHub token override
    if let Ok(token) = env::var("GITHUB_TOKEN") {
        if !token.is_empty() {
            config.github.token = token;
        }
    }

    apply_public_url_default(&mut config);
    // Installs from before presets (#157): a key without [[llm.presets]].
    let seeded = config.llm.seed_for_keys();
    if seeded > 0 {
        log::info!("seeded {seeded} default model presets for the configured provider keys");
    }

    Ok(config)
}

/// The address a self-hosted server uses for its own links when no public
/// URL is configured. It follows the effective port, so it is derived on every
/// load instead of being written to config.toml.
pub fn local_public_url(port: u16) -> String {
    format!("http://localhost:{port}")
}

/// Whether the loaded `public_url` is the derived local default rather than a
/// value the operator configured.
pub fn uses_local_public_url(config: &Config) -> bool {
    config.public_url == local_public_url(config.port)
}

/// The public URL only if a remote model provider can fetch from it.
///
/// The local default (`http://localhost:<port>`) and any other loopback or
/// unspecified host are fine for links shown to the user, who sits on the same
/// machine, but a provider such as Anthropic cannot reach them and rejects the
/// whole request. Provider-facing callers use this and fall back to inline
/// base64 when it returns `None`; user-facing links keep using `public_url`.
pub fn provider_reachable_public_url(public_url: &str) -> Option<&str> {
    let trimmed = public_url.trim();
    if trimmed.is_empty() {
        return None;
    }
    let rest = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host_port = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host_port)| host_port);
    let host = if let Some(bracketed) = host_port.strip_prefix('[') {
        bracketed.split(']').next().unwrap_or_default()
    } else if host_port.parse::<std::net::Ipv6Addr>().is_ok() {
        host_port
    } else {
        host_port
            .rsplit_once(':')
            .map_or(host_port, |(host, _port)| host)
    };
    if host.is_empty() || host_is_local(host) {
        return None;
    }
    Some(trimmed)
}

fn host_is_local(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") || host.to_ascii_lowercase().ends_with(".localhost") {
        return true;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(ip) => {
            let ip = ip.to_canonical();
            ip.is_loopback() || ip.is_unspecified()
        }
        Err(_) => false,
    }
}

/// Every reader of the config (startup, per-turn reloads, background routines)
/// must see the same public URL, or tools that mint links flip between working
/// and "no public URL configured" depending on which path loaded the file.
fn apply_public_url_default(config: &mut Config) {
    if config.public_url.trim().is_empty() {
        config.public_url = local_public_url(config.port);
    }
}

/// Merge updates into the existing document so unrelated/forward-compatible keys
/// survive a settings save. Arrays are replaced intentionally (e.g. MCP removal).
pub fn serialize_config_preserving_keys(config: &Config, original: &str) -> anyhow::Result<String> {
    fn merge(existing: &mut toml::Value, updated: toml::Value) {
        match (existing, updated) {
            (toml::Value::Table(existing), toml::Value::Table(updated)) => {
                for (key, value) in updated {
                    if let Some(old) = existing.get_mut(&key) {
                        merge(old, value);
                    } else {
                        existing.insert(key, value);
                    }
                }
            }
            (existing, updated) => *existing = updated,
        }
    }
    let mut document: toml::Value = toml::from_str(original)?;
    if let Some(root) = document.as_table_mut() {
        for obsolete in ["landing_url", "plan", "landing_auth_token"] {
            root.remove(obsolete);
        }
        if let Some(llm) = root.get_mut("llm").and_then(toml::Value::as_table_mut) {
            for key in RETIRED_LLM_KEYS {
                llm.remove(*key);
            }
        }
    }
    // Avoid duplicate fields when an old token alias and its canonical spelling
    // would otherwise coexist after merging the serialized config.
    if let Some(tokens) = document
        .get_mut("llm")
        .and_then(|v| v.get_mut("tokens"))
        .and_then(toml::Value::as_table_mut)
    {
        for alias in [
            "open_ai",
            "openai",
            "anthropic",
            "brave_search",
            "brave",
            "open_router",
            "openrouter",
            "elevenlabs",
        ] {
            tokens.remove(alias);
        }
        for retired in RETIRED_TOKEN_KEYS {
            tokens.remove(retired);
        }
    }
    // The local default is derived from the port on load; persisting it would
    // pin the old port after a port change and hide that nothing was configured.
    let mut config = config.clone();
    if uses_local_public_url(&config) {
        config.public_url.clear();
    }
    merge(&mut document, toml::Value::try_from(&config)?);
    Ok(toml::to_string_pretty(&document)?)
}

#[cfg(test)]
mod public_url_tests {
    use super::*;

    #[test]
    fn empty_public_url_defaults_to_the_local_server_address() {
        let mut config: Config = toml::from_str("port = 26559\npublic_url = \"\"").unwrap();
        apply_public_url_default(&mut config);
        assert_eq!(config.public_url, "http://localhost:26559");
        assert!(uses_local_public_url(&config));

        let mut blank: Config = toml::from_str("port = 4242\npublic_url = \"  \"").unwrap();
        apply_public_url_default(&mut blank);
        assert_eq!(blank.public_url, "http://localhost:4242");
    }

    #[test]
    fn configured_public_url_is_kept() {
        let mut config: Config =
            toml::from_str("port = 26559\npublic_url = \"https://nolune.example\"").unwrap();
        apply_public_url_default(&mut config);
        assert_eq!(config.public_url, "https://nolune.example");
        assert!(!uses_local_public_url(&config));
    }

    #[test]
    fn derived_local_public_url_is_not_written_to_disk() {
        let original = "port = 26559\npublic_url = \"\"\n";
        let mut config: Config = toml::from_str(original).unwrap();
        apply_public_url_default(&mut config);

        let serialized = serialize_config_preserving_keys(&config, original).unwrap();
        let value: toml::Value = toml::from_str(&serialized).unwrap();
        assert_eq!(value["public_url"].as_str(), Some(""));

        // A reload of the saved file still derives the default.
        let mut reloaded: Config = toml::from_str(&serialized).unwrap();
        apply_public_url_default(&mut reloaded);
        assert_eq!(reloaded.public_url, "http://localhost:26559");
    }

    #[test]
    fn loopback_and_unspecified_hosts_are_not_provider_reachable() {
        for url in [
            "",
            "   ",
            "http://localhost:26559",
            "http://localhost",
            "https://LOCALHOST:8443/",
            "http://app.localhost:3000",
            "http://127.0.0.1:26559",
            "http://127.9.8.7",
            "http://[::1]:26559",
            "http://[::]:26559",
            "http://0.0.0.0:26559",
            "http://[::ffff:127.0.0.1]:26559",
            "http://user:pass@localhost:26559",
            "http://",
        ] {
            assert_eq!(provider_reachable_public_url(url), None, "{url:?}");
        }
        let mut config: Config = toml::from_str("port = 26559\npublic_url = \"\"").unwrap();
        apply_public_url_default(&mut config);
        assert_eq!(provider_reachable_public_url(&config.public_url), None);
    }

    #[test]
    fn routable_hosts_are_provider_reachable() {
        for url in [
            "https://nolune.example",
            "https://nolune.example/",
            "http://nolune.example:26559/base?x=1",
            "http://192.168.1.20:26559",
            "http://10.0.0.5",
            "https://[2001:db8::1]:8443",
            "http://user:pass@nolune.example",
            "https://localhost.example.com",
        ] {
            assert_eq!(provider_reachable_public_url(url), Some(url), "{url:?}");
        }
        assert_eq!(
            provider_reachable_public_url("  https://nolune.example  "),
            Some("https://nolune.example")
        );
        let config: Config =
            toml::from_str("port = 26559\npublic_url = \"https://nolune.example\"").unwrap();
        assert_eq!(
            provider_reachable_public_url(&config.public_url),
            Some("https://nolune.example")
        );
    }

    #[test]
    fn explicit_public_url_survives_save() {
        let original = "port = 26559\npublic_url = \"\"\n";
        let mut config: Config = toml::from_str(original).unwrap();
        config.public_url = "https://nolune.example".into();
        let serialized = serialize_config_preserving_keys(&config, original).unwrap();
        let value: toml::Value = toml::from_str(&serialized).unwrap();
        assert_eq!(value["public_url"].as_str(), Some("https://nolune.example"));
    }
}

#[cfg(test)]
mod llm_config_tests {
    use super::*;

    fn keyed(config: &mut Config, anthropic: bool, openai: bool) {
        config.llm.tokens.anthropic = if anthropic { "a".into() } else { String::new() };
        config.llm.tokens.open_ai = if openai { "o".into() } else { String::new() };
    }

    #[test]
    fn retired_tier_keys_are_dropped_on_load_and_save_without_losing_others() {
        let original = r#"
custom_global = "retained"
[llm]
provider = "api"
model_mode = "fast"
model = "custom-model"
heavy_multiplier = 2.5
custom_llm = "retained"
[llm.profiles.anthropic]
heavy = "claude-historical"
[llm.tokens]
ANTHROPIC = "anthropic-secret"
OPEN_AI = "openai-secret"
custom_token = "retained"
"#;
        let config: Config = toml::from_str(original).unwrap();
        for key in RETIRED_LLM_KEYS {
            assert!(
                !config.llm.extra.contains_key(*key),
                "{key} leaked into extra"
            );
        }
        assert_eq!(config.llm.extra["custom_llm"].as_str(), Some("retained"));
        assert_eq!(config.llm.tokens.open_ai, "openai-secret");
        let serialized = serialize_config_preserving_keys(&config, original).unwrap();
        let value: toml::Value = toml::from_str(&serialized).unwrap();
        for key in RETIRED_LLM_KEYS {
            assert!(value["llm"].get(key).is_none(), "{key} survived save");
        }
        assert_eq!(value["llm"]["custom_llm"].as_str(), Some("retained"));
        assert_eq!(value["custom_global"].as_str(), Some("retained"));
        assert_eq!(
            value["llm"]["tokens"]["custom_token"].as_str(),
            Some("retained")
        );
        let roundtrip: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(roundtrip.llm.tokens, config.llm.tokens);
        assert_eq!(roundtrip.llm.presets, config.llm.presets);
    }

    #[test]
    fn codex_and_unknown_providers_are_rejected() {
        for provider in ["codex", "openrouter"] {
            let raw =
                format!("[[llm.presets]]\nid='x'\nname='x'\nprovider='{provider}'\nmodel='m'");
            assert!(
                toml::from_str::<Config>(&raw).is_err(),
                "{provider} accepted"
            );
        }
    }

    #[test]
    fn default_config_seeds_anthropic_presets_and_slots() {
        let config = Config::default();
        assert_eq!(
            config
                .llm
                .presets
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>(),
            ["sonnet", "opus", "haiku"]
        );
        assert_eq!(config.llm.chat_preset, "sonnet");
        assert_eq!(config.llm.background_preset, "haiku");
        assert!(!config.llm.is_configured());
        assert!(config.llm.setup_required().unwrap().contains("Anthropic"));
        let serialized = toml::to_string_pretty(&config).unwrap();
        assert!(serialized.contains("[[llm.presets]]"), "{serialized}");
        let roundtrip: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(roundtrip.llm.presets, config.llm.presets);
    }

    /// Upgrade path (#25): a config from before presets carries a key and no
    /// `[[llm.presets]]`; loading seeds that provider instead of asking the
    /// person to click "Add defaults".
    #[test]
    fn keys_without_presets_are_seeded_on_load_and_written_presets_are_kept() {
        let mut config: Config = toml::from_str("[llm]\n[llm.tokens]\nOPEN_AI='k'").unwrap();
        assert!(config.llm.presets.is_empty());
        assert_eq!(config.llm.seed_for_keys(), 2);
        assert_eq!(config.llm.chat_preset, "gpt");
        assert_eq!(config.llm.background_preset, "gpt-mini");
        assert!(config.llm.is_configured());
        assert_eq!(config.llm.setup_required(), None);
        assert_eq!(config.llm.seed_for_keys(), 0, "seeding is idempotent");

        // Both keys: every keyed provider gets its presets, and Anthropic,
        // the default provider, fills the slots.
        let mut both: Config =
            toml::from_str("[llm]\n[llm.tokens]\nOPEN_AI='k'\nANTHROPIC='a'").unwrap();
        assert_eq!(both.llm.seed_for_keys(), 5);
        assert_eq!(both.llm.chat_preset, "sonnet");
        assert!(both.llm.is_configured());

        // A config from before presets named its provider (`provider =`,
        // retired since). An OpenAI user with both keys stays on OpenAI.
        let mut openai_user: Config =
            toml::from_str("[llm]\nprovider='openai'\n[llm.tokens]\nOPEN_AI='k'\nANTHROPIC='a'")
                .unwrap();
        assert_eq!(openai_user.llm.seed_for_keys(), 5);
        assert_eq!(openai_user.llm.chat_preset, "gpt");
        assert_eq!(openai_user.llm.background_preset, "gpt-mini");
        assert!(
            !openai_user.llm.extra.contains_key("provider"),
            "the retired key must not be written back"
        );
        let serialized: toml::Value =
            toml::from_str(&toml::to_string(&openai_user).unwrap()).unwrap();
        assert!(serialized["llm"].get("provider").is_none());

        // The legacy spellings of the Anthropic provider still mean Anthropic.
        let mut api_user: Config =
            toml::from_str("[llm]\nprovider='api'\n[llm.tokens]\nOPEN_AI='k'\nANTHROPIC='a'")
                .unwrap();
        assert_eq!(api_user.llm.seed_for_keys(), 5);
        assert_eq!(api_user.llm.chat_preset, "sonnet");

        // A named provider without a key cannot fill the slots: the keyed
        // provider does, and the person is not stuck at setup.
        let mut unkeyed: Config =
            toml::from_str("[llm]\nprovider='openai'\n[llm.tokens]\nANTHROPIC='a'").unwrap();
        assert_eq!(unkeyed.llm.seed_for_keys(), 3);
        assert_eq!(unkeyed.llm.chat_preset, "sonnet");
        assert!(unkeyed.llm.is_configured());

        // No provider key: nothing to seed, setup is still required.
        let mut none: Config = toml::from_str("[llm]\n[llm.tokens]\nBRAVE_SEARCH='b'").unwrap();
        assert_eq!(none.llm.seed_for_keys(), 0);
        assert!(none.llm.presets.is_empty());
        assert!(none.llm.setup_required().is_some());

        // Presets a person wrote are never touched, even when another keyed
        // provider has none.
        let mut custom: Config = toml::from_str(
            "[llm]\nchat_preset='mine'\nbackground_preset='mine'\n[llm.tokens]\nOPEN_AI='k'\nANTHROPIC='a'\n[[llm.presets]]\nid='mine'\nname='Mine'\nprovider='openai'\nmodel='gpt-custom'",
        )
        .unwrap();
        assert_eq!(custom.llm.seed_for_keys(), 0);
        assert_eq!(custom.llm.presets.len(), 1);
        assert_eq!(custom.llm.chat_model(), Some("gpt-custom"));
    }

    #[test]
    fn empty_presets_report_setup_and_seeding_fills_missing_without_duplicates() {
        let mut config: Config = toml::from_str("[llm]\n[llm.tokens]\nOPEN_AI='o'").unwrap();
        assert!(config.llm.presets.is_empty());
        assert!(config.llm.setup_required().unwrap().contains("preset"));

        assert_eq!(config.llm.seed_presets(LlmProvider::Openai), 2);
        assert_eq!(config.llm.chat_preset, "gpt");
        assert_eq!(config.llm.background_preset, "gpt-mini");
        assert!(config.llm.is_configured());
        assert_eq!(config.llm.setup_required(), None);

        // A second provider adds its presets but leaves the chosen slots alone.
        assert_eq!(config.llm.seed_presets(LlmProvider::Anthropic), 3);
        assert_eq!(config.llm.chat_preset, "gpt");
        assert_eq!(config.llm.seed_presets(LlmProvider::Anthropic), 0);
        assert_eq!(config.llm.presets.len(), 5);

        // A user-edited seed keeps its edits.
        config.llm.presets[0].model = "gpt-custom".into();
        assert_eq!(config.llm.seed_presets(LlmProvider::Openai), 0);
        assert_eq!(config.llm.chat_preset().unwrap().model, "gpt-custom");
    }

    #[test]
    fn slots_resolve_independently_across_providers() {
        let mut config = Config::default();
        keyed(&mut config, true, true);
        config.llm.seed_presets(LlmProvider::Openai);
        config.llm.chat_preset = "gpt".into();
        config.llm.background_preset = "haiku".into();
        assert_eq!(
            config.llm.chat_preset().unwrap().provider,
            LlmProvider::Openai
        );
        assert_eq!(config.llm.chat_model(), Some("gpt-5.4"));
        assert_eq!(
            config.llm.background_preset().unwrap().model,
            "claude-haiku-4-5-20251001"
        );
        config.llm.chat_preset = "opus".into();
        assert_eq!(config.llm.chat_model(), Some("claude-opus-4-6"));
        // A dangling background slot is reported, never silently replaced by chat.
        config.llm.background_preset = "gone".into();
        assert!(config.llm.background_preset().is_none());
        assert!(
            config
                .llm
                .validate_presets()
                .unwrap_err()
                .contains("background_preset")
        );
    }

    #[test]
    fn presets_validate_ids_names_models_slots_and_keys() {
        let mut config = Config::default();
        keyed(&mut config, true, false);
        assert_eq!(config.llm.validate_presets(), Ok(()));

        let mut dup = config.clone();
        dup.llm.presets.push(ModelPreset::seeded(
            "sonnet",
            "Again",
            LlmProvider::Anthropic,
            "m",
        ));
        assert!(
            dup.llm
                .validate_presets()
                .unwrap_err()
                .contains("duplicate")
        );

        let mut bad_id = config.clone();
        bad_id.llm.presets[0].id = "no spaces".into();
        assert!(bad_id.llm.validate_presets().unwrap_err().contains("id"));

        let mut no_model = config.clone();
        no_model.llm.presets[1].model = "  ".into();
        assert!(
            no_model
                .llm
                .validate_presets()
                .unwrap_err()
                .contains("model")
        );

        let mut no_name = config.clone();
        no_name.llm.presets[1].name = String::new();
        assert!(no_name.llm.validate_presets().unwrap_err().contains("name"));

        let mut unknown_slot = config.clone();
        unknown_slot.llm.chat_preset = "missing".into();
        assert!(
            unknown_slot
                .llm
                .validate_presets()
                .unwrap_err()
                .contains("missing")
        );

        let mut no_key = config.clone();
        no_key.llm.seed_presets(LlmProvider::Openai);
        no_key.llm.chat_preset = "gpt".into();
        let error = no_key.llm.validate_presets().unwrap_err();
        assert!(error.contains("OpenAI") && error.contains("key"), "{error}");

        let empty: Config = toml::from_str("[llm]").unwrap();
        assert_eq!(
            empty.llm.validate_presets(),
            Ok(()),
            "no presets is a valid empty state"
        );
    }

    #[test]
    fn token_aliases_do_not_create_duplicate_keys_on_save() {
        let original = "[llm]\nprovider='api'\n[llm.tokens]\nopenai='o'\nanthropic='a'\nbrave='b'\nopenrouter='r'\nelevenlabs='e'\ngemini='g'";
        let config: Config = toml::from_str(original).unwrap();
        let serialized = serialize_config_preserving_keys(&config, original).unwrap();
        let restored: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(restored.llm.tokens, config.llm.tokens);
    }
}
#[cfg(test)]
mod tests {
    use super::Config;

    #[test]
    fn obsolete_control_plane_fields_are_ignored_and_removed_on_save() {
        let original = r#"
landing_url = "https://control.example"
plan = "unlimited"
future_setting = "retained"

[llm]
heavy_multiplier = 2.5
"#;
        let config: Config = toml::from_str(original).unwrap();
        let serialized = super::serialize_config_preserving_keys(&config, original).unwrap();
        let value: toml::Value = toml::from_str(&serialized).unwrap();

        assert!(value.get("landing_url").is_none());
        assert!(value.get("plan").is_none());
        assert!(value["llm"].get("heavy_multiplier").is_none());
        assert_eq!(value["future_setting"].as_str(), Some("retained"));
        let json = serde_json::to_value(config).unwrap();
        assert!(json.get("landing_url").is_none());
        assert!(json.get("plan").is_none());
        assert!(json["llm"].get("heavy_multiplier").is_none());
    }

    #[test]
    fn legacy_mcp_entries_load_as_custom_with_no_grants_and_never_expose_headers() {
        let raw = r#"
[[mcp_servers]]
name = "old"
url = "https://old.example/mcp"
[mcp_servers.headers]
Authorization = "Bearer secret-token"

[[mcp_servers]]
name = "brave-search"
url = "https://mcp.bravesearch.com/sse"
trust = "curated"
enabled_tools = ["brave_web_search"]
"#;
        let config: Config = toml::from_str(raw).unwrap();
        let old = &config.mcp_servers[0];
        assert_eq!(old.trust, super::McpTrust::Custom);
        assert!(old.enabled_tools.is_empty());
        assert!(
            !old.allows_tool("anything"),
            "legacy servers grant nothing until reviewed"
        );
        let curated = &config.mcp_servers[1];
        assert_eq!(curated.trust, super::McpTrust::Curated);
        assert!(curated.allows_tool("brave_web_search"));
        assert!(!curated.allows_tool("brave_news"));
        let grants = crate::services::mcp::ServerGrants::from_config(curated, None);
        let json = serde_json::to_string(&grants).unwrap();
        assert!(!json.contains("secret-token") && !json.contains("headers"));
    }

    #[test]
    fn retired_google_ai_token_is_ignored_and_removed_on_save() {
        let original = "[llm]\nprovider='api'\n[llm.tokens]\nopenai='o'\nGOOGLE_AI='g'\ngemini='g2'\ngoogle_ai='g3'";
        let config: Config = toml::from_str(original).unwrap();
        let serialized = super::serialize_config_preserving_keys(&config, original).unwrap();
        let value: toml::Value = toml::from_str(&serialized).unwrap();
        let tokens = &value["llm"]["tokens"];
        for key in ["GOOGLE_AI", "google_ai", "gemini"] {
            assert!(tokens.get(key).is_none(), "retired token {key} was kept");
        }
        assert_eq!(tokens["OPEN_AI"].as_str(), Some("o"));
        let json = serde_json::to_value(config).unwrap();
        assert!(json["llm"]["tokens"].get("GOOGLE_AI").is_none());
    }

    #[test]
    fn new_instance_and_omitted_skin_use_little_moon() {
        assert_eq!(super::InstanceConfig::default().skin, "moon");
        let parsed: super::InstanceConfig = toml::from_str("").unwrap();
        assert_eq!(parsed.skin, "moon");
    }

    use super::InstanceConfig;

    #[test]
    fn legacy_instance_config_preserves_voice_settings() {
        for enabled in [true, false] {
            let config: InstanceConfig = toml::from_str(&format!(
                "music_enabled = {enabled}\nvoice_enabled = true\nelevenlabs_voice_id = 'test-voice'\n"
            )).unwrap();
            assert!(config.voice_enabled);
            assert_eq!(config.elevenlabs_voice_id, "test-voice");
            let saved = toml::to_string(&config).unwrap();
            assert!(!saved.contains("music_enabled"));
            let reloaded: InstanceConfig = toml::from_str(&saved).unwrap();
            assert!(reloaded.voice_enabled);
            assert_eq!(reloaded.elevenlabs_voice_id, "test-voice");
        }
    }
}

#[cfg(test)]
mod embedding_config_tests {
    use super::*;

    #[test]
    fn embedding_defaults_roundtrip_and_safe_status_are_independent_of_chat() {
        let mut cfg: Config = toml::from_str("").unwrap();
        let saved = toml::to_string(&cfg).unwrap();
        assert!(saved.contains("[embedding]"));
        let value = serde_json::to_value(&cfg).unwrap();
        assert_eq!(value["embedding"]["version"], 1);
        assert_eq!(value["embedding"]["provider"], "openai");
        assert_eq!(value["embedding"]["model"], "text-embedding-3-small");
        assert_eq!(value["embedding"]["dimensions"], 768);
        assert_eq!(value["embedding"]["base_url"], "https://api.openai.com/v1");
        for chat in ["sonnet", "gpt", "missing"] {
            cfg.llm.chat_preset = chat.into();
            cfg.llm.tokens.open_ai = "secret-openai-token".into();
            let status = cfg.embedding_status();
            assert_eq!(status["configured"], true);
            assert_eq!(status["provider"], "openai");
            assert!(!status.to_string().contains("secret-openai-token"));
        }
        let restored: Config = toml::from_str(&saved).unwrap();
        assert_eq!(restored.embedding_status()["configured"], false);
    }

    #[test]
    fn invalid_disabled_or_unconfigured_embeddings_report_bm25_without_secrets() {
        for raw in [
            "[embedding]\nenabled=false",
            "[embedding]\nprovider='google'",
            "[embedding]\nversion=99",
            "[embedding]\ndimensions=0",
            "[embedding]\nbase_url='https://user:secret@example.test/v1?key=secret'",
        ] {
            let mut cfg: Config = toml::from_str(raw).unwrap();
            cfg.llm.tokens.open_ai = "secret-token".into();
            let status = cfg.embedding_status();
            assert_eq!(status["configured"], false, "{raw}");
            assert_eq!(status["fallback"], "bm25");
            assert!(!status.to_string().contains("secret"));
        }
    }

    #[test]
    fn compatible_embeddings_allow_only_unauthenticated_loopback_endpoints() {
        for allowed in [
            "http://localhost:11434/v1",
            "http://127.0.0.1:11434/v1",
            "http://127.200.3.4:11434/v1",
            "http://[::1]:11434/v1",
        ] {
            let config = EmbeddingConfig {
                provider: "openai_compatible".into(),
                base_url: allowed.into(),
                ..EmbeddingConfig::default()
            };
            assert_eq!(config.validate(), Ok(()), "{allowed}");
            let status = config.safe_status("");
            assert_eq!(status["configured"], true);
            assert_eq!(status["authentication"], "none");
        }

        for rejected in [
            "http://example.com/v1",
            "https://10.0.0.1/v1",
            "http://192.168.1.2/v1",
            "http://169.254.169.254/latest/meta-data",
            "http://[fe80::1]/v1",
            "http://user:password@localhost:11434/v1",
            "http://localhost:11434/v1?key=value",
            "http://localhost:11434/v1#fragment",
        ] {
            let config = EmbeddingConfig {
                provider: "openai_compatible".into(),
                base_url: rejected.into(),
                ..EmbeddingConfig::default()
            };
            assert!(config.validate().is_err(), "{rejected}");
        }
    }

    #[test]
    fn official_openai_requires_the_exact_endpoint_and_key() {
        let mut config = EmbeddingConfig::default();
        assert_eq!(config.validate(), Ok(()));
        assert_eq!(
            config.unavailable_reason(""),
            Some("OpenAI embedding API key is missing")
        );
        config.base_url = "https://api.openai.com/v1/".into();
        assert!(config.validate().is_err());
    }
}

#[cfg(test)]
mod profile_tests {
    use super::*;

    #[test]
    fn profile_names_are_short_lowercase_slugs() {
        for ok in [
            "a",
            "molinka",
            "a-b",
            "0abc",
            "x-1-y",
            &"a".repeat(32),
            DEFAULT_PROFILE,
        ] {
            assert_eq!(validate_profile_name(ok), Ok(()), "{ok:?} should be valid");
        }
        for bad in [
            "",
            "-a",
            "Molinka",
            "a_b",
            "a.b",
            "a/b",
            "..",
            "a b",
            "é",
            &"a".repeat(33),
        ] {
            let error =
                validate_profile_name(bad).expect_err(&format!("{bad:?} should be invalid"));
            assert!(error.contains("profile"), "{error}");
        }
    }

    #[test]
    fn default_root_is_the_plain_home_and_named_roots_are_siblings() {
        let home = Path::new("/Users/me");
        assert_eq!(
            profile_root(home, DEFAULT_PROFILE),
            PathBuf::from("/Users/me/.nolune")
        );
        let molinka = profile_root(home, "molinka");
        assert_eq!(molinka, PathBuf::from("/Users/me/.nolune-profiles/molinka"));
        assert!(
            !molinka.starts_with("/Users/me/.nolune"),
            "a profile root inside ~/.nolune would be deleted by `nolune uninstall --yes`"
        );
        assert_eq!(
            profiles_dir(home),
            PathBuf::from("/Users/me/.nolune-profiles")
        );
    }

    #[test]
    fn unnamed_and_default_profiles_follow_nolune_home() {
        let home = Path::new("/Users/me");
        let plain = resolve_profile_in(home, None, None).unwrap();
        assert_eq!(plain.name, DEFAULT_PROFILE);
        assert_eq!(plain.root, PathBuf::from("/Users/me/.nolune"));
        assert!(plain.is_default());

        let custom = resolve_profile_in(home, Some(PathBuf::from("/data")), None).unwrap();
        assert_eq!(custom.root, PathBuf::from("/data"));
        let explicit =
            resolve_profile_in(home, Some(PathBuf::from("/data")), Some("default")).unwrap();
        assert_eq!(explicit, custom);
    }

    #[test]
    fn named_profile_resolves_beside_the_default_root() {
        let home = Path::new("/Users/me");
        let profile = resolve_profile_in(home, None, Some("molinka")).unwrap();
        assert_eq!(profile.name, "molinka");
        assert_eq!(
            profile.root,
            PathBuf::from("/Users/me/.nolune-profiles/molinka")
        );
        assert!(!profile.is_default());

        // The service definition sets NOLUNE_HOME to the same root; that is not a conflict.
        let agreeing = resolve_profile_in(home, Some(profile.root.clone()), Some("molinka"));
        assert_eq!(agreeing.unwrap(), profile);
    }

    #[test]
    fn named_profile_refuses_a_nolune_home_that_points_elsewhere() {
        let home = Path::new("/Users/me");
        let error = resolve_profile_in(home, Some(PathBuf::from("/data")), Some("molinka"))
            .expect_err("a named profile under a foreign NOLUNE_HOME must fail closed");
        assert!(error.contains("molinka"), "{error}");
        assert!(error.contains("/data"), "{error}");
        assert!(error.contains("NOLUNE_HOME"), "{error}");
        assert!(
            error.contains("/Users/me/.nolune-profiles/molinka"),
            "{error}"
        );

        let invalid = resolve_profile_in(home, None, Some("Molinka")).unwrap_err();
        assert!(invalid.contains("Molinka"), "{invalid}");
    }
}
