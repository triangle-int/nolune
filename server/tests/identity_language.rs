//! Guard for #105: public copy describes one persistent companion per
//! server. Computers, chats, and relationship scopes are contexts of that one
//! identity, never separate companions or hosted "instances", and the docs
//! only name routes and settings sections that actually ship.

use std::{
    fs,
    path::{Path, PathBuf},
};

fn files(root: &Path, extensions: &[&str]) -> Vec<PathBuf> {
    fn visit(dir: &Path, extensions: &[&str], out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                if path
                    .file_name()
                    .is_some_and(|n| n == "node_modules" || n == ".svelte-kit")
                {
                    continue;
                }
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

/// Every file a user can read without opening the source tree.
fn public_copy(repo: &Path) -> Vec<(String, String)> {
    let mut paths = vec![
        repo.join("README.md"),
        repo.join("CONTRIBUTING.md"),
        repo.join("SECURITY.md"),
    ];
    paths.extend(files(&repo.join("docs"), &["md"]));
    paths.extend(files(&repo.join("landing/src"), &["svelte", "ts", "js"]));
    paths.extend(files(
        &repo.join("client/src/lib/components/onboarding"),
        &["svelte"],
    ));
    paths.push(repo.join("client/src/lib/companion/context.js"));
    paths.extend(files(&repo.join("desktop/src/routes"), &["svelte"]));
    paths
        .into_iter()
        .map(|path| {
            let relative = path
                .strip_prefix(repo)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let text =
                fs::read_to_string(&path).unwrap_or_else(|_| panic!("{relative} is missing"));
            (relative, text)
        })
        .collect()
}

/// Phrases that imply several independent companions, hosted instances, or a
/// managed account, none of which exist.
const FORBIDDEN: &[&str] = &[
    "companion instances",
    "companion instance",
    "your instances",
    "users' instances",
    "each companion",
    "per instance",
    "└── {slug}/",
    "dedicated server",
    "create an account",
    "your account",
    "paid plan",
    "subscription",
    "billing",
    "Stripe",
    "Fly.io",
    "Neon",
    "your subdomain",
    "on the dashboard",
    "child agent",
];

/// The word "instance" is acceptable only as the storage path segment and the
/// API prefix, which #103 kept for compatibility.
fn instance_mentions_outside_paths(text: &str) -> Vec<String> {
    let lowered = text.to_lowercase();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = lowered[from..].find("instance") {
        let start = from + at;
        let line_start = lowered[..start].rfind('\n').map_or(0, |i| i + 1);
        let line_end = lowered[start..]
            .find('\n')
            .map_or(lowered.len(), |i| start + i);
        let line = &text[line_start..line_end];
        let allowed = line.contains("instances/")
            || line.contains("/instances")
            || line.contains("instance_slug")
            || line.contains("instance.toml")
            || line.contains("instanceof")
            || line.contains("InstanceOnboarding")
            || line.contains("importInstance")
            || line.contains("exportInstance")
            || line.contains("instances_count")
            || line.contains("multi-instance")
            || line.contains("hosted-instance");
        if !allowed {
            out.push(line.trim().to_owned());
        }
        from = line_end;
    }
    out.sort();
    out.dedup();
    out
}

#[test]
fn public_copy_describes_one_companion_per_server() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut violations = Vec::new();

    for (path, text) in public_copy(repo) {
        let lowered = text.to_lowercase();
        for phrase in FORBIDDEN {
            if lowered.contains(&phrase.to_lowercase()) {
                violations.push(format!("{path} says {phrase:?}"));
            }
        }
        if path.starts_with("docs/security/") {
            continue;
        }
        for line in instance_mentions_outside_paths(&text) {
            violations.push(format!("{path} calls the companion an instance: {line:?}"));
        }
    }

    let readme = fs::read_to_string(repo.join("README.md")).unwrap();
    for required in [
        "One mind across",
        "instances/\n    └── companion/",
        "never separate companions",
    ] {
        if !readme.contains(required) {
            violations.push(format!("README.md is missing {required:?}"));
        }
    }
    let storage = fs::read_to_string(repo.join("docs/companion-storage.md")).unwrap();
    if !storage.contains("one companion") {
        violations.push("docs/companion-storage.md must state the one-companion invariant".into());
    }
    for page in ["privacy", "terms"] {
        let source =
            fs::read_to_string(repo.join(format!("landing/src/routes/{page}/+page.svelte")))
                .unwrap();
        for required in ["self-hosted", "your own", "support@nolune.dev"] {
            if !source.contains(required) {
                violations.push(format!("landing {page} page is missing {required:?}"));
            }
        }
        if source.contains("legal-logo\">b<") {
            violations.push(format!(
                "landing {page} page still shows the Bolly logo glyph"
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "copy still implies several companions or a hosted account:\n{}",
        violations.join("\n")
    );
}

#[test]
fn docs_only_name_shipped_routes_and_settings_sections() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let tabs = [
        "chat",
        "activity",
        "memory",
        "computers",
        "settings",
        "drops",
    ];
    let sections = [
        "Companion",
        "Connections",
        "Capabilities",
        "Data",
        "Advanced",
    ];
    let mut violations = Vec::new();

    for (path, text) in public_copy(repo) {
        if !path.ends_with(".md") {
            continue;
        }
        // "/{slug}/<tab>" outside the API prefix must be a shipped tab.
        for (at, _) in text.match_indices("/{slug}/") {
            if text[..at]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_ascii_alphanumeric())
            {
                continue; // part of a longer API path such as /api/chat/{slug}/
            }
            let rest = &text[at + "/{slug}/".len()..];
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect();
            if !tabs.contains(&name.as_str()) {
                violations.push(format!("{path} links to unknown route /{{slug}}/{name}"));
            }
        }
        // "Settings → <Section>" must be one of the five sections (#98).
        for (at, _) in text.match_indices("Settings → ") {
            let rest = &text[at + "Settings → ".len()..];
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphabetic())
                .collect();
            if !sections.contains(&name.as_str()) {
                violations.push(format!(
                    "{path} points at unknown settings section {name:?}"
                ));
            }
        }
        for stale in [
            "Settings → Server",
            "Skills tab",
            "Drops tab",
            "Stats tab",
            "Thoughts tab",
            "Agents tab",
        ] {
            if text.contains(stale)
                && !text.contains(&format!("former {stale}"))
                && !text.contains("no Agents")
            {
                violations.push(format!("{path} mentions retired surface {stale:?}"));
            }
        }
    }

    assert!(violations.is_empty(), "{}", violations.join("\n"));
}
