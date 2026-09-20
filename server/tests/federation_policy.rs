//! Guards for #109: the policy engine is pure, the audit log never sees a
//! payload, peer text has no way out but the untrusted block and never
//! reaches a tool, every peer-side transport route goes through the gate,
//! and the storage doc describes the policy and the receipts.

#[path = "../test-support/source_scan.rs"]
mod source_scan;

use std::{
    fs,
    path::{Path, PathBuf},
};

use source_scan::without_cfg_test_items;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn production(relative: &str) -> String {
    let path = repo().join(relative);
    let source = fs::read_to_string(&path).unwrap_or_else(|_| panic!("{relative} is missing"));
    without_cfg_test_items(&source)
}

/// Every `.rs` file under `dir`, recursively, as (relative path, production source).
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
    let mut paths = Vec::new();
    visit(&repo().join(relative), &mut paths);
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let relative = path
                .strip_prefix(repo())
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let source = without_cfg_test_items(&fs::read_to_string(&path).unwrap());
            (relative, source)
        })
        .collect()
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

const LOG_MACROS: [&str; 8] = [
    "error", "warn", "info", "debug", "trace", "log", "event", "span",
];

#[test]
fn the_policy_engine_is_pure_and_fails_closed_on_unknown_names() {
    let engine = production("server/src/services/federation/policy.rs");
    for io in [
        "std::fs",
        "tokio::fs",
        "File::",
        "OpenOptions",
        "reqwest",
        "std::net",
        "tokio::net",
        "SystemTime",
        "Instant::",
        "chrono",
        "getrandom",
        "rand::",
        "std::env",
        "Mutex",
        "RwLock",
        "async fn",
    ] {
        assert!(
            !engine.contains(io),
            "policy.rs must stay pure; it mentions {io:?}"
        );
    }
    for name in LOG_MACROS {
        assert!(
            macro_arguments(&engine, name).is_empty(),
            "policy.rs logs via {name}!"
        );
    }
    for print in ["println", "eprintln", "print", "eprint", "dbg"] {
        assert!(
            macro_arguments(&engine, print).is_empty(),
            "policy.rs prints via {print}!"
        );
    }
    // Wire names are resolved by exact match against the closed enums, and
    // anything else is an unknown-name denial: no catch-all class exists.
    let domain = production("server/src/domain/federation_policy.rs");
    for open_ended in [
        "Other(",
        "Unknown(",
        "Custom(",
        "untagged",
        "serde_json::Value",
    ] {
        assert!(
            !domain.contains(open_ended),
            "the intent and disclosure classes must stay closed; found {open_ended:?}"
        );
    }
    for class in ["IntentClass", "DisclosureClass", "Access", "Verdict"] {
        let definition = domain
            .split(&format!("pub enum {class} "))
            .nth(1)
            .unwrap_or_else(|| panic!("{class} is defined"));
        let attributes = domain.split(&format!("pub enum {class} ")).next().unwrap();
        assert!(
            attributes
                .trim_end()
                .ends_with("#[serde(rename_all = \"snake_case\")]"),
            "{class} is not snake_case-tagged"
        );
        assert!(
            !definition.split("\n}\n").next().unwrap().contains("String"),
            "{class} carries no free-form text"
        );
    }
    assert!(engine.contains("DecisionReason::UnknownIntent"));
    assert!(engine.contains("DecisionReason::UnknownDisclosure"));
    assert!(engine.contains("DecisionReason::UnsupportedDisclosure"));
    // The engine checks the peer's state before anything else and the
    // rate limit before the rules.
    let evaluate = engine
        .split("pub fn evaluate(")
        .nth(1)
        .expect("evaluate exists");
    let state_at = evaluate
        .find("PeerState::Revoked")
        .expect("evaluate checks the peer state");
    let limit_at = evaluate
        .find(".admit(")
        .expect("evaluate applies the rate limit");
    let rules_at = evaluate.find(".rules").expect("evaluate reads the rules");
    assert!(
        state_at < limit_at && limit_at < rules_at,
        "evaluate must check state, then the rate limit, then the rules"
    );
}

