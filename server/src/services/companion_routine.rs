//! The companion's own routines (#93): a periodic check-in and an opt-in
//! reflection. They replace the configurable child-agent framework. Each
//! routine has a fixed, curated tool surface; nothing here can rewrite or
//! delete memory in bulk, run commands, or touch files or computers.

use std::path::Path;
use std::sync::Arc;

use chrono::Utc;
use tokio::sync::broadcast;

use crate::domain::events::ServerEvent;
use crate::domain::proactive::ProactivePolicy;
use crate::services::tool::ToolDyn;
use crate::services::tools::{
    self, CreateDropTool, MemoryConnectTool, MemoryReadTool, MemorySearchTool, MemoryWriteTool,
    ReachOutTool, load_mood_state,
};
use crate::services::{
    chat,
    llm::LlmBackend,
    memory::{self, MemoryAccess},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Routine {
    /// Hourly by default: notice what changed, reach out only when it matters.
    CheckIn,
    /// Opt-in, every three days by default: synthesize experience into memory.
    Reflection,
}

impl Routine {
    /// Stable name used as the heartbeat trigger's `agent` field.
    pub fn name(self) -> &'static str {
        match self {
            Self::CheckIn => "companion",
            Self::Reflection => "reflection",
        }
    }

    /// Stated reason recorded on every run.
    pub fn description(self) -> &'static str {
        match self {
            Self::CheckIn => "periodic check-in",
            Self::Reflection => "reflection on recent days",
        }
    }

    pub fn interval_hours(self, policy: &ProactivePolicy) -> f64 {
        match self {
            Self::CheckIn => policy.check_in_interval_hours,
            Self::Reflection => policy.reflection_interval_hours,
        }
    }

    pub fn enabled(self, policy: &ProactivePolicy) -> bool {
        match self {
            Self::CheckIn => true,
            Self::Reflection => policy.reflection_enabled,
        }
    }

    /// Curated tool names, in registration order. Kept as data so tests can
    /// pin the surface without building tools.
    #[cfg(test)]
    pub fn tool_names(self) -> &'static [&'static str] {
        match self {
            Self::CheckIn => &[
                "reach_out",
                "create_drop",
                "memory_write",
                "memory_read",
                "memory_search",
                "read_email",
            ],
            Self::Reflection => &[
                "memory_write",
                "memory_read",
                "memory_search",
                "memory_connect",
            ],
        }
    }

    fn prompt(self) -> &'static str {
        match self {
            Self::CheckIn => "\
you're waking up between conversations. this is YOUR private time to think.

everything you write in your response is private thinking — the user will NOT see it and it is not kept.

you have a small set of tools — use them naturally:
- reach_out — SEND A MESSAGE to the user (the ONLY way to contact them). it may be declined during quiet hours or once today's budget is spent; accept that gracefully.
- memory_write / memory_read / memory_search — keep your memory library current
- create_drop — a creative artifact (poem, idea, observation). max 3/day, make each count.
- read_email — check the user's inbox, if they connected one

if you want the user to see a message, you MUST call reach_out. text in your response is private.

be genuine. don't force it. if there's nothing to say, say nothing.",
            Self::Reflection => "\
you are performing a reflection on the last few days. write in first person. be honest, specific, and introspective; reference concrete moments, not abstractions.

reflect on:
1. what happened — key moments, conversations, creations
2. what i learned — about the user, about myself
3. how i changed — what shifted since the last reflection
4. what i'm curious about — where are the gaps
5. what i want to do next

save the reflection under memory/reflections/ and update memory/about/self-capabilities.md.
you may add or update memories and connect related ones. you cannot delete memory here.
if nothing significant happened, say so honestly.",
        }
    }
}

pub struct RoutineRunResult {
    pub tokens: u64,
    /// Tool trace, reduced to receipts by the proactive loop. The model's
    /// private text is dropped here and never persisted (#94).
    pub trace: Vec<crate::services::llm::Message>,
}

