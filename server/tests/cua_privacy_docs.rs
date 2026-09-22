//! Guard for #21: the computer-use documentation states the privacy model
//! the code enforces, and nothing shipped claims otherwise.
//!
//! `docs/computer-use.md` is the one complete page: it names both kinds of
//! target (the server machine and a remote desktop), says that computer use
//! is explicit and permissioned, that a screenshot is a one-shot window
//! capture taken during an action the model asked for, that Nolune does not
//! continuously record the screen, how the Cua Driver is pinned and never
//! updated on its own, and where Linux and Windows stand. The README and the
//! settings page carry the same words in one row each; no document or
//! client, desktop or landing copy claims continuous capture; the release
//! checklist names the manual macOS check and the script that walks it; and
//! the capture guard script covers every Cua file.

use std::{
    fs,
    path::{Path, PathBuf},
};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn read(path: &str) -> String {
    fs::read_to_string(repo().join(path)).unwrap_or_else(|error| panic!("{path}: {error}"))
}

/// Every file under `root` with one of `extensions`, skipping build output.
fn files(root: &Path, extensions: &[&str]) -> Vec<PathBuf> {
    fn visit(dir: &Path, extensions: &[&str], out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy();
            if path.is_dir() {
                if !matches!(
                    name.as_ref(),
                    "node_modules" | ".svelte-kit" | "build" | "target" | ".vercel"
                ) {
                    visit(&path, extensions, out);
                }
            } else if path
                .extension()
                .is_some_and(|ext| extensions.contains(&ext.to_string_lossy().as_ref()))
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

/// The section of a Markdown page under `heading`, up to the next heading
/// of the same or a higher level.
fn section<'a>(doc: &'a str, heading: &str) -> &'a str {
    let start = doc
        .find(&format!("\n{heading}\n"))
        .unwrap_or_else(|| panic!("docs/computer-use.md has no {heading:?} section"));
    let body = &doc[start + 1 + heading.len()..];
    let level = heading.chars().take_while(|c| *c == '#').count();
    let end = body
        .match_indices("\n#")
        .find(|(at, _)| {
            let hashes = body[at + 1..].chars().take_while(|c| *c == '#').count();
            hashes <= level && body[at + 1 + hashes..].starts_with(' ')
        })
        .map_or(body.len(), |(at, _)| at);
    &body[..end]
}

/// Phrases that would claim continuous capture, and the words that turn
/// them into a denial of it when they appear just before.
const CAPTURE_CLAIMS: &[&str] = &[
    "continuous capture",
    "continuous recording",
    "continuous screen",
    "continuously record",
    "continuously capture",
    "continuously watch",
    "records the screen",
    "record the screen",
    "records your screen",
    "record your screen",
    "recording your screen",
    "recording the screen",
    "watches your screen",
    "watches the screen",
    "watching your screen",
    "watching the screen",
    "live screen",
    "screen stream",
    "streams the screen",
    "streams your screen",
    "always-on capture",
    "always on capture",
    "passive capture",
    "background capture",
];

const NEGATIONS: &[&str] = &[
    "no ",
    "not ",
    "never",
    "nothing",
    "none",
    "without",
    "neither",
    "nor ",
    "instead of",
    "rather than",
    "as opposed to",
    "isn't",
    "aren't",
    "doesn't",
    "don't",
    "has no",
];

/// Each occurrence of a capture claim in `text` that no negation precedes
/// within the same sentence (a wrapped Markdown line is not a sentence
/// break; a blank line is), as `"<phrase>: <context>"`.
fn capture_claims(text: &str) -> Vec<String> {
    let lowered = text.to_lowercase();
    let mut found = Vec::new();
    for phrase in CAPTURE_CLAIMS {
        for (at, _) in lowered.match_indices(phrase) {
            let punctuation = lowered[..at]
                .rfind(['.', '!', '?', ':', ';', '('])
                .map_or(0, |i| i + 1);
            let paragraph = lowered[..at].rfind("\n\n").map_or(0, |i| i + 2);
            let sentence_start = punctuation.max(paragraph);
            // A wrapped line breaks a negation like "no\ncontinuous"; read
            // it as the one sentence it is.
            let before = lowered[sentence_start..at].replace('\n', " ");
            let negated = NEGATIONS.iter().any(|negation| before.contains(negation));
            if !negated {
                let end = (at + phrase.len() + 40).min(lowered.len());
                let end = (end..=lowered.len())
                    .find(|&i| lowered.is_char_boundary(i))
                    .unwrap_or(lowered.len());
                let start = (sentence_start..=at)
                    .find(|&i| lowered.is_char_boundary(i))
                    .unwrap_or(at);
                found.push(format!(
                    "{phrase:?}: {}",
                    lowered[start..end].replace('\n', " ").trim()
                ));
            }
        }
    }
    found
}

