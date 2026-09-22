//! Guard for #81: continuity records have one documented, versioned format
//! and are written only by explicit task activity (the continuity API and
//! the `task_continuity_update` chat tool), never by screenshots, check-ins,
//! reflections, schedules, or connected-computer events.

#[path = "../test-support/source_scan.rs"]
mod source_scan;

use source_scan::without_cfg_test_items;
use std::{
    fs,
    path::{Path, PathBuf},
};

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

/// The only production files that may touch the store or its writes. The
/// handoff API (#82) records the user's explicit decision on a record and
/// the receipt of the continuation they accepted; see `handoff_cards.rs`.
/// The owner's explicit acceptance of a task a paired companion handed
/// over (#111) creates one record; see `federation_scheduling.rs`.
const WRITERS: &[&str] = &[
    "server/src/services/continuity.rs",
    "server/src/routes/continuity.rs",
    "server/src/services/tools/continuity.rs",
    "server/src/services/handoff.rs",
    "server/src/routes/handoff.rs",
    "server/src/services/peer_proposals.rs",
];

/// Files that may read a record and never write one: the chat tool that
/// hands one of the user's tasks to a paired companion (#111) reads the
/// record it names and sends bounded references; the record here is left
/// as it is.
const READERS: &[&str] = &["server/src/services/tools/peer_proposals.rs"];

/// Module declarations and tool registration, which name the type but never write.
const REGISTRARS: &[&str] = &[
    "server/src/domain/mod.rs",
    "server/src/services/mod.rs",
    "server/src/routes/mod.rs",
    "server/src/app/router.rs",
    "server/src/services/tools/mod.rs",
];

/// Passive or self-started paths that must never know continuity exists.
const PASSIVE: &[&str] = &[
    "server/src/services/heartbeat.rs",
    "server/src/services/companion_routine.rs",
    "server/src/services/scheduler.rs",
    "server/src/services/proactive.rs",
    "server/src/services/machine_registry.rs",
    "server/src/services/tools/computer.rs",
    "server/src/routes/machine_agents.rs",
    "server/src/routes/ws.rs",
];

#[test]
fn continuity_records_are_written_only_by_explicit_task_activity() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut violations = Vec::new();

    for path in files(&repo.join("server/src"), &["rs"]) {
        let relative = path
            .strip_prefix(repo)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let production = without_cfg_test_items(&fs::read_to_string(&path).unwrap());
        let lowered = production.to_lowercase();

        if PASSIVE.contains(&relative.as_str()) {
            if lowered.contains("continuity") {
                violations.push(format!(
                    "{relative} is a passive or self-started path and mentions continuity"
                ));
            }
            continue;
        }
        if WRITERS.contains(&relative.as_str()) || relative == "server/src/domain/continuity.rs" {
            continue;
        }
        if READERS.contains(&relative.as_str()) {
            for write in [".create(", ".update(", ".complete(", ".dismiss(", ".save("] {
                if production.contains(write) {
                    violations.push(format!("{relative} writes continuity records ({write})"));
                }
            }
            continue;
        }
        let names_store = production.contains("ContinuityStore")
            || production.contains("services::continuity")
            || production.contains("continuity::ContinuityStore");
        if names_store && !REGISTRARS.contains(&relative.as_str()) {
            violations.push(format!(
                "{relative} reaches the continuity store; only the API and the tool may write"
            ));
        }
        if REGISTRARS.contains(&relative.as_str()) {
            for write in [".create(", ".update(", ".complete(", ".dismiss(", ".save("] {
                if production.contains("ContinuityStore") && production.contains(write) {
                    violations.push(format!("{relative} writes continuity records ({write})"));
                }
            }
        }
    }

    // The tool is a chat tool only: never part of a routine's curated surface.
    let routine =
        fs::read_to_string(repo.join("server/src/services/companion_routine.rs")).unwrap();
    if routine.contains("task_continuity_update") || routine.contains("TaskContinuityUpdateTool") {
        violations.push("companion routines must not carry the continuity tool".into());
    }
    let tools = fs::read_to_string(repo.join("server/src/services/tools/mod.rs")).unwrap();
    if tools.matches("TaskContinuityUpdateTool::new(").count() != 1 {
        violations.push("tools/mod.rs must register task_continuity_update exactly once".into());
    }
    let tool = fs::read_to_string(repo.join("server/src/services/tools/continuity.rs")).unwrap();
    for forbidden in [
        "\"screenshot\"",
        "screenshot(",
        "ComputerUseTool",
        "MachineRegistry",
        "machine_registry::",
        "heartbeat::",
        "companion_routine::",
        "proactive::",
    ] {
        if without_cfg_test_items(&tool).contains(forbidden) {
            violations.push(format!("the continuity tool must not use {forbidden:?}"));
        }
    }

    assert!(
        violations.is_empty(),
        "continuity records must only come from explicit task activity:\n{}",
        violations.join("\n")
    );
}

#[test]
fn continuity_format_is_documented() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let doc = fs::read_to_string(repo.join("docs/companion-storage.md")).unwrap();
    let domain = fs::read_to_string(repo.join("server/src/domain/continuity.rs")).unwrap();
    assert!(
        domain.contains("pub const CONTINUITY_FORMAT_VERSION: u32 = 1;"),
        "bump the doc alongside the version"
    );
    for required in [
        "#81",
        "continuity/",
        "continuity/{id}.json",
        "task_continuity_update",
        "`active`",
        "`waiting`",
        "`ready_to_resume`",
        "`completed`",
        "`dismissed`",
        "`failed`",
        "provenance",
        "machine_unavailable",
        "resource_missing",
        "never",
        "GET /api/instances/companion/continuity",
        "PUT /api/instances/companion/continuity/{id}",
        "POST /api/instances/companion/continuity/{id}/complete",
        "POST /api/instances/companion/continuity/{id}/dismiss",
    ] {
        assert!(
            doc.contains(required),
            "docs/companion-storage.md is missing {required:?}"
        );
    }
    assert!(
        doc.contains("continuity/*.json"),
        "the directory layout must list the continuity directory"
    );
}
