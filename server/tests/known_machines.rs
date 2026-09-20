//! Guard for #80: known machines are persisted under the companion directory
//! with a documented layout, every change reaches clients as one event, and
//! no route picks a computer for the user.

#[path = "../test-support/source_scan.rs"]
mod source_scan;

use source_scan::without_cfg_test_items;
use std::{fs, path::Path};

fn repo() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

#[test]
fn known_machines_are_persisted_and_documented() {
    let repo = repo();
    let state =
        without_cfg_test_items(&fs::read_to_string(repo.join("server/src/app/state.rs")).unwrap());
    assert!(
        state.contains("MachineRegistry::open(&workspace_dir"),
        "AppState must open the machine registry under its workspace, never the process default"
    );
    let registry = without_cfg_test_items(
        &fs::read_to_string(repo.join("server/src/services/machine_registry.rs")).unwrap(),
    );
    for required in [
        "MACHINES_FILE",
        "deny_unknown_fields",
        "MachineError::Unsupported",
    ] {
        assert!(
            registry.contains(required) || {
                let domain = fs::read_to_string(repo.join("server/src/domain/machine.rs")).unwrap();
                domain.contains(required)
            },
            "known machines must be versioned and fail closed ({required} missing)"
        );
    }

    let doc = fs::read_to_string(repo.join("docs/companion-storage.md")).unwrap();
    for required in [
        "machines.json",
        "machine_updated",
        "machines_format_unsupported",
        "PUT /api/instances/companion/machines/{machine_id}",
        "DELETE /api/instances/companion/machines/{machine_id}",
        "machine_forgotten",
        "#80",
    ] {
        assert!(
            doc.contains(required),
            "storage doc is missing {required:?}"
        );
    }
}

#[test]
fn machine_routes_never_pick_a_computer_for_the_user() {
    let routes = without_cfg_test_items(
        &fs::read_to_string(repo().join("server/src/routes/machine_agents.rs")).unwrap(),
    );
    assert!(
        !routes.contains("machines[0]") && !routes.contains(".first()"),
        "machine-hello and machine-bye must address the chosen or only computer, never the first one listed"
    );
    assert!(
        routes.contains("ambiguous_machine"),
        "several connected computers and no chosen one must be reported as ambiguous"
    );
}

#[test]
fn clients_receive_machine_updates_as_one_event() {
    let repo = repo();
    let events = fs::read_to_string(repo.join("server/src/domain/events.rs")).unwrap();
    assert!(
        events.contains("MachineUpdated {"),
        "ServerEvent must carry machine_updated"
    );
    let types = fs::read_to_string(repo.join("client/src/lib/api/types.ts")).unwrap();
    assert!(
        types.contains("type: \"machine_updated\""),
        "client ServerEvent union must declare machine_updated"
    );
    assert!(
        events.contains("MachineForgotten {") && types.contains("type: \"machine_forgotten\""),
        "a forgotten machine must reach clients as machine_forgotten"
    );
    let machine = &types[types.find("export interface MachineInfo").unwrap()..];
    let machine = &machine[..machine.find("\n}").unwrap()];
    for field in [
        "display_name",
        "online",
        "health",
        "capabilities",
        "permissions",
        "driver_version",
    ] {
        assert!(
            machine.contains(&format!("\t{field}:")),
            "client MachineInfo is missing {field}"
        );
    }
    let client = fs::read_to_string(repo.join("client/src/lib/api/client.ts")).unwrap();
    assert!(
        client.contains("export function renameMachine("),
        "the client must be able to name a computer"
    );
    assert!(
        client.contains("function forgetMachine("),
        "the client must be able to forget an offline computer"
    );
}
