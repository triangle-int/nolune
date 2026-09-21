//! Guard for #24, #25 and #26: each provider's wire format lives in its
//! adapter, and the rest of the server acts on typed `LlmError` variants
//! rather than on status codes found in error strings.

#[path = "../test-support/source_scan.rs"]
mod source_scan;

use source_scan::without_cfg_test_items;
use std::{
    fs,
    path::{Path, PathBuf},
};

const ANTHROPIC_ADAPTER: &str = "server/src/services/llm/anthropic.rs";
const OPENAI_ADAPTER: &str = "server/src/services/llm/openai.rs";
const OPENROUTER_ADAPTER: &str = "server/src/services/llm/openrouter.rs";
const CODEX_ADAPTER: &str = "server/src/services/llm/codex/adapter.rs";
/// The codex app-server's protocol (#27) lives under this directory only.
const CODEX_MODULE: &str = "server/src/services/llm/codex/";
const BACKEND_TYPES: &str = "server/src/services/llm/types.rs";

/// Every adapter that turns a provider's answers into `LlmError` variants.
const ADAPTERS: [&str; 4] = [
    ANTHROPIC_ADAPTER,
    OPENAI_ADAPTER,
    OPENROUTER_ADAPTER,
    CODEX_ADAPTER,
];

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

/// Every production Rust source under `server/src` as `(repo-relative path, source)`.
fn production_sources(repo: &Path) -> Vec<(String, String)> {
    files(&repo.join("server/src"), &["rs"])
        .into_iter()
        .map(|path| {
            let relative = path
                .strip_prefix(repo)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let source = without_cfg_test_items(&fs::read_to_string(&path).unwrap());
            (relative, source)
        })
        .collect()
}

fn source<'a>(sources: &'a [(String, String)], relative: &str) -> &'a str {
    sources
        .iter()
        .find(|(path, _)| path == relative)
        .map(|(_, source)| source.as_str())
        .unwrap_or_else(|| panic!("{relative} is missing"))
}

/// `tokens` must appear in `owner` and in no other production source.
fn assert_owned(
    sources: &[(String, String)],
    owner: &str,
    tokens: &[&str],
    violations: &mut Vec<String>,
) {
    let owned = source(sources, owner);
    for token in tokens {
        if !owned.contains(token) {
            violations.push(format!("{owner} no longer contains {token:?}"));
        }
        for (relative, source) in sources {
            if relative != owner && source.contains(token) {
                violations.push(format!(
                    "{relative} contains {token:?}; only {owner} speaks that wire format"
                ));
            }
        }
    }
}

#[test]
fn anthropic_wire_format_lives_only_in_its_adapter() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let sources = production_sources(repo);
    let mut violations = Vec::new();

    assert_owned(
        &sources,
        ANTHROPIC_ADAPTER,
        &[
            "messages_to_anthropic",
            "x-api-key",
            "anthropic-version",
            "/v1/messages",
        ],
        &mut violations,
    );
    assert_owned(
        &sources,
        BACKEND_TYPES,
        &["\"https://api.anthropic.com\""],
        &mut violations,
    );
    if !source(&sources, ANTHROPIC_ADAPTER).contains("count_tokens") {
        violations.push(format!(
            "{ANTHROPIC_ADAPTER} must own the count_tokens request; orchestration only asks the adapter"
        ));
    }

    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn openai_wire_format_lives_only_in_its_adapter() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let sources = production_sources(repo);
    let mut violations = Vec::new();

    assert_owned(
        &sources,
        OPENAI_ADAPTER,
        &["/v1/responses"],
        &mut violations,
    );
    assert_owned(
        &sources,
        BACKEND_TYPES,
        &["\"https://api.openai.com\""],
        &mut violations,
    );

    // The OpenAI and OpenRouter keys travel as `Authorization: Bearer`. The
    // server's own API token is a bearer token too (app/auth.rs); nothing
    // else sends one.
    let bearer_line = |source: &str| {
        source
            .lines()
            .any(|line| line.contains("Authorization") && line.contains("Bearer"))
    };
    for adapter in [OPENAI_ADAPTER, OPENROUTER_ADAPTER] {
        if !bearer_line(source(&sources, adapter)) {
            violations.push(format!(
                "{adapter} no longer sends the key as an Authorization: Bearer header"
            ));
        }
    }
    for (relative, source) in &sources {
        if relative == OPENAI_ADAPTER
            || relative == OPENROUTER_ADAPTER
            || relative == "server/src/app/auth.rs"
        {
            continue;
        }
        if bearer_line(source) {
            violations.push(format!(
                "{relative} sends an Authorization: Bearer header; only {OPENAI_ADAPTER} and {OPENROUTER_ADAPTER} carry a provider key"
            ));
        }
    }

    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

