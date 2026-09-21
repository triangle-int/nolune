//! Guard for #19: the legacy desktop computer-use executor (enigo pointer and
//! keyboard automation, the `screenshots` capture with its own scaling and
//! scale cache, the `computer_*` Tauri commands and the frontend bridge) is
//! gone, the server offers no coordinate schema for it, and the remote shell
//! and file toolcalls (`remote_bash`, `remote_files`) remain. Seeing and
//! acting in a window goes through the Cua driver only.

#[path = "../test-support/source_scan.rs"]
mod source_scan;

use source_scan::without_cfg_test_items;
use std::{
    fs,
    path::{Path, PathBuf},
};

fn repo() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

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

fn production(path: &str) -> String {
    without_cfg_test_items(&fs::read_to_string(repo().join(path)).unwrap())
}

/// The body of one `[table]` of a Cargo manifest, up to the next table.
fn cargo_table<'a>(manifest: &'a str, header: &str) -> &'a str {
    let start = manifest
        .find(&format!("\n{header}\n"))
        .unwrap_or_else(|| panic!("the desktop manifest has no {header} table"))
        + header.len()
        + 2;
    let body = &manifest[start..];
    let end = body.find("\n[").unwrap_or(body.len());
    &body[..end]
}

fn lists_crate(table: &str, name: &str) -> bool {
    table.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with(&format!("{name} ")) || line.starts_with(&format!("{name}="))
    })
}

