//! Guards for #110 (PR 1): the intent shapes are closed and versioned,
//! decoding them is pure (nothing it does can have a side effect), the
//! receipt has no room for a payload, nothing formats peer text, and the
//! fixtures on disk are the four intents and the three responses.

#[path = "../test-support/source_scan.rs"]
mod source_scan;

use std::{
    fs,
    path::{Path, PathBuf},
};

use source_scan::without_cfg_test_items;

const MODULE: &str = "server/src/domain/federation_intent.rs";
const FIXTURES: &str = "server/tests/fixtures/federation/intents";

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

/// Every `pub struct` and `pub enum` in `source` with the attribute lines
/// directly above it: (kind, name, attributes).
fn items_with_attributes(source: &str) -> Vec<(&'static str, String, String)> {
    let mut out = Vec::new();
    let mut attributes = String::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#[") {
            attributes.push_str(trimmed);
            attributes.push('\n');
            continue;
        }
        let item = trimmed
            .strip_prefix("pub struct ")
            .map(|rest| ("struct", rest))
            .or_else(|| trimmed.strip_prefix("pub enum ").map(|rest| ("enum", rest)));
        if let Some((kind, rest)) = item {
            let name = rest
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .next()
                .unwrap()
                .to_owned();
            out.push((kind, name, std::mem::take(&mut attributes)));
        } else if !trimmed.starts_with("///") {
            attributes.clear();
        }
    }
    out
}

#[test]
fn decoding_an_intent_is_pure_and_cannot_have_a_side_effect() {
    let module = production(MODULE);
    for io in [
        "std::fs",
        "tokio",
        "File::",
        "OpenOptions",
        "reqwest",
        "std::net",
        "SystemTime",
        "Instant::",
        "chrono",
        "getrandom",
        "rand::",
        "std::env",
        "Mutex",
        "RwLock",
        "async fn",
        "std::process",
        "AppState",
        "crate::services",
    ] {
        assert!(
            !module.contains(io),
            "{MODULE} must stay pure; it mentions {io:?}"
        );
    }
    for name in [
        "error", "warn", "info", "debug", "trace", "log", "event", "span", "println", "eprintln",
        "print", "eprint", "dbg",
    ] {
        assert!(
            macro_arguments(&module, name).is_empty(),
            "{MODULE} logs or prints via {name}!"
        );
    }
    // The clock is an argument, never read here.
    assert!(module.contains("pub fn decode(bytes: &[u8], now: u64)"));
}

#[test]
fn every_wire_shape_is_closed_and_every_name_comes_from_a_closed_enum() {
    let module = production(MODULE);
    for open_ended in [
        "serde_json::Value",
        "untagged",
        "flatten",
        "Other(",
        "Unknown(",
        "Custom(",
        "HashMap<String",
        "BTreeMap<String",
    ] {
        assert!(
            !module.contains(open_ended),
            "the intent shapes must stay closed; {MODULE} mentions {open_ended:?}"
        );
    }
    let items = items_with_attributes(&module);
    assert!(items.len() >= 12, "found {} items", items.len());
    let mut strict = 0;
    for (kind, name, attributes) in &items {
        if name == "PeerLabel" {
            assert!(
                !attributes.contains("Deserialize"),
                "PeerLabel validates on deserialize by hand"
            );
            continue;
        }
        if !attributes.contains("Deserialize") {
            continue;
        }
        let tagged = attributes.contains("tag = \"");
        if *kind == "struct" || tagged {
            assert!(
                attributes.contains("deny_unknown_fields"),
                "{name} does not refuse unknown fields:\n{attributes}"
            );
            strict += 1;
        }
        if *kind == "enum" {
            assert!(
                attributes.contains("rename_all = \"snake_case\""),
                "{name} is not snake_case-tagged"
            );
            let variants = module
                .split(&format!("pub enum {name} {{"))
                .nth(1)
                .and_then(|rest| rest.split("\n}\n").next())
                .unwrap();
            if !tagged {
                assert!(
                    !variants.contains('{') && !variants.contains('('),
                    "{name} is an untagged enum with data"
                );
            }
        }
    }
    assert!(strict >= 8, "only {strict} strict shapes found");
    // The four intent tags are the policy engine's class names; a ping is
    // not an intent.
    let class_for_tag = module
        .split("pub fn class_for_tag(")
        .nth(1)
        .and_then(|rest| rest.split("\n    }\n").next())
        .expect("class_for_tag exists");
    assert!(
        class_for_tag.contains("IntentClass::parse(")
            && class_for_tag.contains("IntentClass::Ping"),
        "class_for_tag must resolve through IntentClass and refuse a ping"
    );
    // Versions are bounded both ways.
    for required in [
        "pub const INTENT_VERSION: u32",
        "pub const MIN_INTENT_VERSION: u32",
        "pub const MAX_INTENT_BYTES: usize",
        "pub const MAX_INTENT_LIFETIME_SECS: u64",
        "pub const MAX_CORRELATION_ID_LEN: usize",
        "pub const MAX_LABEL_CHARS: usize",
        "pub const MAX_AVAILABILITY_WINDOWS: usize",
        "VersionTooOld",
        "VersionUnsupported",
    ] {
        assert!(module.contains(required), "{MODULE} lacks {required}");
    }
}

