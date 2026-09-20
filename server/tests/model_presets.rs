//! Guard for #156: users pick models through presets. No cheap/fast/heavy
//! abstraction, no per-message classifier, and no global provider switch
//! survive in server or client source.

#[path = "../test-support/source_scan.rs"]
mod source_scan;

use source_scan::without_cfg_test_items;
use std::{
    fs,
    path::{Path, PathBuf},
};

fn files(root: &Path, extensions: &[&str]) -> Vec<PathBuf> {
    fn visit(dir: &Path, extensions: &[&str], out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(&path, extensions, out);
            } else if path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| extensions.contains(&ext))
            {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    visit(root, extensions, &mut out);
    out.sort();
    out
}

fn read(repo: &Path, relative: &str) -> String {
    fs::read_to_string(repo.join(relative)).unwrap_or_else(|_| panic!("{relative} is missing"))
}

#[test]
fn model_tiers_router_and_provider_switch_are_gone() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut violations = Vec::new();

    for path in files(&repo.join("server/src"), &["rs"]) {
        let relative = path
            .strip_prefix(repo)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let production = without_cfg_test_items(&fs::read_to_string(&path).unwrap());
        for token in [
            "ModelMode",
            ".model_mode",
            "model_mode:",
            "ProviderProfile",
            "cheap_variant",
            "heavy_variant",
            "fast_variant",
            "classify_needs_heavy",
            "fast_model_name",
            "NOLUNE_MODEL_MODE",
            "Codex",
            "/api/config/model-mode",
            "/api/config/provider\"",
        ] {
            if production.contains(token) {
                violations.push(format!("{relative} contains {token:?}"));
            }
        }
    }
    for path in files(&repo.join("client/src"), &["ts", "js", "svelte"]) {
        let relative = path
            .strip_prefix(repo)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let source = fs::read_to_string(&path).unwrap();
        for token in [
            "updateModelMode",
            "model_mode",
            "modelModes",
            "Model mode",
            "setModelMode",
            "updateProvider(",
            "\"heavy\"",
            "'heavy'",
        ] {
            if source.contains(token) {
                violations.push(format!("{relative} contains {token:?}"));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "model tier abstraction remains:\n{}",
        violations.join("\n")
    );
}

#[test]
fn presets_have_a_config_shape_an_api_and_a_client_module() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut violations = Vec::new();

    let config = without_cfg_test_items(&read(repo, "server/src/config.rs"));
    for required in [
        "pub struct ModelPreset",
        "pub presets: Vec<ModelPreset>",
        "pub chat_preset: String",
        "pub background_preset: String",
        "pub fn preset(",
        "pub fn seed_presets(",
    ] {
        if !config.contains(required) {
            violations.push(format!("server/src/config.rs is missing {required:?}"));
        }
    }
    let onboard = without_cfg_test_items(&read(repo, "server/src/onboard.rs"));
    if !onboard.contains("[[llm.presets]]") {
        violations.push("nolune onboard must write example presets into the fresh config".into());
    }
    let routes = without_cfg_test_items(&read(repo, "server/src/routes/config.rs"));
    for required in ["\"/api/config/models\"", "\"/api/config/models/seed\""] {
        if !routes.contains(required) {
            violations.push(format!("routes/config.rs is missing route {required}"));
        }
    }
    let chat_routes = without_cfg_test_items(&read(repo, "server/src/routes/chat.rs"));
    if !chat_routes.contains("/api/chat/{instance_slug}/{chat_id}/preset") {
        violations.push("routes/chat.rs is missing the per-conversation preset route".into());
    }
    let domain = read(repo, "server/src/domain/chat.rs");
    if !domain.contains("pub preset: Option<String>") {
        violations.push("ChatMeta must carry the optional per-conversation preset".into());
    }
    let backend = without_cfg_test_items(&read(repo, "server/src/services/llm/mod.rs"));
    for required in ["pub fn for_preset(", "pub fn background("] {
        if !backend.contains(required) {
            violations.push(format!("llm/mod.rs is missing {required:?}"));
        }
    }
    let tool = without_cfg_test_items(&read(repo, "server/src/services/tools/system.rs"));
    if tool.contains("preset") && tool.contains("pub chat_preset: Option<String>") {
        violations.push("the model-facing update_config tool must not switch presets".into());
    }

    if !repo.join("client/src/lib/models/presets.js").exists() {
        violations.push("client/src/lib/models/presets.js is missing".into());
    }
    let client = read(repo, "client/src/lib/api/client.ts");
    for required in [
        "export function fetchModelPresets(",
        "export function updateModelPresets(",
        "export function seedModelPresets(",
        "export function fetchChatPreset(",
        "export function updateChatPreset(",
    ] {
        if !client.contains(required) {
            violations.push(format!("client.ts is missing {required:?}"));
        }
    }
    let connections = read(
        repo,
        "client/src/routes/[slug]/settings/connections/+page.svelte",
    );
    for required in ["presets", "chat_preset", "background_preset"] {
        if !connections.contains(required) {
            violations.push(format!("Settings › Connections is missing {required:?}"));
        }
    }
    let composer = read(repo, "client/src/lib/components/chat/PromptComposer.svelte");
    if !composer.contains("preset") {
        violations.push("the composer must offer the per-conversation preset picker".into());
    }
    let doc = read(repo, "docs/settings.md");
    for required in ["preset", "Background", "#156"] {
        if !doc.contains(required) {
            violations.push(format!("docs/settings.md is missing {required:?}"));
        }
    }

    assert!(violations.is_empty(), "{}", violations.join("\n"));
}
