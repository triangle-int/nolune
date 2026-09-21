//! Guards for #110 (PR 3): the outbox retries with a bounded backoff,
//! writes an attempt down before it leaves and recovers it after a
//! restart, gives up visibly, and records what was asked and what the
//! peer disclosed from the typed response only; the sending tools are
//! thin typed wrappers gated by this owner's own policy, registered for
//! the chat and never for a routine, and take no free-form method and no
//! peer text; the loop is started and stopped by `main`, the listing and
//! the event reach the client, and the docs say so.

#[path = "../test-support/source_scan.rs"]
mod source_scan;

use std::{
    fs,
    path::{Path, PathBuf},
};

use source_scan::without_cfg_test_items;

const OUTBOX: &str = "server/src/services/federation/outbox.rs";
const TOOLS: &str = "server/src/services/tools/federation.rs";
const TOOLS_MOD: &str = "server/src/services/tools/mod.rs";
const ROUTINE: &str = "server/src/services/companion_routine.rs";
const GATE: &str = "server/src/services/federation/gate.rs";
const ROUTES: &str = "server/src/routes/federation.rs";

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

/// The body of `fn <name>(` up to the end of that function: the next
/// item at its own indentation (a method's sibling, or a top-level
/// item's closing brace).
fn function<'a>(source: &'a str, name: &str) -> &'a str {
    source
        .split(&format!("fn {name}("))
        .nth(1)
        .and_then(|rest| rest.split("\n}\n").next())
        .and_then(|rest| rest.split("\n    fn ").next())
        .and_then(|rest| rest.split("\n    pub fn ").next())
        .and_then(|rest| rest.split("\n    pub(crate) fn ").next())
        .and_then(|rest| rest.split("\n    pub async fn ").next())
        .and_then(|rest| rest.split("\n    async fn ").next())
        .unwrap_or_else(|| panic!("{name} exists"))
}

