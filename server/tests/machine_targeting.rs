//! Guard for #80 (targeting + trail): the desktop tools act on the computer
//! the user chose, never on one they picked themselves; the choice travels
//! from the composer through the chat request into the tools; and the trail
//! names the computer.

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
fn computer_tools_never_default_a_machine() {
    let tools = production("server/src/services/tools/computer.rs");
    for forbidden in [
        "machines[0]",
        "live[0]",
        ".first()",
        "machine_id.unwrap_or",
        "machine_id.clone().unwrap_or",
    ] {
        assert!(
            !tools.contains(forbidden),
            "computer tools must resolve a target through the selection, never pick one ({forbidden} found)"
        );
    }
    for required in [
        "pub enum TargetSelection",
        "ChooseAComputer",
        "\"choose_a_computer\"",
        "\"machine_unavailable\"",
        "\"machine_unhealthy\"",
        "\"permission_denied\"",
        "\"target_mismatch\"",
        "\"server_home\"",
        "SERVER_HOME_TARGET",
    ] {
        assert!(
            tools.contains(required),
            "typed target refusals are missing {required}"
        );
    }
    // Each desktop tool goes through the shared resolution before it sends anything.
    assert_eq!(
        tools.matches(".desktop(&self.registry").count(),
        3,
        "computer_use, remote_bash and remote_files each resolve the target once"
    );
    assert!(
        !tools.contains("execute(&args.machine_id"),
        "no tool may send to the machine the model named without resolving it"
    );

    let routes = production("server/src/routes/machine_agents.rs");
    assert!(
        !routes.contains("machines[0]") && !routes.contains(".first()"),
        "machine-hello and machine-bye address the named or only computer"
    );
}

#[test]
fn the_choice_travels_from_the_composer_to_the_tools() {
    let domain = production("server/src/domain/chat.rs");
    assert!(
        domain.contains("pub machine_id: Option<String>"),
        "ChatRequest carries the chosen machine"
    );
    let route = production("server/src/routes/chat.rs");
    assert!(
        route.contains("request.machine_id"),
        "the chat route hands the choice to the agent loop"
    );
    let chat = production("server/src/services/chat.rs");
    assert!(
        chat.contains("MachineTarget::resolve(") && !chat.contains("MachineTarget::default()"),
        "run_single_turn resolves the request's choice for build_tools instead of a default"
    );
    let tools = production("server/src/services/tools/mod.rs");
    assert!(
        tools.contains("machine_target: MachineTarget,"),
        "build_tools takes the resolved target"
    );
    for name in [
        "\"computer_use\" =>",
        "\"remote_bash\" =>",
        "\"remote_files\" =>",
    ] {
        assert!(
            tools.contains(name),
            "tool_summary must name the machine for {name}"
        );
    }
    let handoff = production("server/src/services/handoff.rs");
    assert!(
        handoff.contains("Some(destination.machine_id"),
        "a handoff continued on a computer targets that computer"
    );

    let client = fs::read_to_string(repo().join("client/src/lib/api/client.ts")).unwrap();
    assert!(
        client.contains("machine_id: machineId"),
        "the client sends the chosen machine with the message"
    );
    let composer =
        fs::read_to_string(repo().join("client/src/lib/components/chat/PromptComposer.svelte"))
            .unwrap();
    assert!(
        composer.contains("<TargetPicker"),
        "the composer offers the computer selector"
    );
    let input =
        fs::read_to_string(repo().join("client/src/lib/components/chat/ChatInput.svelte")).unwrap();
    assert!(
        input.contains("targetOptions(") && input.contains("targetStorageKey("),
        "the chat input builds the selector from the Computers rows and remembers the choice"
    );
    let view =
        fs::read_to_string(repo().join("client/src/lib/components/chat/ChatView.svelte")).unwrap();
    assert!(
        view.contains("chatTarget.machineId") && view.contains("chatTarget.label"),
        "the chat view sends the choice and names it while the companion works"
    );
    let spaces = fs::read_to_string(repo().join("client/src/lib/computers/spaces.js")).unwrap();
    let computer = production("server/src/services/tools/computer.rs");
    assert!(
        spaces.contains("HOME_SPACE_ID = \"server-home\"")
            && computer.contains("SERVER_HOME_TARGET: &str = \"server-home\""),
        "the client and the server agree on the server home's id"
    );
    let receipts = fs::read_to_string(repo().join("client/src/lib/activity/receipts.js")).unwrap();
    assert!(
        receipts.contains("export function targetLabel("),
        "activity receipts name the target machine"
    );
    let doc = fs::read_to_string(repo().join("docs/computer-use.md")).unwrap();
    for required in ["## Choosing a computer", "choose_a_computer", "server-home"] {
        assert!(
            doc.contains(required),
            "computer-use doc is missing {required:?}"
        );
    }
}

/// The trail names the computer after a reload too, and a handoff queued on
/// a running conversation re-targets that conversation's next turn.
#[test]
fn the_trail_survives_a_reload_and_a_queued_handoff_retargets_the_loop() {
    let helpers = production("server/src/services/llm/helpers.rs");
    assert!(
        helpers.contains("tool_trail"),
        "history_to_chat_messages renders a tool call from the trail line persisted with it"
    );
    let agent_loop = production("server/src/services/llm/agent_loop.rs");
    assert!(
        agent_loop.contains("tool_trail") && agent_loop.contains("trail_line("),
        "the streaming loop persists each tool's trail line with its call"
    );
    let tools = production("server/src/services/tools/mod.rs");
    assert!(
        tools.contains("fn trail_line(&self") && tools.contains("pub fn tool_trail_line("),
        "ObservableTool announces the desktop tools' trail line for persistence"
    );
    let types = production("server/src/services/llm/types.rs");
    assert!(
        types.contains("pub tool_trail: Option<"),
        "HistoryEntry carries the trail line beside the message, never inside it"
    );

    let route = production("server/src/routes/chat.rs");
    assert!(
        route.contains("next_turn_target(&state, &key") || route.contains("next_turn_target("),
        "run_agent_loop settles each turn's target from what was queued since the last one"
    );
    assert!(
        route.contains("TargetSelection::check_request(request.machine_id.as_deref())"),
        "post_chat checks the request's machine_id like a registered id before saving the message"
    );
    assert!(
        route.contains("release_agent("),
        "the loop releases the conversation and whatever was queued on it together"
    );
    let handoff = production("server/src/services/handoff.rs");
    assert!(
        handoff.contains("agent_targets"),
        "a handoff accepted while the conversation runs queues its destination on that loop"
    );
    let doc = fs::read_to_string(repo().join("docs/computer-use.md")).unwrap();
    assert!(
        doc.contains("reload") && doc.contains("next turn"),
        "computer-use doc states that the trail survives a reload and how a queued handoff is targeted"
    );
}
