use serde::Serialize;

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    /// Unix timestamp (seconds) when this response was generated.
    pub timestamp: u64,
    /// How long the server has been running, in seconds.
    pub uptime_secs: u64,
}

#[derive(Serialize)]
pub struct ServerMetaResponse {
    pub app: &'static str,
    pub version: &'static str,
    pub commit: &'static str,
    pub port: u16,
    pub workspace_dir: String,
    /// Stable slug of the one companion this server owns.
    pub companion_slug: &'static str,
    /// 1 once the canonical companion exists, otherwise 0.
    pub instances_count: usize,
    pub skills_count: usize,
    pub llm: LlmSummary,
}

#[derive(Serialize)]
pub struct LlmSummary {
    /// Id of the Chat slot preset (#156), when it resolves.
    pub chat_preset: Option<String>,
    pub provider: Option<crate::config::LlmProvider>,
    pub setup_required: Option<String>,
    pub model: Option<String>,
    pub configured: bool,
}
