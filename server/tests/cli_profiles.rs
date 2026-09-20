//! Structural guard for #107: a profile's data root is decided in exactly one place.
//! Production code resolves the workspace through `config`, never by reading
//! `NOLUNE_HOME` or joining `.nolune` itself, so a named profile can never leak into
//! the default root (or the other way round). The docs describe profile roots.

#[path = "../test-support/source_scan.rs"]
mod source_scan;

use source_scan::without_cfg_test_items;
use std::{
    fs,
    path::{Path, PathBuf},
};

fn rust_files(root: &Path) -> Vec<PathBuf> {
    fn visit(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    visit(root, &mut files);
    files.sort();
    files
}

#[test]
fn only_config_resolves_the_workspace_root() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let allowed = "server/src/config.rs";
    // Compared with whitespace removed so rustfmt cannot hide a match across lines.
    let forbidden = [
        "var(\"NOLUNE_HOME\")",
        "var_os(\"NOLUNE_HOME\")",
        "join(\".nolune",
        "from(\".nolune",
    ];

    let mut violations = Vec::new();
    let mut allowed_seen = false;
    for path in rust_files(&repo.join("server/src")) {
        let relative = path
            .strip_prefix(repo)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let production = without_cfg_test_items(&fs::read_to_string(&path).unwrap());
        let compact: String = production.split_whitespace().collect();
        if relative == allowed {
            allowed_seen = compact.contains("var_os(\"NOLUNE_HOME\")");
            continue;
        }
        for token in forbidden {
            if compact.contains(token) {
                violations.push(format!("{relative} contains {token:?}"));
            }
        }
    }

    assert!(
        allowed_seen,
        "{allowed} should be the one reader of NOLUNE_HOME"
    );
    assert!(
        violations.is_empty(),
        "workspace resolution outside {allowed}:\n{}",
        violations.join("\n")
    );
}

#[test]
fn profile_roots_and_commands_are_documented() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();

    let readme = fs::read_to_string(repo.join("README.md")).unwrap();
    for required in ["--profile", "nolune gateway run", ".nolune-profiles/"] {
        assert!(
            readme.contains(required),
            "README.md is missing {required:?}"
        );
    }

    let storage = fs::read_to_string(repo.join("docs/companion-storage.md")).unwrap();
    for required in [
        "~/.nolune-profiles/<name>/",
        "--profile",
        "one profile = one server = one companion",
        "#107",
    ] {
        assert!(
            storage.contains(required),
            "docs/companion-storage.md is missing {required:?}"
        );
    }
    assert!(
        !storage.contains("~/.nolune/profiles"),
        "profile roots must be siblings of ~/.nolune, never nested inside it"
    );
}
