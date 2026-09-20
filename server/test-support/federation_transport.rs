//! Two-server conformance tests for the federation transport (#108, PR 3),
//! run against the full router.
//!
//! Included from `app/router.rs` beside the pairing tests, whose harness it
//! reuses: two `AppState`s over two temp workspaces, a `Wire` that
//! `oneshot`s every peer-side POST through the other server's
//! `build_router`. Everything a peer sends goes through the public
//! `/federation/v1/*` routes exactly as it would over HTTP.

use super::federation_tests::{
    MAX_BODY, ORIGIN_A, ORIGIN_B, Server, TOKEN_A, TOKEN_B, Wire, accept_body, mint_invite,
};
use crate::{
    domain::federation::{FEDERATION_VERSION, TransportEnvelope, TransportMessage},
    services::federation::{
        envelope::{self, ENVELOPE_LIFETIME_SECS, MAX_CLOCK_SKEW_SECS},
        identity,
        peers::Clock,
        rotation::{ROTATION_GRACE_SECS, endorse},
    },
};
use axum::http::{Method, StatusCode};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

const TOKEN_C: &str = "issue-108-owner-token-for-c";
const ORIGIN_C: &str = "https://c.test:8443/moved";
const T0: u64 = 1_800_000_000;
const PING: &[u8] = br#"{"kind":"ping","version":1}"#;

/// Both servers share one adjustable clock.
async fn two_servers_at(now: &Arc<AtomicU64>) -> (Server, Server, Arc<Wire>) {
    let wire = Arc::new(Wire::default());
    let a = Server::start_in(
        &wire,
        TOKEN_A,
        ORIGIN_A,
        tempfile::tempdir().unwrap(),
        clock(now),
    )
    .await;
    let b = Server::start_in(
        &wire,
        TOKEN_B,
        ORIGIN_B,
        tempfile::tempdir().unwrap(),
        clock(now),
    )
    .await;
    (a, b, wire)
}

fn clock(now: &Arc<AtomicU64>) -> Clock {
    let read = now.clone();
    Arc::new(move || read.load(Ordering::SeqCst))
}

