//! Guards for #110 (PR 2): inbound intents are opened, decoded, checked
//! for redelivery, judged, and only then delivered and answered; peer text
//! leaves the handler only inside the untrusted block and never reaches a
//! tool; the store and the inbox listing have no room for a payload; the
//! client renders the block as visibly untrusted through a pure helper.

#[path = "../test-support/source_scan.rs"]
mod source_scan;

use std::{
    fs,
    path::{Path, PathBuf},
};

use source_scan::without_cfg_test_items;

const HANDLER: &str = "server/src/services/federation/inbound.rs";
const DELIVERY: &str = "server/src/services/peer_delivery.rs";
const ROUTES: &str = "server/src/routes/federation.rs";
const GATE: &str = "server/src/services/federation/gate.rs";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn read(relative: &str) -> String {
    fs::read_to_string(repo().join(relative)).unwrap_or_else(|_| panic!("{relative} is missing"))
}

fn production(relative: &str) -> String {
    without_cfg_test_items(&read(relative))
}

fn sources_under(relative: &str) -> Vec<(String, String)> {
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
    let root = repo();
    let mut paths = Vec::new();
    visit(&root.join(relative), &mut paths);
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let relative = path.strip_prefix(&root).unwrap().display().to_string();
            let source = fs::read_to_string(&path).unwrap();
            (relative, without_cfg_test_items(&source))
        })
        .collect()
}

/// The body of `fn <name>(` up to the next function.
fn function<'a>(source: &'a str, name: &str) -> &'a str {
    source
        .split(&format!("fn {name}("))
        .nth(1)
        .and_then(|rest| rest.split("\nfn ").next())
        .unwrap_or_else(|| panic!("{name} exists"))
}

#[test]
fn an_intent_is_opened_decoded_deduplicated_and_judged_before_it_is_delivered_or_answered() {
    let handler = production(HANDLER);
    let receive = function(&handler, "receive_intent");
    let order = [
        (".hold()", "holds the identity lock"),
        (".open(", "opens the envelope"),
        ("FederationIntent::decode(", "decodes fail-closed"),
        (
            "check_sender(",
            "checks the intent's sender against the envelope's",
        ),
        (".one_at_a_time()", "serialises deliveries"),
        (".find(", "looks the request up before judging it"),
        ("judge_held(", "judges through the gate"),
        ("deliver(", "delivers only what was allowed"),
        (".record(", "records before answering"),
        (".seal(", "answers last"),
    ];
    // Each step is found after the one before it (an unknown name is judged
    // before the lookup, since there is no request to look up).
    let mut last = 0;
    for (needle, what) in order {
        let at = receive[last..]
            .find(needle)
            .map(|at| last + at)
            .unwrap_or_else(|| {
                panic!("receive_intent must {what} after the step before it ({needle})")
            });
        last = at + needle.len();
    }
    // A settled request is answered from the store, before the gate judges
    // the request (the judgement of an unknown name comes earlier and has
    // no request to look up).
    let lookup_at = receive.find(".find(").unwrap();
    let settled_at = receive[lookup_at..]
        .find("InboundStatus::Pending")
        .expect("distinguishes pending");
    let judge_at = receive[lookup_at..].find("judge_held(").unwrap();
    assert!(
        settled_at < judge_at,
        "the store is consulted before the gate"
    );
    // Nothing decodes twice: the strict decode is the one entry.
    assert_eq!(receive.matches("FederationIntent::decode(").count(), 1);
    // A refusal from `open` is recorded like the ping route records it.
    assert!(receive.contains("record_refused_sender("));
}

