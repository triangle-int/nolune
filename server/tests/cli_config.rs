//! `nolune config …`: the settings the companion used to change through the
//! get_settings/update_config tools, now a command it runs through its
//! built-in configure-nolune skill. Driven through the real binary against a
//! throwaway `NOLUNE_HOME`.

use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

const BIN: &str = env!("CARGO_BIN_EXE_nolune");
const SKILL: &str = include_str!("../builtin-skills/configure-nolune/SKILL.md");

fn nolune(home: &Path, args: &[&str], stdin: Option<&str>) -> Output {
    let mut child = Command::new(BIN)
        .env("NOLUNE_HOME", home)
        .env("RUST_LOG", "warn")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut pipe = child.stdin.take().unwrap();
    if let Some(input) = stdin {
        pipe.write_all(input.as_bytes()).unwrap();
    }
    drop(pipe);
    child.wait_with_output().unwrap()
}

fn ok(home: &Path, args: &[&str], stdin: Option<&str>) -> String {
    let out = nolune(home, args, stdin);
    assert!(
        out.status.success(),
        "{args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn fails(home: &Path, args: &[&str], stdin: Option<&str>) -> String {
    let out = nolune(home, args, stdin);
    assert!(!out.status.success(), "{args:?} succeeded");
    String::from_utf8(out.stderr).unwrap()
}

fn onboarded() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    ok(home.path(), &["onboard", "--json"], None);
    home
}

#[test]
fn config_needs_an_onboarded_data_root() {
    let home = tempfile::tempdir().unwrap();
    let error = fails(home.path(), &["config", "show"], None);
    assert!(error.contains("run `nolune onboard` first"), "{error}");
}

#[test]
fn timezone_and_name_are_plain_arguments() {
    let home = onboarded();
    let root = home.path();
    let shown = ok(root, &["config", "show"], None);
    assert!(shown.contains("timezone: UTC (not set)"), "{shown}");
    assert!(shown.contains("companion name: (not set)"), "{shown}");

    ok(root, &["config", "set", "timezone", "Asia/Kathmandu"], None);
    ok(root, &["config", "set", "name", "Luna"], None);
    let shown = ok(root, &["config", "show"], None);
    assert!(shown.contains("timezone: Asia/Kathmandu"), "{shown}");
    assert!(shown.contains("companion name: Luna"), "{shown}");

    let error = fails(
        root,
        &["config", "set", "timezone", "Mars/Olympus_Mons"],
        None,
    );
    assert!(error.contains("invalid timezone"), "{error}");
    let error = fails(root, &["config", "set", "name"], None);
    assert!(error.contains("nolune config unset"), "{error}");

    ok(root, &["config", "unset", "timezone"], None);
    let state = fs::read_to_string(root.join("instances/companion/project_state.json")).unwrap();
    let state: serde_json::Value = serde_json::from_str(&state).unwrap();
    assert_eq!(state["timezone"], "");
    assert_eq!(state["identity"]["name"], "Luna");
}

#[test]
fn secrets_come_from_stdin_and_are_never_printed() {
    let home = onboarded();
    let root = home.path();
    let key = "sk-ant-cli-config-test-0123456789";

    let error = fails(root, &["config", "set", "anthropic-key", key], None);
    assert!(error.contains("stdin"), "{error}");
    let error = fails(root, &["config", "set", "anthropic-key"], Some("\n"));
    assert!(error.contains("no anthropic key on stdin"), "{error}");

    let said = ok(
        root,
        &["config", "set", "anthropic-key"],
        Some(&format!("{key}\n")),
    );
    assert!(!said.contains(key), "{said}");
    let config = fs::read_to_string(root.join("config.toml")).unwrap();
    assert!(config.contains(key), "{config}");
    // The auth token onboard wrote survives the save.
    assert!(config.contains("auth_token"), "{config}");
    let shown = ok(root, &["config", "show"], None);
    assert!(shown.contains("api keys: anthropic"), "{shown}");
    assert!(!shown.contains(key), "{shown}");

    ok(
        root,
        &["config", "set", "github-token"],
        Some("ghp_cliconfigtest\n"),
    );
    let instance = fs::read_to_string(root.join("instances/companion/instance.toml")).unwrap();
    assert!(instance.contains("ghp_cliconfigtest"), "{instance}");
    let shown = ok(root, &["config", "show"], None);
    assert!(shown.contains("github: token configured"), "{shown}");
    assert!(!shown.contains("ghp_cliconfigtest"), "{shown}");

    ok(root, &["config", "unset", "anthropic-key"], None);
    let config = fs::read_to_string(root.join("config.toml")).unwrap();
    assert!(!config.contains(key), "{config}");
}

#[test]
fn email_accounts_take_their_password_from_stdin() {
    let home = onboarded();
    let root = home.path();
    let add = [
        "config",
        "email",
        "add",
        "--smtp-host",
        "smtp.example.com",
        "--smtp-user",
        "me@example.com",
        "--from",
        "me@example.com",
        "--imap-host",
        "imap.example.com",
    ];

    let error = fails(root, &add, None);
    assert!(error.contains("no password on stdin"), "{error}");
    ok(root, &add, Some("smtp-secret\nimap-secret\n"));
    let error = fails(root, &add, Some("smtp-secret\n"));
    assert!(error.contains("already exists"), "{error}");

    let email = fs::read_to_string(root.join("instances/companion/email.toml")).unwrap();
    for expected in [
        "smtp.example.com",
        "smtp-secret",
        "imap-secret",
        "smtp_port = 587",
        "imap_port = 993",
        "imap_user = \"me@example.com\"",
    ] {
        assert!(email.contains(expected), "{expected}: {email}");
    }
    let shown = ok(root, &["config", "show"], None);
    assert!(
        shown.contains("email accounts (smtp/imap): me@example.com"),
        "{shown}"
    );
    assert!(!shown.contains("secret"), "{shown}");

    ok(root, &["config", "email", "remove", "me@example.com"], None);
    let shown = ok(root, &["config", "show"], None);
    assert!(
        shown.contains("email accounts (smtp/imap): none"),
        "{shown}"
    );
    let error = fails(root, &["config", "email", "remove", "me@example.com"], None);
    assert!(error.contains("no email account"), "{error}");
}

/// The skill teaches exactly the commands and keys the binary accepts.
#[test]
fn the_configure_skill_matches_the_command() {
    let home = tempfile::tempdir().unwrap();
    let help = ok(home.path(), &["config", "--help"], None);
    for command in ["show", "set", "unset", "email"] {
        assert!(help.contains(command), "{command}: {help}");
        assert!(
            SKILL.contains(&format!("nolune config {command}")),
            "the skill never shows `nolune config {command}`"
        );
    }
    let help = ok(home.path(), &["config", "set", "--help"], None);
    for key in [
        "timezone",
        "name",
        "openai-key",
        "anthropic-key",
        "brave-search-key",
        "github-token",
    ] {
        assert!(help.contains(key), "{key}: {help}");
        assert!(SKILL.contains(key), "the skill never names {key}");
    }
    let help = ok(home.path(), &["config", "email", "add", "--help"], None);
    for flag in [
        "--smtp-host",
        "--smtp-port",
        "--smtp-user",
        "--from",
        "--imap-host",
        "--imap-port",
        "--imap-user",
    ] {
        assert!(help.contains(flag), "{flag}: {help}");
        assert!(SKILL.contains(flag), "the skill never names {flag}");
    }
}