/// The argument text of every `<macro>!(...)` invocation in `source`.
fn macro_arguments(source: &str, name: &str) -> Vec<String> {
    let needle = format!("{name}!(");
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = source[from..].find(&needle) {
        let open = from + at + needle.len() - 1;
        let bytes = source.as_bytes();
        let mut depth = 0usize;
        let mut index = open;
        let mut in_string = false;
        while index < bytes.len() {
            match bytes[index] {
                b'\\' if in_string => index += 1,
                b'"' => in_string = !in_string,
                b'(' if !in_string => depth += 1,
                b')' if !in_string => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            index += 1;
        }
        out.push(source[open + 1..index].to_owned());
        from = index;
    }
    out
}

/// The value of `pub const NAME: <type> = <expr>;`, evaluated for the
/// simple products the constants are written as.
fn constant(source: &str, name: &str) -> u64 {
    let line = source
        .lines()
        .find(|line| line.trim_start().starts_with(&format!("pub const {name}:")))
        .unwrap_or_else(|| panic!("{name} is defined"));
    let expression = line.split('=').nth(1).unwrap().trim().trim_end_matches(';');
    expression
        .split('*')
        .map(|factor| factor.trim().replace('_', "").parse::<u64>().unwrap())
        .product()
}

#[test]
fn the_outbox_retries_with_a_bounded_backoff_and_gives_up_visibly() {
    let outbox = production(OUTBOX);
    let attempts = constant(&outbox, "MAX_DELIVERY_ATTEMPTS");
    let base = constant(&outbox, "RETRY_BASE_SECS");
    let cap = constant(&outbox, "RETRY_MAX_SECS");
    let lifetime = constant(&outbox, "OUTBOX_INTENT_LIFETIME_SECS");
    let owner_retry = constant(&outbox, "OWNER_RETRY_SECS");
    assert!((3..=16).contains(&attempts), "a bounded number of attempts");
    assert!(base >= 10 && cap >= base && cap <= 6 * 60 * 60);
    assert!(
        lifetime <= 7 * 24 * 60 * 60,
        "a queued intent lives no longer than the decoder allows"
    );
    assert!(owner_retry >= 5 * 60 && owner_retry < lifetime);
    assert!(outbox.contains("pub fn backoff_secs("));
    assert!(
        outbox.contains("RETRY_MAX_SECS)") || outbox.contains(".min(RETRY_MAX_SECS"),
        "the backoff is capped"
    );

    // An attempt is written before the envelope leaves, so a crash leaves
    // a mark that a restart recovers on the same correlation id.
    let attempt = function(&outbox, "attempt");
    let written = attempt
        .find("AttemptOutcome::InFlight {}")
        .expect("the attempt is written as in flight");
    let posted = attempt
        .find(".post_transport(")
        .expect("the attempt posts through the transport");
    assert!(
        written < posted,
        "the in-flight mark must be written before the post"
    );
    assert!(
        attempt[written..posted].contains(".save("),
        "the in-flight mark is persisted before the post"
    );
    assert!(outbox.contains("pub fn recover_on_restart("));
    let recover = function(&outbox, "recover_on_restart");
    assert!(recover.contains("AttemptOutcome::Interrupted {}"));
    assert!(
        !recover.contains("correlation_id:") && !recover.contains("new_correlation_id("),
        "recovery never mints a new request"
    );
    assert!(
        function(&outbox, "enqueue").contains("new_correlation_id("),
        "enqueue is the one place a correlation id is minted"
    );
    assert_eq!(
        outbox.matches("new_correlation_id()").count()
            - outbox.matches("fn new_correlation_id()").count(),
        1,
        "one call site mints ids"
    );

    // Settling records the audit line first, then the intent receipt,
    // then the entry, and every intent receipt is built from a typed
    // response.
    let settle = function(&outbox, "settle");
    let audit_at = settle
        .find("record_requesting(")
        .expect("settling records the requesting-side audit receipt");
    let receipt_at = settle
        .find("IntentReceipt::new(")
        .expect("settling writes the intent receipt");
    let saved_at = settle.find(".save(").expect("settling saves the entry");
    assert!(
        audit_at < receipt_at && receipt_at < saved_at,
        "a decision is not made without its receipt"
    );
    assert!(
        outbox.contains("IntentResponse::decode(") && outbox.contains(".check_against("),
        "the peer's answer is decoded fail-closed and checked against the intent"
    );

    // The entry's shape is pinned: the intent it sends, its history, and
    // the peer's typed response; nothing else the peer said has a field.
    let record = outbox
        .split("pub struct OutboxEntry {")
        .nth(1)
        .and_then(|rest| rest.split("\n}\n").next())
        .expect("OutboxEntry is defined");
    let fields: Vec<&str> = record
        .lines()
        .filter_map(|line| line.trim().strip_prefix("pub "))
        .map(|line| line.split(':').next().unwrap())
        .collect();
    assert_eq!(
        fields,
        [
            "version",
            "recipient",
            "pairing_id",
            "intent",
            "status",
            "attempts",
            "next_attempt_at",
            "response",
            "receipt_id",
            "chat_id",
            "created_at",
            "updated_at",
        ]
    );
    assert!(record.contains("pub response: Option<IntentResponse>"));
    assert!(
        outbox.contains("#[serde(deny_unknown_fields)]\npub struct OutboxEntry"),
        "unknown fields are refused"
    );
    for open_ended in ["serde_json::Value", "untagged", "flatten"] {
        assert!(!outbox.contains(open_ended), "{OUTBOX} uses {open_ended}");
    }

    // Nothing here logs or prints an intent, an entry, a text, or an
    // answer; the loop's one warning names the error alone.
    for print in ["println", "eprintln", "dbg", "print", "eprint"] {
        assert!(
            macro_arguments(&outbox, print).is_empty(),
            "{OUTBOX} prints via {print}!"
        );
    }
    for name in ["error", "warn", "info", "debug", "trace", "log"] {
        for arguments in macro_arguments(&outbox, name) {
            for token in [
                "intent", "entry", "text", "body", "response", "answer", "envelope", "payload",
            ] {
                assert!(
                    !arguments.to_lowercase().contains(token),
                    "{OUTBOX} formats {token:?} in {name}!({arguments})"
                );
            }
        }
    }
    let file = outbox
        .split("struct OutboxFile {")
        .nth(1)
        .and_then(|rest| rest.split('}').next())
        .expect("OutboxFile is defined");
    assert!(file.contains("entries") && file.contains("receipts"));
    assert!(
        outbox.contains("MAX_OUTBOX_ENTRIES") && outbox.contains("MAX_OUTBOX_RECEIPTS_PER_PAIRING"),
        "both lists are bounded"
    );
}

#[test]
fn the_sending_tools_are_thin_typed_wrappers_gated_by_this_owners_policy() {
    let tools = production(TOOLS);
    for name in [
        "\"send_peer_message\"",
        "\"ask_peer_availability\"",
        "\"propose_peer_reminder\"",
    ] {
        assert!(
            tools.contains(&format!("const NAME: &'static str = {name};")),
            "{TOOLS} lacks the tool {name}"
        );
    }
    assert_eq!(
        tools.matches("const NAME: &'static str").count(),
        3,
        "exactly three sending tools"
    );
    // No free-form remote method, command, or payload: every argument
    // struct names a peer, the two labels, and its class's own fields.
    for args in [
        "SendPeerMessageArgs",
        "AskPeerAvailabilityArgs",
        "ProposePeerReminderArgs",
    ] {
        let definition = tools
            .split(&format!("pub struct {args} {{"))
            .nth(1)
            .and_then(|rest| rest.split("\n}\n").next())
            .unwrap_or_else(|| panic!("{args} is defined"));
        let fields: Vec<&str> = definition
            .lines()
            .filter_map(|line| line.trim().strip_prefix("pub "))
            .map(|line| line.split(':').next().unwrap())
            .collect();
        for forbidden in [
            "method",
            "command",
            "tool",
            "rpc",
            "payload",
            "json",
            "args",
            "arguments",
            "action",
            "body",
            "memory",
            "query",
        ] {
            assert!(
                !fields.contains(&forbidden),
                "{args} takes a free-form {forbidden:?}"
            );
        }
        assert!(
            fields.contains(&"peer")
                && fields.contains(&"on_behalf_of")
                && fields.contains(&"purpose"),
            "{args} names the peer and the two labels: {fields:?}"
        );
        assert!(
            !definition.contains("serde_json::Value") && !definition.contains("HashMap"),
            "{args} takes no open-ended value"
        );
    }
    // Every call builds one typed request and goes through the outbox;
    // the tool never sees the peer's answer, let alone its text.
    assert_eq!(
        tools.matches("OutboxRequest::").count(),
        3,
        "one typed request per tool"
    );
    assert!(tools.contains(".enqueue("));
    for leak in [
        "IntentResponse",
        ".response",
        ".answer",
        "windows",
        "post_transport",
        "run_due",
        "inbound",
        "chat::",
        "save_user_message",
        "save_delivered_message",
    ] {
        assert!(!tools.contains(leak), "{TOOLS} mentions {leak:?}");
    }
    let queued = function(&tools, "queued");
    for leak in [
        "text",
        "body",
        "response",
        "answer",
        "represented_owner",
        "purpose",
    ] {
        assert!(
            !queued.contains(leak),
            "the tool answer carries the {leak}: {queued}"
        );
    }
    assert!(
        queued.contains("correlation_id") && queued.contains("recipient"),
        "the tool answer names the request and the peer"
    );

    // The outbox asks this owner's own gate before it writes anything.
    let outbox = production(OUTBOX);
    let enqueue = function(&outbox, "enqueue");
    let gated_at = enqueue
        .find(".admit_outbound(")
        .expect("enqueue asks the own policy gate");
    let saved_at = enqueue.find(".save(").expect("enqueue persists the entry");
    assert!(gated_at < saved_at, "the gate comes before the write");
    let gate = production(GATE);
    let admit = function(&gate, "admit_outbound");
    for check in [
        "known_peer(",
        "PeerState::Paired",
        "Access::Deny",
        "default_access(",
    ] {
        assert!(admit.contains(check), "admit_outbound lacks {check}");
    }
    assert!(
        !admit.contains(".record(") && !admit.contains("record_answering("),
        "an outgoing request is recorded when it is answered, not when it is queued"
    );

    // Registered for the chat only: build_tools carries them behind the
    // federation side the chat route passes, the routines never do, and
    // the routine surfaces are unchanged.
    let tools_mod = production(TOOLS_MOD);
    assert!(tools_mod.contains("pub mod federation;"));
    let build = function(&tools_mod, "build_tools");
    assert!(
        build.contains("federation::federation_tools("),
        "build_tools registers the sending tools"
    );
    assert!(
        build.contains("peer_sending: Option<federation::PeerSending>"),
        "the sending tools need the federation side the chat route passes"
    );
    let routine = read(ROUTINE);
    for forbidden in ["federation", "peer_sending", "PeerSending", "outbox"] {
        assert!(
            !routine.to_lowercase().contains(&forbidden.to_lowercase()),
            "{ROUTINE} carries {forbidden}"
        );
    }
    let check_in = routine
        .split("Self::CheckIn => &[")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .expect("the check-in tool names are pinned");
    assert_eq!(
        check_in
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>(),
        [
            "\"reach_out\"",
            "\"create_drop\"",
            "\"memory_write\"",
            "\"memory_read\"",
            "\"memory_list\"",
            "\"memory_search\"",
            "\"read_email\"",
        ]
    );
    let chat = production("server/src/services/chat.rs");
    assert!(chat.contains("peer_sending: Option<tools::federation::PeerSending>"));
    let route = production("server/src/routes/chat.rs");
    assert!(
        route.contains("outbox: state.federation_outbox.clone()"),
        "the chat route hands the outbox to the turn"
    );
}

#[test]
fn the_outbox_is_started_by_main_listed_to_the_owner_and_shown_on_the_activity_page() {
    let state = production("server/src/app/state.rs");
    assert!(
        state.contains("pub federation_outbox: Arc<crate::services::federation::outbox::Outbox>")
    );
    let main = production("server/src/main.rs");
    assert!(
        main.contains(".federation_outbox\n        .start(")
            || main.contains("federation_outbox.start("),
        "main starts the sender loop"
    );
    assert!(
        main.contains("outbox.shutdown().await"),
        "main stops it on shutdown"
    );
    let routes = production(ROUTES);
    assert!(
        routes.contains(".route(\"/api/federation/outbox\", get(list_outbox))"),
        "the listing is an owner GET"
    );
    let owner_router = routes
        .split("pub fn router()")
        .nth(1)
        .and_then(|rest| rest.split("pub fn public_router()").next())
        .expect("the owner router");
    assert!(owner_router.contains("/api/federation/outbox"));
    let public_router = routes
        .split("pub fn public_router()")
        .nth(1)
        .and_then(|rest| rest.split("\n}\n").next())
        .expect("the public router");
    assert!(
        !public_router.contains("outbox"),
        "nothing about the outbox is public"
    );
    let events = production("server/src/domain/events.rs");
    assert!(events.contains("OutboxUpdated {"));
    let outbox = production(OUTBOX);
    assert!(
        outbox.contains("ServerEvent::OutboxUpdated {"),
        "every entry change is broadcast"
    );

    // Client: the event and the listing are typed, the labels are pure
    // helpers with node tests, and the activity page shows the section.
    let types = read("client/src/lib/api/types.ts");
    for required in [
        "type: \"outbox_updated\"",
        "export interface FederationOutboxEntry",
        "export interface FederationOutbox ",
    ] {
        assert!(types.contains(required), "types.ts is missing {required}");
    }
    let client = read("client/src/lib/api/client.ts");
    assert!(client.contains("export function fetchFederationOutbox("));
    let receipts = read("client/src/lib/activity/receipts.js");
    for required in [
        "export function outboxLabel(",
        "export function outboxStatusLabel(",
        "export function outboxNote(",
        "export function upsertOutboxEntry(",
    ] {
        assert!(receipts.contains(required), "receipts.js lacks {required}");
    }
    let tests = read("client/tests/activity-receipts.test.mjs");
    assert!(
        tests.contains("outboxStatusLabel"),
        "the labels need node tests"
    );
    let section = read("client/src/lib/components/activity/PeerRequests.svelte");
    assert!(section.contains("fetchFederationOutbox(") && section.contains("outbox_updated"));
    for forbidden in ["{@html", "localStorage", "sessionStorage", "console."] {
        assert!(
            !section.contains(forbidden),
            "PeerRequests.svelte uses {forbidden}"
        );
    }
    let activity = read("client/src/lib/components/activity/ActivityView.svelte");
    assert!(activity.contains("<PeerRequests"));

    // Docs.
    let federation = read("docs/federation.md");
    for required in [
        "outbox",
        "backoff",
        "GET /api/federation/outbox",
        "send_peer_message",
        "ask_peer_availability",
        "propose_peer_reminder",
        "Activity",
    ] {
        assert!(
            federation.contains(required),
            "docs/federation.md lacks {required:?}"
        );
    }
    let storage = read("docs/companion-storage.md");
    for required in ["outbox.json", "attempts", "next_attempt_at"] {
        assert!(
            storage.contains(required),
            "docs/companion-storage.md lacks {required:?}"
        );
    }
    let design = read("docs/design-system.md");
    assert!(
        design.contains("PeerRequests.svelte"),
        "docs/design-system.md describes the section"
    );
}