#[test]
fn the_computer_use_doc_states_the_privacy_model_and_covers_both_targets() {
    let doc = read("docs/computer-use.md");
    for heading in [
        "## Privacy",
        "## Platform status",
        "## The Cua Driver pin",
        "## Choosing a computer",
        "## Desktop targets (#17)",
        "## Permissions (#20)",
        "## Typed machine tools (#18)",
        "## Release check",
    ] {
        assert!(
            doc.contains(&format!("\n{heading}\n")),
            "docs/computer-use.md is missing the {heading:?} section"
        );
    }

    let privacy = section(&doc, "## Privacy");
    for required in [
        "explicit",
        "permissioned",
        "one-shot screenshot",
        "during an action",
        "does not continuously record the screen",
        "never",
        "server-local",
        "remote desktop",
        "Accessibility",
        "Screen Recording",
        "include_screenshot",
        "no continuous capture",
        "no recording",
        "upload",
    ] {
        assert!(
            privacy.contains(required),
            "the Privacy section of docs/computer-use.md must say {required:?}"
        );
    }

    let platforms = section(&doc, "## Platform status");
    for required in ["macOS", "Linux", "Windows", "headless", "unsupported"] {
        assert!(
            platforms.contains(required),
            "the Platform status section must name {required:?}"
        );
    }

    let pin = section(&doc, "## The Cua Driver pin");
    for required in [
        "cua_driver_pin",
        "cua-driver.pin",
        "nolune cua install",
        "nolune cua status",
        "sha256",
        "never updated on its own",
        "self-updater",
        "release",
    ] {
        assert!(
            pin.contains(required),
            "the Cua Driver pin section must say {required:?}"
        );
    }

    let check = section(&doc, "## Release check");
    assert!(
        check.contains("scripts/release-check-computer-use.sh")
            && check.contains("docs/release-checklist.md")
            && check.contains("macOS"),
        "the Release check section names the script and the checklist"
    );

    // The usage side: both kinds of target and how each appears.
    for required in [
        "server-local:",
        "cua_request",
        "list_machines",
        "discover_windows",
        "get_window_state",
        "verify_state",
        "delivery_mode",
        "background",
        "nolune cua install",
        "NOLUNE_CUA_DRIVER",
    ] {
        assert!(
            doc.contains(required),
            "docs/computer-use.md is missing {required:?}"
        );
    }
    assert!(
        !doc.contains("(#16)\n\nThe machine the Nolune server runs on can be one of"),
        "the page is the whole computer-use document, not the server-local slice"
    );
}

#[test]
fn the_readme_and_the_settings_page_carry_the_privacy_wording() {
    let readme = read("README.md");
    assert!(
        readme.contains("one-shot") && readme.contains("docs/computer-use.md"),
        "README.md names the one-shot capture and points at docs/computer-use.md"
    );
    let capabilities = readme
        .lines()
        .find(|line| line.starts_with("- **Computer use**"))
        .expect("README.md lists computer use among the capabilities");
    assert!(
        capabilities.contains("one-shot") && capabilities.to_lowercase().contains("never"),
        "the capability row says what a capture is and what never happens: {capabilities}"
    );
    assert!(
        readme.contains("Nolune never records the screen"),
        "README.md states that Nolune never records the screen"
    );

    let settings = read("docs/settings.md");
    let computers = settings
        .lines()
        .find(|line| line.starts_with("| Computers |"))
        .expect("docs/settings.md has the Computers row");
    assert!(
        computers.contains("one-shot")
            && computers.contains("never")
            && computers.contains("computer-use.md#privacy"),
        "the Computers row states the privacy model and links the section: {computers}"
    );
}

