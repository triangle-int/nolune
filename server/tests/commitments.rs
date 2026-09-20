//! Guard for #85: commitments are first-class companion state with a
//! documented record, a store under the companion directory, and routes
//! behind the one-companion boundary.

use std::{fs, path::Path};

#[test]
fn commitment_routes_sit_behind_the_companion_boundary() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let router = fs::read_to_string(repo.join("server/src/app/router.rs")).unwrap();
    assert!(
        router.contains("routes::commitments::router()"),
        "commitment routes must be merged into the authenticated, companion-scoped API router"
    );
    let routes = fs::read_to_string(repo.join("server/src/routes/commitments.rs")).unwrap();
    for route in ["/commitments", "/snooze", "/complete", "/cancel"] {
        assert!(
            routes.contains("{instance_slug}/commitments") && routes.contains(route),
            "commitment routes are missing {route:?}"
        );
    }
    // Completion is refused without the user's confirmation or recorded evidence.
    let store = fs::read_to_string(repo.join("server/src/services/commitments.rs")).unwrap();
    assert!(
        store.contains("EvidenceRequired") && store.contains("is_sufficient"),
        "the store must refuse completion without confirmation or evidence"
    );
}

#[test]
fn commitment_checks_run_through_the_scheduler_tick_and_tools_stay_in_chat() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut violations = Vec::new();

    // The evaluator starts every check as one run of the proactive loop.
    let evaluator =
        fs::read_to_string(repo.join("server/src/services/commitment_evaluator.rs")).unwrap();
    if !evaluator.contains("Trigger::Commitment") || !evaluator.contains(".begin_at(") {
        violations.push("the evaluator must admit checks through ProactiveLoop::begin_at with Trigger::Commitment".into());
    }
    if !evaluator.contains("companion_routine::run") || !evaluator.contains("Some(&task)") {
        violations.push("the evaluator must run the check-in with a task override".into());
    }
    // It ticks with the scheduler under the canonical companion.
    let scheduler = fs::read_to_string(repo.join("server/src/services/scheduler.rs")).unwrap();
    if !scheduler.contains("commitment_evaluator::tick") {
        violations.push("scheduler::check_and_trigger must tick the commitment evaluator".into());
    }
    // A connected computer is a named event commitments can wait on.
    let machines = fs::read_to_string(repo.join("server/src/routes/machine_agents.rs")).unwrap();
    if !machines.contains("observe_event") {
        violations.push("machine connections must be observed for waiting commitments".into());
    }
    // The chat tools are registered once and never handed to a routine.
    let tools = fs::read_to_string(repo.join("server/src/services/tools/mod.rs")).unwrap();
    if tools.matches("commitment_tools(").count() != 1 {
        violations.push("tools/mod.rs must register the commitment tools exactly once".into());
    }
    let routine =
        fs::read_to_string(repo.join("server/src/services/companion_routine.rs")).unwrap();
    for forbidden in ["commitment_tools", "commitment_complete", "CommitmentStore"] {
        if routine.contains(forbidden) {
            violations.push(format!("companion routines must not carry {forbidden}"));
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn commitments_are_documented() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let doc = fs::read_to_string(repo.join("docs/proactive-loop.md")).unwrap();
    for required in [
        "#85",
        "commitments/",
        "next_check",
        "waiting_on",
        "dependencies",
        "evidence",
        "snooze",
        "dismissed",
        "/api/instances/companion/commitments",
        "what changed",
        "why now",
        "last_check",
        "machine_connected:",
        "commitment_completed:",
        "commitment_create",
        "commitment_complete",
        "commitment_snooze",
        "commitment_cancel",
    ] {
        assert!(
            doc.contains(required),
            "docs/proactive-loop.md is missing {required:?}"
        );
    }
    let storage = fs::read_to_string(repo.join("docs/companion-storage.md")).unwrap();
    assert!(
        storage.contains("commitments/"),
        "docs/companion-storage.md must list the commitments directory"
    );
}
