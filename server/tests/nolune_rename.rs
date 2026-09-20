//! Guard for #68: the rename from Bolly to Nolune is complete and stays
//! complete. Source, packaging, scripts, workflows and public docs carry only
//! Nolune identifiers. The few intentional remnants (the separate
//! `bolly-skills` registry, the legacy browser credential names the auth
//! cleanup removes, other guards' forbidden strings, and the cutover record's
//! history) are listed exactly, so any new reference fails CI.

#[path = "../test-support/source_scan.rs"]
mod source_scan;

use source_scan::without_cfg_test_items;
use std::{
    fs,
    path::{Path, PathBuf},
};

const SCANNED_DIRS: &[&str] = &[
    "client/src",
    "client/tests",
    "landing/src",
    "desktop/src",
    "desktop/src-tauri/src",
    "server/src",
    "server/tests",
    "server/test-support",
    "scripts",
    ".github",
    "docs",
];

const SCANNED_FILES: &[&str] = &[
    "Cargo.toml",
    "server/Cargo.toml",
    "desktop/src-tauri/Cargo.toml",
    "desktop/src-tauri/tauri.conf.json",
    "client/package.json",
    "desktop/package.json",
    "landing/package.json",
    "README.md",
    "SECURITY.md",
    "CONTRIBUTING.md",
];

const SKIPPED_DIRS: &[&str] = &["node_modules", "target", ".svelte-kit", ".claude"];

/// This guard spells out the remnants below, so it is the one scanned file
/// excused from its own scan.
const SELF: &str = "server/tests/nolune_rename.rs";

/// Intentional remnants: (path, exact substring, occurrences). Each substring
/// is removed from its file before the case-insensitive scan, and its count
/// must match so a remnant cannot quietly grow or move.
const ALLOWED: &[(&str, &str, usize)] = &[
    // Legacy browser credentials the auth cleanup deletes (#113, #135).
    (
        "client/src/lib/api/legacy-auth-cleanup.js",
        "[\"nolune_auth_token\", \"bolly_auth_token\", \"bolly_token\"]",
        1,
    ),
    (
        "client/src/lib/api/legacy-auth-cleanup.js",
        "[\"nolune_token\", \"bolly_token\", \"bolly_auth_token\"]",
        1,
    ),
    (
        "client/src/lib/api/legacy-auth-cleanup.js",
        "historical Bolly/PWA clients",
        1,
    ),
    (
        "client/tests/legacy-auth-cleanup.test.mjs",
        "the Bolly-era keys",
        1,
    ),
    (
        "client/tests/legacy-auth-cleanup.test.mjs",
        "['nolune_auth_token', 'bolly_auth_token', 'bolly_token']",
        1,
    ),
    (
        "client/tests/legacy-auth-cleanup.test.mjs",
        "['nolune_token', 'bolly_token', 'bolly_auth_token']",
        1,
    ),
    (
        "server/test-support/router_security.rs",
        "[\"nolune_token\", \"bolly_token\", \"bolly_auth_token\"]",
        1,
    ),
    // The separate, unrenamed skills registry repository.
    (
        "server/src/config.rs",
        "https://raw.githubusercontent.com/triangle-int/bolly-skills/main/registry.json",
        1,
    ),
    (
        "landing/src/routes/skills/+page.svelte",
        "https://raw.githubusercontent.com/triangle-int/bolly-skills/main/registry.json",
        1,
    ),
    (
        "landing/src/routes/skills/+page.svelte",
        "https://github.com/triangle-int/bolly-skills",
        2,
    ),
    // Other guards name the old strings they forbid.
    ("server/tests/self_hosted_surface.rs", "\"bollyai.dev\",", 1),
    (
        "server/tests/identity_language.rs",
        "still shows the Bolly logo glyph",
        1,
    ),
    // The cutover record's history.
    (
        "docs/nolune-cutover.md",
        "renamed from Bolly (`triangle-int/bolly`, `bollyai.dev`)",
        1,
    ),
    (
        "docs/nolune-cutover.md",
        "no Bolly configuration aliases",
        1,
    ),
    (
        "docs/nolune-cutover.md",
        "`bollyai.dev` and `www.bollyai.dev` answer 308",
        1,
    ),
    ("docs/nolune-cutover.md", "titles itself \"Bolly Docs\"", 1),
    ("docs/nolune-cutover.md", "links to `bollyai.dev`", 1),
    ("docs/nolune-cutover.md", "`triangle-int/bolly-skills`", 1),
];

