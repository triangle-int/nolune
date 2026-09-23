//! Guard for #29: the provider documentation keeps up with the code. The
//! lists it checks against are read from the source, not repeated here,
//! so a new `LlmProvider` variant, a new `LlmTokens` field, a new
//! `Capabilities` flag, a changed adapter constant or a new OpenRouter top
//! model fails this test until the docs say so.
//!
//! - `README.md` lists every API-key environment override in its
//!   configuration table, one row per variable.
//! - `docs/providers.md` covers every provider, every token key with its
//!   environment override, where the keys live, the OpenRouter top models
//!   onboarding offers, the retired `[llm]` keys, a capability table that
//!   matches each adapter's `Capabilities`, and the Codex process model.
//! - `docs/release-checklist.md` says where model ids still live in the
//!   source (nothing is seeded, #156) and how to bump the pinned Codex
//!   version, and `CLAUDE.md` points at it from the versioning section.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

const CONFIG: &str = "server/src/config.rs";
const CONTRACT: &str = "server/src/services/llm/contract.rs";
const CODEX_MOD: &str = "server/src/services/llm/codex/mod.rs";
const OPENROUTER: &str = "server/src/services/llm/openrouter.rs";
const BACKEND: &str = "server/src/services/llm/mod.rs";
const README: &str = "README.md";
const PROVIDERS_DOC: &str = "docs/providers.md";
const CHECKLIST_DOC: &str = "docs/release-checklist.md";
const CONVENTIONS: &str = "CLAUDE.md";

/// Where each provider's `Capabilities` constant lives, by its label.
const ADAPTER_CAPABILITIES: &[(&str, &str, &str)] = &[
    (
        "Anthropic",
        "server/src/services/llm/anthropic.rs",
        "CAPABILITIES",
    ),
    (
        "OpenAI",
        "server/src/services/llm/openai.rs",
        "CAPABILITIES",
    ),
    ("OpenRouter", OPENROUTER, "DEFAULT_CAPABILITIES"),
    (
        "Codex",
        "server/src/services/llm/codex/adapter.rs",
        "CAPABILITIES",
    ),
];

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn read(relative: &str) -> String {
    fs::read_to_string(repo().join(relative)).unwrap_or_else(|_| panic!("{relative} is missing"))
}

/// The body between the first `{` after `header` and its matching `}`.
fn item_body<'a>(source: &'a str, header: &str) -> &'a str {
    let start = source
        .find(header)
        .unwrap_or_else(|| panic!("{header:?} not found"));
    let open = start + source[start..].find('{').unwrap();
    let mut depth = 0usize;
    for (offset, byte) in source[open..].bytes().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &source[open + 1..open + offset];
                }
            }
            _ => {}
        }
    }
    panic!("{header:?} has no closing brace");
}

/// The value of the first `key = "..."` inside `text`.
fn quoted_after<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let at = text.find(key)? + key.len();
    let rest = text[at..].trim_start_matches([' ', '=']);
    let rest = rest.strip_prefix('"')?;
    rest.split_once('"').map(|(value, _)| value)
}

/// Every `LlmProvider` variant with its `label()`.
fn providers(config: &str) -> Vec<(String, String)> {
    let variants: Vec<String> = item_body(config, "pub enum LlmProvider")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("//") && !line.starts_with('#'))
        .map(|line| line.trim_end_matches(',').to_owned())
        .collect();
    let labels = item_body(item_body(config, "impl LlmProvider"), "fn label(self)");
    variants
        .into_iter()
        .map(|variant| {
            let arm = format!("LlmProvider::{variant} =>");
            let label = quoted_after(labels, &arm)
                .unwrap_or_else(|| panic!("LlmProvider::{variant} has no label"));
            (variant, label.to_owned())
        })
        .collect()
}

