//! Guard for #137: Anthropic prompt-cache entries carry an explicit ttl that
//! follows the execution scope. Conversations cache for an hour, subagent
//! runs (companion routines, background one-shots) for five minutes, and the
//! two literals live in exactly one mapping.

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

fn production(repo: &Path, relative: &str) -> String {
    without_cfg_test_items(
        &fs::read_to_string(repo.join(relative))
            .unwrap_or_else(|_| panic!("{relative} is missing")),
    )
}

/// `fn name(` and its body, up to the next item at the same indentation.
fn method(source: &str, name: &str) -> String {
    let start = source
        .find(&format!("fn {name}("))
        .unwrap_or_else(|| panic!("fn {name} is missing"));
    let rest = &source[start..];
    let end = ["\n    /// ", "\n    #[", "\n    pub ", "\n}"]
        .iter()
        .filter_map(|marker| rest.find(marker))
        .min()
        .unwrap_or(rest.len());
    rest[..end].to_owned()
}

#[test]
fn anthropic_breakpoints_always_carry_a_ttl() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let adapter = production(repo, "server/src/services/llm/anthropic.rs");
    let mut violations = Vec::new();

    let mut breakpoints = 0;
    for (at, _) in adapter.match_indices("\"ephemeral\"") {
        breakpoints += 1;
        let object = &adapter[at..];
        let object = &object[..object.find('}').unwrap_or(object.len())];
        if !object.contains("\"ttl\"") {
            violations.push(format!(
                "anthropic.rs writes a cache_control without a ttl (the API would default to 5m): {object}"
            ));
        }
    }
    if breakpoints == 0 {
        violations.push("anthropic.rs no longer writes any ephemeral cache_control".into());
    }
    if adapter.matches("\"cache_control\"").count() < 3 {
        violations.push(
            "anthropic.rs must keep the system-block, last-tool and top-level breakpoints".into(),
        );
    }
    if !adapter.contains(".cache_ttl()") {
        violations.push("anthropic.rs must take the ttl from ExecutionScope::cache_ttl".into());
    }

    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn cache_ttl_literals_live_only_in_the_scope_mapping() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut violations = Vec::new();

    let contract = production(repo, "server/src/services/llm/contract.rs");
    for required in [
        "pub enum ExecutionScope",
        "Conversation,",
        "Subagent,",
        "pub scope: ExecutionScope",
    ] {
        if !contract.contains(required) {
            violations.push(format!("contract.rs is missing {required:?}"));
        }
    }
    let mapping = method(&contract, "cache_ttl");
    for (literal, scope) in [("\"1h\"", "Conversation"), ("\"5m\"", "Subagent")] {
        let inside = mapping.matches(literal).count();
        if inside != 1 {
            violations.push(format!(
                "ExecutionScope::cache_ttl must map {scope} to {literal} exactly once, found {inside}"
            ));
        }
        let total = contract.matches(literal).count();
        if total != inside {
            violations.push(format!(
                "contract.rs repeats {literal} outside ExecutionScope::cache_ttl"
            ));
        }
    }

    for path in files(&repo.join("server/src"), &["rs"]) {
        let relative = path
            .strip_prefix(repo)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        if relative == "server/src/services/llm/contract.rs" {
            continue;
        }
        let source = without_cfg_test_items(&fs::read_to_string(&path).unwrap());
        for literal in ["\"1h\"", "\"5m\""] {
            if source.contains(literal) {
                violations.push(format!(
                    "{relative} contains {literal}; the cache ttl is defined once in ExecutionScope::cache_ttl"
                ));
            }
        }
    }

    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn entry_points_declare_their_execution_scope() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut violations = Vec::new();

    let backend = production(repo, "server/src/services/llm/mod.rs");
    let expectations = [
        ("chat_with_tools_streaming", "Conversation", "Subagent"),
        ("chat", "Subagent", "Conversation"),
        ("chat_json", "Subagent", "Conversation"),
        ("chat_with_tools_only", "Subagent", "Conversation"),
        ("chat_with_tools_traced", "Subagent", "Conversation"),
    ];
    for (name, wanted, forbidden) in expectations {
        let body = method(&backend, name);
        if !body.contains(&format!("ExecutionScope::{wanted}")) {
            violations.push(format!(
                "LlmBackend::{name} must run in ExecutionScope::{wanted}"
            ));
        }
        if body.contains(&format!("ExecutionScope::{forbidden}")) {
            violations.push(format!(
                "LlmBackend::{name} must not run in ExecutionScope::{forbidden}"
            ));
        }
    }

    // The loops forward the caller's scope instead of choosing one.
    let loops = production(repo, "server/src/services/llm/agent_loop.rs");
    if loops.matches("LlmRequest::new(scope,").count() != 2 {
        violations.push(
            "agent_loop.rs: complete_once and stream_once must build LlmRequest::new(scope, ..)"
                .into(),
        );
    }
    for variant in ["ExecutionScope::Conversation", "ExecutionScope::Subagent"] {
        if loops.contains(variant) {
            violations.push(format!(
                "agent_loop.rs hard-codes {variant}; the scope belongs to the LlmBackend entry point"
            ));
        }
    }

    assert!(violations.is_empty(), "{}", violations.join("\n"));
}
