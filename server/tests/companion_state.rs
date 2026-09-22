//! Guard for #86 (companion state): Little Moon is driven by the pure
//! companion-state reducer, never by a bare "thinking" flag; every state has
//! an accessible, linked status element and a still expression; the overlays
//! run the same model; and the docs, the gallery and the SVG assets stay in
//! step with the expression table.

#[path = "../test-support/source_scan.rs"]
mod source_scan;

use source_scan::without_cfg_test_items;
use std::{fs, path::Path};

fn repo() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

fn read(path: &str) -> String {
    fs::read_to_string(repo().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn production(path: &str) -> String {
    without_cfg_test_items(&read(path))
}

#[test]
fn the_chat_moon_reads_the_companion_state_not_the_thinking_flag() {
    // The moon lives in the chat, under the last message; the full-screen
    // scene moon is gone.
    assert!(
        !repo()
            .join("client/src/lib/components/SharedScene.svelte")
            .exists(),
        "the full-screen scene moon is retired; the chat carries the only moon"
    );
    let presence = read("client/src/lib/components/companion/CompanionPresence.svelte");
    for forbidden in [
        "store.thinking",
        "scene.thinking",
        "avatar.thinking",
        "avatar.idle",
        "\"Nolune is thinking\"",
    ] {
        assert!(
            !presence.contains(forbidden),
            "CompanionPresence must not derive the moon from the raw thinking flag ({forbidden} found)"
        );
    }
    for component in ["MoonExpression", "CompanionStatus"] {
        assert!(
            presence.contains(component),
            "CompanionPresence renders the shared {component} component"
        );
    }
    let chat = read("client/src/lib/components/chat/ChatView.svelte");
    assert!(
        chat.contains("<CompanionPresence") && chat.contains("state={scene.companion}"),
        "ChatView renders CompanionPresence from the reducer state held by the scene store"
    );

    let store = production("client/src/lib/stores/scene.svelte.ts");
    assert!(
        !store.contains("setThinking") && !store.contains("thinking: boolean"),
        "the scene store no longer carries a bare thinking flag"
    );
    assert!(
        store.contains("$lib/companion/state.js") && store.contains("reduceCompanion"),
        "the scene store binds the reducer"
    );

    let moon = read("client/src/lib/components/companion/MoonExpression.svelte");
    assert!(
        moon.contains("$lib/companion/expressions.js") && moon.contains("companionExpression"),
        "the moon's face and motion come from the pure expression table"
    );
    assert!(
        moon.contains("prefers-reduced-motion"),
        "reduced motion is honored where the motion is defined"
    );
    let status = read("client/src/lib/components/companion/CompanionStatus.svelte");
    assert!(
        status.contains("$lib/companion/state.js") && status.contains("companionStatusSegments"),
        "the status element renders the linked segments from the reducer module"
    );
    assert!(
        status.contains("aria-live=\"polite\"") && status.contains("role=\"status\""),
        "the status element is a polite live region"
    );
}

#[test]
fn the_chat_view_feeds_persisted_state_and_the_overlays_run_the_same_model() {
    let chat = read("client/src/lib/components/chat/ChatView.svelte");
    assert!(
        !chat.contains("setThinking"),
        "ChatView no longer sets a bare thinking flag on the scene"
    );
    assert!(
        chat.contains("type: \"snapshot\""),
        "ChatView feeds the conversation snapshot (agent_running) into the reducer"
    );
    // The server broadcasts `chat_snapshot{agent_running:false}` right before
    // `agent_stopped` (server/src/routes/chat.rs), so the live snapshot must
    // not end the run in the reducer: only the snapshots ChatView loads are
    // persisted state. `reconcileSnapshot` handles the live event.
    let reconcile = chat
        .split("function reconcileSnapshot(")
        .nth(1)
        .and_then(|rest| rest.split("\n\t}\n").next())
        .expect("ChatView defines reconcileSnapshot");
    assert!(
        !reconcile.contains("companionEvent"),
        "the live chat_snapshot (reconcileSnapshot) is not fed to the reducer as persisted state"
    );
    let resync = chat
        .split("=== \"resync\"")
        .nth(1)
        .and_then(|rest| rest.split("return;").next())
        .expect("ChatView handles resync");
    assert!(
        resync.contains("fetchMessages(") && resync.contains("type: \"snapshot\""),
        "a resync loads the conversation and feeds its agent_running as persisted state"
    );

    let layout = read("client/src/routes/+layout.svelte");
    assert!(
        layout.contains("companionEventFromServer(") && layout.contains("approval_resolved"),
        "the root layout feeds every websocket event and the answered request into the reducer"
    );

    let overlay = read("client/src/routes/overlay/[slug]/+page.svelte");
    for required in ["companionExpression", "MoonExpression", "CompanionStatus"] {
        assert!(
            overlay.contains(required),
            "the client overlay reads the shared model ({required})"
        );
    }
    assert!(
        !overlay.contains("agent_running ?? false") && !overlay.contains("setInterval"),
        "the client overlay no longer polls a bare thinking flag"
    );
    assert!(
        overlay.contains("fetchMessages(")
            && overlay.contains("type: \"snapshot\"")
            && overlay.contains("ws.connected"),
        "the client overlay loads the default conversation's persisted agent_running on every connection"
    );

    let desktop = read("desktop/src/routes/overlay/+page.svelte");
    for required in [
        "\"computer-use-action\"",
        "\"computer-use-idle\"",
        "overlay-state",
        "companionExpression",
        "aria-live=\"polite\"",
        "prefers-reduced-motion",
    ] {
        assert!(
            desktop.contains(required),
            "the desktop overlay keeps its Tauri events and reads the shared model ({required})"
        );
    }
    let bridge = read("desktop/src/lib/overlay-state.js");
    assert!(
        bridge.contains("client/src/lib/companion/state.js"),
        "the desktop overlay reduces through the client's reducer, not a copy"
    );
}

#[test]
fn every_expression_has_an_asset_and_the_docs_and_gallery_list_the_states() {
    let table = read("client/src/lib/companion/expressions.js");
    let files: Vec<&str> = table
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix("{ kind: \"")?;
            let file = rest.split("file: \"").nth(1)?;
            file.split('"').next()
        })
        .collect();
    assert!(
        files.len() >= 11,
        "the table lists every companion state with its SVG ({} rows)",
        files.len()
    );
    let docs = read("docs/design-system.md");
    let gallery = read("client/src/routes/design-system/+page.svelte");
    for file in &files {
        let path = format!("client/static/skins/moon/{file}");
        assert!(repo().join(&path).is_file(), "{path} is missing");
        assert!(
            docs.contains(file),
            "docs/design-system.md names {file} in the companion-state table"
        );
    }
    for required in [
        "## Companion state",
        "prefers-reduced-motion",
        "MoonExpression.svelte",
        "CompanionStatus.svelte",
        "expressions.js",
        "overlay-state.js",
    ] {
        assert!(
            docs.contains(required),
            "docs/design-system.md describes {required}"
        );
    }
    for required in [
        "STATE_EXAMPLES",
        "companionExpression",
        "MoonExpression",
        "CompanionStatus",
        "Reduced motion",
    ] {
        assert!(
            gallery.contains(required),
            "/design-system previews every state with its expression and status ({required})"
        );
    }
}
