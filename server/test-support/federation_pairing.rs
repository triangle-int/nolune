//! Two-server pairing tests for #108 (PR 2), run against the full router.
//!
//! Included from `app/router.rs`. Two `AppState`s over two temp workspaces
//! stand in for two companions; the transport between them dispatches every
//! peer-side call through `build_router` of the other server, so the public
//! `/federation/v1/pair` routes are exercised exactly as over HTTP.

use super::*;
use crate::{
    domain::federation::{FEDERATION_VERSION, PairingMessage, SignedEnvelope, TransportEnvelope},
    services::federation::{
        envelope, identity,
        pairing::{FederationState, PeerTransport, sign_message},
        peers::{Clock, system_clock},
    },
};
use axum::{
    body::Body,
    http::{Method, Request, StatusCode, header},
};
use futures::future::BoxFuture;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tower::ServiceExt;

pub(super) const TOKEN_A: &str = "issue-108-owner-token-for-a";
pub(super) const TOKEN_B: &str = "issue-108-owner-token-for-b";
pub(super) const ORIGIN_A: &str = "http://a.test";
pub(super) const ORIGIN_B: &str = "http://b.test";
pub(super) const MAX_BODY: usize = 64 * 1024;

/// The wire between the two servers: a base URL maps to the `AppState`
/// listening there, and a POST is a `oneshot` through its router.
#[derive(Default)]
pub(super) struct Wire {
    pub(super) servers: Mutex<HashMap<String, AppState>>,
}

impl Wire {
    /// POSTs `json` to `path` on the server at `origin` and returns the
    /// status and body, exactly as HTTP would.
    async fn post_json(
        &self,
        url: &str,
        json: Vec<u8>,
    ) -> Result<(StatusCode, axum::body::Bytes), crate::domain::federation::FederationError> {
        use crate::domain::federation::FederationError;
        let at = url
            .find("/federation/")
            .expect("peer URL has a federation path");
        let (origin, path) = (&url[..at], &url[at..]);
        let state = self
            .servers
            .lock()
            .unwrap()
            .get(origin)
            .cloned()
            .ok_or_else(|| FederationError::Transport(format!("no route to {origin}")))?;
        let request = Request::builder()
            .method(Method::POST)
            .uri(path)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json))
            .unwrap();
        let response = build_router(state, None).oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), MAX_BODY)
            .await
            .unwrap();
        if !status.is_success() {
            let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
            return Err(FederationError::PeerRefused {
                status: status.as_u16(),
                error: body["error"].as_str().unwrap_or("unknown").to_owned(),
            });
        }
        Ok((status, bytes))
    }
}

impl PeerTransport for Wire {
    fn post<'a>(
        &'a self,
        url: &'a str,
        envelope: &'a SignedEnvelope,
    ) -> BoxFuture<'a, Result<SignedEnvelope, crate::domain::federation::FederationError>> {
        Box::pin(async move {
            let (_, bytes) = self
                .post_json(url, serde_json::to_vec(envelope).unwrap())
                .await?;
            identity::parse_envelope(std::str::from_utf8(&bytes).unwrap())
        })
    }

    fn post_transport<'a>(
        &'a self,
        url: &'a str,
        envelope: &'a TransportEnvelope,
    ) -> BoxFuture<'a, Result<TransportEnvelope, crate::domain::federation::FederationError>> {
        Box::pin(async move {
            let (_, bytes) = self
                .post_json(url, serde_json::to_vec(envelope).unwrap())
                .await?;
            envelope::parse(std::str::from_utf8(&bytes).unwrap())
        })
    }
}

pub(super) struct Server {
    pub(super) workspace: tempfile::TempDir,
    pub(super) state: AppState,
    pub(super) token: &'static str,
}

impl Server {
    pub(super) async fn start(wire: &Arc<Wire>, token: &'static str, origin: &'static str) -> Self {
        let workspace = tempfile::tempdir().unwrap();
        Self::start_in(wire, token, origin, workspace, system_clock()).await
    }

