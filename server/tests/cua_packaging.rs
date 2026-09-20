//! Guard for #20: the Cua Driver Nolune depends on is one pinned, attributed
//! release. The pin lives in `cua_protocol::cua_driver_pin` and the server
//! reads it from there (the desktop reference lands with #17), the MIT
//! attribution is checked in, and nothing asks the driver to update itself.

#[path = "../test-support/source_scan.rs"]
mod source_scan;

use cua_protocol::cua_driver_pin::{PINNED_VERSION, RELEASE_REPOSITORY};
use source_scan::without_cfg_test_items;
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Directories whose code, scripts and workflows may run the driver.
const SCANNED_DIRS: &[&str] = &[
    "server/src",
    "desktop/src",
    "desktop/src-tauri/src",
    "cua-protocol/src",
    "scripts",
    ".github/workflows",
];

/// This guard spells out the forbidden invocation, so it is excused from its
/// own scan.
const SELF: &str = "server/tests/cua_packaging.rs";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn files_under(root: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if matches!(
                path.file_name().and_then(|name| name.to_str()),
                Some("node_modules" | "target" | ".svelte-kit" | "__pycache__")
            ) {
                continue;
            }
            files_under(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn production_rust_files(root: &Path) -> Vec<(String, String)> {
    let repo = repo();
    let mut files = Vec::new();
    files_under(root, &mut files);
    files.sort();
    files
        .into_iter()
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .map(|path| {
            let relative = path
                .strip_prefix(&repo)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let source = without_cfg_test_items(&fs::read_to_string(&path).unwrap());
            (relative, source)
        })
        .collect()
}

/// True when `source` invokes the driver's self-update, however the argv is
/// spelled: one shell string, a Rust argument list, or an `.arg()` chain.
/// The source is read as words (`cua-driver`, `update`, `--apply`), with
/// punctuation and the `arg`/`args` builder calls dropped, so
/// `.arg("update").arg("--apply")` reads the same as `update --apply`.
fn invokes_driver_self_update(source: &str) -> bool {
    let words: Vec<&str> = source
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .filter(|word| !word.is_empty() && *word != "arg" && *word != "args")
        .collect();
    words
        .windows(2)
        .any(|pair| pair == ["cua-driver", "update"] || pair == ["update", "--apply"])
}

#[test]
fn third_party_notice_attributes_the_pinned_cua_driver() {
    let repo = repo();
    let notice = fs::read_to_string(repo.join("THIRD_PARTY_NOTICES.md"))
        .expect("THIRD_PARTY_NOTICES.md must exist at the repository root");
    for required in [
        "Cua Driver",
        PINNED_VERSION,
        &format!("https://github.com/{RELEASE_REPOSITORY}"),
        "MIT License",
        "Copyright (c) 2025 Cua AI, Inc.",
        "Permission is hereby granted, free of charge, to any person obtaining a copy",
        "THE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND",
    ] {
        assert!(
            notice.contains(required),
            "THIRD_PARTY_NOTICES.md is missing {required:?}"
        );
    }

    let readme = fs::read_to_string(repo.join("README.md")).unwrap();
    let mention = readme
        .lines()
        .find(|line| line.contains("THIRD_PARTY_NOTICES.md"))
        .expect("README.md must point at THIRD_PARTY_NOTICES.md");
    assert!(
        mention.contains("Cua Driver") && mention.contains(PINNED_VERSION),
        "the README line must name Cua Driver and the pinned version: {mention:?}"
    );
}

#[test]
fn server_reads_the_driver_pin_from_the_shared_protocol_crate() {
    let repo = repo();
    let referenced: Vec<String> = production_rust_files(&repo.join("server/src"))
        .into_iter()
        .filter(|(_, source)| source.contains("cua_driver_pin::PINNED_VERSION"))
        .map(|(relative, _)| relative)
        .collect();
    assert!(
        !referenced.is_empty(),
        "server/src must read cua_protocol::cua_driver_pin::PINNED_VERSION instead of \
         carrying its own driver version"
    );

    let literal: Vec<String> = production_rust_files(&repo.join("server/src"))
        .into_iter()
        .filter(|(_, source)| source.contains(&format!("\"{PINNED_VERSION}\"")))
        .map(|(relative, _)| relative)
        .collect();
    assert!(
        literal.is_empty(),
        "server/src repeats the driver version literal instead of the pin: {literal:?}"
    );
}

#[test]
fn nothing_asks_the_driver_to_update_itself() {
    let repo = repo();
    let mut files = Vec::new();
    for dir in SCANNED_DIRS {
        files_under(&repo.join(dir), &mut files);
    }
    files.sort();
    let mut violations = Vec::new();
    for path in files {
        let relative = path
            .strip_prefix(&repo)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        if relative == SELF {
            continue;
        }
        let Ok(source) = fs::read_to_string(&path) else {
            continue;
        };
        if invokes_driver_self_update(&source) {
            violations.push(relative);
        }
    }
    assert!(
        violations.is_empty(),
        "Nolune never updates Cua Driver on its own; move the pin instead: {violations:?}"
    );
}

#[test]
fn self_update_guard_recognises_every_argv_spelling() {
    for spelling in [
        "Command::new(\"cua-driver\").args([\"update\", \"--apply\"])",
        "Command::new(driver).arg(\"update\").arg(\"--apply\")",
        "sh -c 'cua-driver update --apply'",
        "run: cua-driver   update\n  --apply",
        "exec \"$DRIVER\" update --apply --yes",
    ] {
        assert!(
            invokes_driver_self_update(spelling),
            "{spelling:?} must be caught"
        );
    }
    for allowed in [
        "Command::new(\"cua-driver\").args([\"health\", \"--json\"])",
        "cua-driver permissions status --json",
        "nolune update",
        "let applied = update.apply();",
        "cargo update --workspace",
    ] {
        assert!(
            !invokes_driver_self_update(allowed),
            "{allowed:?} is not a driver self-update"
        );
    }
}
