//! Guards for #108 (PR 2): pairing secrets never reach a log line, a URL, or
//! a query string; the peer-side routes verify signatures and nothing else;
//! and the peer store is documented beside the keystore.

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

/// Every production file that handles pairing: the federation services and
/// the federation routes.
fn pairing_sources() -> Vec<(String, String)> {
    let mut files = vec!["server/src/routes/federation.rs".to_owned()];
    let dir = repo().join("server/src/services/federation");
    let mut services: Vec<String> = fs::read_dir(&dir)
        .expect("server/src/services/federation exists")
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .map(|path| {
            path.strip_prefix(repo())
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    services.sort();
    files.extend(services);
    assert!(
        files.iter().any(|f| f.ends_with("peers.rs"))
            && files.iter().any(|f| f.ends_with("pairing.rs")),
        "peer store and pairing modules are missing: {files:?}"
    );
    files
        .into_iter()
        .map(|relative| {
            let source = production(&relative);
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

const PRINT_MACROS: [&str; 5] = ["println", "eprintln", "print", "eprint", "dbg"];

#[test]
fn pairing_code_never_logs_or_prints_a_secret() {
    // Identifiers that hold the invite secret, a body that contains it, or a
    // parsed request that contains it, in every module that touches pairing.
    let secret_tokens = [
        "secret",
        "expose(",
        "InviteSecret",
        "AcceptInvite",
        "PairRequest",
        "IssuedInvite",
        "MintedInvite",
        "minted",
        "{accept",
        "&accept",
        "{body",
        "&body",
        "{bytes",
        "&bytes",
        "{envelope",
        "&envelope",
        "{request",
        "&request",
        "{message",
        "&message",
        "{text",
        "&text",
        "{json",
        "&json",
        "{parsed",
        "&parsed",
    ];
    let mut violations = Vec::new();
    for (path, source) in pairing_sources() {
        for print in PRINT_MACROS {
            if !macro_arguments(&source, print).is_empty() {
                violations.push(format!("{path} calls {print}!"));
            }
        }
        for name in LOG_MACROS {
            for arguments in macro_arguments(&source, name) {
                for token in secret_tokens {
                    if arguments.to_lowercase().contains(&token.to_lowercase()) {
                        violations
                            .push(format!("{path} formats {token:?} in {name}!({arguments})"));
                    }
                }
            }
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn federation_routes_take_secrets_from_signed_bodies_only() {
    let routes = production("server/src/routes/federation.rs");
    // Nothing is ever read from the query string or the raw URI.
    for forbidden in [
        "Query<",
        "RawQuery",
        "extract::Query",
        ".query()",
        "uri()",
        "query_pairs",
        "?secret",
        "?code",
        "?token",
    ] {
        assert!(
            !routes.contains(forbidden),
            "routes/federation.rs reads the URL via {forbidden:?}"
        );
    }
    // Path parameters name public things only.
    let mut params = Vec::new();
    for (at, _) in routes.match_indices('{') {
        let rest = &routes[at + 1..];
        if let Some(end) = rest.find('}') {
            let name = &rest[..end];
            if name.chars().all(|c| c.is_ascii_lowercase() || c == '_')
                && !name.is_empty()
                && routes[..at].ends_with('/')
            {
                params.push(name.to_owned());
            }
        }
    }
    params.sort();
    params.dedup();
    assert_eq!(
        params,
        ["companion_id", "id"],
        "federation routes may only take a companion id or an invite id in the path"
    );
    // Every peer-side path lives under /federation/v1/pair, is named by the
    // same constant the transport posts to, and is a POST.
    let pairing = production("server/src/services/federation/pairing.rs");
    for (constant, path) in [
        ("PAIR_PATH", "/federation/v1/pair"),
        ("CONFIRM_PATH", "/federation/v1/pair/confirm"),
        ("REVOKE_PATH", "/federation/v1/pair/revoke"),
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
    assert_eq!(
        routes.matches("/federation/v1/").count(),
        routes
            .lines()
            .filter(|line| line.trim_start().starts_with("//") && line.contains("/federation/v1/"))
            .count(),
        "peer-side paths appear in routes/federation.rs only through the constants"
    );
}

#[test]
fn peer_side_routes_verify_signatures_and_nothing_about_the_deployment() {
    // No owner credential, no request address, no host, and no profile can
    // ever admit a peer: none of these names may appear in pairing code.
    let shortcuts = [
        "auth_middleware",
        "AuthContext",
        "COOKIE_NAME",
        "cookie",
        "Bearer",
        "auth_token",
        "browser_sessions",
        "ConnectInfo",
        "remote_addr",
        "peer_addr",
        "is_loopback",
        "127.0.0.1",
        "\"localhost\"",
        "x-forwarded",
        "X-Forwarded",
        "header::HOST",
        "\"host\"",
        "request_host",
        "same_origin",
        "\"profile\"",
        "workspace_root()",
        "NOLUNE_HOME",
        "companion_dir(",
        "\"instances",
    ];
    let mut violations = Vec::new();
    for (path, source) in pairing_sources() {
        for shortcut in shortcuts {
            if source.contains(shortcut) {
                violations.push(format!("{path} mentions {shortcut:?}"));
            }
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));

    // The router mounts the peer-side router outside the API auth layer.
    let router = production("server/src/app/router.rs");
    let api_router = router
        .split("fn api_router")
        .nth(1)
        .and_then(|rest| rest.split("fn build_router").next())
        .expect("api_router precedes build_router");
    assert!(
        api_router.contains("routes::federation::router()"),
        "owner routes must be part of the authenticated API router"
    );
    assert!(
        !api_router.contains("routes::federation::public_router()"),
        "peer routes must not sit behind the API auth middleware"
    );
    let build_router = router
        .split("fn build_router")
        .nth(1)
        .expect("build_router exists");
    assert!(
        build_router.contains("routes::federation::public_router()"),
        "peer routes must be mounted publicly"
    );

    // Verification stays strict (the PR 1 guard counts the calls; this pins
    // that the peer store only trusts documents that verified).
    let peers = production("server/src/services/federation/peers.rs");
    assert!(
        peers.contains("verify_document("),
        "peers.rs must re-verify stored documents"
    );
}

#[test]
fn peer_store_lives_beside_the_keystore_and_is_documented() {
    let peers = production("server/src/services/federation/peers.rs");
    assert!(
        peers.contains(r#"pub const PEERS_FILE: &str = "peers.json";"#),
        "the peer store file name moved"
    );
    assert!(
        peers.contains("federation_dir(") || peers.contains("FEDERATION_DIR"),
        "peers.json must be placed through the keystore's directory helper"
    );
    let doc = fs::read_to_string(repo().join("docs/companion-storage.md")).unwrap();
    for required in [
        "federation/peers.json",
        "/api/federation/",
        "/federation/v1/pair",
        "pending",
        "paired",
        "revoked",
        "approved_origins",
        "one-time",
    ] {
        assert!(
            doc.contains(required),
            "storage doc is missing {required:?}"
        );
    }
}