    /// A server over an existing workspace (which may already hold a
    /// federation identity) with an explicit clock.
    pub(super) async fn start_in(
        wire: &Arc<Wire>,
        token: &'static str,
        origin: &'static str,
        workspace: tempfile::TempDir,
        clock: Clock,
    ) -> Self {
        let config = crate::config::Config {
            auth_token: token.into(),
            public_url: origin.into(),
            ..crate::config::Config::default()
        };
        let mut state = AppState::new_in(config, workspace.path().to_owned()).await;
        state.federation = Arc::new(FederationState::with_transport_and_clock(
            workspace.path(),
            wire.clone(),
            clock,
        ));
        wire.servers
            .lock()
            .unwrap()
            .insert(origin.to_owned(), state.clone());
        Self {
            workspace,
            state,
            token,
        }
    }

    pub(super) fn companion_id(&self) -> String {
        self.state
            .federation
            .identity()
            .unwrap()
            .companion_id()
            .to_owned()
    }

    pub(super) fn public_key(&self) -> String {
        self.state
            .federation
            .identity()
            .unwrap()
            .document()
            .public_key
            .clone()
    }

    pub(super) fn ping(&self) -> SignedEnvelope {
        self.state
            .federation
            .identity()
            .unwrap()
            .sign_envelope(br#"{"kind":"ping"}"#)
    }

    /// Sends `request` through this server's router and asserts that neither
    /// owner token ever appears in the response.
    pub(super) async fn send(&self, request: Request<Body>) -> (StatusCode, serde_json::Value) {
        let response = build_router(self.state.clone(), None)
            .oneshot(request)
            .await
            .unwrap();
        let status = response.status();
        for value in response.headers().values() {
            let text = String::from_utf8_lossy(value.as_bytes());
            assert!(
                !text.contains(TOKEN_A) && !text.contains(TOKEN_B),
                "a response header disclosed an owner token"
            );
        }
        let bytes = axum::body::to_bytes(response.into_body(), MAX_BODY)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            !text.contains(TOKEN_A) && !text.contains(TOKEN_B),
            "a response body disclosed an owner token: {text}"
        );
        (status, serde_json::from_slice(&bytes).unwrap_or_default())
    }

    /// An owner request with this server's bearer token.
    pub(super) async fn owner(
        &self,
        method: Method,
        uri: &str,
        body: Option<serde_json::Value>,
    ) -> (StatusCode, serde_json::Value) {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {}", self.token));
        let body = match body {
            Some(json) => {
                builder = builder.header(header::CONTENT_TYPE, "application/json");
                Body::from(serde_json::to_vec(&json).unwrap())
            }
            None => Body::empty(),
        };
        self.send(builder.body(body).unwrap()).await
    }

    /// An anonymous request, as a peer (or anyone) would send it.
    pub(super) async fn anonymous(
        &self,
        method: Method,
        uri: &str,
        body: Option<Vec<u8>>,
        headers: &[(&str, &str)],
    ) -> (StatusCode, serde_json::Value) {
        let mut builder = Request::builder().method(method).uri(uri);
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        let body = match body {
            Some(bytes) => {
                builder = builder.header(header::CONTENT_TYPE, "application/json");
                Body::from(bytes)
            }
            None => Body::empty(),
        };
        self.send(builder.body(body).unwrap()).await
    }
}

pub(super) async fn two_servers() -> (Server, Server, Arc<Wire>) {
    let wire = Arc::new(Wire::default());
    let a = Server::start(&wire, TOKEN_A, ORIGIN_A).await;
    let b = Server::start(&wire, TOKEN_B, ORIGIN_B).await;
    (a, b, wire)
}

pub(super) async fn mint_invite(server: &Server) -> serde_json::Value {
    let (status, invite) = server
        .owner(Method::POST, "/api/federation/invites", None)
        .await;
    assert_eq!(status, StatusCode::CREATED, "{invite}");
    invite
}

pub(super) fn accept_body(invite: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "origin": invite["origin"],
        "secret": invite["secret"],
        "issuer": invite["issuer"],
    })
}