#[test]
fn peer_text_leaves_the_handler_only_inside_the_untrusted_block_and_never_reaches_a_tool() {
    // The handler never touches the payload: delivery is a seam it is
    // handed, and the federation state never reaches into the companion's
    // directory (the #108 guard pins that for the whole directory).
    let handler = production(HANDLER);
    for leak in [
        "render_untrusted_block",
        "PeerText",
        "IntentPayload::Message",
        "IntentPayload::Reminder",
        "IntentPayload::Proposal",
        "description",
        "services::tools",
        "services::llm",
        "services::mcp",
        "services::chat",
        "AppState",
        "CANONICAL_SLUG",
    ] {
        assert!(
            !handler.contains(leak),
            "{HANDLER} must not reach the payload or the companion via {leak:?}"
        );
    }
    let receive = function(&handler, "receive_intent");
    assert!(
        receive.contains("deliver: impl FnOnce(&FederationIntent, u64)"),
        "receive_intent takes its delivery as a seam"
    );
    // The delivery is the one reader of the text, and hands it to the
    // block renderer and nowhere else.
    let delivery = production(DELIVERY);
    assert!(delivery.contains("render_untrusted_block("));
    for leak in [
        "services::tools",
        "services::llm",
        "services::mcp",
        "run_agent_loop",
        "run_single_turn",
        "ToolCall",
        "tool_name:",
        "save_system_message",
        "PeerText::new(",
    ] {
        assert!(
            !delivery.contains(leak),
            "{DELIVERY} must not reach peer text or a tool via {leak:?}"
        );
    }
    // The commitment a reminder becomes is written from ids, never the text.
    let deliver = function(&delivery, "deliver");
    let promise = deliver
        .split("promise: ")
        .nth(1)
        .and_then(|rest| rest.split('\n').next())
        .expect("a reminder becomes a commitment with a promise");
    for word in ["text", "body", "description", "rendered", "block"] {
        assert!(
            !promise.contains(word),
            "the commitment's promise must not carry the peer's text: {promise}"
        );
    }
    // The line above the block is built from the class, the sender's id,
    // and numbers; it never formats a label or a text.
    let preface = function(&delivery, "preface");
    for leak in [
        "represented_owner",
        "purpose",
        "body",
        ".text",
        "description",
    ] {
        assert!(!preface.contains(leak), "preface formats the peer's {leak}");
    }
    // No tool knows the intents, the inbox, or the delivery.
    for (path, source) in sources_under("server/src/services/tools") {
        for token in [
            "federation_intent",
            "federation::inbound",
            "peer_delivery",
            "InboundStore",
            "IntentPayload",
        ] {
            assert!(
                !source.contains(token),
                "{path} mentions {token:?}: peer text must never become a tool argument"
            );
        }
    }
    // Nothing logs or prints from either module at all.
    for (name, source) in [(HANDLER, &handler), (DELIVERY, &delivery)] {
        for print in ["println", "eprintln", "dbg", "print", "eprint"] {
            assert!(
                !source.contains(&format!("{print}!(")),
                "{name} prints via {print}!"
            );
        }
        for log in ["log::info!(", "log::debug!(", "log::trace!("] {
            assert!(!source.contains(log), "{name} logs via {log}");
        }
    }
}

#[test]
fn the_store_and_the_listing_have_no_room_for_a_payload_and_are_bounded() {
    let handler = production(HANDLER);
    let record = handler
        .split("pub struct InboundIntent {")
        .nth(1)
        .and_then(|rest| rest.split("\n}\n").next())
        .expect("InboundIntent is defined");
    let fields: Vec<&str> = record
        .lines()
        .filter_map(|line| line.trim().strip_prefix("pub "))
        .map(|line| line.split(':').next().unwrap())
        .collect();
    assert_eq!(
        fields,
        [
            "version",
            "sender",
            "correlation_id",
            "pairing_id",
            "intent",
            "disclosure",
            "represented_owner",
            "purpose",
            "status",
            "response",
            "approval_id",
            "receipt_id",
            "message_id",
            "requested_at",
            "updated_at",
            "expires_at",
        ]
    );
    for payload in [
        "PeerText",
        "IntentPayload",
        "FederationIntent",
        "body",
        "text",
    ] {
        assert!(
            !record.contains(&format!("{payload}:")) && !record.contains(&format!(": {payload}")),
            "the record must not carry the payload ({payload})"
        );
    }
    assert!(
        handler
            .split("pub struct InboundIntent {")
            .next()
            .unwrap()
            .trim_end()
            .ends_with("#[serde(deny_unknown_fields)]"),
        "a record refuses fields it does not know"
    );
    for required in [
        "pub const MAX_INBOUND_INTENTS: usize",
        "pub const MAX_INTENT_RECEIPTS: usize",
        "pub const MAX_INTENT_RECEIPTS_PER_PAIRING: usize",
        "pub const INTENT_RECEIPT_RETENTION_SECS: u64",
        "const MAX_INBOUND_FILE_BYTES: u64",
        "replace_private(",
        "unloadable",
    ] {
        assert!(handler.contains(required), "{HANDLER} lacks {required}");
    }
    // The listing is the store's view and nothing more.
    let routes = production(ROUTES);
    let listing = routes
        .split("async fn list_inbox(")
        .nth(1)
        .and_then(|rest| rest.split("\n}\n").next())
        .expect("list_inbox exists");
    assert!(listing.contains("federation_inbox.view()"));
}

