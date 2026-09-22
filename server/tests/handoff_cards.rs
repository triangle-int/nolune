//! Guard for #82: a handoff is continued only after the user accepts it,
//! through the one proactive loop, and never by a passive or self-started
//! path; the card and its routes are documented and the client offers the
//! four decisions.

#[path = "../test-support/source_scan.rs"]
mod source_scan;

use source_scan::without_cfg_test_items;
use std::{fs, path::Path};

fn read(repo: &Path, relative: &str) -> String {
    fs::read_to_string(repo.join(relative)).unwrap_or_else(|_| panic!("{relative} is missing"))
}

/// Paths that must never know handoffs exist: nothing continues a task on
/// a schedule, a heartbeat, a connect event, or a computer-use path.
const PASSIVE: &[&str] = &[
    "server/src/services/heartbeat.rs",
    "server/src/services/companion_routine.rs",
    "server/src/services/scheduler.rs",
    "server/src/services/commitment_evaluator.rs",
    "server/src/services/proactive.rs",
    "server/src/services/machine_registry.rs",
    "server/src/services/tools/computer.rs",
    "server/src/services/tools/mod.rs",
    "server/src/routes/machine_agents.rs",
    "server/src/routes/ws.rs",
];

#[test]
fn a_handoff_is_continued_only_after_the_user_accepts() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut violations = Vec::new();

    for relative in PASSIVE {
        let production = without_cfg_test_items(&read(repo, relative)).to_lowercase();
        // The chat tool that hands one of the user's tasks to a paired
        // companion (#111) is registered by name in tools/mod.rs; it sends
        // bounded references to another owner for their review and never
        // continues a task here.
        let production = production.replace("handoff_task_to_peer", "");
        if production.contains("handoff") {
            violations.push(format!(
                "{relative} is a passive or self-started path and mentions handoffs"
            ));
        }
    }

    // The service admits exactly one run per acceptance through the loop,
    // bound to the chosen computer, and never drives a computer itself. The
    // one thing it asks a desktop is a read-only folder listing through the
    // conversation's own `remote_files` tool, at acceptance and never
    // before, to confirm a file the record places on the destination; no
    // screen, input, shell, or write action exists here.
    let service = without_cfg_test_items(&read(repo, "server/src/services/handoff.rs"));
    for required in [
        "Trigger::Handoff",
        "Target::Machine",
        "decide_handoff(",
        "record_handoff_outcome(",
        "validate_references(",
        "operation: \"list\"",
    ] {
        if !service.contains(required) {
            violations.push(format!("services/handoff.rs must use {required:?}"));
        }
    }
    if service.matches("RemoteFilesArgs {").count() != 1 {
        violations.push(
            "services/handoff.rs asks a desktop exactly one thing, the folder listing".into(),
        );
    }
    for relative in [
        "server/src/services/handoff.rs",
        "server/src/routes/handoff.rs",
    ] {
        let production = without_cfg_test_items(&read(repo, relative));
        for forbidden in [
            ".execute(",
            "AgentToolCall",
            "ComputerUseTool",
            "RemoteBashTool",
            "\"screenshot\"",
            "\"left_click\"",
            "\"bash\"",
            "operation: \"read\"",
            "operation: \"write\"",
            "companion_routine::",
            "Routine::",
        ] {
            if production.contains(forbidden) {
                violations.push(format!("{relative} must not use {forbidden:?}"));
            }
        }
    }
    let route = without_cfg_test_items(&read(repo, "server/src/routes/handoff.rs"));
    if route.contains("RemoteFilesTool") {
        violations.push("routes/handoff.rs must not touch a desktop".into());
    }

    // Only the handoff API records a decision; the store and the record
    // merely persist it, and no chat tool can accept on the user's behalf.
    for path in [
        "server/src/services/tools/continuity.rs",
        "server/src/routes/continuity.rs",
    ] {
        let production = without_cfg_test_items(&read(repo, path));
        if production.contains("decide_handoff(")
            || production.contains("HandoffDecision::Accepted")
        {
            violations.push(format!("{path} must not decide handoffs"));
        }
    }

    // Retrying a handoff run from the activity view goes back through the
    // same checks instead of leaving an orphan run holding the dedupe key.
    let activity = without_cfg_test_items(&read(repo, "server/src/routes/activity.rs"));
    if !activity.contains("Trigger::Handoff") {
        violations.push(
            "routes/activity.rs must route a handoff retry through the handoff service".into(),
        );
    }

    assert!(
        violations.is_empty(),
        "handoffs must start only from the user's acceptance:\n{}",
        violations.join("\n")
    );
}

#[test]
fn handoff_cards_are_documented_and_the_client_offers_the_four_decisions() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut violations = Vec::new();

    let doc = read(repo, "docs/companion-storage.md");
    for required in [
        "#82",
        "handoff",
        "`accepted`",
        "`kept`",
        "`dismissed`",
        "idempotent",
        "GET /api/instances/companion/handoffs",
        "GET /api/instances/companion/continuity/{id}/handoff",
        "GET /api/instances/companion/continuity/{id}/handoff/preview",
        "POST /api/instances/companion/continuity/{id}/handoff/accept",
        "POST /api/instances/companion/continuity/{id}/handoff/keep",
        "POST /api/instances/companion/continuity/{id}/handoff/dismiss",
        "handoff_updated",
        "Continue here",
        "Continue on",
        "Keep there",
        "Dismiss",
    ] {
        if !doc.contains(required) {
            violations.push(format!("docs/companion-storage.md is missing {required:?}"));
        }
    }
    let proactive_doc = read(repo, "docs/proactive-loop.md");
    if proactive_doc.contains("future handoff") {
        violations.push("docs/proactive-loop.md still calls the handoff trigger future".into());
    }

    let card = read(
        repo,
        "client/src/lib/components/continuity/HandoffCard.svelte",
    );
    for required in ["Continue here", "Continue on", "Keep there", "Dismiss"] {
        if !card.contains(required) {
            violations.push(format!(
                "HandoffCard.svelte is missing the {required:?} action"
            ));
        }
    }
    if !repo.join("client/src/lib/continuity/handoff.js").exists() {
        violations
            .push("card logic must be a pure module (client/src/lib/continuity/handoff.js)".into());
    }
    if !repo.join("client/tests/handoff.test.mjs").exists() {
        violations.push("client/tests/handoff.test.mjs is missing".into());
    }
    let client = read(repo, "client/src/lib/api/client.ts");
    for required in [
        "export function fetchHandoffs(",
        "export function fetchHandoff(",
        "export function previewHandoff(",
        "export function acceptHandoff(",
        "export function keepHandoff(",
        "export function dismissHandoff(",
    ] {
        if !client.contains(required) {
            violations.push(format!("client.ts is missing {required:?}"));
        }
    }
    let types = read(repo, "client/src/lib/api/types.ts");
    for required in ["handoff_updated", "HandoffCard", "ContinuationPreview"] {
        if !types.contains(required) {
            violations.push(format!("types.ts is missing {required:?}"));
        }
    }
    let activity = read(
        repo,
        "client/src/lib/components/activity/ActivityView.svelte",
    );
    if !activity.contains("HandoffCards") {
        violations.push("the Activity view must render the handoff cards".into());
    }
    let reference = read(repo, "client/src/routes/design-system/+page.svelte");
    if !reference.contains("HandoffCard") {
        violations.push("the /design-system reference page must show the handoff card".into());
    }

    assert!(violations.is_empty(), "{}", violations.join("\n"));
}