#[test]
fn the_legacy_desktop_executor_is_gone() {
    let repo = repo();
    let mut violations = Vec::new();

    for removed in [
        "desktop/src-tauri/src/computer_use.rs",
        "desktop/src/lib/computer-use.ts",
    ] {
        if repo.join(removed).exists() {
            violations.push(format!("{removed} still exists"));
        }
    }

    // No enigo automation, no screenshots capture, no scaling or scale
    // cache, and none of the coordinate toolcalls the executor answered.
    let forbidden = [
        "enigo",
        "screenshots::",
        "MAX_SCREENSHOT_EDGE",
        "cached_scale",
        "\"left_click\"",
        "switch_desktop",
        "computer_screenshot",
        "screen_width",
        "screen_height",
        "computer_use::",
        "mod computer_use;",
    ];
    for path in files(&repo.join("desktop/src-tauri/src"), &["rs"]) {
        let relative = path
            .strip_prefix(repo)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let production = without_cfg_test_items(&fs::read_to_string(&path).unwrap());
        for token in forbidden {
            if production.contains(token) {
                violations.push(format!("{relative} contains {token:?}"));
            }
        }
    }

    // The desktop app no longer links the executor's crates. `base64` is a
    // test fixture's only (the protocol's 1x1 PNG), never a production
    // dependency.
    let manifest = fs::read_to_string(repo.join("desktop/src-tauri/Cargo.toml")).unwrap();
    let dependencies = cargo_table(&manifest, "[dependencies]");
    for krate in ["enigo", "screenshots", "image", "base64"] {
        if lists_crate(dependencies, krate) {
            violations.push(format!(
                "desktop/src-tauri/Cargo.toml still depends on {krate}"
            ));
        }
    }

    // The overlay still hears every action on the events it always listened
    // to, and the shell and file toolcalls the desktop executes remain.
    let bridge = production("desktop/src-tauri/src/computer_use_bridge.rs");
    for required in [
        "fn execute_bash(",
        "\"bash\" =>",
        "\"file_read\" =>",
        "\"file_write\" =>",
        "\"file_list\" =>",
        "\"upload_file\" =>",
        "\"get_window_state\"",
    ] {
        if !bridge.contains(required) {
            violations.push(format!(
                "desktop/src-tauri/src/computer_use_bridge.rs lost {required:?}"
            ));
        }
    }
    let overlay = production("desktop/src-tauri/src/overlay.rs");
    for event in ["\"computer-use-action\"", "\"computer-use-idle\""] {
        if !overlay.contains(event) {
            violations.push(format!(
                "desktop/src-tauri/src/overlay.rs no longer emits {event}"
            ));
        }
    }
    let overlay_page =
        fs::read_to_string(repo.join("desktop/src/routes/overlay/+page.svelte")).unwrap();
    for event in ["\"computer-use-action\"", "\"computer-use-idle\""] {
        if !overlay_page.contains(event) {
            violations.push(format!(
                "desktop/src/routes/overlay/+page.svelte no longer listens to {event}"
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "the legacy desktop executor remains:\n{}",
        violations.join("\n")
    );
}

#[test]
fn the_server_offers_no_coordinate_schema_and_keeps_the_remote_tools() {
    let repo = repo();
    let mut violations = Vec::new();

    // The coordinate tool, its argument schema and the screenshot result it
    // read are gone from every production file. The `"computer_use"` name
    // itself may stay in the trail summaries, which reload histories that
    // recorded it.
    let forbidden = [
        "ComputerUseTool",
        "ComputerUseArgs",
        "Available actions:",
        "coordinate: Option<[i32; 2]>",
        "scroll_direction: Option<String>",
        "\"switch_desktop\" =>",
        "result_type",
        "Need::ScreenCapture",
        "Need::Accessibility",
    ];
    for path in files(&repo.join("server/src"), &["rs"]) {
        let relative = path
            .strip_prefix(repo)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let production = without_cfg_test_items(&fs::read_to_string(&path).unwrap());
        for token in forbidden {
            if production.contains(token) {
                violations.push(format!("{relative} contains {token:?}"));
            }
        }
    }

    // The screen size came from the `screenshots` crate at registration;
    // nothing reports, records or renders it any more.
    for relative in [
        "server/src/routes/machine_agents.rs",
        "server/src/domain/machine.rs",
        "server/src/services/chat.rs",
        "server/src/services/tools/computer.rs",
    ] {
        let production = production(relative);
        for token in ["screen_width", "screen_height"] {
            if production.contains(token) {
                violations.push(format!("{relative} contains {token:?}"));
            }
        }
    }
    for relative in [
        "client/src/lib/api/types.ts",
        "client/src/lib/api/client.ts",
        "client/src/lib/computers/spaces.js",
    ] {
        let source = fs::read_to_string(repo.join(relative)).unwrap();
        for token in [
            "screen_width",
            "screen_height",
            "computer_use_request",
            "submitComputerUseResult",
            "built-in path",
        ] {
            if source.contains(token) {
                violations.push(format!("{relative} contains {token:?}"));
            }
        }
    }

    // The remote shell and file tools stay, still resolving the target the
    // user chose, and the desktop's shell and file toolcalls behind them.
    let computer = production("server/src/services/tools/computer.rs");
    for required in [
        "pub struct RemoteBashTool",
        "pub struct RemoteFilesTool",
        "action: \"bash\".into()",
        "format!(\"file_{}\", args.operation)",
    ] {
        if !computer.contains(required) {
            violations.push(format!(
                "server/src/services/tools/computer.rs lost {required:?}"
            ));
        }
    }
    let registrar = production("server/src/services/tools/mod.rs");
    for required in ["RemoteBashTool::new(", "RemoteFilesTool::new("] {
        if registrar.matches(required).count() != 1 {
            violations.push(format!(
                "server/src/services/tools/mod.rs must register {required} exactly once"
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "the coordinate computer-use surface remains:\n{}",
        violations.join("\n")
    );
}

#[test]
fn the_capture_guard_and_the_docs_describe_the_cua_path_only() {
    let repo = repo();
    let guard =
        fs::read_to_string(repo.join("scripts/tests/no-continuous-screen-recording.py")).unwrap();
    assert!(
        !guard.contains("assertIn('\"screenshot\" =>', desktop_bridge)"),
        "the capture guard must not expect the legacy screenshot toolcall on the desktop"
    );
    for required in [
        "desktop/src-tauri/src/computer_use.rs",
        "screenshots::",
        "enigo",
        "get_window_state",
    ] {
        assert!(
            guard.contains(required),
            "the capture guard must name the removed executor and the Cua capture path ({required:?} missing)"
        );
    }

    let doc = fs::read_to_string(repo.join("docs/computer-use.md")).unwrap();
    assert!(
        !doc.contains("until #19") && !doc.contains("stays in `tools/computer.rs`"),
        "docs/computer-use.md still describes the coordinate tool as pending removal"
    );
    for required in ["#19", "enigo", "remote_bash"] {
        assert!(
            doc.contains(required),
            "docs/computer-use.md is missing {required:?}"
        );
    }
    let storage = fs::read_to_string(repo.join("docs/companion-storage.md")).unwrap();
    assert!(
        !storage.contains("\"screen_width\": 2560"),
        "docs/companion-storage.md still shows a screen size in a machine record"
    );
}