#[test]
fn the_intent_route_is_public_and_dispatches_through_the_handler_and_the_gate() {
    let routes = production(ROUTES);
    let public = routes
        .split("pub fn public_router")
        .nth(1)
        .and_then(|rest| rest.split("\n}\n").next())
        .expect("public_router exists");
    assert!(
        public.contains("INTENT_PATH, post(intent)"),
        "the intent route is mounted as POST in the public router"
    );
    let handler = routes
        .split("async fn intent(")
        .nth(1)
        .and_then(|rest| rest.split("\n}\n").next())
        .expect("intent handler exists");
    assert!(
        handler.contains("parse_transport(")
            && handler.contains("inbound::receive_intent(")
            && handler.contains("peer_delivery::deliver("),
        "the intent handler parses a transport envelope, dispatches through inbound::receive_intent, and delivers through peer_delivery"
    );
    assert!(
        !handler.contains("federation.open(") && !handler.contains("federation_gate."),
        "the route does not open or judge on its own"
    );
    let owner = routes
        .split("pub fn router()")
        .nth(1)
        .unwrap()
        .split("pub fn public_router")
        .next()
        .unwrap();
    assert!(owner.contains("\"/api/federation/inbox\", get(list_inbox)"));
    for code in [
        "\"intent_expired\"",
        "\"intent_issued_in_future\"",
        "\"unknown_intent_type\"",
        "\"invalid_intent\"",
    ] {
        assert!(routes.contains(code), "routes/federation.rs lacks {code}");
    }
    // The gate's held-lock entry judges exactly like admit, and the
    // approval a judgement consumed is what the receipt names.
    let gate = production(GATE);
    let judge_held = gate
        .split("pub(crate) fn judge_held(")
        .nth(1)
        .and_then(|rest| rest.split("\n    pub").next())
        .expect("judge_held exists");
    assert!(judge_held.contains("self.judge("));
    assert!(gate.contains("pub struct Judgement {"));
    assert!(
        gate.contains("approval_id = Some(entry.id)"),
        "a judgement carries the approval it consumed"
    );
}

#[test]
fn the_client_renders_peer_text_as_visibly_untrusted_through_a_pure_helper() {
    let helper = read("client/src/lib/federation/peer-content.js");
    for required in [
        "export function peerContent(",
        "<<<UNTRUSTED PEER CONTENT",
        "<<<END UNTRUSTED PEER CONTENT",
    ] {
        assert!(
            helper.contains(required),
            "peer-content.js lacks {required}"
        );
    }
    for forbidden in ["innerHTML", "eval(", "new Function", "marked", "DOMPurify"] {
        assert!(
            !helper.contains(forbidden),
            "peer-content.js uses {forbidden}"
        );
    }
    assert!(
        read("client/tests/federation-peer-content.test.mjs").contains("peer-content.js"),
        "the helper needs node tests"
    );
    let bubble = read("client/src/lib/components/chat/MessageBubble.svelte");
    assert!(
        bubble.contains("peerContent("),
        "MessageBubble renders peer content through the helper"
    );
    let branch = bubble
        .split("{#if peer}")
        .nth(1)
        .and_then(|rest| rest.split("{:else").next())
        .expect("MessageBubble has a branch for peer content");
    assert!(
        !branch.contains("{@html"),
        "peer text is never rendered as HTML"
    );
    assert!(
        branch.to_lowercase().contains("untrusted"),
        "the bubble says the content is untrusted"
    );
    let inbox = read("client/src/lib/federation/inbox.js");
    assert!(inbox.contains("export function inboxView("));
    assert!(read("client/tests/federation-inbox.test.mjs").contains("inbox.js"));
    let client = read("client/src/lib/api/client.ts");
    assert!(client.contains("export function fetchFederationInbox("));
    let types = read("client/src/lib/api/types.ts");
    for required in [
        "export interface FederationInboundIntent",
        "export interface FederationIntentReceipt",
    ] {
        assert!(types.contains(required), "types.ts is missing {required}");
    }
    for relative in [
        "client/src/lib/federation/peer-content.js",
        "client/src/lib/federation/inbox.js",
        "client/src/lib/components/federation/InboundIntents.svelte",
    ] {
        let source = read(relative);
        for forbidden in [
            "localStorage",
            "sessionStorage",
            "console.",
            "document.cookie",
        ] {
            assert!(
                !source.contains(forbidden),
                "{relative} contains {forbidden:?}"
            );
        }
    }
}

#[test]
fn the_docs_describe_inbound_delivery_the_inbox_and_the_untrusted_rendering() {
    let federation = read("docs/federation.md");
    for required in [
        "/federation/v1/intent",
        "/api/federation/inbox",
        "inbound.json",
        "byte for byte",
        "needs_owner",
        "untrusted",
    ] {
        assert!(
            federation.contains(required),
            "docs/federation.md is missing {required:?}"
        );
    }
    let storage = read("docs/companion-storage.md");
    assert!(storage.contains("inbound.json"));
    let design = read("docs/design-system.md");
    for required in ["peer-content.js", "InboundIntents.svelte", "untrusted"] {
        assert!(
            design.contains(required),
            "docs/design-system.md is missing {required:?}"
        );
    }
}