#[test]
fn the_audit_log_never_touches_a_payload_and_receipts_have_no_room_for_one() {
    let audit = production("server/src/services/federation/audit.rs");
    for payload in [
        "PeerText",
        "render_untrusted",
        "TransportEnvelope",
        "TransportMessage",
        "SignedEnvelope",
        ".body",
        "body:",
        "body,",
        "text:",
        ".text",
        "payload:",
        "excerpt",
    ] {
        assert!(
            !audit.contains(payload),
            "audit.rs must never handle a payload; it mentions {payload:?}"
        );
    }
    for name in LOG_MACROS {
        for arguments in macro_arguments(&audit, name) {
            for token in ["receipt", "{line", "&line", "{text", "&text", "summary"] {
                assert!(
                    !arguments.to_lowercase().contains(token),
                    "audit.rs formats {token:?} in {name}!({arguments})"
                );
            }
        }
    }
    // The receipt shape is closed: every field is a name, a decision, a
    // time, or the one-line summary built from them.
    let domain = production("server/src/domain/federation_policy.rs");
    let receipt = domain
        .split("pub struct AuditReceipt {")
        .nth(1)
        .and_then(|rest| rest.split("\n}\n").next())
        .expect("AuditReceipt is defined");
    let fields: Vec<&str> = receipt
        .lines()
        .filter_map(|line| line.trim().strip_prefix("pub "))
        .map(|line| line.split(':').next().unwrap())
        .collect();
    assert_eq!(
        fields,
        [
            "version",
            "id",
            "side",
            "requester",
            "responder",
            "intent",
            "disclosure",
            "detail",
            "decision",
            "at",
            "summary",
        ]
    );
    assert!(
        domain
            .split("pub struct AuditReceipt {")
            .next()
            .unwrap()
            .trim_end()
            .ends_with("#[serde(deny_unknown_fields)]"),
        "a receipt refuses fields it does not know"
    );
    assert!(
        audit.contains("pub const MAX_AUDIT_RECEIPTS: usize")
            && audit.contains("pub const MAX_RECEIPTS_PER_PEER: usize")
            && audit.contains("pub const AUDIT_RETENTION_DAYS: u64"),
        "retention must stay bounded"
    );
}

#[test]
fn peer_text_has_no_way_out_but_the_untrusted_block_and_never_reaches_a_tool() {
    let domain = production("server/src/domain/federation_policy.rs");
    let peer_text = domain
        .split("pub struct PeerText(")
        .nth(1)
        .expect("PeerText is defined");
    for escape in [
        "impl fmt::Display for PeerText {",
        "impl std::fmt::Display for PeerText {",
        "impl Deref for PeerText {",
        "impl std::ops::Deref for PeerText {",
        "impl AsRef<str> for PeerText {",
        "impl From<PeerText> for String",
        "impl Borrow<str> for PeerText {",
        "fn as_str(",
        "fn into_inner(",
        "fn into_string(",
        "fn expose(",
        "fn raw(",
        "fn text(",
        "pub fn get(",
        "pub struct PeerText(pub ",
    ] {
        assert!(
            !domain.contains(escape),
            "PeerText must not be readable as a string; found {escape:?}"
        );
    }
    assert!(
        peer_text.contains("pub fn render_untrusted_block("),
        "the untrusted block is the one way out"
    );
    assert!(
        domain.contains("impl fmt::Debug for PeerText"),
        "Debug must be hand-written so the text never formats"
    );
    // Nothing that builds a tool call or a model prompt may take peer text
    // other than through the block, and no tool takes it at all.
    for (path, source) in sources_under("server/src/services/tools") {
        for token in ["PeerText", "render_untrusted_block", "federation_policy"] {
            assert!(
                !source.contains(token),
                "{path} mentions {token:?}: peer text must never become a tool argument"
            );
        }
    }
    for (path, source) in sources_under("server/src/services/llm") {
        if source.contains("PeerText") {
            assert!(
                source.contains("render_untrusted_block("),
                "{path} handles PeerText without rendering it as an untrusted block"
            );
        }
    }
    for (path, source) in sources_under("server/src") {
        if path.starts_with("server/src/domain/federation_policy.rs") {
            continue;
        }
        assert!(
            !source.contains("PeerText("),
            "{path} constructs or destructures PeerText directly"
        );
    }
}