/// Pairs A (issuer) and B (accepter) over the owner routes.
async fn pair(a: &Server, b: &Server) {
    let invite = mint_invite(a).await;
    let (status, body) = b
        .owner(
            Method::POST,
            "/api/federation/accept",
            Some(accept_body(&invite)),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = a
        .owner(
            Method::POST,
            &format!("/api/federation/peers/{}/confirm", b.companion_id()),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["notified"], true);
}

/// POSTs a transport envelope at `path` on `server` and returns the status
/// and the parsed JSON body.
async fn post(
    server: &Server,
    path: &str,
    envelope: &TransportEnvelope,
    headers: &[(&str, &str)],
) -> (StatusCode, serde_json::Value) {
    server
        .anonymous(
            Method::POST,
            path,
            Some(serde_json::to_vec(envelope).unwrap()),
            headers,
        )
        .await
}

fn as_envelope(body: &serde_json::Value) -> TransportEnvelope {
    serde_json::from_value(body.clone()).expect("a transport envelope")
}

#[tokio::test]
async fn an_envelope_from_a_paired_peer_verifies_once_over_the_public_route() {
    let now = Arc::new(AtomicU64::new(T0));
    let (a, b, _wire) = two_servers_at(&now).await;
    let (a_id, b_id) = (a.companion_id(), b.companion_id());
    pair(&a, &b).await;

    // B pings A: the answer is a pong sealed for B, which opens exactly once.
    let ping = b.state.federation.seal(&a_id, PING).unwrap();
    let (status, body) = post(&a, "/federation/v1/ping", &ping, &[]).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let pong = as_envelope(&body);
    assert_eq!(pong.sender, a_id);
    assert_eq!(pong.recipient, b_id);
    let opened = b.state.federation.open(&pong).unwrap();
    assert_eq!(
        serde_json::from_slice::<TransportMessage>(&opened.body).unwrap(),
        TransportMessage::Pong {
            version: FEDERATION_VERSION
        }
    );
    assert_eq!(
        b.state.federation.open(&pong).unwrap_err(),
        crate::domain::federation::FederationError::Replayed
    );

    // The same envelope again is a replay.
    let (status, body) = post(&a, "/federation/v1/ping", &ping, &[]).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"], "replayed");

    // Every other failure is typed, and none of them consumes a nonce.
    let stranger = identity::load_or_create_at(tempfile::tempdir().unwrap().path(), T0).unwrap();
    let from_stranger = envelope::seal(&stranger, &a_id, PING, T0).unwrap();
    let (status, body) = post(&a, "/federation/v1/ping", &from_stranger, &[]).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["error"], "unknown_peer");

    let for_b = b.state.federation.seal(&b_id, PING).unwrap();
    let (status, body) = post(&a, "/federation/v1/ping", &for_b, &[]).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"], "recipient_mismatch");

    let genuine = b.state.federation.seal(&a_id, PING).unwrap();
    let mut tampered = genuine.clone();
    tampered.body = crate::services::federation::encode(br#"{"kind":"ping","version":1} "#);
    let (status, body) = post(&a, "/federation/v1/ping", &tampered, &[]).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"], "body_hash_mismatch");
    let mut resigned = genuine.clone();
    resigned.signature = from_stranger.signature.clone();
    let (status, body) = post(&a, "/federation/v1/ping", &resigned, &[]).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"], "signature_mismatch");
    let mut downgraded = genuine.clone();
    downgraded.version = 0;
    let (status, body) = post(&a, "/federation/v1/ping", &downgraded, &[]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "version_too_old");
    let mut unknown = genuine.clone();
    unknown.version = FEDERATION_VERSION + 1;
    let (status, body) = post(&a, "/federation/v1/ping", &unknown, &[]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "version_unsupported");

    now.store(
        T0 + ENVELOPE_LIFETIME_SECS + MAX_CLOCK_SKEW_SECS,
        Ordering::SeqCst,
    );
    let (status, body) = post(&a, "/federation/v1/ping", &genuine, &[]).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"], "expired");
    now.store(T0 - MAX_CLOCK_SKEW_SECS - 1, Ordering::SeqCst);
    let (status, body) = post(&a, "/federation/v1/ping", &genuine, &[]).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"], "issued_in_future");
    now.store(T0, Ordering::SeqCst);

    // Headers change nothing: the owner token, a browser session, a Host
    // header, or a loopback-looking origin neither admit nor refuse.
    let (status, body) = post(
        &a,
        "/federation/v1/ping",
        &tampered,
        &[
            ("authorization", &format!("Bearer {TOKEN_A}")),
            ("cookie", "nolune_session=abc.def"),
            ("host", "localhost:26559"),
            ("x-forwarded-for", "127.0.0.1"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"], "body_hash_mismatch");
    let (status, body) = post(
        &a,
        "/federation/v1/ping",
        &genuine,
        &[
            ("authorization", "Bearer not-a-token"),
            ("host", "evil.example"),
        ],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a genuine envelope with junk headers still verifies: {body}"
    );

    // Method and size limits, and a body that is not an envelope.
    for path in ["/federation/v1/ping", "/federation/v1/rotate"] {
        let (status, _) = a.anonymous(Method::GET, path, None, &[]).await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED, "{path}");
        let (status, body) = a
            .anonymous(Method::POST, path, Some(vec![b'x'; MAX_BODY + 1]), &[])
            .await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{path}: {body}");
        let (status, body) = a
            .anonymous(
                Method::POST,
                path,
                Some(b"{\"not\":\"an envelope\"}".to_vec()),
                &[],
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{path}: {body}");
        assert_eq!(body["error"], "malformed");
        assert!(
            !body.to_string().contains("an envelope"),
            "the body was echoed"
        );
    }

    // Revoked on A: B's envelopes stop verifying at once.
    let (status, _) = a
        .owner(
            Method::POST,
            &format!("/api/federation/peers/{b_id}/revoke"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = post(&a, "/federation/v1/ping", &genuine, &[]).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"], "peer_revoked");

    // The owner rotation route sits behind the API auth like the rest.
    let (status, _) = a
        .anonymous(Method::POST, "/api/federation/rotate", None, &[])
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_moved_identity_verifies_from_a_new_origin_and_rotates_with_an_audit_trail() {
    let now = Arc::new(AtomicU64::new(T0));
    let (a, b, wire) = two_servers_at(&now).await;
    let (a_id, b_id) = (a.companion_id(), b.companion_id());
    pair(&a, &b).await;
    let old_b = b.state.federation.identity().unwrap();

    // B's federation directory (identity, key, peers) is carried to a third
    // workspace served from another origin, port, and path. Nothing in the
    // files names the old ones.
    let moved = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(moved.path().join("federation")).unwrap();
    for file in ["identity.json", "signing_key.json", "peers.json"] {
        std::fs::copy(
            b.workspace.path().join("federation").join(file),
            moved.path().join("federation").join(file),
        )
        .unwrap();
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for file in ["identity.json", "signing_key.json", "peers.json"] {
            std::fs::set_permissions(
                moved.path().join("federation").join(file),
                std::fs::Permissions::from_mode(0o600),
            )
            .unwrap();
        }
    }
    let c = Server::start_in(&wire, TOKEN_C, ORIGIN_C, moved, clock(&now)).await;
    assert_eq!(
        c.companion_id(),
        b_id,
        "the identity travelled with the files"
    );
    assert_eq!(c.public_key(), b.public_key());
    let (_, on_c) = c.owner(Method::GET, "/api/federation/peers", None).await;
    assert_eq!(on_c["peers"][0]["companion_id"], a_id);
    assert_eq!(on_c["peers"][0]["state"], "paired");

    // A verifies the moved companion by its key alone. Its record still
    // approves only the origin B's owner reported; nothing about C's host,
    // port, or path was learned or trusted.
    let ping = c.state.federation.seal(&a_id, PING).unwrap();
    let (status, body) = post(&a, "/federation/v1/ping", &ping, &[]).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(c.state.federation.open(&as_envelope(&body)).is_ok());
    let (_, on_a) = a.owner(Method::GET, "/api/federation/peers", None).await;
    assert_eq!(on_a["peers"][0]["companion_id"], b_id);
    assert_eq!(
        on_a["peers"][0]["approved_origins"],
        serde_json::json!([ORIGIN_B])
    );
    assert!(!on_a.to_string().contains("c.test"));

    // The moved companion rotates its key through the owner route. A is
    // told at its approved origin, re-keys its record, and keeps the
    // transition.
    now.store(T0 + 10, Ordering::SeqCst);
    let (status, rotated) = c.owner(Method::POST, "/api/federation/rotate", None).await;
    assert_eq!(status, StatusCode::OK, "{rotated}");
    let new_id = rotated["identity"]["companion_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(new_id, b_id);
    assert_eq!(rotated["rotation"]["previous"]["companion_id"], b_id);
    assert_eq!(rotated["rotation"]["identity"]["companion_id"], new_id);
    assert_eq!(rotated["notified"], serde_json::json!([a_id]));
    assert_eq!(rotated["unreachable"], serde_json::json!([]));
    assert!(
        !rotated.to_string().contains("secret_key"),
        "the rotation report holds public documents only"
    );
    assert_eq!(c.companion_id(), new_id);
    let (_, on_c) = c.owner(Method::GET, "/api/federation/peers", None).await;
    assert_eq!(on_c["companion_id"], new_id);
    assert_eq!(on_c["rotations"].as_array().unwrap().len(), 1);
    assert_eq!(on_c["rotations"][0], rotated["rotation"]);
    let (_, on_a) = a.owner(Method::GET, "/api/federation/peers", None).await;
    assert_eq!(on_a["peers"].as_array().unwrap().len(), 1);
    assert_eq!(on_a["peers"][0]["companion_id"], new_id);
    assert_eq!(
        on_a["peers"][0]["public_key"],
        rotated["identity"]["public_key"]
    );
    assert_eq!(on_a["peers"][0]["state"], "paired");
    assert_eq!(
        on_a["peers"][0]["approved_origins"],
        serde_json::json!([ORIGIN_B])
    );
    let history = on_a["peers"][0]["rotation_history"].as_array().unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0]["rotation"], rotated["rotation"]);
    assert_eq!(history[0]["accepted_at"], T0 + 10);
    let stored = std::fs::read_to_string(a.workspace.path().join("federation/peers.json")).unwrap();
    assert!(stored.contains(&new_id) && stored.contains(&b_id));
    assert!(
        c.workspace
            .path()
            .join("federation/rotations.json")
            .is_file(),
        "the rotating side keeps its own proof"
    );

    // The new key works at once; the old one for the grace window only.
    let ping = c.state.federation.seal(&a_id, PING).unwrap();
    let (status, body) = post(&a, "/federation/v1/ping", &ping, &[]).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let stale = envelope::seal(&old_b, &a_id, PING, T0 + 10).unwrap();
    let (status, body) = post(&a, "/federation/v1/ping", &stale, &[]).await;
    assert_eq!(status, StatusCode::OK, "inside the grace window: {body}");
    // The copy left behind at B signs with the retired key too.
    let from_b = b.state.federation.seal(&a_id, PING).unwrap();
    let (status, body) = post(&a, "/federation/v1/ping", &from_b, &[]).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    now.store(T0 + 10 + ROTATION_GRACE_SECS, Ordering::SeqCst);
    let retired = envelope::seal(&old_b, &a_id, PING, T0 + 10 + ROTATION_GRACE_SECS).unwrap();
    let (status, body) = post(&a, "/federation/v1/ping", &retired, &[]).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"], "key_retired");
    let ping = c.state.federation.seal(&a_id, PING).unwrap();
    let (status, body) = post(&a, "/federation/v1/ping", &ping, &[]).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // A rotation notice at the public route needs the retiring key: a
    // stranger is unknown, and a peer cannot rotate someone else's record.
    let stranger = identity::load_or_create_at(tempfile::tempdir().unwrap().path(), T0).unwrap();
    let elsewhere = identity::load_or_create_at(tempfile::tempdir().unwrap().path(), T0).unwrap();
    let notice = |from: &identity::SigningIdentity, rotation| {
        let body = serde_json::to_vec(&TransportMessage::KeyRotation {
            version: FEDERATION_VERSION,
            rotation: Box::new(rotation),
        })
        .unwrap();
        envelope::seal(from, &a_id, &body, T0 + 10 + ROTATION_GRACE_SECS).unwrap()
    };
    let (status, body) = post(
        &a,
        "/federation/v1/rotate",
        &notice(&stranger, endorse(&stranger, &elsewhere, T0)),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["error"], "unknown_peer");
    let current_c = c.state.federation.identity().unwrap();
    let (status, body) = post(
        &a,
        "/federation/v1/rotate",
        &notice(&current_c, endorse(&stranger, &elsewhere, T0)),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"], "rotation_mismatch");
    let (_, on_a) = a.owner(Method::GET, "/api/federation/peers", None).await;
    assert_eq!(on_a["peers"][0]["companion_id"], new_id);
    assert_eq!(
        on_a["peers"][0]["rotation_history"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
