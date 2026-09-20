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
