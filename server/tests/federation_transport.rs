//! Guards for #108 (PR 3): the transport routes are public POSTs verified by
//! signature and nothing else, the verifier is bounded and consumes a nonce
//! only after everything else passed, the transport modules never log, the
//! wire fixtures came from the second encoder, and the storage doc describes
//! the transport and rotation.

#[path = "../test-support/source_scan.rs"]
mod source_scan;

use std::{
    fs,
    path::{Path, PathBuf},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
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

/// The text of `fn <name>` up to the next top-level `fn`.
fn function_body(source: &str, name: &str) -> String {
    let start = source
        .find(&format!("fn {name}("))
        .unwrap_or_else(|| panic!("fn {name} is missing"));
    let rest = &source[start + 3..];
    let end = rest
        .find("\n    pub fn ")
        .or_else(|| rest.find("\n    fn "))
        .or_else(|| rest.find("\nfn "))
        .or_else(|| rest.find("\npub fn "))
        .unwrap_or(rest.len());
    rest[..end].to_owned()
}

#[test]
fn transport_routes_are_public_posts_verified_by_signature_only() {
    let routes = production("server/src/routes/federation.rs");
    let pairing = production("server/src/services/federation/pairing.rs");
    for (constant, path) in [
        ("PING_PATH", "/federation/v1/ping"),
        ("ROTATE_PATH", "/federation/v1/rotate"),
    ] {
        assert!(
            pairing.contains(&format!("pub const {constant}: &str = \"{path}\";")),
            "{constant} is not {path}"
        );
        assert!(
            routes.contains(&format!(".route({constant}, post(")),
            "peer-side route {constant} is not mounted as a POST"
        );
    }
    // The public router carries no layer of its own: nothing between the
    // wire and the signature check.
    let public_router = routes
        .split("pub fn public_router")
        .nth(1)
        .and_then(|rest| rest.split("\n}\n").next())
        .expect("public_router exists");
    for forbidden in [".layer(", "route_layer", "middleware", "auth", "Extension"] {
        assert!(
            !public_router.contains(forbidden),
            "public_router attaches {forbidden:?}"
        );
    }
    // And `build_router` merges it bare, outside the API auth layer.
    let router = production("server/src/app/router.rs");
    let build_router = router
        .split("fn build_router")
        .nth(1)
        .expect("build_router exists");
    let merged = build_router
        .lines()
        .find(|line| line.contains("routes::federation::public_router()"))
        .expect("public_router is mounted in build_router");
    assert!(
        !merged.contains("layer"),
        "public_router is mounted behind a layer: {merged}"
    );
    // The owner rotation route is in the authenticated router and takes no
    // path parameter that could name a deployment.
    assert!(routes.contains("\"/api/federation/rotate\""));
    assert!(
        routes
            .split("pub fn router()")
            .nth(1)
            .unwrap()
            .split("pub fn public_router")
            .next()
            .unwrap()
            .contains("/api/federation/rotate"),
        "the owner rotation route belongs to the owner router"
    );
}

#[test]
fn transport_verifier_is_bounded_strict_and_consumes_nonces_last() {
    let envelope = production("server/src/services/federation/envelope.rs");
    assert!(
        envelope.contains("pub const MAX_REPLAY_ENTRIES: usize = 65_536;"),
        "the replay set must stay bounded like the resource capability set"
    );
    for constant in [
        "pub const MAX_CLOCK_SKEW_SECS: u64",
        "pub const MAX_ENVELOPE_LIFETIME_SECS: u64",
        "pub const ENVELOPE_LIFETIME_SECS: u64",
    ] {
        assert!(envelope.contains(constant), "envelope.rs lacks {constant}");
    }
    assert!(
        envelope.contains(".verify_strict("),
        "the transport envelope must be verified strictly"
    );
    assert!(
        !envelope.contains("HashSet"),
        "nonces are kept with their retirement time, not as a bare set"
    );
    let rotation = production("server/src/services/federation/rotation.rs");
    assert!(rotation.contains("pub const ROTATION_GRACE_SECS: u64"));
    assert!(
        rotation.matches(".verify_strict(").count() >= 2,
        "both rotation signatures must be verified strictly"
    );
    // In `open`, the nonce is reserved after the signature and state checks.
    let pairing = production("server/src/services/federation/pairing.rs");
    let open = function_body(&pairing, "open");
    let verify_at = open
        .find("envelope::verify(")
        .expect("open verifies through envelope::verify");
    let state_at = open
        .find("PeerState::Revoked")
        .expect("open checks the peer state");
    let reserve_at = open.find(".reserve(").expect("open reserves the nonce");
    assert!(
        verify_at < reserve_at && state_at < reserve_at,
        "open must consume the nonce only after the signature and state checks"
    );
    // The transport and rotation modules never log at all, like the keystore.
    for relative in [
        "server/src/services/federation/envelope.rs",
        "server/src/services/federation/rotation.rs",
    ] {
        let source = production(relative);
        for name in LOG_MACROS {
            assert!(
                macro_arguments(&source, name).is_empty(),
                "{relative} logs via {name}!"
            );
        }
        for print in ["println", "eprintln", "print", "eprint", "dbg"] {
            assert!(
                macro_arguments(&source, print).is_empty(),
                "{relative} prints via {print}!"
            );
        }
    }
}

#[test]
fn transport_fixtures_have_the_v1_shape_and_come_from_the_second_encoder() {
    let dir = repo().join("server/tests/fixtures/federation");
    let read = |name: &str| -> serde_json::Value {
        serde_json::from_str(&fs::read_to_string(dir.join(name)).unwrap()).unwrap()
    };
    let identity = read("identity_v1.json");
    let peer = read("peer_identity_v1.json");
    let rotated = read("rotated_identity_v1.json");
    let transport = read("transport_v1.json");
    let rotation = read("rotation_v1.json");

    fn keys(value: &serde_json::Value) -> Vec<&str> {
        value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect()
    }
    let decode = |value: &serde_json::Value| -> Vec<u8> {
        let text = value.as_str().unwrap();
        assert!(!text.contains('='), "no padding in {text}");
        URL_SAFE_NO_PAD.decode(text).unwrap()
    };

    let mut transport_keys = [
        "version",
        "sender",
        "recipient",
        "nonce",
        "issued_at",
        "expires_at",
        "body_hash",
        "body",
        "signature",
    ];
    transport_keys.sort_unstable();
    assert_eq!(keys(&transport), transport_keys);
    assert_eq!(transport["version"], 1);
    assert_eq!(transport["sender"], identity["companion_id"]);
    assert_eq!(transport["recipient"], peer["companion_id"]);
    assert_ne!(transport["sender"], transport["recipient"]);
    assert_eq!(decode(&transport["nonce"]).len(), 16);
    assert_eq!(decode(&transport["signature"]).len(), 64);
    let body = decode(&transport["body"]);
    assert_eq!(body, br#"{"kind":"ping","version":1}"#);
    assert_eq!(
        decode(&transport["body_hash"]),
        Sha256::digest(&body).as_slice(),
        "body_hash is the SHA-256 of the body"
    );
    let issued = transport["issued_at"].as_u64().unwrap();
    let expires = transport["expires_at"].as_u64().unwrap();
    assert!(issued > 0 && expires > issued && expires - issued <= 300);

    let mut rotation_keys = [
        "version",
        "previous",
        "identity",
        "rotated_at",
        "endorsement",
        "signature",
    ];
    rotation_keys.sort_unstable();
    assert_eq!(keys(&rotation), rotation_keys);
    assert_eq!(rotation["version"], 1);
    assert_eq!(rotation["previous"], identity);
    assert_eq!(rotation["identity"], rotated);
    assert_ne!(rotated["public_key"], identity["public_key"]);
    assert_ne!(rotated["companion_id"], identity["companion_id"]);
    assert_eq!(decode(&rotation["endorsement"]).len(), 64);
    assert_eq!(decode(&rotation["signature"]).len(), 64);
    assert_ne!(rotation["endorsement"], rotation["signature"]);
    assert!(rotation["rotated_at"].as_u64().unwrap() > identity["created_at"].as_u64().unwrap());
    for document in [&peer, &rotated] {
        assert_eq!(document["version"], 1);
        assert_eq!(decode(&document["public_key"]).len(), 32);
        assert_eq!(decode(&document["signature"]).len(), 64);
    }

    // The generator is the second implementation of the canonical
    // encoding, so it must name the same domain tags as the Rust module.
    let generator = fs::read_to_string(dir.join("generate.py")).unwrap();
    let module = fs::read_to_string(repo().join("server/src/services/federation/mod.rs")).unwrap();
    for domain in [
        "nolune/federation/transport/v1",
        "nolune/federation/rotation/v1",
    ] {
        assert!(generator.contains(domain), "generate.py lacks {domain}");
        assert!(module.contains(domain), "mod.rs lacks {domain}");
    }
    for name in [
        "peer_identity_v1.json",
        "rotated_identity_v1.json",
        "transport_v1.json",
        "rotation_v1.json",
    ] {
        assert!(
            generator.contains(name),
            "generate.py does not write {name}"
        );
    }
}

#[test]
fn storage_doc_describes_the_transport_and_rotation() {
    let doc = fs::read_to_string(repo().join("docs/companion-storage.md")).unwrap();
    for required in [
        "federation/rotations.json",
        "/federation/v1/ping",
        "/federation/v1/rotate",
        "/api/federation/rotate",
        "nonce",
        "expires_at",
        "rotation_history",
        "skew",
        "replay",
    ] {
        assert!(
            doc.contains(required),
            "storage doc is missing {required:?}"
        );
    }
}