#[test]
fn no_shipped_copy_claims_continuous_capture() {
    let repo = repo();
    let mut paths = vec![repo.join("README.md")];
    paths.extend(files(&repo.join("docs"), &["md"]));
    for (dir, extensions) in [
        ("client/src", &["svelte", "js", "ts"][..]),
        ("desktop/src", &["svelte", "js", "ts"][..]),
        ("landing/src", &["svelte", "js", "ts"][..]),
    ] {
        paths.extend(files(&repo.join(dir), extensions));
    }
    assert!(paths.len() > 20, "the scan found the copy: {}", paths.len());

    let mut violations = Vec::new();
    for path in paths {
        let text = fs::read_to_string(&path).unwrap();
        let relative = path.strip_prefix(&repo).unwrap().to_string_lossy();
        for claim in capture_claims(&text) {
            violations.push(format!("{relative}: {claim}"));
        }
    }
    assert!(
        violations.is_empty(),
        "copy claims continuous capture:\n{}",
        violations.join("\n")
    );
}

#[test]
fn the_capture_claim_scan_reads_negations_and_flags_claims() {
    assert!(capture_claims("Nolune does not continuously record the screen.").is_empty());
    assert!(capture_claims("There is no continuous capture, no recording.").is_empty());
    assert!(capture_claims("a one-shot window snapshot, never a live screen feed").is_empty());
    assert!(
        capture_claims("There is no\ncontinuous capture here.").is_empty(),
        "a negation split from its phrase by a wrapped line still counts"
    );
    assert!(capture_claims("It records the screen while you work.").len() == 1);
    assert!(
        capture_claims("Nothing is recorded. It watches your screen continuously.").len() == 1,
        "a negation in an earlier sentence does not cover the next one"
    );
    assert_eq!(
        capture_claims("continuous capture keeps the model informed")[0]
            .split(':')
            .next()
            .unwrap(),
        "\"continuous capture\""
    );
}

#[test]
fn the_release_checklist_names_the_manual_computer_use_check() {
    let script = repo().join("scripts/release-check-computer-use.sh");
    assert!(
        script.is_file(),
        "scripts/release-check-computer-use.sh is the manual macOS release check"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&script).unwrap().permissions().mode();
        assert!(mode & 0o111 != 0, "the release check script is executable");
    }
    let source = fs::read_to_string(&script).unwrap();
    assert!(
        source.starts_with("#!/usr/bin/env bash"),
        "the script runs under bash"
    );
    assert!(
        source.contains("set -euo pipefail"),
        "the script fails closed"
    );
    for check in [
        "list_machines",
        "get_window_state",
        "verified click",
        "stale",
        "permission",
        "disconnect",
        "background window",
    ] {
        assert!(source.contains(check), "the release check walks {check:?}");
    }
    for prerequisite in ["Darwin", "Aqua", "nolune cua status", "prerequisite"] {
        assert!(
            source.contains(prerequisite),
            "the script checks the {prerequisite:?} prerequisite"
        );
    }
    assert!(
        source.contains("exit 1") || source.contains("exit 2"),
        "a missing prerequisite exits non-zero"
    );
    for absent in ["record", "stream", "watch "] {
        assert!(
            !source
                .to_lowercase()
                .contains(&format!("cua-driver {absent}")),
            "the script never runs the driver's {absent:?} surface"
        );
    }

    let checklist = read("docs/release-checklist.md");
    assert!(
        checklist.contains("scripts/release-check-computer-use.sh"),
        "docs/release-checklist.md names the manual computer-use check"
    );
}

#[test]
fn the_capture_guard_script_covers_every_cua_file() {
    let guard = read("scripts/tests/no-continuous-screen-recording.py");
    for covered in [
        "cua-protocol/src",
        "server/src/services/cua",
        "server/src/services/tools/cua.rs",
        "server/src/services/tools/computer.rs",
        "server/src/services/machine_registry.rs",
        "server/src/routes/machine_agents.rs",
        "desktop/src-tauri/src/cua_runtime.rs",
        "desktop/src-tauri/src/cua_permissions.rs",
        "desktop/src-tauri/src/cua_install.rs",
        "desktop/src-tauri/src/computer_use_bridge.rs",
        "desktop/src/lib/cua-permissions.js",
        "client/src/lib/computers/spaces.js",
        "scripts/cua-driver.sh",
        "scripts/release-check-computer-use.sh",
    ] {
        assert!(
            guard.contains(covered),
            "scripts/tests/no-continuous-screen-recording.py must scan {covered}"
        );
    }
    assert!(
        guard.contains("def test_every_cua_file_takes_one_shot_captures_only"),
        "the guard has one test over every Cua file"
    );
}
