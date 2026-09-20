//! Guard for #16: the server speaks the shared Cua machine protocol.
//!
//! The Cua targets registry is typed by `cua_protocol`, the legacy
//! coordinate-only action vocabulary of `computer_use` is frozen so new machine
//! actions land in the protocol instead, and CI runs the protocol crate's tests.

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

#[test]
fn ci_runs_the_cua_protocol_tests() {
    let ci = fs::read_to_string(repo().join(".github/workflows/ci.yml")).unwrap();
    assert!(
        ci.contains("manifest: cua-protocol/Cargo.toml"),
        "the Rust matrix must include cua-protocol so its tests run in CI"
    );
}
