//! Guard for #27 (slice d): the Codex login routes expose setup and login
//! state without exposing credentials. No route or log formats a
//! token-bearing field, nothing under the codex module reads codex's own
//! auth store, and the OpenAI provider path (the `OPEN_AI` key in
//! `config.rs`, the OpenAI and OpenRouter adapters) never touches Codex
//! OAuth state: a ChatGPT login is not an OpenAI API key.

#[path = "../test-support/source_scan.rs"]
mod source_scan;

use source_scan::without_cfg_test_items;
use std::{
    fs,
    path::{Path, PathBuf},
};

const AUTH: &str = "server/src/services/llm/codex/auth.rs";
const ROUTES: &str = "server/src/routes/codex.rs";
const CONFIG: &str = "server/src/config.rs";
const OPENAI_ADAPTER: &str = "server/src/services/llm/openai.rs";
const OPENROUTER_ADAPTER: &str = "server/src/services/llm/openrouter.rs";
const ANTHROPIC_ADAPTER: &str = "server/src/services/llm/anthropic.rs";

/// Members of codex's protocol and auth store that carry a credential, and
/// the names a copy of one would go by.
const TOKEN_BEARING: &[&str] = &[
    "access_token",
    "accessToken",
    "refresh_token",
    "refreshToken",
    "id_token",
    "idToken",
    "chatgptAuthTokens",
    "chatgpt_account_id",
    "auth.json",
    "OPENAI_API_KEY",
    "CODEX_API_KEY",
    "\"apiKey\":",
    "api_key:",
    "bearer",
    "Bearer",
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

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Production source of one file: what is left once `#[cfg(test)]` items
/// are removed.
fn production(relative: &str) -> String {
    let text = fs::read_to_string(repo().join(relative))
        .unwrap_or_else(|_| panic!("{relative} is missing"));
    without_cfg_test_items(&text)
}

/// The macros that write a log record; `log::info!(` contains `info!(`.
const LOG_MACROS: &[&str] = &["trace!(", "debug!(", "info!(", "warn!(", "error!(", "log!("];

/// What the fields of a login or an account are called; a log call that
/// formats one puts what a person was handed, or their address, on disk.
const LOGGED_FIELDS: &[&str] = &["user_code", "auth_url", "verification_url", "email"];

/// Every log macro call in `source`, each as one string from the line
/// that opens it to the line that closes its parentheses, with the number
/// of its first line. rustfmt wraps a call's arguments onto lines of their
/// own, and an argument is part of the call that formats it: scanning one
/// line at a time would miss every wrapped call. Parentheses inside string
/// literals do not count.
fn log_calls(source: &str) -> Vec<(usize, String)> {
    let mut calls = Vec::new();
    // The call being read: its first line, its text so far, and how many
    // parentheses are open.
    let mut open: Option<(usize, String, usize)> = None;
    let mut in_string = false;
    for (index, line) in source.lines().enumerate() {
        let from = match &open {
            Some(_) => 0,
            None => match LOG_MACROS.iter().filter_map(|m| line.find(m)).min() {
                Some(start) => {
                    open = Some((index + 1, String::new(), 0));
                    start
                }
                None => continue,
            },
        };
        let (first, text, depth) = open.as_mut().unwrap();
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(line.trim());
        let mut escaped = false;
        for byte in line[from..].bytes() {
            match byte {
                _ if escaped => escaped = false,
                b'\\' if in_string => escaped = true,
                b'"' => in_string = !in_string,
                b'(' if !in_string => *depth += 1,
                b')' if !in_string => {
                    *depth -= 1;
                    if *depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
        if *depth == 0 {
            calls.push((*first, std::mem::take(text)));
            open = None;
        }
    }
    calls
}

/// The log calls in `source` that format one of [`LOGGED_FIELDS`].
fn leaking_log_calls(relative: &str, source: &str) -> Vec<String> {
    let mut violations = Vec::new();
    for (line, call) in log_calls(source) {
        for field in LOGGED_FIELDS {
            if call.contains(field) {
                violations.push(format!("{relative}:{line} logs {field}: {call}"));
            }
        }
    }
    violations
}

#[test]
fn the_log_scan_reads_a_wrapped_call_as_one_call() {
    // A call rustfmt wrapped: the macro on one line, the arguments that
    // name the fields on the lines after it.
    let wrapped = concat!(
        "fn started(status: &LoginStatus) {\n",
        "    log::info!(\n",
        "        \"[codex] login started: open {} and type {}\",\n",
        "        status.verification_url.as_deref().unwrap_or(\"(none)\"),\n",
        "        status.user_code.as_deref().unwrap_or(\"\")\n",
        "    );\n",
        "    log::info!(\"[codex] logged out\");\n",
        "    let email = status.id.clone(); // not a log\n",
        "    log::warn!(\"[codex] login failed ({}): {why}\", status.method);\n",
        "}\n",
    );
    let calls = log_calls(wrapped);
    let lines: Vec<usize> = calls.iter().map(|(line, _)| *line).collect();
    assert_eq!(
        lines,
        [2, 7, 9],
        "one call each, at its first line: {calls:?}"
    );
    assert!(
        calls[0].1.contains("verification_url") && calls[0].1.contains("user_code"),
        "the wrapped arguments belong to the call: {}",
        calls[0].1
    );
    let violations = leaking_log_calls("fixture.rs", wrapped);
    assert_eq!(violations.len(), 2, "{violations:?}");
    assert!(
        violations
            .iter()
            .any(|v| v.starts_with("fixture.rs:2 logs user_code"))
    );
    assert!(
        violations
            .iter()
            .any(|v| v.starts_with("fixture.rs:2 logs verification_url"))
    );
    // A one-line call is still one call, and a field named outside a log
    // call is nobody's business here.
    let single = "log::info!(\"[codex] account {}\", account.email);\nlet user_code = 1;\n";
    let violations = leaking_log_calls("fixture.rs", single);
    assert_eq!(
        violations,
        ["fixture.rs:1 logs email: log::info!(\"[codex] account {}\", account.email);"]
    );
    assert!(leaking_log_calls("fixture.rs", "fn quiet() { let email = 1; }\n").is_empty());
}

#[test]
fn the_login_routes_and_state_name_no_token_bearing_field() {
    let mut violations = Vec::new();
    for relative in [AUTH, ROUTES] {
        let source = production(relative);
        for token in TOKEN_BEARING {
            if source.contains(token) {
                violations.push(format!("{relative} names {token:?}"));
            }
        }
        // A log call that formats what a person is handed to finish a
        // login, or the account's address, would put it on disk.
        violations.extend(leaking_log_calls(relative, &source));
    }
    // The routes answer the status types as they are; a route that builds
    // its own JSON could add what the types leave out.
    let routes = production(ROUTES);
    for required in ["Json<Status>", "Json<LoginStatus>"] {
        if !routes.contains(required) {
            violations.push(format!("{ROUTES} does not answer {required}"));
        }
    }
    for required in [
        "\"/api/config/codex/status\"",
        "\"/api/config/codex/login\"",
        "\"/api/config/codex/logout\"",
    ] {
        if !routes.contains(required) {
            violations.push(format!("{ROUTES} is missing route {required}"));
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn nothing_under_the_codex_module_reads_the_auth_store() {
    let mut violations = Vec::new();
    for path in files(&repo().join("server/src/services/llm/codex"), &["rs"]) {
        let relative = path
            .strip_prefix(repo())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let source = without_cfg_test_items(&fs::read_to_string(&path).unwrap());
        for token in [
            "auth.json",
            ".codex/",
            "CODEX_HOME",
            "codexHome",
            "home_dir(",
        ] {
            if source.contains(token) {
                violations.push(format!("{relative} reaches into codex's home: {token:?}"));
            }
        }
        for token in [
            "accessToken",
            "refreshToken",
            "idToken",
            "chatgptAuthTokens",
        ] {
            if source.contains(token) {
                violations.push(format!("{relative} handles a token: {token:?}"));
            }
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn the_openai_key_path_never_touches_codex_auth() {
    let mut violations = Vec::new();
    // `config.rs` owns `tokens.OPEN_AI` and `key_for`; a provider that logs
    // in through codex has no key there and config never asks codex.
    let config = production(CONFIG);
    for token in [
        "codex::auth",
        "codex::",
        "services::llm::codex",
        "app_server",
        "app-server",
        "auth.json",
        "NOLUNE_CODEX_BIN",
        ".codex/",
    ] {
        if config.contains(token) {
            violations.push(format!("{CONFIG} touches codex auth: {token:?}"));
        }
    }
    // The API-key adapters know nothing of the login.
    for relative in [OPENAI_ADAPTER, OPENROUTER_ADAPTER, ANTHROPIC_ADAPTER] {
        let source = production(relative);
        for token in ["codex", "app-server", "app_server"] {
            if source.contains(token) {
                violations.push(format!("{relative} mentions {token:?}"));
            }
        }
    }
    // And the login side never reads an API key: not the config's tokens,
    // not the environment.
    let auth = production(AUTH);
    for token in [
        "tokens.open_ai",
        "OPEN_AI",
        "key_for(",
        "LlmTokens",
        "env::var(\"OPENAI",
    ] {
        if auth.contains(token) {
            violations.push(format!("{AUTH} reads an API key: {token:?}"));
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn the_login_state_is_reached_through_the_codex_module_only() {
    // The login routes, the app state, the entrypoint and the LLM layer
    // (which shares the app-server) name the auth type; tools, the other
    // routes, the domain and the other services never reach the login.
    let allowed = ["server/src/routes/codex.rs", "server/src/main.rs"];
    let mut violations = Vec::new();
    for path in files(&repo().join("server/src"), &["rs"]) {
        let relative = path
            .strip_prefix(repo())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        if relative.starts_with("server/src/services/llm/")
            || relative.starts_with("server/src/app/")
            || allowed.contains(&relative.as_str())
        {
            continue;
        }
        let source = without_cfg_test_items(&fs::read_to_string(&path).unwrap());
        for token in ["codex::auth", "codex_auth"] {
            if source.contains(token) {
                violations.push(format!("{relative} reaches the login state: {token:?}"));
            }
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn the_settings_doc_describes_the_login_without_a_key() {
    let doc = fs::read_to_string(repo().join("docs/settings.md")).unwrap();
    let mut violations = Vec::new();
    for required in [
        "/api/config/codex/status",
        "/api/config/codex/login",
        "/api/config/codex/logout",
        "device code",
        "#27",
        // The tile (slice e): where it is, what it shows, and the gate.
        "CodexLogin.svelte",
        "connectOnboardingCodex",
        "Log in",
        "Log out",
    ] {
        if !doc.contains(required) {
            violations.push(format!("docs/settings.md is missing {required:?}"));
        }
    }
    if doc.contains("follows in a later slice") {
        violations.push("docs/settings.md still promises the tile for later".into());
    }
    let providers = fs::read_to_string(repo().join("docs/providers.md")).unwrap();
    if !providers.contains("/api/config/codex/status") {
        violations.push("docs/providers.md does not point at the login routes".into());
    }
    // Bumping the pin is a release step with a checklist, next to the pin.
    for required in ["CODEX_VERSION", "Release checklist", "re-record"] {
        if !providers.contains(required) {
            violations.push(format!("docs/providers.md is missing {required:?}"));
        }
    }
    if providers.contains("follows in the remaining slice") {
        violations.push("docs/providers.md still promises the tile for later".into());
    }
    let readme = fs::read_to_string(repo().join("README.md")).unwrap();
    for required in ["Codex", "NOLUNE_CODEX_BIN"] {
        if !readme.contains(required) {
            violations.push(format!("README.md is missing {required:?}"));
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

/// The client files that speak of the login: scanned whole (their node
/// test, which says what it forbids, is not).
const CLIENT_CODEX_FILES: &[&str] = &[
    "client/src/lib/models/codex.js",
    "client/src/lib/components/settings/CodexLogin.svelte",
];

/// The client files where the login shares a module with keyed providers
/// (`api_key` is OpenAI's payload there): only the blocks that name
/// `Codex` are scanned.
const CLIENT_SHARED_FILES: &[&str] = &[
    "client/src/lib/api/client.ts",
    "client/src/lib/components/onboarding/provider.js",
    "client/src/lib/components/onboarding/InstanceOnboarding.svelte",
];

/// Every brace-delimited block whose opening line names `Codex` (a type,
/// an interface, a function, a Svelte `{#if}` is not one), from that line
/// to the line that closes it, with the number of its first line.
fn codex_blocks(source: &str) -> Vec<(usize, String)> {
    let mut blocks = Vec::new();
    let mut open: Option<(usize, String, i32)> = None;
    for (index, line) in source.lines().enumerate() {
        if open.is_none() {
            if !(line.contains("Codex") || line.contains("codex")) || !line.contains('{') {
                continue;
            }
            open = Some((index + 1, String::new(), 0));
        }
        let (_, text, depth) = open.as_mut().unwrap();
        text.push_str(line);
        text.push('\n');
        for byte in line.bytes() {
            match byte {
                b'{' => *depth += 1,
                b'}' => *depth -= 1,
                _ => {}
            }
        }
        if *depth <= 0 {
            let (first, text, _) = open.take().unwrap();
            blocks.push((first, text));
        }
    }
    blocks
}

/// A member or a word a token could hide behind, matched the way the
/// route tests match the answers (`assert_no_token_bearing_member`).
fn names_a_credential(text: &str) -> Option<&'static str> {
    let lowered = text.to_lowercase();
    ["token", "secret", "apikey", "api_key:", "password", "cookie"]
        .into_iter()
        .find(|banned| lowered.contains(banned))
        .or_else(|| TOKEN_BEARING.iter().copied().find(|banned| text.contains(banned)))
}

#[test]
fn the_client_shows_the_login_and_carries_no_token_bearing_field() {
    let mut violations = Vec::new();

    // The pure module and the tile: no word for a credential.
    for relative in CLIENT_CODEX_FILES {
        let Ok(source) = fs::read_to_string(repo().join(relative)) else {
            violations.push(format!("{relative} is missing"));
            continue;
        };
        for (number, line) in source.lines().enumerate() {
            if let Some(banned) = names_a_credential(line) {
                violations.push(format!("{relative}:{} names {banned:?}", number + 1));
            }
        }
    }
    // The shared modules: every block about Codex, whole.
    for relative in CLIENT_SHARED_FILES {
        let source = fs::read_to_string(repo().join(relative))
            .unwrap_or_else(|_| panic!("{relative} is missing"));
        for (first, block) in codex_blocks(&source) {
            if let Some(banned) = names_a_credential(&block) {
                violations.push(format!(
                    "{relative}:{first} (a Codex block) names {banned:?}"
                ));
            }
        }
    }

    // The API: status, login and logout, and the provider union.
    let client = fs::read_to_string(repo().join("client/src/lib/api/client.ts")).unwrap();
    for required in [
        "export function fetchCodexStatus(",
        "export async function startCodexLogin(",
        "export async function logoutCodex(",
        "\"/api/config/codex/status\"",
        "\"/api/config/codex/login\"",
        "\"/api/config/codex/logout\"",
        "| \"codex\"",
    ] {
        if !client.contains(required) {
            violations.push(format!("client.ts is missing {required:?}"));
        }
    }
    // The provider list: codex logs in, the others hold a key.
    let presets = fs::read_to_string(repo().join("client/src/lib/models/presets.js")).unwrap();
    for required in ["id: \"codex\"", "auth: \"login\"", "export function providerAuth("] {
        if !presets.contains(required) {
            violations.push(format!("presets.js is missing {required:?}"));
        }
    }
    // The tile sits in Settings › Connections and onboarding offers Codex.
    let connections = fs::read_to_string(
        repo().join("client/src/routes/[slug]/settings/connections/+page.svelte"),
    )
    .unwrap();
    if !connections.contains("<CodexLogin") {
        violations.push("Settings › Connections does not render the Codex tile".into());
    }
    let onboarding = fs::read_to_string(
        repo().join("client/src/lib/components/onboarding/InstanceOnboarding.svelte"),
    )
    .unwrap();
    for required in ["pickProvider(\"codex\")", "connectOnboardingCodex", "startCodexLogin"] {
        if !onboarding.contains(required) {
            violations.push(format!("onboarding is missing {required:?}"));
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn the_client_block_scan_reads_a_codex_block_whole() {
    let source = "export interface CodexStatus {\n\tbinary: CodexBinaryStatus;\n\tnope: string;\n}\nexport interface Other {\n\tid_token: string;\n}\nfunction fetchCodexStatus() {\n\treturn json(\"/x\", { headers: { \"X-Y\": \"z\" } });\n}\n";
    let blocks = codex_blocks(source);
    assert_eq!(blocks.len(), 2, "{blocks:?}");
    assert_eq!(blocks[0].0, 1);
    assert!(blocks[0].1.contains("nope: string"));
    assert!(!blocks[0].1.contains("Other"));
    assert_eq!(blocks[1].0, 8);
    assert!(blocks[1].1.contains("X-Y"), "nested braces close in order");
    assert_eq!(names_a_credential(&blocks[0].1), None);
    assert_eq!(
        names_a_credential("export interface CodexLogin { refreshToken: string }"),
        Some("token")
    );
    assert_eq!(names_a_credential("user_code: string"), None);
    assert_eq!(names_a_credential("auth_url?: string"), None);
}
