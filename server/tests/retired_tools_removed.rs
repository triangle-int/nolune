//! Guard for trimmed tool surface: the model reads the clock from the
//! terminal (`date` via run_command), not from a `get_time` tool or a
//! `[current time: …]` block on every user message; tools that duplicated a
//! general one stay gone (edit_soul → edit_file, list_skills → the skills
//! prompt, read_skill_reference → read_file, memory_list → memory_read on a
//! folder); settings go through `nolune config` and the configure-nolune
//! skill, not get_settings/update_config; and the prompt names no tool that
//! is not registered.

#[path = "../test-support/source_scan.rs"]
mod source_scan;

use source_scan::without_cfg_test_items;
use std::{
    fs,
    path::{Path, PathBuf},
};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn retired_and_phantom_tool_names_are_absent() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    rust_files(&repo.join("server/src"), &mut files);
    files.sort();

    let forbidden = [
        "GetTimeTool",
        "\"get_time\"",
        "[current time:",
        "current_time",
        "SearchCodeTool",
        "search_code",
        "install_package",
        "get_project_state",
        "update_project_state",
        "github_clone",
        "github_*",
        "`browse`",
        "EditSoulTool",
        "\"edit_soul\"",
        "ListSkillsTool",
        "\"list_skills\"",
        "ReadSkillReferenceTool",
        "\"read_skill_reference\"",
        "MemoryListTool",
        "\"memory_list\"",
        "GetSettingsTool",
        "\"get_settings\"",
        "get_settings,",
        "UpdateConfigTool",
        "\"update_config\"",
        "update_config.",
        "call update_config",
    ];

    let mut violations = Vec::new();
    for path in files {
        let text = without_cfg_test_items(&fs::read_to_string(&path).unwrap());
        for needle in forbidden {
            if text.contains(needle) {
                violations.push(format!(
                    "{} contains {needle:?}",
                    path.strip_prefix(repo).unwrap().display()
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "retired or phantom tool names remain:\n{}",
        violations.join("\n")
    );
}
