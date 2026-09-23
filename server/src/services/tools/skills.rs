use std::path::{Path, PathBuf};

use crate::services::tool::{Tool, ToolDefinition};
use schemars::JsonSchema;
use serde::Deserialize;

use super::{ToolExecError, openai_schema};

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