fn files(root: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(root).unwrap_or_else(|_| panic!("{} is missing", root.display())) {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if path
                .file_name()
                .is_some_and(|name| SKIPPED_DIRS.iter().any(|skip| name == *skip))
            {
                continue;
            }
            files(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn scanned_sources(repo: &Path) -> Vec<(String, String)> {
    let mut paths = Vec::new();
    for dir in SCANNED_DIRS {
        files(&repo.join(dir), &mut paths);
    }
    paths.extend(SCANNED_FILES.iter().map(|file| repo.join(file)));
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let relative = path
                .strip_prefix(repo)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let bytes = fs::read(&path).unwrap_or_else(|_| panic!("{relative} is missing"));
            (relative, String::from_utf8_lossy(&bytes).into_owned())
        })
        .filter(|(relative, _)| relative != SELF)
        .collect()
}

#[test]
fn no_bolly_reference_survives_outside_the_allowlist() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut violations = Vec::new();
    let mut sources = scanned_sources(repo);

    for (path, _, _) in ALLOWED {
        if !sources.iter().any(|(scanned, _)| scanned == path) {
            violations.push(format!("{path} is allowed but was not scanned"));
        }
    }

    for (path, source) in &mut sources {
        // Configuration variables were renamed without aliases, so no file
        // may read, document or export a BOLLY_* variable.
        if source.contains("BOLLY_") {
            violations.push(format!("{path} references a BOLLY_ variable"));
        }
        for (allowed_path, allowed, expected) in ALLOWED {
            if path != allowed_path {
                continue;
            }
            let actual = source.matches(allowed).count();
            if actual != *expected {
                violations.push(format!(
                    "{path}: expected {expected} exact allowed occurrence(s) of {allowed:?}, found {actual}"
                ));
            }
            *source = source.replace(allowed, "");
        }
        for line in source.lines() {
            if line.to_lowercase().contains("bolly") {
                violations.push(format!("{path} still says bolly: {:?}", line.trim()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "the rename is incomplete:\n{}",
        violations.join("\n")
    );
}

#[test]
fn nolune_identifiers_are_wired() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let read = |path: &str| fs::read_to_string(repo.join(path)).unwrap();
    let mut violations = Vec::new();

    for (path, required) in [
        (
            "desktop/src-tauri/tauri.conf.json",
            "\"productName\": \"Nolune\"",
        ),
        (
            "desktop/src-tauri/tauri.conf.json",
            "\"identifier\": \"com.triangle-int.nolune-desktop\"",
        ),
        ("server/Cargo.toml", "[[bin]]\nname = \"nolune\""),
        ("desktop/src-tauri/Cargo.toml", "name = \"nolune-desktop\""),
    ] {
        if !read(path).contains(required) {
            violations.push(format!("{path} is missing {required:?}"));
        }
    }

    let config = without_cfg_test_items(&read("server/src/config.rs"));
    for variable in ["NOLUNE_HOME", "NOLUNE_AUTH_TOKEN", "NOLUNE_PUBLIC_URL"] {
        if !config.contains(&format!("env::var(\"{variable}\")"))
            && !config.contains(&format!("env::var_os(\"{variable}\")"))
        {
            violations.push(format!("server/src/config.rs does not read {variable}"));
        }
    }

    assert!(
        violations.is_empty(),
        "Nolune identifiers are not wired:\n{}",
        violations.join("\n")
    );
}

#[test]
fn cutover_record_is_current() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let record = fs::read_to_string(repo.join("docs/nolune-cutover.md")).unwrap();
    let mut violations = Vec::new();

    // Services that never existed must not be listed as cutover work.
    for stale in ["docs.nolune.dev", "OAuth", "hosted-instance"] {
        if record.contains(stale) {
            violations.push(format!("docs/nolune-cutover.md still mentions {stale:?}"));
        }
    }
    // The outstanding external work must be named precisely.
    for required in [
        "support@nolune.dev",
        "security@nolune.dev",
        "MX",
        "triangle-int/docs",
        "#32",
    ] {
        if !record.contains(required) {
            violations.push(format!("docs/nolune-cutover.md is missing {required:?}"));
        }
    }

    assert!(violations.is_empty(), "{}", violations.join("\n"));
}
