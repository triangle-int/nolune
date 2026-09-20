//! Guards for #108 (PR 1): the federation keystore never logs or prints key
//! material, keeps the signing key outside the companion export root, ships
//! wire fixtures that a second encoder produced, and documents its files.

#[path = "../test-support/source_scan.rs"]
mod source_scan;

use std::{
    fs,
    path::{Path, PathBuf},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use source_scan::without_cfg_test_items;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Production source of every file under `server/src/services/federation/`.
fn federation_sources() -> Vec<(String, String)> {
    let dir = repo().join("server/src/services/federation");
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("server/src/services/federation exists")
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .collect();
    paths.sort();
    assert!(
        paths.iter().any(|p| p.ends_with("identity.rs")),
        "identity keystore module is missing"
    );
    paths
        .into_iter()
        .map(|path| {
            let relative = path
                .strip_prefix(repo())
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let production = without_cfg_test_items(&fs::read_to_string(&path).unwrap());
            (relative, production)
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

/// Logging macros of the `log` and `tracing` facades. Matching on the bare
/// name catches `warn!(`, `log::warn!(` and `tracing::warn!(` alike.
const LOG_MACROS: [&str; 8] = [
    "error", "warn", "info", "debug", "trace", "log", "event", "span",
];

/// Macros whose message reaches stderr, a panic hook, or a formatter.
const OUTPUT_MACROS: [&str; 7] = [
    "panic",
    "unreachable",
    "assert",
    "assert_eq",
    "assert_ne",
    "write",
    "writeln",
];

/// Ways to bring a logging facade into scope so its macros appear bare.
const FACADE_IMPORTS: [&str; 7] = [
    "use log::",
    "use log;",
    "use tracing::",
    "use tracing;",
    "extern crate log",
    "extern crate tracing",
    "#[macro_use]",
];

#[test]
fn federation_code_never_logs_or_prints_key_material() {
    // Every identifier that holds the seed or a buffer that contained it,
    // including the raw file buffers in the keystore, so logging a buffer is
    // caught even when the word "secret" never appears in the call.
    let secret_tokens = [
        "secret",
        "seed",
        "signing_key",
        "private_key",
        "SigningKey",
        "to_bytes",
        "to_scalar",
        "key_text",
        "key_bytes",
        "key_json",
        "raw",
        "decoded",
        "stored",
        "Zeroizing",
    ];
    let mut violations = Vec::new();
    for (path, production) in federation_sources() {
        for print in ["println", "eprintln", "print", "eprint", "dbg"] {
            if !macro_arguments(&production, print).is_empty() {
                violations.push(format!("{path} calls {print}!"));
            }
        }
        for import in FACADE_IMPORTS {
            if production.contains(import) {
                violations.push(format!(
                    "{path} brings a logging facade into scope via {import:?}"
                ));
            }
        }
        for name in LOG_MACROS {
            let invocations = macro_arguments(&production, name);
            // The keystore module never logs at all: every function in it
            // holds the seed or a buffer that held it.
            if path.ends_with("identity.rs") && !invocations.is_empty() {
                violations.push(format!(
                    "{path} logs via {name}! ({} call(s)); the keystore never logs",
                    invocations.len()
                ));
            }
            for arguments in invocations {
                for token in secret_tokens {
                    if arguments.to_lowercase().contains(&token.to_lowercase()) {
                        violations
                            .push(format!("{path} formats {token:?} in {name}!({arguments})"));
                    }
                }
            }
        }
        for name in OUTPUT_MACROS {
            for arguments in macro_arguments(&production, name) {
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
fn federation_code_only_verifies_signatures_strictly() {
    // Lax `Verifier::verify` accepts a universal forgery from a small-order
    // key and a small-order `R` from a genuine one; only `verify_strict`
    // refuses both. The unit tests prove it with forged signatures; this
    // keeps the lax entry points out of the production source altogether.
    let mut violations = Vec::new();
    let mut strict_calls = 0;
    for (path, production) in federation_sources() {
        for lax in [".verify(", "Verifier", "verify_prehashed", "DigestVerifier"] {
            if production.contains(lax) {
                violations.push(format!("{path} uses lax verification via {lax:?}"));
            }
        }
        strict_calls += production.matches(".verify_strict(").count();
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
    assert!(
        strict_calls >= 2,
        "expected strict verification of both documents and envelopes, found {strict_calls} call(s)"
    );
}

#[test]
fn signing_key_is_stored_outside_the_companion_export_root() {
    let identity = fs::read_to_string(repo().join("server/src/services/federation/identity.rs"))
        .expect("identity.rs exists");
    assert!(
        identity.contains(r#"pub const FEDERATION_DIR: &str = "federation";"#),
        "the keystore directory must be the workspace-root federation/ directory"
    );
    let mut violations = Vec::new();
    for (path, production) in federation_sources() {
        for token in [
            "\"instances",
            "companion_dir(",
            "CANONICAL_SLUG",
            "MediaStore",
        ] {
            if production.contains(token) {
                violations.push(format!(
                    "{path} reaches into the companion directory via {token:?}"
                ));
            }
        }
        for deployment in ["hostname", "profile", "port", "service_label", "launchctl"] {
            if production.contains(&format!("\"{deployment}\"")) {
                violations.push(format!(
                    "{path} stores deployment metadata {deployment:?} in identity state"
                ));
            }
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn wire_fixtures_have_the_v1_shape_and_share_a_signer() {
    let dir = repo().join("server/tests/fixtures/federation");
    let identity: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.join("identity_v1.json")).unwrap()).unwrap();
    let envelope: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.join("envelope_v1.json")).unwrap()).unwrap();

    // serde_json sorts object keys; the on-disk order is pinned by the
    // byte-for-byte unit test against the Rust struct.
    fn keys(value: &serde_json::Value) -> Vec<&str> {
        value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect()
    }
    let mut identity_keys = [
        "version",
        "companion_id",
        "public_key",
        "created_at",
        "signature",
    ];
    identity_keys.sort_unstable();
    assert_eq!(keys(&identity), identity_keys);
    let mut envelope_keys = ["version", "sender", "body", "signature"];
    envelope_keys.sort_unstable();
    assert_eq!(keys(&envelope), envelope_keys);
    assert_eq!(identity["version"], 1);
    assert_eq!(envelope["version"], 1);

    let decode = |value: &serde_json::Value| -> Vec<u8> {
        let text = value.as_str().unwrap();
        assert!(!text.contains('='), "no padding in {text}");
        URL_SAFE_NO_PAD.decode(text).unwrap()
    };
    assert_eq!(decode(&identity["public_key"]).len(), 32);
    assert_eq!(decode(&identity["signature"]).len(), 64);
    assert_eq!(decode(&identity["companion_id"]).len(), 32);
    assert_eq!(decode(&envelope["signature"]).len(), 64);
    assert_eq!(decode(&envelope["body"]), br#"{"kind":"ping"}"#);
    assert_eq!(envelope["sender"], identity["companion_id"]);
    assert!(identity["created_at"].as_u64().unwrap() > 0);

    // The generator is the second implementation of the canonical encoding,
    // so it must name the same domain tags as the Rust module.
    let generator = fs::read_to_string(dir.join("generate.py")).unwrap();
    let module = fs::read_to_string(repo().join("server/src/services/federation/mod.rs")).unwrap();
    for domain in [
        "nolune/federation/companion-id/v1",
        "nolune/federation/identity/v1",
        "nolune/federation/envelope/v1",
    ] {
        assert!(generator.contains(domain), "generate.py lacks {domain}");
        assert!(module.contains(domain), "mod.rs lacks {domain}");
    }
}

#[test]
fn storage_doc_describes_the_federation_keystore() {
    let doc = fs::read_to_string(repo().join("docs/companion-storage.md")).unwrap();
    for required in [
        "federation/",
        "signing_key.json",
        "identity.json",
        "0600",
        "companion_id",
        "export",
    ] {
        assert!(
            doc.contains(required),
            "storage doc is missing {required:?}"
        );
    }
}