/// Every `LlmTokens` field's serialized key (`ANTHROPIC`, `OPEN_AI`, ...)
/// with the environment variable `load_config` reads it from.
fn token_keys(config: &str) -> Vec<(String, String)> {
    let fields = item_body(config, "pub struct LlmTokens");
    let mut out = Vec::new();
    let mut cursor = 0;
    while let Some(at) = fields[cursor..].find("pub ") {
        let start = cursor + at;
        let attribute = &fields[cursor..start];
        let key = quoted_after(attribute, "rename")
            .unwrap_or_else(|| panic!("an LlmTokens field has no serde rename: {attribute}"));
        let field: String = fields[start + "pub ".len()..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        let assignment = format!("config.llm.tokens.{field} = ");
        let env = config
            .lines()
            .collect::<Vec<_>>()
            .windows(3)
            .find(|window| window.iter().any(|line| line.contains(&assignment)))
            .and_then(|window| {
                window
                    .iter()
                    .find_map(|line| quoted_after(line, "env::var(").map(str::to_owned))
            })
            .unwrap_or_else(|| panic!("load_config reads no environment override into {field}"));
        out.push((key.to_owned(), env));
        cursor = start + "pub ".len();
    }
    out
}

/// The field names of `Capabilities`, in order.
fn capability_fields(contract: &str) -> Vec<String> {
    item_body(contract, "pub struct Capabilities")
        .lines()
        .map(str::trim)
        .filter_map(|line| line.strip_prefix("pub "))
        .map(|line| line.split(':').next().unwrap().to_owned())
        .collect()
}

/// `field -> true/false` of one adapter's `Capabilities` constant.
fn adapter_capabilities(source: &str, name: &str) -> BTreeMap<String, bool> {
    item_body(source, &format!("const {name}: Capabilities"))
        .lines()
        .filter_map(|line| line.trim().split_once(':'))
        .map(|(field, value)| {
            let value = value.trim().trim_end_matches(',');
            (
                field.to_owned(),
                match value {
                    "true" => true,
                    "false" => false,
                    other => panic!("{name}.{field} is {other:?}, not a literal"),
                },
            )
        })
        .collect()
}

/// The retired `[llm]` keys and token keys `config.rs` drops on load.
fn retired_keys(config: &str) -> Vec<String> {
    let mut out = Vec::new();
    for constant in ["RETIRED_LLM_KEYS", "RETIRED_TOKEN_KEYS"] {
        let start = config
            .find(&format!("pub const {constant}"))
            .unwrap_or_else(|| panic!("{constant} not found"));
        // Past the type (`[&str; 3]`) to the list itself.
        let assign = start + config[start..].find('=').unwrap();
        let open = assign + config[assign..].find('[').unwrap();
        let close = open + config[open..].find(']').unwrap();
        out.extend(
            config[open + 1..close]
                .split(',')
                .map(str::trim)
                .filter(|item| item.starts_with('"'))
                .map(|item| item.trim_matches('"').to_owned()),
        );
    }
    assert!(!out.is_empty(), "config.rs retires no keys");
    out
}

/// The OpenRouter model ids onboarding offers first: every string literal
/// in `pub(crate) const TOP_MODELS: &[&str] = &[ ... ];`. Nothing else in
/// the source names a model a person is offered (#156).
fn top_models(openrouter: &str) -> Vec<String> {
    let header = "const TOP_MODELS: &[&str] = &[";
    let start = openrouter
        .find(header)
        .unwrap_or_else(|| panic!("{header:?} not found in {OPENROUTER}"))
        + header.len();
    let end = start
        + openrouter[start..]
            .find("];")
            .unwrap_or_else(|| panic!("TOP_MODELS is not closed in {OPENROUTER}"));
    // A model id has one '/', so "//" starts a comment.
    let out: Vec<String> = openrouter[start..end]
        .lines()
        .filter_map(|line| line.split("//").next())
        .flat_map(|line| line.split(','))
        .map(str::trim)
        .filter_map(|item| item.strip_prefix('"')?.strip_suffix('"'))
        .map(str::to_owned)
        .collect();
    assert!(!out.is_empty(), "TOP_MODELS names no model");
    out
}

/// The rows of the first Markdown table whose header starts with `first`.
fn table_rows(doc: &str, first: &str) -> Vec<Vec<String>> {
    let cells = |line: &str| -> Vec<String> {
        line.trim()
            .trim_matches('|')
            .split('|')
            .map(|cell| cell.trim().to_owned())
            .collect()
    };
    let mut lines = doc.lines().peekable();
    while let Some(line) = lines.next() {
        if !line.trim_start().starts_with('|') {
            continue;
        }
        let header = cells(line);
        if header.first().map(String::as_str) != Some(first) {
            continue;
        }
        let mut rows = vec![header];
        for line in lines.by_ref() {
            if !line.trim_start().starts_with('|') {
                break;
            }
            let row = cells(line);
            if row
                .iter()
                .all(|cell| cell.chars().all(|c| c == '-' || c == ':'))
            {
                continue;
            }
            rows.push(row);
        }
        return rows;
    }
    panic!("no table whose first header cell is {first:?}");
}

#[test]
fn readme_lists_every_key_override_in_its_own_row() {
    let config = read(CONFIG);
    let readme = read(README);
    let mut violations = Vec::new();
    let rows = table_rows(&readme, "Environment variable");
    for (_, env) in token_keys(&config) {
        let matching: Vec<&Vec<String>> = rows
            .iter()
            .skip(1)
            .filter(|row| row[0].contains(&env))
            .collect();
        match matching.as_slice() {
            [] => violations.push(format!(
                "README.md configuration table has no row for {env}"
            )),
            [row] => {
                if row[0] != format!("`{env}`") {
                    violations.push(format!(
                        "README.md row for {env} must name that variable alone, not {:?}",
                        row[0]
                    ));
                }
            }
            _ => violations.push(format!("README.md lists {env} more than once")),
        }
    }
    if !readme.contains("docs/providers.md") {
        violations.push("README.md does not point at docs/providers.md".into());
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn providers_doc_covers_every_provider_key_and_top_model() {
    let config = read(CONFIG);
    let openrouter = read(OPENROUTER);
    let doc = read(PROVIDERS_DOC);
    let mut violations = Vec::new();
    for (variant, label) in providers(&config) {
        if !doc.contains(&label) {
            violations.push(format!(
                "docs/providers.md never names {label} (LlmProvider::{variant})"
            ));
        }
        if !doc.contains(&format!("## {label}")) {
            violations.push(format!(
                "docs/providers.md has no `## {label}` setup section"
            ));
        }
    }
    for (key, env) in token_keys(&config) {
        if !doc.contains(&format!("`{key}`")) {
            violations.push(format!(
                "docs/providers.md never names the `{key}` token key"
            ));
        }
        if !doc.contains(&format!("`{env}`")) {
            violations.push(format!("docs/providers.md never names the {env} override"));
        }
    }
    for required in [
        "[llm.tokens]",
        "config.toml",
        "NOLUNE_HOME",
        "[[llm.presets]]",
        "chat_preset",
        "background_preset",
        "vendor/model",
        "NOLUNE_CODEX_BIN",
        "read-only",
        "approvalPolicy",
        "mcp_servers",
        "enabled = false",
        "dynamicTools",
        "release-checklist.md",
        "TOP_MODELS",
        "first_key_probe_model",
        "/api/config/models/available",
        "/api/config/models/choose",
    ] {
        if !doc.contains(required) {
            violations.push(format!("docs/providers.md is missing {required:?}"));
        }
    }
    for key in retired_keys(&config) {
        if !doc.contains(&format!("`{key}`")) {
            violations.push(format!(
                "docs/providers.md does not say what happens to the retired `{key}` key"
            ));
        }
    }
    for model in top_models(&openrouter) {
        if !doc.contains(&format!("`{model}`")) {
            violations.push(format!(
                "docs/providers.md does not list the OpenRouter top model `{model}`"
            ));
        }
    }
    // Nothing is seeded (#156): the retired seeding API must not come back
    // as documentation.
    for retired in [
        "default_presets",
        "seed_for_keys",
        "/api/config/models/seed",
    ] {
        if doc.contains(retired) {
            violations.push(format!(
                "docs/providers.md still names the retired {retired:?}"
            ));
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

/// The capability table has one column per provider and one row per
/// `Capabilities` field, and every cell says what the adapter's constant
/// says: `yes` or `no`, or `per model` where the adapter decides per model
/// id (it then has a `capabilities_for` function).
#[test]
fn providers_doc_capability_table_matches_the_adapters() {
    let config = read(CONFIG);
    let contract = read(CONTRACT);
    let doc = read(PROVIDERS_DOC);
    let mut violations = Vec::new();
    let rows = table_rows(&doc, "Capability");
    let header = &rows[0];
    let labels: Vec<String> = providers(&config)
        .into_iter()
        .map(|(_, label)| label)
        .collect();
    for label in &labels {
        if !header.contains(label) {
            violations.push(format!(
                "the capability table has no {label} column: {header:?}"
            ));
        }
    }
    let fields = capability_fields(&contract);
    for field in &fields {
        if !rows
            .iter()
            .skip(1)
            .any(|row| row[0] == format!("`{field}`"))
        {
            violations.push(format!("the capability table has no `{field}` row"));
        }
    }
    for row in rows.iter().skip(1) {
        let field = row[0].trim_matches('`');
        if !fields.iter().any(|known| known == field) {
            violations.push(format!(
                "the capability table names {field:?}, which Capabilities has no field for"
            ));
        }
    }
    for (label, path, constant) in ADAPTER_CAPABILITIES {
        let source = read(path);
        let expected = adapter_capabilities(&source, constant);
        let per_model = source.contains("fn capabilities_for(");
        let Some(column) = header.iter().position(|cell| cell == label) else {
            continue;
        };
        for field in &fields {
            let Some(row) = rows
                .iter()
                .skip(1)
                .find(|row| row[0] == format!("`{field}`"))
            else {
                continue;
            };
            let cell = row.get(column).map(String::as_str).unwrap_or("");
            let want = if expected[field] { "yes" } else { "no" };
            let ok = cell.starts_with(want) || (per_model && cell.starts_with("per model"));
            if !ok {
                violations.push(format!(
                    "{label} `{field}` is {want} in {path} but the table says {cell:?}"
                ));
            }
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn release_checklist_covers_model_ids_and_the_codex_pin() {
    let openrouter = read(OPENROUTER);
    let backend = read(BACKEND);
    let codex = read(CODEX_MOD);
    let checklist = read(CHECKLIST_DOC);
    let conventions = read(CONVENTIONS);
    let mut violations = Vec::new();
    let pin = quoted_after(&codex, "pub const CODEX_VERSION: &str").expect("the codex pin");
    // The model ids a release still has to look after live in these two
    // items: they must exist where the checklist says they are.
    for (source, path, item) in [
        (&openrouter, OPENROUTER, "const TOP_MODELS"),
        (&backend, BACKEND, "fn first_key_probe_model("),
    ] {
        if !source.contains(item) {
            violations.push(format!(
                "{path} no longer has {item:?}; update docs/release-checklist.md"
            ));
        }
        if !checklist.contains(path) {
            violations.push(format!(
                "docs/release-checklist.md does not say {item:?} lives in {path}"
            ));
        }
    }
    for required in [
        "TOP_MODELS",
        "first_key_probe_model",
        "bump-version.sh",
        "fixtures/codex-",
        "providers.md",
        "docs/providers.md",
        "CODEX_VERSION",
    ] {
        if !checklist.contains(required) {
            violations.push(format!("docs/release-checklist.md is missing {required:?}"));
        }
    }
    for retired in ["default_presets", "CODEX_MODELS"] {
        if checklist.contains(retired) {
            violations.push(format!(
                "docs/release-checklist.md still names the retired {retired:?}"
            ));
        }
    }
    if !checklist.contains(pin) {
        violations.push(format!(
            "docs/release-checklist.md does not name the pinned codex release {pin}"
        ));
    }
    let doc = read(PROVIDERS_DOC);
    if !doc.contains(pin) {
        violations.push(format!(
            "docs/providers.md does not name the pinned codex release {pin}"
        ));
    }
    let versioning = conventions
        .split("## Versioning")
        .nth(1)
        .and_then(|rest| rest.split("\n## ").next())
        .unwrap_or("");
    if !versioning.contains("docs/release-checklist.md") {
        violations.push(
            "CLAUDE.md's Versioning section does not point at docs/release-checklist.md".into(),
        );
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}
