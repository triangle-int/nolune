//! Guard for #32: the landing app is a fully static product site. Every
//! route prerenders at build time, nothing runs on a request path, the two
//! script endpoints serve the repository's own installers from the deployed
//! commit, and the site carries no analytics so the privacy page stays true.

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

fn landing_sources(repo: &Path) -> Vec<(String, String)> {
    let mut paths = files(&repo.join("landing/src"), &["svelte", "ts", "js", "json"]);
    paths.push(repo.join("landing/package.json"));
    paths.push(repo.join("landing/svelte.config.js"));
    paths.push(repo.join("landing/vite.config.ts"));
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

fn source<'a>(sources: &'a [(String, String)], path: &str) -> &'a str {
    sources
        .iter()
        .find(|(relative, _)| relative == path)
        .map(|(_, text)| text.as_str())
        .unwrap_or_else(|| panic!("{path} must exist"))
}

#[test]
fn every_landing_route_prerenders_and_nothing_runs_at_request_time() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let sources = landing_sources(repo);
    let mut violations = Vec::new();

    // Global prerender: the root layout opts every page in, so a new page
    // cannot silently become a serverless function.
    let layout = source(&sources, "landing/src/routes/+layout.ts");
    if !layout.contains("export const prerender = true") {
        violations.push("landing/src/routes/+layout.ts must export prerender = true".to_owned());
    }

    // Endpoints do not inherit the layout option, so each +server.ts must opt
    // in itself, and none may reach the network at build time either.
    for (path, text) in &sources {
        if !path.ends_with("/+server.ts") {
            continue;
        }
        if !text.contains("export const prerender = true") {
            violations.push(format!("{path} must export prerender = true"));
        }
        if text.contains("fetch(") {
            violations.push(format!("{path} must not fetch at request or build time"));
        }
    }

    // Server-only modules would need a runtime; a static site has none.
    for (path, _) in &sources {
        let name = Path::new(path).file_name().unwrap().to_string_lossy();
        if name == "hooks.server.ts"
            || name == "hooks.server.js"
            || name == "+page.server.ts"
            || name == "+layout.server.ts"
        {
            violations.push(format!("{path} has no place in a static site"));
        }
    }

    assert!(
        violations.is_empty(),
        "landing is not fully static:\n{}",
        violations.join("\n")
    );
}

#[test]
fn install_scripts_are_served_from_the_repository_at_build_time() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let sources = landing_sources(repo);
    let mut violations = Vec::new();

    for (route, script) in [
        (
            "landing/src/routes/install.sh/+server.ts",
            "scripts/install.sh",
        ),
        (
            "landing/src/routes/uninstall.sh/+server.ts",
            "scripts/uninstall.sh",
        ),
    ] {
        let text = source(&sources, route);
        if !text.contains(&format!("{script}?raw")) {
            violations.push(format!("{route} must import ../../../../{script}?raw"));
        }
        if text.contains("raw.githubusercontent.com") {
            violations.push(format!("{route} must not proxy GitHub at request time"));
        }
        if !repo.join(script).exists() {
            violations.push(format!("{script} is missing"));
        }
    }

    assert!(
        violations.is_empty(),
        "script endpoints are not static:\n{}",
        violations.join("\n")
    );
}

#[test]
fn landing_has_no_analytics_so_the_privacy_page_is_true() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let sources = landing_sources(repo);
    let mut violations = Vec::new();

    for (path, text) in &sources {
        for needle in ["@vercel/analytics", "injectAnalytics", "track("] {
            if text.contains(needle) {
                violations.push(format!("{path} contains {needle:?}"));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "landing still ships analytics:\n{}",
        violations.join("\n")
    );
}

#[test]
fn skills_library_is_reachable_from_every_page() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let sources = landing_sources(repo);
    let mut violations = Vec::new();

    for component in [
        "landing/src/lib/components/Nav.svelte",
        "landing/src/lib/components/Footer.svelte",
    ] {
        let text = source(&sources, component);
        if !text.contains("href=\"/skills\"") {
            violations.push(format!("{component} must link href=\"/skills\""));
        }
        // Nav and Footer also render on /skills, /privacy and /terms, where
        // the home-page sections do not exist; section links must be absolute.
        for anchor in [
            "href=\"#how\"",
            "href=\"#install\"",
            "href=\"#companion\"",
            "href=\"#computer-use\"",
        ] {
            if text.contains(anchor) {
                violations.push(format!(
                    "{component} uses page-relative {anchor}; use href=\"/{}\"",
                    &anchor[6..anchor.len() - 1]
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "skills page is orphaned:\n{}",
        violations.join("\n")
    );
}