#[test]
fn every_peer_side_transport_route_goes_through_the_gate() {
    let routes = production("server/src/routes/federation.rs");
    for handler in ["async fn ping(", "async fn rotation_notice("] {
        let body = routes
            .split(handler)
            .nth(1)
            .and_then(|rest| rest.split("\n}\n").next())
            .unwrap_or_else(|| panic!("{handler} exists"));
        assert!(
            body.contains("federation_gate"),
            "{handler} must dispatch through the policy gate"
        );
        assert!(
            !body.contains("federation.receive_ping(")
                && !body.contains("federation.receive_rotation("),
            "{handler} bypasses the gate"
        );
    }
    // The gate judges a ping only after `open` verified it, and answers
    // only after the receipt was written.
    let gate = production("server/src/services/federation/gate.rs");
    let receive_ping = gate
        .split("pub fn receive_ping(")
        .nth(1)
        .and_then(|rest| rest.split("\n    pub fn ").next())
        .expect("receive_ping exists");
    let open_at = receive_ping
        .find(".open(")
        .expect("receive_ping opens the envelope");
    let admit_at = receive_ping
        .find("self.admit(")
        .expect("receive_ping judges the ping");
    let seal_at = receive_ping
        .find(".seal(")
        .expect("receive_ping seals the pong");
    assert!(
        open_at < admit_at && admit_at < seal_at,
        "receive_ping must open, then judge, then answer"
    );
    let admit = gate
        .split("pub fn admit(")
        .nth(1)
        .and_then(|rest| rest.split("\n    pub fn ").next())
        .expect("admit exists");
    let evaluate_at = admit
        .find("policy::evaluate(")
        .expect("admit runs the engine");
    let record_at = admit
        .find("self.record_answering(")
        .expect("admit records the receipt");
    assert!(
        evaluate_at < record_at,
        "admit must evaluate before it records"
    );
    assert!(
        admit.contains("PolicyRefused"),
        "admit must refuse anything that is not allowed"
    );
    // The owner routes are read-only and live in the owner router.
    let owner = routes
        .split("pub fn router()")
        .nth(1)
        .unwrap()
        .split("pub fn public_router")
        .next()
        .unwrap();
    assert!(owner.contains("\"/api/federation/policy\", get("));
    assert!(owner.contains("\"/api/federation/receipts\", get("));
    for verb in ["post(show_policy", "post(list_receipts", "put(", "patch("] {
        assert!(!owner.contains(verb), "the policy routes are list-only");
    }
    // Refusals are typed and carry a retry-after when they say so.
    for code in [
        "\"policy_denied\"",
        "\"approval_required\"",
        "\"deferred\"",
        "\"rate_limited\"",
        "header::RETRY_AFTER",
        "StatusCode::TOO_MANY_REQUESTS",
    ] {
        assert!(routes.contains(code), "routes/federation.rs lacks {code}");
    }
}

#[test]
fn storage_doc_describes_the_policy_and_the_receipts() {
    let doc = fs::read_to_string(repo().join("docs/companion-storage.md")).unwrap();
    for required in [
        "federation/policy.json",
        "federation/audit.jsonl",
        "/api/federation/policy",
        "/api/federation/receipts",
        "quiet hours",
        "rate limit",
        "untrusted",
        "allow",
        "ask",
        "deny",
        "expires_at",
        "never contain",
    ] {
        assert!(
            doc.contains(required),
            "storage doc is missing {required:?}"
        );
    }
}