#[tokio::test]
async fn owners_pair_two_companions_and_both_lists_bind_the_same_keys() {
    let (a, b, _wire) = two_servers().await;
    let (a_id, b_id) = (a.companion_id(), b.companion_id());
    assert_ne!(a_id, b_id);

    let invite = mint_invite(&a).await;
    let secret = invite["secret"].as_str().unwrap().to_owned();
    assert_eq!(secret.len(), 43);
    assert_eq!(invite["id"].as_str().unwrap().len(), 16);
    assert_eq!(invite["expires_in_secs"], 600);
    assert_eq!(invite["origin"], ORIGIN_A);
    assert_eq!(invite["issuer"]["companion_id"], a_id);
    assert_eq!(invite["issuer"]["public_key"], a.public_key());

    // The list shows the outstanding invite, never its secret.
    let (status, listed) = a.owner(Method::GET, "/api/federation/peers", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed["companion_id"], a_id);
    assert_eq!(listed["identity"]["public_key"], a.public_key());
    assert_eq!(listed["invites"][0]["id"], invite["id"]);
    assert_eq!(listed["invites"][0]["state"], "invited");
    assert!(!listed.to_string().contains(&secret));
    assert_eq!(listed["peers"].as_array().unwrap().len(), 0);

    // B's owner accepts: B posts the signed pair request to A over the wire.
    let (status, accepted) = b
        .owner(
            Method::POST,
            "/api/federation/accept",
            Some(accept_body(&invite)),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{accepted}");
    assert_eq!(accepted["peer"]["companion_id"], a_id);
    assert_eq!(accepted["peer"]["public_key"], a.public_key());
    assert_eq!(accepted["peer"]["state"], "pending");
    assert_eq!(accepted["peer"]["role"], "accepter");
    assert_eq!(accepted["peer"]["pairing_id"], invite["id"]);
    assert_eq!(
        accepted["peer"]["approved_origins"],
        serde_json::json!([ORIGIN_A])
    );
    assert!(!accepted.to_string().contains(&secret));

    // A sees B pending with the origin B reported, nothing approved yet.
    let (_, listed) = a.owner(Method::GET, "/api/federation/peers", None).await;
    assert_eq!(listed["invites"].as_array().unwrap().len(), 0);
    let pending = &listed["peers"][0];
    assert_eq!(pending["companion_id"], b_id);
    assert_eq!(pending["public_key"], b.public_key());
    assert_eq!(pending["state"], "pending");
    assert_eq!(pending["role"], "issuer");
    assert_eq!(pending["pending_origin"], ORIGIN_B);
    assert_eq!(pending["approved_origins"], serde_json::json!([]));

    // Only A's owner can turn pending into paired.
    let (status, body) = b
        .owner(
            Method::POST,
            &format!("/api/federation/peers/{a_id}/confirm"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "peer_not_paired");
    let (status, confirmed) = a
        .owner(
            Method::POST,
            &format!("/api/federation/peers/{b_id}/confirm"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{confirmed}");
    assert_eq!(confirmed["peer"]["state"], "paired");
    assert_eq!(
        confirmed["peer"]["approved_origins"],
        serde_json::json!([ORIGIN_B])
    );
    assert!(confirmed["peer"].get("pending_origin").is_none());
    assert_eq!(confirmed["notified"], true);

    // Both lists show the same companion ids bound to the same keys.
    let (_, on_a) = a.owner(Method::GET, "/api/federation/peers", None).await;
    let (_, on_b) = b.owner(Method::GET, "/api/federation/peers", None).await;
    assert_eq!(on_a["peers"][0]["companion_id"], b_id);
    assert_eq!(on_a["peers"][0]["public_key"], b.public_key());
    assert_eq!(on_a["peers"][0]["state"], "paired");
    assert_eq!(on_b["peers"][0]["companion_id"], a_id);
    assert_eq!(on_b["peers"][0]["public_key"], a.public_key());
    assert_eq!(on_b["peers"][0]["state"], "paired");
    assert_eq!(
        on_b["peers"][0]["pairing_id"],
        on_a["peers"][0]["pairing_id"]
    );
    for listed in [&on_a, &on_b] {
        let text = listed.to_string();
        for absent in [&secret, "hostname", "\"profile\"", "\"port\""] {
            assert!(!text.contains(absent), "peer list carries {absent:?}");
        }
    }

    // Verification works in both directions.
    assert!(a.state.federation.verify_from_peer(&b.ping()).is_ok());
    assert!(b.state.federation.verify_from_peer(&a.ping()).is_ok());

    // Replaying the invite fails: through B's owner route and straight at A.
    let (status, body) = b
        .owner(
            Method::POST,
            "/api/federation/accept",
            Some(accept_body(&invite)),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert_eq!(body["error"], "peer_refused");
    assert_eq!(body["peer_status"], 401);
    assert_eq!(body["peer_error"], "invalid_invite");
    let stranger = identity::load_or_create_at(tempfile::tempdir().unwrap().path(), 1).unwrap();
    let replay = sign_message(
        &stranger,
        &PairingMessage::Request {
            version: FEDERATION_VERSION,
            secret: crate::domain::federation::InviteSecret::new(secret.clone()),
            issuer: a_id.clone(),
            accepter: stranger.document().clone(),
            origin: "https://stranger.test".into(),
        },
    );
    let (status, body) = a
        .anonymous(
            Method::POST,
            "/federation/v1/pair",
            Some(serde_json::to_vec(&replay).unwrap()),
            &[],
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(body["error"], "invalid_invite");
    let (_, on_a) = a.owner(Method::GET, "/api/federation/peers", None).await;
    assert_eq!(on_a["peers"].as_array().unwrap().len(), 1);

    // Revoking on B stops verification on both sides.
    let (status, revoked) = b
        .owner(
            Method::POST,
            &format!("/api/federation/peers/{a_id}/revoke"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{revoked}");
    assert_eq!(revoked["peer"]["state"], "revoked");
    assert_eq!(revoked["notified"], true);
    let (_, on_a) = a.owner(Method::GET, "/api/federation/peers", None).await;
    assert_eq!(on_a["peers"][0]["state"], "revoked");
    assert_eq!(
        a.state.federation.verify_from_peer(&b.ping()).unwrap_err(),
        crate::domain::federation::FederationError::PeerRevoked
    );
    assert_eq!(
        b.state.federation.verify_from_peer(&a.ping()).unwrap_err(),
        crate::domain::federation::FederationError::PeerRevoked
    );

    // And the other way round, after pairing again with a fresh invite.
    let invite = mint_invite(&a).await;
    let (status, _) = b
        .owner(
            Method::POST,
            "/api/federation/accept",
            Some(accept_body(&invite)),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = a
        .owner(
            Method::POST,
            &format!("/api/federation/peers/{b_id}/confirm"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(a.state.federation.verify_from_peer(&b.ping()).is_ok());
    assert!(b.state.federation.verify_from_peer(&a.ping()).is_ok());
    let (status, revoked) = a
        .owner(
            Method::POST,
            &format!("/api/federation/peers/{b_id}/revoke"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{revoked}");
    assert_eq!(revoked["notified"], true);
    assert!(a.state.federation.verify_from_peer(&b.ping()).is_err());
    assert!(b.state.federation.verify_from_peer(&a.ping()).is_err());
    let (_, on_b) = b.owner(Method::GET, "/api/federation/peers", None).await;
    assert_eq!(on_b["peers"][0]["state"], "revoked");
    assert_eq!(on_b["peers"].as_array().unwrap().len(), 1);

    // The peer stores live beside the keystore, outside the companion export root.
    for server in [&a, &b] {
        let root = server.state.workspace_dir.clone();
        assert_eq!(root, server.workspace.path());
        assert!(root.join("federation/peers.json").is_file());
        assert!(root.join("federation/identity.json").is_file());
        assert!(!root.join("instances/companion/federation").exists());
        let text = std::fs::read_to_string(root.join("federation/peers.json")).unwrap();
        assert!(
            !text.contains(&secret),
            "peers.json holds the invite secret"
        );
    }
}

#[tokio::test]
async fn owner_routes_need_the_owner_and_peer_routes_need_a_signature() {
    let (a, b, _wire) = two_servers().await;
    let a_id = a.companion_id();

    // Owner routes sit behind the API auth middleware.
    for (method, uri) in [
        (Method::POST, "/api/federation/invites"),
        (Method::POST, "/api/federation/accept"),
        (Method::GET, "/api/federation/peers"),
        (Method::POST, "/api/federation/peers/x/confirm"),
        (Method::POST, "/api/federation/peers/x/revoke"),
        (Method::DELETE, "/api/federation/invites/x"),
    ] {
        let (status, _) = a.anonymous(method.clone(), uri, None, &[]).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri}");
        let (status, _) = a
            .anonymous(
                method.clone(),
                uri,
                None,
                &[("authorization", &format!("Bearer {TOKEN_B}"))],
            )
            .await;
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "{method} {uri} with B's token"
        );
    }
    let (status, _) = a
        .owner(Method::POST, "/api/federation/peers/nobody/confirm", None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = a
        .owner(Method::DELETE, "/api/federation/invites/nobody", None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let invite = mint_invite(&a).await;
    let (status, _) = a
        .owner(
            Method::DELETE,
            &format!("/api/federation/invites/{}", invite["id"].as_str().unwrap()),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = b
        .owner(
            Method::POST,
            "/api/federation/accept",
            Some(accept_body(&invite)),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::BAD_GATEWAY,
        "a cancelled invite: {body}"
    );
    assert_eq!(body["peer_error"], "invalid_invite");

    // Peer routes take nothing but a signed envelope. The owner token, a
    // browser cookie, a Host header, or a loopback-looking origin change
    // nothing: a garbage body is a garbage body.
    let invite = mint_invite(&a).await;
    let secret = invite["secret"].as_str().unwrap().to_owned();
    for headers in [
        vec![],
        vec![("authorization", format!("Bearer {TOKEN_A}"))],
        vec![("cookie", "nolune_session=abc.def".to_owned())],
        vec![
            ("host", "localhost:26559".to_owned()),
            ("x-forwarded-for", "127.0.0.1".to_owned()),
        ],
    ] {
        let headers: Vec<(&str, &str)> = headers
            .iter()
            .map(|(name, value)| (*name, value.as_str()))
            .collect();
        let (status, body) = a
            .anonymous(
                Method::POST,
                "/federation/v1/pair",
                Some(b"{\"not\":\"an envelope\"}".to_vec()),
                &headers,
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{headers:?}: {body}");
        assert_eq!(body["error"], "malformed");
        assert!(
            !body.to_string().contains("an envelope"),
            "the body was echoed"
        );
    }
    for path in [
        "/federation/v1/pair",
        "/federation/v1/pair/confirm",
        "/federation/v1/pair/revoke",
    ] {
        let (status, _) = a.anonymous(Method::GET, path, None, &[]).await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED, "{path}");
        let (status, body) = a
            .anonymous(Method::POST, path, Some(vec![b'x'; MAX_BODY + 1]), &[])
            .await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{path}: {body}");
    }

    // A secret in the query string is never read: the signed body decides.
    let stranger = identity::load_or_create_at(tempfile::tempdir().unwrap().path(), 1).unwrap();
    let wrong = sign_message(
        &stranger,
        &PairingMessage::Request {
            version: FEDERATION_VERSION,
            secret: crate::domain::federation::InviteSecret::new("wrong".into()),
            issuer: a_id.clone(),
            accepter: stranger.document().clone(),
            origin: "https://stranger.test".into(),
        },
    );
    let (status, body) = a
        .anonymous(
            Method::POST,
            &format!("/federation/v1/pair?secret={secret}&code={secret}"),
            Some(serde_json::to_vec(&wrong).unwrap()),
            &[],
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(body["error"], "invalid_invite");
    let (_, listed) = a.owner(Method::GET, "/api/federation/peers", None).await;
    assert_eq!(
        listed["invites"].as_array().unwrap().len(),
        1,
        "the invite survives"
    );
    assert_eq!(listed["peers"].as_array().unwrap().len(), 0);

    // A request signed by one key but carrying another's document.
    let other = identity::load_or_create_at(tempfile::tempdir().unwrap().path(), 1).unwrap();
    let spoofed = sign_message(
        &other,
        &PairingMessage::Request {
            version: FEDERATION_VERSION,
            secret: crate::domain::federation::InviteSecret::new(secret.clone()),
            issuer: a_id.clone(),
            accepter: stranger.document().clone(),
            origin: "https://stranger.test".into(),
        },
    );
    let (status, body) = a
        .anonymous(
            Method::POST,
            "/federation/v1/pair",
            Some(serde_json::to_vec(&spoofed).unwrap()),
            &[],
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"], "sender_mismatch");

    // A downgraded envelope is refused before anything else.
    let mut downgraded = sign_message(
        &stranger,
        &PairingMessage::Request {
            version: FEDERATION_VERSION,
            secret: crate::domain::federation::InviteSecret::new(secret.clone()),
            issuer: a_id.clone(),
            accepter: stranger.document().clone(),
            origin: "https://stranger.test".into(),
        },
    );
    downgraded.version = 0;
    let (status, body) = a
        .anonymous(
            Method::POST,
            "/federation/v1/pair",
            Some(serde_json::to_vec(&downgraded).unwrap()),
            &[],
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "version_too_old");

    // Notices from strangers are refused without touching the store.
    let notice = sign_message(
        &stranger,
        &PairingMessage::Confirm {
            version: FEDERATION_VERSION,
            pairing_id: "0011223344556677".into(),
            issuer: stranger.companion_id().to_owned(),
            accepter: a_id.clone(),
        },
    );
    let (status, body) = a
        .anonymous(
            Method::POST,
            "/federation/v1/pair/confirm",
            Some(serde_json::to_vec(&notice).unwrap()),
            &[],
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["error"], "unknown_peer");
    let (status, body) = a
        .anonymous(
            Method::POST,
            "/federation/v1/pair/revoke",
            Some(serde_json::to_vec(&notice).unwrap()),
            &[],
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["error"], "unknown_peer");

    // Strangers guessing at the public route cannot lock the owner out:
    // there is no failure counter to trip, because the secret is 32 random
    // bytes and a counter would only serve a denial of service.
    for _ in 0..64 {
        let (status, body) = a
            .anonymous(
                Method::POST,
                "/federation/v1/pair",
                Some(serde_json::to_vec(&wrong).unwrap()),
                &[],
            )
            .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
        assert_eq!(body["error"], "invalid_invite");
        assert!(body.get("retry_after").is_none());
    }

    // The genuine request still works after all that noise.
    let (status, accepted) = b
        .owner(
            Method::POST,
            "/api/federation/accept",
            Some(accept_body(&invite)),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{accepted}");
    assert_eq!(accepted["peer"]["state"], "pending");
}

#[tokio::test]
async fn accept_refuses_bad_input_before_touching_the_wire() {
    let (a, b, _wire) = two_servers().await;
    let invite = mint_invite(&a).await;
    for (body, expected) in [
        (serde_json::json!({}), "invalid_body"),
        (
            serde_json::json!({ "origin": "a.test", "secret": invite["secret"], "issuer": invite["issuer"] }),
            "invalid_origin",
        ),
        (
            serde_json::json!({ "origin": "http://unreachable.test", "secret": invite["secret"], "issuer": invite["issuer"] }),
            "peer_unreachable",
        ),
        (
            serde_json::json!({ "origin": invite["origin"], "secret": invite["secret"], "issuer": invite["issuer"], "profile": "molinka" }),
            "invalid_body",
        ),
    ] {
        let (status, response) = b
            .owner(Method::POST, "/api/federation/accept", Some(body.clone()))
            .await;
        assert_eq!(response["error"], expected, "{body} -> {status} {response}");
        assert!(
            status.is_client_error() || status == StatusCode::BAD_GATEWAY,
            "{status}"
        );
        assert!(
            !response
                .to_string()
                .contains(invite["secret"].as_str().unwrap()),
            "the error echoed the secret"
        );
    }
    let (_, listed) = a.owner(Method::GET, "/api/federation/peers", None).await;
    assert_eq!(listed["invites"].as_array().unwrap().len(), 1);
    let (_, listed) = b.owner(Method::GET, "/api/federation/peers", None).await;
    assert_eq!(listed["peers"].as_array().unwrap().len(), 0);
}

/// #108 PR 4: the invite response also carries the whole invite as one line
/// (`invite`), which the accepting owner pastes into `nolune federation
/// accept` or the Companions section; the accept route takes that line in
/// place of the three fields and refuses a URL in its place.
#[tokio::test]
async fn owners_accept_with_the_one_line_invite_token() {
    use crate::services::federation::invite_token::{INVITE_TOKEN_PREFIX, decode_invite};
    let (a, b, _wire) = two_servers().await;
    let (a_id, b_id) = (a.companion_id(), b.companion_id());

    let invite = mint_invite(&a).await;
    let secret = invite["secret"].as_str().unwrap().to_owned();
    let token = invite["invite"].as_str().expect("invite line").to_owned();
    assert!(token.starts_with(INVITE_TOKEN_PREFIX), "{token}");
    assert!(!token.contains("://"), "an invite line is never a URL");
    let decoded = decode_invite(&token).unwrap();
    assert_eq!(decoded.origin, ORIGIN_A);
    assert_eq!(decoded.secret.expose(), secret);
    assert_eq!(decoded.issuer.companion_id, a_id);

    // The list never shows the line again, in any field.
    let (_, listed) = a.owner(Method::GET, "/api/federation/peers", None).await;
    let text = listed.to_string();
    assert!(!text.contains(&token) && !text.contains(&secret), "{text}");

    // A URL, a line with extra fields, and a line mixed with the fields are
    // refused before anything goes over the wire.
    for (body, expected) in [
        (
            serde_json::json!({ "invite": format!("{ORIGIN_A}/federation/v1/pair?invite={token}") }),
            "malformed",
        ),
        (serde_json::json!({ "invite": "hello" }), "malformed"),
        (serde_json::json!({ "invite": "" }), "malformed"),
        (
            serde_json::json!({ "invite": token, "origin": ORIGIN_A }),
            "invalid_body",
        ),
        (serde_json::json!({ "invite": 7 }), "invalid_body"),
    ] {
        let (status, response) = b
            .owner(Method::POST, "/api/federation/accept", Some(body.clone()))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body} -> {response}");
        assert_eq!(response["error"], expected, "{body} -> {response}");
        assert!(
            !response.to_string().contains(&secret),
            "the error echoed the secret"
        );
    }
    let (_, listed) = a.owner(Method::GET, "/api/federation/peers", None).await;
    assert_eq!(listed["invites"].as_array().unwrap().len(), 1);
    assert_eq!(listed["peers"].as_array().unwrap().len(), 0);

    // The genuine line redeems the invite exactly once.
    let (status, accepted) = b
        .owner(
            Method::POST,
            "/api/federation/accept",
            Some(serde_json::json!({ "invite": token })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{accepted}");
    assert_eq!(accepted["peer"]["companion_id"], a_id);
    assert_eq!(accepted["peer"]["state"], "pending");
    assert!(!accepted.to_string().contains(&secret));
    let (status, replayed) = b
        .owner(
            Method::POST,
            "/api/federation/accept",
            Some(serde_json::json!({ "invite": token })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{replayed}");
    assert_eq!(replayed["peer_error"], "invalid_invite");

    // Both sides record when they last verified the other (#108 PR 4).
    let (_, listed) = a.owner(Method::GET, "/api/federation/peers", None).await;
    assert_eq!(listed["peers"][0]["companion_id"], b_id);
    assert!(listed["peers"][0]["last_seen_at"].as_u64().is_some());
    let (_, listed) = b.owner(Method::GET, "/api/federation/peers", None).await;
    assert!(listed["peers"][0]["last_seen_at"].as_u64().is_some());
}