/// OpenRouter (#26): the Chat Completions path, the model catalog path and
/// the attribution header names live in the adapter; the host in types.rs.
#[test]
fn openrouter_wire_format_lives_only_in_its_adapter() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let sources = production_sources(repo);
    let mut violations = Vec::new();

    assert_owned(
        &sources,
        OPENROUTER_ADAPTER,
        &[
            "/api/v1/chat/completions",
            "/api/v1/models",
            "HTTP-Referer",
            "X-Title",
        ],
        &mut violations,
    );
    assert_owned(
        &sources,
        BACKEND_TYPES,
        &["\"https://openrouter.ai\""],
        &mut violations,
    );
    // Attribution is opt-in: the adapter reads what `[llm.openrouter]`
    // says and never the instance's own address.
    if source(&sources, OPENROUTER_ADAPTER).contains("public_url") {
        violations.push(format!(
            "{OPENROUTER_ADAPTER} reads public_url; attribution comes from [llm.openrouter] only"
        ));
    }

    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

/// Codex (#27): the app-server's methods, params and error shapes are
/// spoken under `services/llm/codex/` only; the adapter turns them into the
/// provider-neutral events and typed errors like the HTTP adapters do, and
/// the module never reads the login itself.
#[test]
fn codex_wire_format_lives_only_under_its_module() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let sources = production_sources(repo);
    let mut violations = Vec::new();

    let wire = [
        "thread/start",
        "thread/resume",
        "turn/start",
        "turn/interrupt",
        "item/tool/call",
        "item/agentMessage/delta",
        "turn/completed",
        "dynamicTools",
        "developerInstructions",
        "approvalPolicy",
        "codexErrorInfo",
        "account/read",
    ];
    for (relative, source) in &sources {
        if relative.starts_with(CODEX_MODULE) {
            continue;
        }
        for token in wire {
            if source.contains(token) {
                violations.push(format!(
                    "{relative} contains {token:?}; only {CODEX_MODULE} speaks the app-server protocol"
                ));
            }
        }
    }
    let adapter = source(&sources, CODEX_ADAPTER);
    for required in [
        "thread/start",
        "thread/resume",
        "turn/start",
        "turn/interrupt",
        "item/tool/call",
        "dynamicTools",
        "\"read-only\"",
        "\"never\"",
        "LlmEvent::TextDelta",
        "LlmEvent::ToolCallStarted",
        "LlmEvent::Usage",
    ] {
        if !adapter.contains(required) {
            violations.push(format!("{CODEX_ADAPTER} no longer contains {required:?}"));
        }
    }
    // Nolune never touches the login: no path into codex's home, no token
    // field, and the OpenAI key never stands in for a login.
    for (relative, source) in &sources {
        if !relative.starts_with(CODEX_MODULE) {
            continue;
        }
        for token in [
            "auth.json",
            "access_token",
            "refresh_token",
            "id_token",
            "OPENAI_API_KEY",
            "tokens.open_ai",
            "api_key",
        ] {
            if source.contains(token) {
                violations.push(format!(
                    "{relative} contains {token:?}; the codex module never reads or forwards a credential"
                ));
            }
        }
    }

    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn provider_errors_are_matched_by_variant_not_by_status_strings() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let sources = production_sources(repo);
    let mut violations = Vec::new();

    // Status codes and provider phrases are only read where the wire
    // format is: the adapters turn them into variants.
    for (relative, source) in &sources {
        if ADAPTERS.contains(&relative.as_str()) {
            continue;
        }
        for token in [
            "\"429\"",
            "\"401\"",
            "\"529\"",
            "Too Many Requests",
            "\"overloaded\"",
            "\"rate_limit\"",
            "contains(\"rate limit",
            "contains(\"authentication",
        ] {
            if source.contains(token) {
                violations.push(format!(
                    "{relative} matches provider errors by string {token}; match on LlmError instead"
                ));
            }
        }
    }

    // The three call sites that used to string-match name the variants.
    for (relative, required) in [
        (
            "server/src/services/llm/helpers.rs",
            &["LlmError::RateLimited"][..],
        ),
        (
            "server/src/services/chat.rs",
            &["LlmError::RateLimited"][..],
        ),
        (
            "server/src/routes/chat.rs",
            &[
                "LlmError::RateLimited",
                "LlmError::Authentication",
                "LlmError::ContextLength",
            ][..],
        ),
    ] {
        let source = source(&sources, relative);
        for token in required {
            if !source.contains(token) {
                violations.push(format!("{relative} does not match on {token}"));
            }
        }
    }

    // Every adapter produces every variant the callers act on.
    for adapter in ADAPTERS {
        let source = source(&sources, adapter);
        for variant in [
            "LlmError::Authentication",
            "LlmError::RateLimited",
            "LlmError::ContextLength",
        ] {
            if !source.contains(variant) {
                violations.push(format!("{adapter} never produces {variant}"));
            }
        }
    }

    assert!(violations.is_empty(), "{}", violations.join("\n"));
}
