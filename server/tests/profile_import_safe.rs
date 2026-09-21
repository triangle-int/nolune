//! Structural guard for #74: the companion restore never shells out to `tar`,
//! never stages under `instances/`, and every entry point (the multipart route,
//! the `restore_backup` tool, `nolune restore`, the Data page) reaches the
//! companion only through the validating, transactional restore.

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

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn production(relative: &str) -> String {
    without_cfg_test_items(&fs::read_to_string(repo().join(relative)).unwrap())
}

/// The production text between `start` and the next `#[cfg(test)]` item.
fn segment(source: &str, start: &str) -> String {
    source
        .split(start)
        .nth(1)
        .unwrap_or_else(|| panic!("segment {start:?} is missing"))
        .to_owned()
}

/// `source` with every run of whitespace removed, so a method chain that
/// rustfmt breaks across lines still matches its one-line spelling.
fn compact(source: &str) -> String {
    source.split_whitespace().collect()
}

#[test]
fn no_production_code_runs_the_tar_command() {
    let repo = repo();
    let mut violations = Vec::new();
    for path in rust_files(&repo.join("server/src")) {
        let relative = path
            .strip_prefix(&repo)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let production = without_cfg_test_items(&fs::read_to_string(&path).unwrap());
        let compact: String = production.split_whitespace().collect();
        for token in [
            "Command::new(\"tar\")",
            "Command::new(\"gtar\")",
            ".arg(\"tar\")",
            "\"tar\",",
            "\"tarczf",
            "\"tarxzf",
        ] {
            if compact.contains(token) {
                violations.push(format!("{relative} contains {token:?}"));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "an external tar is still invoked:\n{}",
        violations.join("\n")
    );
}

#[test]
fn import_staging_lives_under_a_top_level_imports_directory_never_under_instances() {
    let media = production("server/src/services/media_text.rs");
    assert!(
        media.contains("const IMPORTS_DIR: &str = \"imports\";"),
        "the staging root constant must be a top-level imports/ directory"
    );
    let import_path = segment(&media, "fn import_path(");
    let import_path = import_path.split("\n    }\n").next().unwrap();
    assert!(
        import_path.contains("Path::new(IMPORTS_DIR).join("),
        "every import name must resolve under IMPORTS_DIR: {import_path}"
    );
    assert!(
        !import_path.contains("instances"),
        "import names must never resolve under instances/"
    );

    // `cap_std::fs::Dir` is the capability type; only ambient `std::fs` is forbidden.
    let restore = production("server/src/services/profile_import.rs").replace("cap_std::fs::", "");
    for forbidden in [
        "\"instances\"",
        "\"instances/",
        "instances_dir",
        "std::fs::",
    ] {
        assert!(
            !restore.contains(forbidden),
            "profile_import.rs contains {forbidden:?}"
        );
    }
    for required in [
        "create_import(",
        "stash_companion(",
        "publish_import(",
        "remove_import(",
        "lifecycle_lock(",
    ] {
        assert!(
            restore.contains(required),
            "profile_import.rs does not go through {required}"
        );
    }
}

#[test]
fn the_import_route_streams_the_body_to_staging_and_never_buffers_or_extracts_itself() {
    let routes = production("server/src/routes/instances.rs");
    assert!(
        routes.contains("DefaultBodyLimit::max("),
        "the import route must bound its request body"
    );
    assert!(
        routes.contains("post(import_instance)"),
        "the import route must be registered"
    );
    let handler = segment(&routes, "async fn import_instance");
    for required in [
        "Multipart",
        ".chunk()",
        "create_import_upload(",
        "restore_companion(",
        "remove_import_upload(",
        "CONFLICT",
        "running_agent_tasks(",
    ] {
        assert!(
            handler.contains(required),
            "the import route does not go through {required}"
        );
    }
    // `std::fs::File` is the handle type the store hands back; only ambient
    // path operations are forbidden.
    for forbidden in [
        ".bytes()",
        "to_bytes(",
        "read_to_end",
        "Vec<u8>",
        "Command::new",
        "fs::read",
        "fs::write",
        "fs::create_dir",
        "fs::remove",
        "fs::rename",
        "fs::metadata",
        "File::open(",
        "File::create(",
        "extract_into(",
        "workspace_dir",
        "\"instances\"",
        ".join(",
        "open_ambient_dir",
        "NOT_IMPLEMENTED",
    ] {
        assert!(
            !handler.contains(forbidden),
            "the import route contains {forbidden:?}"
        );
    }
}

#[test]
fn the_restore_tool_takes_upload_ids_only_and_the_cli_posts_to_the_local_api() {
    let tools = production("server/src/services/tools/system.rs");
    let tool = segment(&tools, "pub struct ImportProfileTool");
    for required in [
        "starts_with(\"upload_\")",
        "open_upload_blob(",
        "restore_companion_from_agent(",
        "confirmed_by_user",
    ] {
        assert!(
            tool.contains(required),
            "restore_backup does not go through {required}"
        );
    }
    for forbidden in [
        "Path::new(",
        "PathBuf::from(",
        "workspace_dir",
        "instance_dir",
        "archive_path",
        "fs::",
        "Command::new",
        "extract_into(",
        "temporarily unavailable",
    ] {
        assert!(
            !tool.contains(forbidden),
            "restore_backup contains {forbidden:?}"
        );
    }
    let call = segment(&tool, "async fn call(");
    assert!(
        call.find("confirmed_by_user").unwrap() < call.find("open_upload_blob(").unwrap(),
        "restore_backup must refuse an unconfirmed call before it looks the upload up"
    );

    let cli = production("server/src/cli.rs");
    assert!(
        cli.contains("Restore {"),
        "cli.rs must declare the restore subcommand"
    );
    // The restore section ends at the next section header of cli.rs.
    let restore = segment(&cli, "fn restore_cmd(");
    let restore = restore.split("\n// \u{2500}\u{2500}").next().unwrap();
    for required in ["multipart::Form", "bearer_auth(", "/import"] {
        assert!(
            restore.contains(required),
            "nolune restore does not go through {required}"
        );
    }
    for forbidden in [
        "extract_into(",
        "tar::",
        "flate2",
        "join(\"instances\")",
        "instances_dir",
        "Command::new",
    ] {
        assert!(
            !restore.contains(forbidden),
            "nolune restore contains {forbidden:?}"
        );
    }
}

/// Every writer that reaches the companion tree through plain paths holds
/// the import gate shared, and a restart reconciles `imports/` before any
/// of them starts.
#[test]
fn ambient_writers_hold_the_import_gate_and_startup_reconciles_imports() {
    let boundary = production("server/src/app/companion_boundary.rs");
    let middleware = compact(&segment(&boundary, "pub async fn companion_boundary("));
    for required in [
        "import_gate()",
        ".writer().await",
        "IMPORT_ROUTE",
        "MatchedPath",
    ] {
        assert!(
            middleware.contains(required),
            "the companion boundary does not hold the import gate: {required}"
        );
    }
    assert!(
        middleware.find(".writer().await").unwrap() < middleware.find("admit(").unwrap(),
        "the gate must be held before the companion is created or opened"
    );

    let chat = production("server/src/routes/chat.rs");
    let post_chat = segment(&chat, "async fn post_chat(");
    let post_chat = compact(post_chat.split("\n}\n").next().unwrap());
    assert!(
        post_chat.contains("import_gate()") && post_chat.contains(".writer().await"),
        "POST /api/chat must hold the import gate while it saves and registers the loop"
    );
    assert!(
        post_chat.find(".writer().await").unwrap() < post_chat.find("save_user_message(").unwrap()
    );

    let proactive = production("server/src/services/proactive.rs");
    let admit = segment(&proactive, "fn admit(");
    assert!(
        admit.contains("try_writer()") && admit.contains("SkipReason::Import"),
        "ProactiveLoop::begin must take the import gate or skip"
    );
    assert!(
        admit.find("try_writer()").unwrap() < admit.find("self.policy()").unwrap(),
        "the gate comes before anything is read or written"
    );
    assert!(
        proactive.contains("_writer: Option<WriterGuard>"),
        "the run handle must hold the gate until the run is finished"
    );
    let scheduler = production("server/src/services/scheduler.rs");
    assert!(
        scheduler.contains("SkipReason::Import"),
        "the scheduler must leave a schedule in place while an import runs"
    );
    let state = production("server/src/app/state.rs");
    assert!(
        state.contains(".with_import_gate(vector_store.media_store().import_gate())"),
        "the one proactive loop must be given the process-wide gate"
    );

    let main = production("server/src/main.rs");
    let recovery = main
        .find("profile_import::recover_on_startup(")
        .expect("main.rs must reconcile imports/ at startup");
    for later in [
        "obsolete_instance_dirs(",
        "migrate_companion(",
        "notify_restart(",
        "scheduler::start(",
        "heartbeat::start(",
        "build_router(",
    ] {
        assert!(
            recovery < main.find(later).unwrap(),
            "the startup recovery must run before {later}"
        );
    }
    let restore = production("server/src/services/profile_import.rs");
    assert!(
        restore.contains("pub async fn recover_on_startup(")
            && restore.contains("has_companion_tree(")
            && restore.contains("list_imports(")
            && restore.contains("reset_collection(")
    );
}

#[test]
fn the_data_page_says_import_replaces_and_asks_before_it_does() {
    let repo = repo();
    let page = fs::read_to_string(repo.join("client/src/routes/[slug]/settings/data/+page.svelte"))
        .unwrap();
    assert!(
        page.contains("replaces"),
        "the Data page must say that import replaces the companion"
    );
    assert!(
        !page.contains("merges"),
        "the Data page still says import merges"
    );
    for required in [
        "confirmImport",
        "cancelImport",
        "role=\"alert\"",
        "role=\"status\"",
        "$lib/components/ui/alert-dialog/index.js",
        "<AlertDialog.Action",
        "<AlertDialog.Cancel",
        "<AlertDialog.Description",
    ] {
        assert!(
            page.contains(required),
            "the Data page lost its import control {required:?}"
        );
    }
    assert!(
        !page.contains("data-confirm"),
        "the Data page must confirm through the shared AlertDialog, not an inline block"
    );

    let client = fs::read_to_string(repo.join("client/src/lib/api/client.ts")).unwrap();
    let import = client
        .split("export function importInstance(")
        .nth(1)
        .expect("importInstance");
    let import = import.split("\n}\n").next().unwrap();
    assert!(
        import.contains("onProgress"),
        "importInstance must report upload progress"
    );
    assert!(
        !import.contains("fetch("),
        "importInstance must not buffer the archive through fetch without progress"
    );
    assert!(client.contains("export interface ImportOutcome"));

    for (doc, required) in [
        ("docs/settings.md", "nolune restore"),
        ("docs/settings.md", "replaces"),
        ("docs/companion-storage.md", "nolune restore"),
        ("docs/security/resource-url-inventory.md", "Import"),
    ] {
        let text = fs::read_to_string(repo.join(doc)).unwrap();
        assert!(text.contains(required), "{doc} is missing {required:?}");
    }
    let storage = fs::read_to_string(repo.join("docs/companion-storage.md")).unwrap();
    assert!(
        !storage.contains("answers `501`"),
        "docs/companion-storage.md still describes the import route as disabled"
    );
}
