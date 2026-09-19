//! Guard for #98: the companion has five primary destinations, Settings is
//! split by owner into small routes, and raw server/protocol fields live only
//! on the Advanced page.

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

fn read(repo: &Path, relative: &str) -> String {
    fs::read_to_string(repo.join(relative)).unwrap_or_else(|_| panic!("{relative} is missing"))
}

/// Fields that describe how the server is wired, not what the companion is
/// like. Common setup must never show them; Advanced must keep them reachable.
const RAW_FIELD_TOKENS: &[&str] = &[
    "server-port",
    "Auth token",
    "update-channel",
    "setModelMode",
    "voice-id",
    "smtp_host",
    "imap_host",
    "github-token",
];

const CONSUMER_PAGES: &[&str] = &[
    "client/src/routes/[slug]/settings/companion/+page.svelte",
    "client/src/routes/[slug]/settings/connections/+page.svelte",
    "client/src/routes/[slug]/settings/capabilities/+page.svelte",
    "client/src/routes/[slug]/settings/data/+page.svelte",
];

#[test]
fn primary_navigation_is_five_companion_destinations() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();

    assert!(
        repo.join("client/src/lib/companion/navigation.js").exists(),
        "primary navigation must be a pure module the layout reads"
    );
    assert!(
        repo.join("client/src/routes/[slug]/computers/+page.svelte")
            .exists(),
        "Connected computers needs its own route"
    );
    assert!(
        !repo
            .join("client/src/routes/[slug]/skills/+page.svelte")
            .exists(),
        "Skills is a capability under Settings, not a primary tab"
    );

    let layout = read(repo, "client/src/routes/[slug]/+layout.svelte");
    assert!(
        layout.contains("navigation.js"),
        "the companion layout must render tabs from lib/companion/navigation.js"
    );
    for stale in [
        "\"drops\"",
        "\"skills\"",
        "\"agents\"",
        "\"thoughts\"",
        "\"stats\"",
    ] {
        assert!(
            !layout.contains(stale),
            "the layout still hard-codes a {stale} tab"
        );
    }

    let mut violations = Vec::new();
    for path in files(&repo.join("client/src"), &["svelte", "ts", "js"]) {
        let relative = path
            .strip_prefix(repo)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let source = fs::read_to_string(&path).unwrap();
        for token in ["/{slug}/skills", "${slug}/skills"] {
            if source.contains(token) {
                violations.push(format!("{relative} links to the retired skills tab"));
            }
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn settings_is_split_by_owner_and_common_setup_hides_raw_fields() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();

    assert!(
        !repo
            .join("client/src/routes/[slug]/settings/+page.svelte")
            .exists(),
        "the monolithic Settings page still exists"
    );
    assert!(
        read(repo, "client/src/routes/[slug]/settings/+page.ts").contains("redirect("),
        "/settings must redirect to its first section"
    );
    assert!(
        repo.join("client/src/lib/settings/sections.js").exists(),
        "setting ownership must be a pure module with tests"
    );
    let layout = read(repo, "client/src/routes/[slug]/settings/+layout.svelte");
    assert!(
        layout.contains("sections.js") && layout.contains("aria-current"),
        "the settings layout must render the section nav from lib/settings/sections.js"
    );

    let mut violations = Vec::new();
    for page in CONSUMER_PAGES {
        let source = read(repo, page);
        for token in RAW_FIELD_TOKENS {
            if source.contains(token) {
                violations.push(format!("{page} exposes raw field {token:?}"));
            }
        }
        if source.lines().count() > 700 {
            violations.push(format!("{page} is not a small page any more"));
        }
    }
    let advanced = read(
        repo,
        "client/src/routes/[slug]/settings/advanced/+page.svelte",
    );
    for token in RAW_FIELD_TOKENS {
        if !advanced.contains(token) {
            violations.push(format!(
                "Advanced no longer reaches the documented control {token:?}"
            ));
        }
    }
    for (page, required) in [
        (
            "client/src/routes/[slug]/settings/companion/+page.svelte",
            &["rhythm-label", "initiative-label", "Timezone"][..],
        ),
        (
            "client/src/routes/[slug]/settings/connections/+page.svelte",
            &["setProvider", "apiKeyDefs", "Paired browsers", "computers"][..],
        ),
        (
            "client/src/routes/[slug]/settings/capabilities/+page.svelte",
            &["SkillsView", "acknowledgeUntrusted", "toggleGrant"][..],
        ),
        (
            "client/src/routes/[slug]/settings/data/+page.svelte",
            &["handleExport", "/memory"][..],
        ),
    ] {
        let source = read(repo, page);
        for token in required {
            if !source.contains(token) {
                violations.push(format!("{page} lost its owned control {token:?}"));
            }
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn connected_computers_have_a_read_route_and_the_split_is_documented() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();

    let routes = without_cfg_test_items(&read(repo, "server/src/routes/machine_agents.rs"));
    assert!(
        routes.contains("/api/instances/{instance_slug}/machines")
            && routes.contains("get(list_machines)"),
        "the client needs GET /api/instances/{{slug}}/machines to list connected computers"
    );
    let client = read(repo, "client/src/lib/api/client.ts");
    assert!(
        client.contains("export function fetchMachines("),
        "client API must expose fetchMachines"
    );

    let doc = read(repo, "docs/settings.md");
    for required in [
        "#98",
        "server-global",
        "companion-specific",
        "Companion",
        "Connections",
        "Capabilities",
        "Data",
        "Advanced",
        "config.toml",
        "Computers",
    ] {
        assert!(
            doc.contains(required),
            "docs/settings.md is missing {required:?}"
        );
    }
}
