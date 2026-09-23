use std::path::{Path, PathBuf};

use crate::services::tool::{Tool, ToolDefinition};
use schemars::JsonSchema;
use serde::Deserialize;

use super::{ToolExecError, openai_schema};

// ---------------------------------------------------------------------------
// list_skills
// ---------------------------------------------------------------------------

/// The installed skills, read on every call: the system prompt names only the
/// built-in ones so installing a skill mid-conversation never changes it (and
/// its cache), and this is where the model finds the rest.
pub struct ListSkillsTool {
    workspace_dir: PathBuf,
}

impl ListSkillsTool {
    pub fn new(workspace_dir: &Path) -> Self {
        Self {
            workspace_dir: workspace_dir.to_path_buf(),
        }
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct ListSkillsArgs {
    /// Optional filter: "enabled", "disabled", or "all" (default: "all").
    pub filter: Option<String>,
}

impl Tool for ListSkillsTool {
    const NAME: &'static str = "list_skills";
    type Error = ToolExecError;
    type Args = ListSkillsArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "list_skills".into(),
            description: "List the installed skills with their descriptions, including any \
                installed during this conversation. Check it when a task might match a skill."
                .into(),
            parameters: openai_schema::<ListSkillsArgs>(),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let filter = args.filter.as_deref().unwrap_or("all").to_lowercase();
        let listed: Vec<_> = crate::services::skills::list_skills(&self.workspace_dir)
            .into_iter()
            .filter(|s| match filter.as_str() {
                "enabled" => s.enabled,
                "disabled" => !s.enabled,
                _ => true,
            })
            .collect();
        if listed.is_empty() {
            return Ok(match filter.as_str() {
                "enabled" | "disabled" => format!("no {filter} skills"),
                _ => "no skills installed".into(),
            });
        }

        let mut out = String::new();
        for s in &listed {
            let mut tags = Vec::new();
            if s.builtin {
                tags.push("built in".to_owned());
            }
            if !s.enabled {
                tags.push("disabled".to_owned());
            }
            if let Some(source) = &s.source {
                tags.push(format!("from {}", source.repo));
            }
            if s.resources.iter().any(|r| r.starts_with("references/")) {
                tags.push("has references".to_owned());
            }
            let tags = if tags.is_empty() {
                String::new()
            } else {
                format!(" [{}]", tags.join(", "))
            };
            out.push_str(&format!(
                "- {} (id: `{}`){tags}: {}\n",
                s.name, s.id, s.description
            ));
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// activate_skill
// ---------------------------------------------------------------------------

pub struct ActivateSkillTool {
    workspace_dir: PathBuf,
}

impl ActivateSkillTool {
    pub fn new(workspace_dir: &Path, _api_key: &str) -> Self {
        Self {
            workspace_dir: workspace_dir.to_path_buf(),
        }
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct ActivateSkillArgs {
    /// The name of the skill to activate (must match an installed, enabled skill).
    pub skill_name: String,
}

impl Tool for ActivateSkillTool {
    const NAME: &'static str = "activate_skill";
    type Error = ToolExecError;
    type Args = ActivateSkillArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "activate_skill".into(),
            description: "Activate a skill before using it. Returns instructions for the skill."
                .into(),
            parameters: openai_schema::<ActivateSkillArgs>(),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let skills = crate::services::skills::list_skills(&self.workspace_dir);
        let needle = args.skill_name.to_lowercase();
        let found = skills
            .iter()
            .find(|s| s.name.to_lowercase() == needle || s.id == needle);
        match found {
            Some(s) if s.enabled => {
                // Local skill: return instructions (prompt injection)
                let mut result = format!("# {} skill activated\n\n{}", s.name, s.instructions);
                let refs: Vec<_> = s
                    .resources
                    .iter()
                    .filter(|r| r.starts_with("references/"))
                    .collect();
                let skill_dir = self.workspace_dir.join("skills").join(&s.id);
                if !s.builtin {
                    result.push_str(&format!("\n\nskill directory: {}", skill_dir.display()));
                }
                if !refs.is_empty() {
                    result.push_str(
                        "\n\n## reference files\n\
                        **MANDATORY**: read these with read_file BEFORE running any commands:\n",
                    );
                    for r in refs {
                        result.push_str(&format!("- {}\n", skill_dir.join(r).display()));
                    }
                }
                Ok(result)
            }
            Some(s) => Err(ToolExecError(format!("skill '{}' is disabled", s.name))),
            None => Err(ToolExecError(format!(
                "skill '{}' not found",
                args.skill_name
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_skill_installed_mid_conversation_is_listed_on_the_next_call() {
        let workspace = tempfile::tempdir().unwrap();
        let tool = ListSkillsTool::new(workspace.path());
        let listed = tool.call(ListSkillsArgs { filter: None }).await.unwrap();
        assert!(
            listed.contains("configure-nolune (id: `configure-nolune`) [built in]:"),
            "{listed}"
        );
        assert!(!listed.contains("weather"), "{listed}");

        let folder = workspace.path().join("skills/weather");
        std::fs::create_dir_all(folder.join("references")).unwrap();
        std::fs::write(
            folder.join("SKILL.md"),
            "---\nname: weather\ndescription: Look up forecasts.\n---\nuse the weather cli\n",
        )
        .unwrap();
        std::fs::write(folder.join("references/api.md"), "the api").unwrap();

        let listed = tool.call(ListSkillsArgs { filter: None }).await.unwrap();
        assert!(
            listed.contains("- weather (id: `weather`) [has references]: Look up forecasts."),
            "{listed}"
        );
        let disabled = tool
            .call(ListSkillsArgs {
                filter: Some("disabled".into()),
            })
            .await
            .unwrap();
        assert_eq!(disabled, "no disabled skills");
    }
}