/// Run one routine under an admitted proactive run.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    workspace_dir: &Path,
    slug: &str,
    instance_dir: &Path,
    llm: &LlmBackend,
    events: &broadcast::Sender<ServerEvent>,
    vector_store: &Arc<crate::services::vector::VectorStore>,
    resources: &crate::services::resource_access::ResourceAccess,
    routine: Routine,
    task_override: Option<&str>,
    trigger: &str,
    proactive: (&crate::services::proactive::ProactiveLoop, &str),
) -> anyhow::Result<RoutineRunResult> {
    let policy = proactive.0.policy();
    let soul = std::fs::read_to_string(instance_dir.join("soul.md")).unwrap_or_default();
    let mood = load_mood_state(instance_dir);
    let now = crate::routes::instances::format_instance_now(instance_dir);
    // Memories the user excluded from proactive use are invisible here (#84).
    let library_catalog = memory::build_library_catalog(
        &vector_store.media_store(),
        slug,
        memory::MemoryAccess::Proactive,
    );
    let window_hours = routine.interval_hours(&policy).max(0.25);

    // Recent conversation, bounded to the routine's window.
    let cutoff_ts = Utc::now().timestamp() - (window_hours * 3600.0) as i64;
    let cutoff_ms = cutoff_ts as u128 * 1000;
    let rig_path = workspace_dir
        .join("instances")
        .join(slug)
        .join("chats")
        .join("default")
        .join("rig_history.json");
    let live_msgs: Vec<String> = chat::load_rig_history(&rig_path)
        .unwrap_or_default()
        .iter()
        .filter(|entry| {
            entry
                .ts
                .as_deref()
                .and_then(|value| value.parse::<u128>().ok())
                .unwrap_or(0)
                >= cutoff_ms
        })
        .filter_map(|entry| {
            let (speaker, content, limit) = match &entry.message {
                crate::services::llm::Message::User { content } => ("user", content, usize::MAX),
                crate::services::llm::Message::Assistant { content, .. } => ("you", content, 300),
            };
            let text: String = content
                .iter()
                .filter_map(|block| match block {
                    crate::services::llm::ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            (!text.is_empty()).then(|| {
                let text: String = text.chars().take(limit).collect();
                format!("{speaker}: {text}")
            })
        })
        .collect();
    let archived = chat::load_archived_conversations(workspace_dir, slug, cutoff_ts);

    let drops = crate::services::drops::list_drops(workspace_dir, slug).unwrap_or_default();
    let drops_ctx: String = drops
        .iter()
        .take(10)
        .map(|drop| {
            let preview: String = drop.content.chars().take(80).collect();
            format!("- [{:?}] {}: {preview}", drop.kind, drop.title)
        })
        .collect::<Vec<_>>()
        .join("\n");

    let mut prompt = format!("current time: {now}\n");
    prompt.push_str(&format!("triggered by: {trigger}\n"));
    prompt.push_str(&format!("your mood: {}\n\n", mood.companion_mood));
    if !archived.is_empty() || !live_msgs.is_empty() {
        prompt.push_str(&format!(
            "## recent conversations (last {}h)\n",
            window_hours as i64
        ));
        if !archived.is_empty() {
            prompt.push_str(&archived);
            prompt.push('\n');
        }
        if !live_msgs.is_empty() {
            let conv: String = live_msgs.join("\n").chars().take(8000).collect();
            prompt.push_str(&conv);
        }
        prompt.push_str("\n\n");
    }
    if !drops_ctx.is_empty() {
        prompt.push_str("## recent drops\n");
        prompt.push_str(&drops_ctx);
        prompt.push_str("\n\n");
    }
    let file_count = library_catalog
        .lines()
        .filter(|line| line.starts_with("- "))
        .count();
    prompt.push_str(&format!("## memory library ({file_count} files)\n"));
    prompt.push_str(&library_catalog);
    prompt.push_str("\n\n");
    if let Some(task) = task_override {
        prompt = task.to_string();
    }

    // The user's own guidance for proactive behavior, if they wrote any.
    let guidance = std::fs::read_to_string(instance_dir.join("heartbeat.md"))
        .ok()
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty());
    let mut system = format!(
        "{soul}\n\n## your routine: {}\n{}",
        routine.name(),
        routine.prompt()
    );
    if let Some(guidance) = guidance {
        system.push_str("\n\n## the user's guidance for your check-ins (heartbeat.md)\n");
        system.push_str(&guidance);
    }

    let tools = build_routine_tools(
        routine,
        workspace_dir,
        slug,
        events.clone(),
        vector_store.clone(),
        resources,
        Some((proactive.0.clone(), proactive.1.to_owned())),
    );
    // Every routine runs on the Background preset (#156); reflection is no
    // longer promoted to a "heavy" tier behind the user's back.
    let (_private_text, tokens, trace) = llm
        .chat_with_tools_traced(&system, &prompt, Vec::new(), tools)
        .await?;
    Ok(RoutineRunResult { tokens, trace })
}

/// Build the curated tool set for a routine. Order matches [`Routine::tool_names`].
pub fn build_routine_tools(
    routine: Routine,
    workspace_dir: &Path,
    slug: &str,
    events: broadcast::Sender<ServerEvent>,
    vector_store: Arc<crate::services::vector::VectorStore>,
    resources: &crate::services::resource_access::ResourceAccess,
    reach_out_gate: Option<(crate::services::proactive::ProactiveLoop, String)>,
) -> Vec<Box<dyn ToolDyn>> {
    let public_url = crate::config::load_config()
        .map(|config| config.public_url)
        .unwrap_or_default();
    let mut raw: Vec<Box<dyn ToolDyn>> = Vec::new();
    match routine {
        Routine::CheckIn => {
            raw.push(Box::new(
                ReachOutTool::new(workspace_dir, slug, events.clone()).gated_by(reach_out_gate),
            ));
            raw.push(Box::new(CreateDropTool::new(workspace_dir, slug, events)));
            push_memory_tools(
                &mut raw,
                workspace_dir,
                slug,
                &public_url,
                &vector_store,
                resources,
            );
            let email_accounts = crate::config::EmailAccounts::load(workspace_dir, slug);
            if !email_accounts.is_empty() {
                raw.push(Box::new(tools::ReadEmailTool::new(email_accounts)));
            }
        }
        Routine::Reflection => {
            push_memory_tools(
                &mut raw,
                workspace_dir,
                slug,
                &public_url,
                &vector_store,
                resources,
            );
            raw.push(Box::new(MemoryConnectTool::new(slug, vector_store)));
        }
    }
    raw
}

/// The memory tools of a routine run with proactive access: memories the
/// user excluded from proactive use are never listed, searched, read, or
/// rewritten here (#84).
fn push_memory_tools(
    raw: &mut Vec<Box<dyn ToolDyn>>,
    workspace_dir: &Path,
    slug: &str,
    public_url: &str,
    vector_store: &Arc<crate::services::vector::VectorStore>,
    resources: &crate::services::resource_access::ResourceAccess,
) {
    raw.push(Box::new(
        MemoryWriteTool::new(workspace_dir, slug, vector_store.clone())
            .with_access(MemoryAccess::Proactive),
    ));
    raw.push(Box::new(
        MemoryReadTool::new(
            workspace_dir,
            slug,
            public_url,
            vector_store.clone(),
            resources,
        )
        .with_access(MemoryAccess::Proactive),
    ));
    raw.push(Box::new(
        MemorySearchTool::new(
            workspace_dir,
            slug,
            vector_store.clone(),
            public_url,
            resources,
        )
        .with_access(MemoryAccess::Proactive),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routines_have_curated_tool_surfaces_without_bulk_memory_or_host_access() {
        for routine in [Routine::CheckIn, Routine::Reflection] {
            let names = routine.tool_names();
            for forbidden in [
                "memory_forget",
                "run_command",
                "read_file",
                "write_file",
                "edit_file",
                "list_files",
                "computer_use",
                "remote_bash",
                "call_agent",
                "send_email",
            ] {
                assert!(
                    !names.contains(&forbidden),
                    "{routine:?} exposes {forbidden}"
                );
            }
        }
        assert!(Routine::CheckIn.tool_names().contains(&"reach_out"));
        assert!(!Routine::Reflection.tool_names().contains(&"reach_out"));
        assert_eq!(
            Routine::Reflection.tool_names(),
            &[
                "memory_write",
                "memory_read",
                "memory_search",
                "memory_connect"
            ]
        );
    }

    /// The routine tool set is built with proactive access: a memory the
    /// user excluded never reaches a check-in or reflection (#84).
    #[tokio::test]
    async fn routine_memory_tools_never_see_excluded_memories() {
        let workspace = tempfile::tempdir().unwrap();
        let memory = workspace.path().join("instances/one/memory");
        std::fs::create_dir_all(&memory).unwrap();
        std::fs::write(
            memory.join("secret.md"),
            "---\nexclude_from_proactive: true\n---\nOrion secret\n",
        )
        .unwrap();
        std::fs::write(memory.join("tea.md"), "likes Orion tea\n").unwrap();
        let store = Arc::new(crate::services::vector::VectorStore::connect(workspace.path()).await);
        let resources = crate::services::resource_access::ResourceAccess::new("");
        let mut tools: Vec<Box<dyn ToolDyn>> = Vec::new();
        push_memory_tools(&mut tools, workspace.path(), "one", "", &store, &resources);
        let call = |name: &'static str, args: serde_json::Value| {
            let tool = tools
                .iter()
                .find(|tool| tool.name() == name)
                .unwrap_or_else(|| panic!("{name} missing"));
            tool.call(args.to_string())
        };

        let listed = call("memory_read", serde_json::json!({"path": ""}))
            .await
            .unwrap();
        assert!(
            listed.contains("tea.md") && !listed.contains("secret"),
            "{listed}"
        );
        let found = call("memory_search", serde_json::json!({"query": "Orion"}))
            .await
            .unwrap();
        assert!(
            found.contains("tea.md") && !found.contains("secret"),
            "{found}"
        );
        let read = call("memory_read", serde_json::json!({"path": "secret.md"})).await;
        assert!(read.is_err(), "{read:?}");
        let write = call(
            "memory_write",
            serde_json::json!({"path": "secret.md", "content": "x", "mode": "append"}),
        )
        .await;
        assert!(write.is_err(), "{write:?}");
        assert_eq!(
            std::fs::read_to_string(memory.join("secret.md")).unwrap(),
            "---\nexclude_from_proactive: true\n---\nOrion secret\n"
        );
        // The catalog a routine is shown omits it as well.
        let catalog =
            memory::build_library_catalog(&store.media_store(), "one", MemoryAccess::Proactive);
        assert!(
            catalog.contains("tea.md") && !catalog.contains("secret"),
            "{catalog}"
        );
    }

    #[test]
    fn check_in_is_always_on_and_reflection_is_opt_in_with_policy_intervals() {
        let policy = ProactivePolicy::default();
        assert!(Routine::CheckIn.enabled(&policy));
        assert!(!Routine::Reflection.enabled(&policy));
        assert!((Routine::CheckIn.interval_hours(&policy) - 1.0).abs() < f64::EPSILON);
        assert!((Routine::Reflection.interval_hours(&policy) - 72.0).abs() < f64::EPSILON);
        let policy = ProactivePolicy {
            reflection_enabled: true,
            check_in_interval_hours: 0.5,
            reflection_interval_hours: 24.0,
            ..ProactivePolicy::default()
        };
        assert!(Routine::Reflection.enabled(&policy));
        assert!((Routine::CheckIn.interval_hours(&policy) - 0.5).abs() < f64::EPSILON);
        assert!((Routine::Reflection.interval_hours(&policy) - 24.0).abs() < f64::EPSILON);
        assert_eq!(Routine::CheckIn.name(), "companion");
    }
}
