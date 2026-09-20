//! Guard for #16: the server speaks the shared Cua machine protocol.
//!
//! The Cua targets registry is typed by `cua_protocol`, the legacy
//! coordinate-only action vocabulary of `computer_use` is frozen so new machine
//! actions land in the protocol instead, CI runs the protocol crate's tests,
//! and the server-local target is started by the gateway, stopped by its
//! shutdown hook, sessioned only by the runtime, and documented.

#[path = "../test-support/source_scan.rs"]
mod source_scan;

use source_scan::without_cfg_test_items;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

/// The coordinate-only actions the legacy desktop agent understands. Anything
/// new must be a `cua_protocol::CuaAction`, never another entry here.
const LEGACY_ACTIONS: [&str; 10] = [
    "screenshot",
    "left_click",
    "right_click",
    "middle_click",
    "double_click",
    "mouse_move",
    "type",
    "key",
    "scroll",
    "switch_desktop",
];

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn rust_files(root: &Path) -> Vec<PathBuf> {
    fn visit(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    visit(root, &mut out);
    out.sort();
    out
}

fn production(path: &Path) -> String {
    without_cfg_test_items(&fs::read_to_string(path).unwrap())
}

#[test]
fn server_depends_on_and_uses_the_shared_cua_protocol() {
    let repo = repo();
    let manifest = fs::read_to_string(repo.join("server/Cargo.toml")).unwrap();
    assert!(
        manifest.contains("cua-protocol = { path = \"../cua-protocol\" }"),
        "server/Cargo.toml must depend on the workspace cua-protocol crate"
    );

    let registry = production(&repo.join("server/src/services/machine_registry.rs"));
    for required in [
        "cua_protocol::",
        "CheckedCuaAdapter",
        "MachineDescriptor",
        "select_machine(",
        "pub struct CuaTargets",
    ] {
        assert!(
            registry.contains(required),
            "machine_registry.rs must reference {required}"
        );
    }

    let tools = production(&repo.join("server/src/services/tools/computer.rs"));
    assert!(
        tools.contains(".cua()"),
        "list_machines must enumerate the Cua targets beside the legacy agents"
    );
}

#[test]
fn the_server_local_runtime_is_wired_sessioned_and_documented() {
    let repo = repo();

    // Constructed with the state, started and stopped by the gateway.
    let state = production(&repo.join("server/src/app/state.rs"));
    assert!(
        state.contains("CuaRuntime::new(") && state.contains(".cua()"),
        "AppState must construct the server-local Cua runtime over the registry's targets"
    );
    let main = production(&repo.join("server/src/main.rs"));
    assert!(
        main.contains(".cua.start()"),
        "main.rs must start the server-local runtime before serving"
    );
    assert!(
        main.contains(".cua.shutdown()"),
        "main.rs must end every driver session and stop the driver on shutdown"
    );
    let module = fs::read_to_string(repo.join("server/src/services/cua/mod.rs")).unwrap();
    assert!(
        !module.contains("allow(dead_code)"),
        "the driver modules are wired now; nothing in services/cua is dead"
    );

    // The [cua] section is real config.
    let config = production(&repo.join("server/src/config.rs"));
    assert!(
        config.contains("pub struct CuaConfig") && config.contains("pub cua: CuaConfig"),
        "config.rs must carry the [cua] section"
    );

    // Sessions are opened and closed by the runtime alone, so cleanup is
    // deterministic: no other production code starts or ends one.
    for path in rust_files(&repo.join("server/src")) {
        let relative = path
            .strip_prefix(&repo)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        if relative.starts_with("server/src/services/cua/") {
            continue;
        }
        let source = production(&path);
        for managed in ["CuaAction::StartSession(", "CuaAction::EndSession("] {
            assert!(
                !source.contains(managed),
                "{relative} manages driver sessions itself; go through services::cua::runtime"
            );
        }
    }

    // Documented: how the target appears, when it does not, and what it never does.
    let doc = fs::read_to_string(repo.join("docs/computer-use.md"))
        .expect("docs/computer-use.md documents the server-local target");
    for required in [
        "server-local:",
        "[cua]",
        "NOLUNE_CUA_DRIVER",
        "driver_path",
        "DISPLAY",
        "headless",
        "start_session",
        "end_session",
        "one-shot",
        "#16",
    ] {
        assert!(
            doc.contains(required),
            "docs/computer-use.md is missing {required:?}"
        );
    }
    let readme = fs::read_to_string(repo.join("README.md")).unwrap();
    assert!(
        readme.contains("| `NOLUNE_CUA_DRIVER` |"),
        "README.md must list NOLUNE_CUA_DRIVER in the environment table"
    );
    assert!(
        readme.contains("docs/computer-use.md"),
        "README.md must point at docs/computer-use.md"
    );
}

#[test]
fn legacy_coordinate_actions_are_frozen() {
    let repo = repo();
    let computer = production(&repo.join("server/src/services/tools/computer.rs"));

    // The tool description is the model-facing contract; it lists exactly the
    // frozen vocabulary and nothing else.
    let start = computer
        .find("Available actions:")
        .expect("computer_use description lists its actions");
    let clause = &computer[start + "Available actions:".len()..];
    let end = clause.find('.').expect("action list ends with a period");
    let advertised: BTreeSet<String> = clause[..end]
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .collect();
    let frozen: BTreeSet<String> = LEGACY_ACTIONS.iter().map(|s| (*s).to_owned()).collect();
    assert_eq!(
        advertised, frozen,
        "computer_use advertises a coordinate-only action outside the frozen set"
    );

    // Only the legacy tools build raw agent toolcalls; every other machine
    // action goes through the typed protocol.
    for path in rust_files(&repo.join("server/src")) {
        let relative = path
            .strip_prefix(&repo)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        if relative == "server/src/services/tools/computer.rs" {
            continue;
        }
        let source = production(&path);
        assert!(
            !source.contains("AgentToolCall {")
                || relative == "server/src/services/machine_registry.rs",
            "{relative} builds a legacy AgentToolCall; use cua_protocol::CuaAction instead"
        );
    }
}

/// The `include:` entries of the Rust CI matrix for one component, each as its
/// own block of trimmed `key: value` lines. An entry ends at the next line that
/// is indented no deeper than its `- component:` header (the next entry or the
/// end of the list); a commented-out entry never starts a block.
fn rust_matrix_entries(ci: &str, component: &str) -> Vec<String> {
    let header = format!("- component: {component}");
    let mut entries = Vec::new();
    let mut open: Option<(usize, String)> = None;
    for line in ci.lines() {
        let trimmed = line.trim();
        let indent = line.len() - line.trim_start().len();
        if let Some((start, block)) = open.as_mut() {
            if !trimmed.is_empty() && indent <= *start {
                entries.push(std::mem::take(block));
                open = None;
            } else {
                block.push_str(trimmed);
                block.push('\n');
            }
        }
        if trimmed == header {
            open = Some((indent, String::new()));
        }
    }
    if let Some((_, block)) = open {
        entries.push(block);
    }
    entries
}

/// Checks that some `cua-protocol` matrix entry runs `cargo test` against the
/// protocol crate's manifest, not merely that the manifest is mentioned.
fn cua_protocol_tests_run_in(ci: &str) -> Result<(), String> {
    let entries = rust_matrix_entries(ci, "cua-protocol");
    if entries.is_empty() {
        return Err("the Rust matrix must include cua-protocol so its tests run in CI".into());
    }
    let runs_tests = entries.iter().any(|entry| {
        let has = |key: &str| entry.lines().any(|line| line == key);
        has("manifest: cua-protocol/Cargo.toml") && has("check: test")
    });
    if runs_tests {
        Ok(())
    } else {
        Err(format!(
            "no cua-protocol matrix entry pairs `manifest: cua-protocol/Cargo.toml` with \
             `check: test`; the `Run tests` step only runs for check == 'test'. Entries:\n{}",
            entries.join("---\n")
        ))
    }
}

#[test]
fn ci_runs_the_cua_protocol_tests() {
    let ci = fs::read_to_string(repo().join(".github/workflows/ci.yml")).unwrap();
    if let Err(reason) = cua_protocol_tests_run_in(&ci) {
        panic!("{reason}");
    }
}

#[test]
fn cua_protocol_ci_guard_rejects_entries_that_never_run_cargo_test() {
    let matrix = |entries: &str| {
        format!(
            "jobs:\n  rust:\n    strategy:\n      matrix:\n        include:\n{entries}    runs-on: x\n"
        )
    };
    let entry = |component: &str, check: &str, prefix: &str| {
        format!(
            "          {prefix}- component: {component}\n            {prefix}manifest: {component}/Cargo.toml\n            {prefix}os: ubuntu-latest\n            {prefix}check: {check}\n"
        )
    };

    let server_test = entry("server", "test", "");
    let protocol_test = entry("cua-protocol", "test", "");
    let protocol_clippy = entry("cua-protocol", "clippy", "");
    let protocol_commented = entry("cua-protocol", "test", "# ");

    assert!(
        cua_protocol_tests_run_in(&matrix(&format!("{server_test}{protocol_test}"))).is_ok(),
        "a cua-protocol entry with check: test satisfies the guard"
    );
    assert!(
        cua_protocol_tests_run_in(&matrix(&format!("{protocol_clippy}{protocol_test}"))).is_ok(),
        "a clippy entry beside the test entry still satisfies the guard"
    );
    assert!(
        cua_protocol_tests_run_in(&matrix(&format!("{server_test}{protocol_clippy}"))).is_err(),
        "a clippy-only cua-protocol entry mentions the manifest but never runs its tests"
    );
    assert!(
        cua_protocol_tests_run_in(&matrix(&format!("{server_test}{protocol_commented}"))).is_err(),
        "a commented-out cua-protocol entry does not run anything"
    );
    assert!(
        cua_protocol_tests_run_in(&matrix(&server_test)).is_err(),
        "a matrix without cua-protocol fails"
    );
    // The `check: test` of a neighbouring entry must not leak into the
    // cua-protocol block.
    assert!(
        cua_protocol_tests_run_in(&matrix(&format!("{protocol_clippy}{server_test}"))).is_err(),
        "the following entry's check: test does not count for cua-protocol"
    );
}
