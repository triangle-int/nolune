//! Guard for #18: the typed machine tools drive every Cua target through one
//! orchestrator whose snapshot ledger fails closed, whose verification gate
//! reports success only for a verified outcome, and whose delivery is
//! background only; what they observe reaches the model as an image beside
//! a bounded elements table, the prompt states the loop, the legacy
//! coordinate tool is no longer offered (its type stays until #19 deletes
//! it), and the policy is documented.

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
        "ActionEffect::Refused",
        "PredicateStatus::Satisfied",
        "fn confirmed_by_readback(",
        "include_screenshot: false",
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
        "TargetRefusal::ChooseAComputer",
        "TargetRefusal::Mismatch",
        "TargetRefusal::Unavailable",
        "\"no_cua_target\"",
        "\"no_server_local_target\"",
        "\"no_cua_driver\"",
        "\"driver_unavailable\"",
        "\"discover_windows\"",
        "\"get_window_state\"",
        "\"act\"",
        "\"verify_state\"",
        "\"delivery_mode\"",
        "DeliveryModeArg::Background",
        "pub enum DeliveryModeArg {\n    Background,\n}",
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
        !registry.contains("ComputerUseTool::new("),
        "build_tools no longer offers the coordinate tool; the typed tools are the machine \
         surface (the type itself stays in tools/computer.rs until #19 deletes it)"
    );
    let computer = production("server/src/services/tools/computer.rs");
    assert!(
        computer.contains("pub struct ComputerUseTool"),
        "the coordinate tool's type stays until #19"
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

/// Slice 3: what `get_window_state` observes reaches the model as an image
/// beside a bounded elements table, through the upload path every other
/// image takes (provenance-trusted, so the URL is renewed on later turns),
/// and never as bytes the tool inlines itself.
#[test]
fn window_state_reaches_the_model_as_an_image_and_a_table() {
    let tools = production("server/src/services/tools/cua.rs");
    for required in [
        "const TRUSTS_RESOURCE_PROVENANCE: bool = true;",
        "save_upload(",
        "screenshot_image_block(",
        "pub struct CaptureStore",
        "\"element_columns\"",
        "\"element_token\"",
        "\"enabled\"",
        "\"selected\"",
        "\"frame\"",
        "MAX_RENDERED_ELEMENTS",
        "MAX_RENDERED_CHARS",
        "\"type\": \"text\"",
    ] {
        assert!(
            tools.contains(required),
            "services/tools/cua.rs must reference {required}"
        );
    }
    assert!(
        !tools.contains("\"type\": \"image\"") && !tools.contains("\"data\":"),
        "the image block is built by the shared upload path in tools/computer.rs, not by the \
         typed tool"
    );
    let registry = production("server/src/services/tools/mod.rs");
    assert!(
        registry.contains("CaptureStore::new("),
        "build_tools hands the typed tools the upload path for captures"
    );
    assert!(
        registry.contains("fn bound_tool_result(") && registry.contains("fn multimodal_blocks("),
        "a tool result carrying an image is bounded per text block, never cut through the image"
    );
}

/// Slice 3: the system prompt states the loop the orchestrator enforces,
/// rule by rule, and no longer sends the model to the coordinate tool.
#[test]
fn the_prompt_states_the_loop() {
    let chat = production("server/src/services/chat.rs");
    for required in [
        "### computers",
        "list_machines",
        "discover_windows",
        "get_window_state",
        "element_token",
        "verify_state",
        "unknown",
        "unverifiable",
        "suspected_noop",
        "refused",
        "background",
        "foreground",
        "never",
    ] {
        assert!(
            chat.contains(required),
            "services/chat.rs must state {required:?} in the computer-use section"
        );
    }
    for stale in [
        "then `computer_use` to interact",
        "### desktop app & computer use",
    ] {
        assert!(
            !chat.contains(stale),
            "services/chat.rs still describes the coordinate loop: {stale:?}"
        );
    }
    let computer = production("server/src/services/tools/computer.rs");
    assert!(
        !computer.contains("conversation: computer_use, remote_bash and"),
        "the target's prompt line names the typed tools, not the coordinate one"
    );

    let guard =
        fs::read_to_string(repo().join("scripts/tests/no-continuous-screen-recording.py")).unwrap();
    assert!(
        guard.contains("GetWindowStateTool::new")
            && guard.contains("include_screenshot")
            && !guard.contains("assertIn(\"ComputerUseTool::new\""),
        "the capture guard names the one-shot capture the typed tools take"
    );
}

#[test]
fn the_rendering_is_documented() {
    let doc = fs::read_to_string(repo().join("docs/computer-use.md")).unwrap();
    for required in [
        "element_columns",
        "shown to the model",
        "public_url",
        "no longer offered",
    ] {
        assert!(
            doc.contains(required),
            "docs/computer-use.md is missing {required:?}"
        );
    }
    for stale in [
        "showing the image to the\nmodel is the next slice of #18",
        "stays beside them for desktops without a\ndriver until #19",
        "Every desktop tool (`computer_use`, `remote_bash`, `remote_files`)",
    ] {
        assert!(
            !doc.contains(stale),
            "docs/computer-use.md still says {stale:?}"
        );
    }
}