#[test]
fn the_receipt_has_no_room_for_a_payload_and_nothing_formats_peer_text() {
    let module = production(MODULE);
    let receipt = module
        .split("pub struct IntentReceipt {")
        .nth(1)
        .and_then(|rest| rest.split("\n}\n").next())
        .expect("IntentReceipt is defined");
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
            "pairing_id",
            "correlation_id",
            "requester",
            "represented_owner",
            "responder",
            "intent",
            "purpose",
            "requested",
            "granted",
            "outcome",
            "basis",
            "at",
            "summary",
        ]
    );
    assert!(
        !receipt.contains("PeerText") && !receipt.contains("IntentPayload"),
        "the receipt must not carry the payload"
    );
    assert!(
        module
            .split("pub struct IntentReceipt {")
            .next()
            .unwrap()
            .trim_end()
            .ends_with("#[serde(deny_unknown_fields)]"),
        "a receipt refuses fields it does not know"
    );
    // No Display impl reaches into a body, and PeerText is only ever a
    // field type here: it is never constructed, read, or rendered.
    for block in module.split("impl fmt::Display for ").skip(1) {
        let body = block.split("\n}\n").next().unwrap();
        for leak in [
            ".body",
            ".text",
            ".description",
            "PeerText",
            "render_untrusted",
        ] {
            assert!(
                !body.contains(leak),
                "a Display impl formats peer text via {leak:?}:\n{body}"
            );
        }
    }
    for leak in ["render_untrusted_block", "PeerText::"] {
        assert!(
            !module.contains(leak),
            "{MODULE} touches peer text via {leak:?}"
        );
    }
    // The summary is built from ids and classes; the labels are their own
    // fields.
    let summarize = module
        .split("pub fn summarize(&self)")
        .nth(1)
        .and_then(|rest| rest.split("\n    }\n").next())
        .expect("summarize exists");
    for label in ["represented_owner", "purpose"] {
        assert!(
            !summarize.contains(label),
            "summarize formats the peer's {label}"
        );
    }
}

#[test]
fn the_fixtures_are_the_four_intents_and_the_three_responses() {
    let dir = repo().join(FIXTURES);
    let mut names: Vec<String> = fs::read_dir(&dir)
        .unwrap_or_else(|_| panic!("{FIXTURES} is missing"))
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "availability_v1.json",
            "message_v1.json",
            "proposal_v1.json",
            "reminder_v1.json",
            "response_accepted_v1.json",
            "response_denied_v1.json",
            "response_needs_owner_v1.json",
        ]
    );
    let header = [
        "correlation_id",
        "disclosure",
        "expires_at",
        "intent",
        "issued_at",
        "purpose",
        "represented_owner",
        "sender",
        "version",
    ];
    for name in &names {
        let text = fs::read_to_string(dir.join(name)).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        let object = json.as_object().unwrap();
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(json["version"], 1, "{name}");
        if let Some(stem) = name
            .strip_suffix("_v1.json")
            .and_then(|s| s.strip_prefix("response_"))
        {
            assert_eq!(json["outcome"].as_str().unwrap(), stem, "{name}");
            assert!(keys.contains(&"correlation_id") && keys.contains(&"responder"));
            assert!(
                !keys.contains(&"deferred_until"),
                "a response never says when quiet hours end"
            );
        } else {
            assert_eq!(keys, header, "{name}");
            let stem = name.strip_suffix("_v1.json").unwrap();
            assert_eq!(json["intent"]["type"].as_str().unwrap(), stem, "{name}");
            assert!(
                json["expires_at"].as_u64() > json["issued_at"].as_u64(),
                "{name}"
            );
        }
    }
}

#[test]
fn the_federation_doc_describes_intents_and_their_receipts() {
    let doc = fs::read_to_string(repo().join("docs/federation.md")).unwrap();
    for required in [
        "## Intents",
        "correlation_id",
        "represented_owner",
        "purpose",
        "expires_at",
        "needs_owner",
        "accepted",
        "denied",
        "unknown",
        "receipt",
    ] {
        assert!(
            doc.contains(required),
            "docs/federation.md is missing {required:?}"
        );
    }
}
