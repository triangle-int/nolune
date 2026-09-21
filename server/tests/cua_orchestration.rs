//! Guard for #18 (slices 1 and 2): the typed machine tools drive every Cua
//! target through one orchestrator whose snapshot ledger fails closed, whose
//! verification gate reports success only for a verified outcome, and whose
//! delivery is background only. The legacy coordinate tool stays beside
//! them until #19, and the policy is documented.

#[path = "../test-support/source_scan.rs"]
mod source_scan;

use source_scan::without_cfg_test_items;
use std::{fs, path::Path};

fn repo() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

fn production(path: &str) -> String {
    without_cfg_test_items(&fs::read_to_string(repo().join(path)).unwrap())
}

#[test]
fn the_orchestrator_owns_the_ledger_and_the_gate() {
    let orchestrator = production("server/src/services/cua/orchestrator.rs");
    for required in [
        "pub struct SnapshotLedger",
        "pub struct Orchestrator",
        "StaleSnapshot",
        "SnapshotConsumed",
        "PixelRefused",
        "ActionRefused",
        "Unconfirmed",
        "Unverified",
        "CuaAction::VerifyState(",
        "DeliveryMode::Background",
    ] {
        assert!(
            orchestrator.contains(required),
            "services/cua/orchestrator.rs must reference {required}"
        );
    }
    assert!(
        !orchestrator.contains("todo!("),
        "services/cua/orchestrator.rs is implemented"
    );
    assert!(
        !orchestrator.contains("EscalationTarget::Foreground =>"),
        "the orchestrator never branches into foreground delivery; a recommendation is only quoted"
    );
    let module = fs::read_to_string(repo().join("server/src/services/cua/mod.rs")).unwrap();
    assert!(
        module.contains("pub mod orchestrator;"),
        "services/cua/mod.rs declares the orchestrator"
    );
}

#[test]
fn the_typed_tools_go_through_the_orchestrator_and_the_chosen_computer() {
    let tools = production("server/src/services/tools/cua.rs");
    for required in [
        "orchestrator::",
        "Orchestrator",
        "MachineTarget",
        "TargetSelection",
        "\"choose_a_computer\"",
        "\"target_mismatch\"",
        "\"no_server_local_target\"",
        "\"no_cua_driver\"",
        "\"discover_windows\"",
        "\"get_window_state\"",
        "\"act\"",
        "\"verify_state\"",
        "DeliveryModeArg",
        "Background",
    ] {
        assert!(
            tools.contains(required),
            "services/tools/cua.rs must reference {required}"
        );
    }
    assert!(
        !tools.contains("todo!("),
        "services/tools/cua.rs is implemented"
    );
    for forbidden in [
        "machines[0]",
        ".first()",
        "machine_id.unwrap_or",
        "AgentToolCall",
        "\"base64\"",
    ] {
        assert!(
            !tools.contains(forbidden),
            "services/tools/cua.rs must not contain {forbidden}: no defaulting, no legacy \
             toolcalls, no image bytes to the model"
        );
    }
    assert!(
        !tools.contains("CuaAction::StartSession(") && !tools.contains("CuaAction::EndSession("),
        "sessions are the runtime's job, never a tool's"
    );

    let registry = production("server/src/services/tools/mod.rs");
    for tool in [
        "DiscoverWindowsTool::new(",
        "GetWindowStateTool::new(",
        "ActTool::new(",
        "VerifyStateTool::new(",
    ] {
        assert_eq!(
            registry.matches(tool).count(),
            1,
            "tools/mod.rs registers {tool} exactly once"
        );
    }
    assert!(
        registry.contains("CuaTools::new("),
        "the four tools share one orchestrator per turn"
    );
    assert!(
        registry.contains("ComputerUseTool::new("),
        "the coordinate tool stays for legacy desktops until #19"
    );
    for name in [
        "\"discover_windows\" =>",
        "\"get_window_state\" =>",
        "\"act\" =>",
        "\"verify_state\" =>",
    ] {
        assert!(
            registry.contains(name),
            "tool_summary must name the machine for {name}"
        );
    }
}

#[test]
fn the_registry_hands_the_server_local_runtime_to_the_tools() {
    let registry = production("server/src/services/machine_registry.rs");
    assert!(
        registry.contains("pub fn server_local_runtime("),
        "CuaTargets exposes the runtime that runs the server-local target"
    );
    let runtime = production("server/src/services/cua/runtime.rs");
    assert!(
        runtime.contains("pub struct RuntimeHandle")
            && runtime.contains("set_server_local_runtime("),
        "the runtime registers a weak handle with its targets"
    );
    assert!(
        !runtime.contains("#[allow(dead_code)] // Every typed machine tool")
            && !runtime.contains("#[allow(dead_code)] // The typed machine tools"),
        "run and execute have their consumer now; the allow-markers from #192 are retired"
    );
}

#[test]
fn the_loop_policy_is_documented() {
    let doc = fs::read_to_string(repo().join("docs/computer-use.md")).unwrap();
    for required in [
        "## Typed machine tools (#18)",
        "discover_windows",
        "get_window_state",
        "verify_state",
        "element_token",
        "snapshot_required",
        "stale_snapshot",
        "snapshot_consumed",
        "pixel_refused",
        "action_refused",
        "verification_failed",
        "unverified",
        "background",
        "foreground",
        "never",
        "no_server_local_target",
        "no_cua_driver",
        "choose_a_computer",
    ] {
        assert!(
            doc.contains(required),
            "docs/computer-use.md is missing {required:?}"
        );
    }
    assert!(
        !doc.contains("Driving the server-local Cua target through typed machine tools is #18"),
        "the doc no longer defers the typed tools to #18"
    );
}
