//! Two-server tests for the outbox and the sending tools (#110, PR 3),
//! run against the full router.
//!
//! Included from `app/router.rs` beside the inbound tests, whose harness
//! it reuses: two `AppState`s over two temp workspaces on one adjustable
//! clock, paired through the `Wire`. A queues intents for B through the
//! chat tools or the outbox directly, drives the sender loop one pass at a
//! time, and B answers over its public intent route exactly as it would
//! over HTTP. The wire can be cut (B unreachable) or made to lose B's
//! answer after B acted, which is what a retry into B's dedupe has to
//! survive.

use super::federation_tests::{
    ORIGIN_A, ORIGIN_B, Server, TOKEN_A, TOKEN_B, Wire, accept_body, mint_invite,
};
use crate::{
    app::state::AppState,
    domain::{
        chat::ChatRole,
        companion::CANONICAL_SLUG,
        events::ServerEvent,
        federation_intent::{
            IntentAnswer, IntentOutcome, IntentPayload, IntentReceipt, IntentResponse,
            MAX_INTENT_CLOCK_SKEW_SECS, ReceiptBasis,
        },
        federation_policy::{
            Access, AuditReceipt, DecisionReason, DisclosureClass, IntentClass, PolicyRule,
            ReceiptSide, Verdict,
        },
    },
    services::{
        chat,
        commitments::ListFilter,
        federation::{
            gate::FederationGate,
            inbound::{InboundIntent, InboundStatus, InboundStore},
            outbox::{
                AttemptOutcome, MAX_DELIVERY_ATTEMPTS, OUTBOX_INTENT_LIFETIME_SECS,
                OWNER_RETRY_SECS, Outbox, OutboxEntry, OutboxRequest, OutboxStatus, Outgoing,
                RETRY_MAX_SECS, backoff_secs,
            },
            pairing::FederationState,
            peers::Clock,
        },
        peer_delivery::INBOUND_CHAT_ID,
        tool::ToolDyn,
        tools::federation::{PeerSending, federation_tools},
    },
};
use axum::http::{Method, StatusCode};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

const T0: u64 = 1_800_000_000;

fn clock(now: &Arc<AtomicU64>) -> Clock {
    let read = now.clone();
    Arc::new(move || read.load(Ordering::SeqCst))
}

/// A server over `workspace` whose federation state, gate, inbox, and
/// outbox share `clock` and `wire`.
async fn start_in(
    wire: &Arc<Wire>,
    token: &'static str,
    origin: &'static str,
    workspace: tempfile::TempDir,
    clock: Clock,
) -> Server {
    let config = crate::config::Config {
        auth_token: token.into(),
        public_url: origin.into(),
        ..crate::config::Config::default()
    };
    let mut state = AppState::new_in(config, workspace.path().to_owned()).await;
    state.federation = Arc::new(FederationState::with_transport_and_clock(
        workspace.path(),
        wire.clone(),
        clock.clone(),
    ));
    state.federation_gate = Arc::new(FederationGate::with_transport_and_clock(
        workspace.path(),
        wire.clone(),
        clock.clone(),
    ));
    state.federation_inbox = Arc::new(InboundStore::with_clock(workspace.path(), clock.clone()));
    state.federation_outbox = Arc::new(
        Outbox::with_transport_and_clock(workspace.path(), wire.clone(), clock)
            .with_events(state.events.clone(), CANONICAL_SLUG),
    );
    wire.servers
        .lock()
        .unwrap()
        .insert(origin.to_owned(), state.clone());
    Server {
        workspace,
        state,
        token,
    }
}

/// Two paired servers (A issued, B accepted) on one clock.
async fn paired(now: &Arc<AtomicU64>) -> (Server, Server, Arc<Wire>) {
    let wire = Arc::new(Wire::default());
    let a = start_in(
        &wire,
        TOKEN_A,
        ORIGIN_A,
        tempfile::tempdir().unwrap(),
        clock(now),
    )
    .await;
    let b = start_in(
        &wire,
        TOKEN_B,
        ORIGIN_B,
        tempfile::tempdir().unwrap(),
        clock(now),
    )
    .await;
    let invite = mint_invite(&a).await;
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
    (a, b, wire)
}

fn rule(
    server: &Server,
    peer: &str,
    intent: IntentClass,
    disclosure: DisclosureClass,
    access: Access,
) {
    server
        .state
        .federation_gate
        .update_policy(|document| {
            let rules = &mut document.peers.entry(peer.to_owned()).or_default().rules;
            rules.retain(|rule| !(rule.intent == intent && rule.disclosure == disclosure));
            rules.push(PolicyRule {
                intent,
                disclosure,
                access,
                granted_at: T0,
                expires_at: None,
            });
            Ok(())
        })
        .unwrap();
}

fn allow(server: &Server, peer: &str, intent: IntentClass, disclosure: DisclosureClass) {
    rule(server, peer, intent, disclosure, Access::Allow);
}

fn sending(server: &Server) -> PeerSending {
    PeerSending {
        federation: server.state.federation.clone(),
        gate: server.state.federation_gate.clone(),
        outbox: server.state.federation_outbox.clone(),
    }
}

/// The sending tool `name` as the chat would carry it.
fn tool(server: &Server, name: &str) -> Box<dyn ToolDyn> {
    federation_tools(
        sending(server),
        server.workspace.path(),
        CANONICAL_SLUG,
        "default",
    )
    .into_iter()
    .find(|tool| tool.name() == name)
    .unwrap_or_else(|| panic!("{name} is registered"))
}

/// One pass of the sender loop on `server`.
async fn run(server: &Server) -> usize {
    server
        .state
        .federation_outbox
        .run_due(&server.state.federation, &server.state.federation_gate)
        .await
        .unwrap()
}

fn message(server: &Server, text: &str) -> Outgoing {
    Outgoing {
        peer: server.companion_id(),
        represented_owner: "Bob".into(),
        purpose: "plans for the weekend".into(),
        request: OutboxRequest::Message { text: text.into() },
        chat_id: "default".into(),
    }
}

fn enqueue(from: &Server, outgoing: Outgoing) -> OutboxEntry {
    from.state
        .federation_outbox
        .enqueue(
            &from.state.federation,
            &from.state.federation_gate,
            outgoing,
        )
        .unwrap()
}

async fn outbox(server: &Server) -> (Vec<OutboxEntry>, Vec<IntentReceipt>) {
    let (status, body) = server
        .owner(Method::GET, "/api/federation/outbox", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    (
        serde_json::from_value(body["entries"].clone()).expect("entries"),
        serde_json::from_value(body["receipts"].clone()).expect("receipts"),
    )
}

async fn inbox(server: &Server) -> (Vec<InboundIntent>, Vec<IntentReceipt>) {
    let (status, body) = server
        .owner(Method::GET, "/api/federation/inbox", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    (
        serde_json::from_value(body["intents"].clone()).expect("records"),
        serde_json::from_value(body["receipts"].clone()).expect("receipts"),
    )
}

async fn audit(server: &Server) -> Vec<AuditReceipt> {
    let (status, body) = server
        .owner(Method::GET, "/api/federation/receipts", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    serde_json::from_value(body["receipts"].clone()).expect("a list of receipts")
}

/// The requesting-side audit lines on `server`, oldest first.
async fn requesting_audit(server: &Server) -> Vec<AuditReceipt> {
    let mut lines: Vec<AuditReceipt> = audit(server)
        .await
        .into_iter()
        .filter(|receipt| receipt.side == ReceiptSide::Requesting)
        .collect();
    lines.sort_by_key(|receipt| receipt.at);
    lines
}

fn chat_messages(server: &Server) -> Vec<crate::domain::chat::ChatMessage> {
    chat::load_messages(server.workspace.path(), CANONICAL_SLUG, INBOUND_CHAT_ID)
        .map(|response| response.messages)
        .unwrap_or_default()
}

/// The outbox entries broadcast on `rx` since it was last drained.
fn broadcast_entries(rx: &mut tokio::sync::broadcast::Receiver<ServerEvent>) -> Vec<OutboxEntry> {
    let mut entries = Vec::new();
    while let Ok(event) = rx.try_recv() {
        if let ServerEvent::OutboxUpdated {
            instance_slug,
            entry,
        } = event
        {
            assert_eq!(instance_slug, CANONICAL_SLUG);
            entries.push(entry);
        }
    }
    entries
}

fn outcomes(entry: &OutboxEntry) -> Vec<AttemptOutcome> {
    entry
        .attempts
        .iter()
        .map(|attempt| attempt.outcome.clone())
        .collect()
}

#[cfg(unix)]
fn mode(path: &std::path::Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[tokio::test]
async fn a_failed_delivery_is_retried_with_bounded_backoff_and_becomes_visibly_failed() {
    let now = Arc::new(AtomicU64::new(T0));
    let (a, b, wire) = paired(&now).await;
    let (a_id, b_id) = (a.companion_id(), b.companion_id());
    allow(&b, &a_id, IntentClass::Message, DisclosureClass::None);
    let mut events = a.state.events.subscribe();

    // B is unreachable from the start.
    wire.servers.lock().unwrap().remove(ORIGIN_B);
    let entry = enqueue(&a, message(&b, "see you on Friday at the lake"));
    assert_eq!(entry.status, OutboxStatus::Queued);
    assert_eq!(entry.recipient, b_id);
    assert_eq!(entry.intent.sender, a_id);
    assert_eq!(entry.intent.issued_at, T0);
    assert_eq!(entry.intent.expires_at, T0 + OUTBOX_INTENT_LIFETIME_SECS);
    assert!(entry.attempts.is_empty());
    assert_eq!(entry.next_attempt_at, Some(T0), "due at once");
    assert_eq!(entry.chat_id, "default");
    let key = entry.correlation_id().to_owned();
    assert!(key.len() >= 16 && key.len() <= 64, "{key}");
    let queued = broadcast_entries(&mut events);
    assert_eq!(queued.len(), 1, "queueing is broadcast");
    assert_eq!(queued[0].status, OutboxStatus::Queued);
    let path = a.state.federation_outbox.path();
    assert!(path.is_file());
    #[cfg(unix)]
    assert_eq!(mode(&path), 0o600);

    // Every failed attempt waits twice as long as the one before, from
    // the base up to the cap, and each is broadcast with its history.
    let mut expected_next = T0;
    for failure in 1..MAX_DELIVERY_ATTEMPTS {
        assert_eq!(now.load(Ordering::SeqCst), expected_next);
        assert_eq!(run(&a).await, 1, "attempt {failure} is due");
        let entry = a.state.federation_outbox.get(&key).unwrap().unwrap();
        assert_eq!(entry.status, OutboxStatus::Queued, "attempt {failure}");
        assert_eq!(entry.attempts.len() as u32, failure);
        assert_eq!(entry.failures(), failure);
        assert_eq!(
            entry.attempts.last().unwrap().outcome,
            AttemptOutcome::Unreachable {}
        );
        expected_next = now.load(Ordering::SeqCst) + backoff_secs(failure);
        assert_eq!(
            entry.next_attempt_at,
            Some(expected_next),
            "attempt {failure} backs off by {}s",
            backoff_secs(failure)
        );
        assert!(
            entry.response.is_none() && entry.receipt_id.is_none(),
            "nothing is settled while retrying"
        );
        let broadcast = broadcast_entries(&mut events);
        assert!(
            broadcast.last().is_some_and(|last| last == &entry),
            "the last broadcast is the record as stored: {broadcast:?}"
        );
        // Not due yet: a pass in between does nothing.
        now.store(expected_next - 1, Ordering::SeqCst);
        assert_eq!(run(&a).await, 0);
        assert_eq!(a.state.federation_outbox.get(&key).unwrap().unwrap(), entry);
        now.store(expected_next, Ordering::SeqCst);
    }
    assert_eq!(
        backoff_secs(MAX_DELIVERY_ATTEMPTS - 1),
        RETRY_MAX_SECS,
        "the ladder reaches the cap before the last attempt"
    );
    assert_eq!(
        a.state.federation_outbox.next_due().unwrap(),
        Some(expected_next)
    );

    // The last allowed attempt fails too: the entry is visibly failed,
    // with one receipt on this side saying the peer was never reached,
    // and it is never tried again.
    assert_eq!(run(&a).await, 1);
    let failed = a.state.federation_outbox.get(&key).unwrap().unwrap();
    assert_eq!(failed.status, OutboxStatus::Failed);
    assert_eq!(failed.attempts.len() as u32, MAX_DELIVERY_ATTEMPTS);
    assert_eq!(failed.next_attempt_at, None);
    assert!(failed.response.is_none(), "no peer answer was ever seen");
    assert_eq!(failed.updated_at, now.load(Ordering::SeqCst));
    let receipt_id = failed.receipt_id.clone().expect("a receipt is written");
    let broadcast = broadcast_entries(&mut events);
    assert_eq!(
        broadcast.last().map(|entry| entry.status),
        Some(OutboxStatus::Failed)
    );
    let (entries, receipts) = outbox(&a).await;
    assert_eq!(entries, vec![failed.clone()]);
    assert_eq!(receipts.len(), 1, "{receipts:?}");
    let receipt = &receipts[0];
    assert_eq!(receipt.id, receipt_id);
    assert_eq!(receipt.side, ReceiptSide::Requesting);
    assert_eq!(receipt.requester, a_id);
    assert_eq!(receipt.responder, b_id);
    assert_eq!(receipt.correlation_id, key);
    assert_eq!(receipt.intent, IntentClass::Message);
    assert_eq!(receipt.requested, DisclosureClass::None);
    assert_eq!(receipt.granted, DisclosureClass::None);
    assert_eq!(receipt.outcome, IntentOutcome::Denied);
    assert_eq!(
        receipt.basis,
        ReceiptBasis::Policy {
            reason: DecisionReason::Unreachable,
            rule_id: None
        }
    );
    assert_eq!(receipt.represented_owner.to_string(), "Bob");
    assert!(
        !receipt.summary.contains("Bob") && !receipt.summary.contains("lake"),
        "{}",
        receipt.summary
    );
    let lines = requesting_audit(&a).await;
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].intent, "message");
    assert_eq!(lines[0].requester, a_id);
    assert_eq!(lines[0].responder, b_id);
    assert_eq!(lines[0].decision.verdict, Verdict::Deny);
    assert_eq!(lines[0].decision.reason, DecisionReason::Unreachable);

    now.store(now.load(Ordering::SeqCst) + 3600, Ordering::SeqCst);
    assert_eq!(run(&a).await, 0, "a failed entry is never retried");
    assert_eq!(a.state.federation_outbox.next_due().unwrap(), None);
    assert!(broadcast_entries(&mut events).is_empty());

    // B saw nothing, and B back online changes nothing about it.
    wire.servers
        .lock()
        .unwrap()
        .insert(ORIGIN_B.to_owned(), b.state.clone());
    assert_eq!(run(&a).await, 0);
    assert!(chat_messages(&b).is_empty());
    let (records, receipts) = inbox(&b).await;
    assert!(records.is_empty() && receipts.is_empty());
    assert!(requesting_audit(&b).await.is_empty());
}

#[tokio::test]
async fn a_status_from_something_in_front_of_the_peer_is_a_transient_failure_not_a_verdict() {
    let now = Arc::new(AtomicU64::new(T0));
    let (a, b, wire) = paired(&now).await;
    let (a_id, b_id) = (a.companion_id(), b.companion_id());
    allow(&b, &a_id, IntentClass::Message, DisclosureClass::None);
    let mut events = a.state.events.subscribe();
    let key = enqueue(&a, message(&b, "see you on Friday at the lake"))
        .correlation_id()
        .to_owned();

    // A reverse proxy's or a tunnel's own error page (no typed code in
    // the body), a rate limit imposed in front of the peer, and a proxy
    // that does not route the path: each is one failed attempt with the
    // status on record, backed off like an unreachable peer, never a
    // verdict on the request.
    let mut expected_next = T0;
    for (failure, (status, body)) in [
        (502u16, "<html><body><h1>502 Bad Gateway</h1></body></html>"),
        (429, "<html><body>Too Many Requests</body></html>"),
        (404, "<html><body>Not Found</body></html>"),
        (503, r#"{"error":"federation_unavailable","message":"..."}"#),
        (500, r#"{"error":"Internal Server Error"}"#),
    ]
    .into_iter()
    .enumerate()
    {
        let failure = failure as u32 + 1;
        wire.answered_in_front
            .lock()
            .unwrap()
            .insert(ORIGIN_B.to_owned(), (status, body.to_owned()));
        now.store(expected_next, Ordering::SeqCst);
        assert_eq!(run(&a).await, 1, "attempt {failure} is due");
        let entry = a.state.federation_outbox.get(&key).unwrap().unwrap();
        assert_eq!(entry.status, OutboxStatus::Queued, "HTTP {status}");
        assert_eq!(entry.failures(), failure, "HTTP {status} counts");
        let code = match status {
            503 => "federation_unavailable",
            _ => "unknown",
        };
        assert_eq!(
            entry.attempts.last().unwrap().outcome,
            AttemptOutcome::Refused {
                status: Some(status),
                code: code.into()
            },
            "the record says what answered"
        );
        expected_next = now.load(Ordering::SeqCst) + backoff_secs(failure);
        assert_eq!(entry.next_attempt_at, Some(expected_next), "HTTP {status}");
        assert!(entry.response.is_none() && entry.receipt_id.is_none());
        assert!(outbox(&a).await.1.is_empty(), "no receipt while retrying");
        assert_eq!(
            broadcast_entries(&mut events).last(),
            Some(&entry),
            "HTTP {status}"
        );
    }
    assert!(chat_messages(&b).is_empty(), "nothing reached B");
    assert!(requesting_audit(&a).await.is_empty());

    // The proxy is fixed: the next attempt reaches B and is delivered
    // once, with one receipt.
    wire.answered_in_front.lock().unwrap().clear();
    now.store(expected_next, Ordering::SeqCst);
    assert_eq!(run(&a).await, 1);
    let delivered = a.state.federation_outbox.get(&key).unwrap().unwrap();
    assert_eq!(delivered.status, OutboxStatus::Delivered);
    assert_eq!(delivered.attempts.len(), 6);
    assert_eq!(chat_messages(&b).len(), 1);
    let (_, receipts) = outbox(&a).await;
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].outcome, IntentOutcome::Accepted);

    // A typed refusal from the peer's own route that will not change (a
    // 4xx naming a condition) fails the entry at once, as `peer_refused`,
    // with the status and the code on record.
    wire.answered_in_front.lock().unwrap().insert(
        ORIGIN_B.to_owned(),
        (
            403,
            r#"{"error":"policy_denied","message":"..."}"#.to_owned(),
        ),
    );
    now.store(expected_next + 1, Ordering::SeqCst);
    let refused = enqueue(&a, message(&b, "one more thing"))
        .correlation_id()
        .to_owned();
    assert_eq!(run(&a).await, 1);
    let failed = a.state.federation_outbox.get(&refused).unwrap().unwrap();
    assert_eq!(failed.status, OutboxStatus::Failed);
    assert_eq!(
        outcomes(&failed),
        [AttemptOutcome::Refused {
            status: Some(403),
            code: "policy_denied".into()
        }]
    );
    assert_eq!(failed.next_attempt_at, None);
    assert!(failed.response.is_none());
    let (_, receipts) = outbox(&a).await;
    assert_eq!(receipts.len(), 2);
    assert_eq!(receipts[0].correlation_id, refused);
    assert_eq!(receipts[0].outcome, IntentOutcome::Denied);
    assert_eq!(
        receipts[0].basis,
        ReceiptBasis::Policy {
            reason: DecisionReason::PeerRefused,
            rule_id: None
        }
    );
    let lines = requesting_audit(&a).await;
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1].decision.reason, DecisionReason::PeerRefused);
    assert_eq!(chat_messages(&b).len(), 1, "B saw nothing more");
    assert_eq!(b_id, failed.recipient);
}

#[tokio::test]
async fn a_peer_over_its_rate_limit_is_asked_again_with_a_growing_wait_and_still_bounds_the_entry()
{
    let now = Arc::new(AtomicU64::new(T0));
    let (a, b, _wire) = paired(&now).await;
    let (a_id, b_id) = (a.companion_id(), b.companion_id());
    allow(&b, &a_id, IntentClass::Message, DisclosureClass::None);
    // B admits nothing from A for now: every request is answered with
    // B's rate limit and how long its window has left.
    const WINDOW: u64 = 45;
    b.state
        .federation_gate
        .update_policy(|document| {
            document.peers.entry(a_id.clone()).or_default().rate_limit =
                Some(crate::domain::federation_policy::RateLimitPolicy {
                    max_requests: 0,
                    window_secs: WINDOW,
                });
            Ok(())
        })
        .unwrap();
    let mut events = a.state.events.subscribe();
    let key = enqueue(&a, message(&b, "see you on Friday at the lake"))
        .correlation_id()
        .to_owned();

    // Each rate-limited answer is a failed attempt: the entry waits out
    // the larger of B's window and the backoff, which grows, and B's
    // typed answer is kept, with no receipt yet.
    let mut expected_next = T0;
    for failure in 1..MAX_DELIVERY_ATTEMPTS {
        now.store(expected_next, Ordering::SeqCst);
        assert_eq!(run(&a).await, 1, "attempt {failure} is due");
        let entry = a.state.federation_outbox.get(&key).unwrap().unwrap();
        assert_eq!(entry.status, OutboxStatus::Queued, "attempt {failure}");
        assert_eq!(entry.failures(), failure, "a rate-limited answer counts");
        assert_eq!(
            entry.attempts.last().unwrap().outcome,
            AttemptOutcome::Answered {
                outcome: IntentOutcome::Denied,
                reason: Some(DecisionReason::RateLimited)
            }
        );
        assert!(
            matches!(
                &entry.response,
                Some(IntentResponse::Denied {
                    reason: DecisionReason::RateLimited,
                    retry_after_secs: Some(WINDOW),
                    responder,
                    ..
                }) if responder == &b_id
            ),
            "{:?}",
            entry.response
        );
        expected_next = now.load(Ordering::SeqCst) + backoff_secs(failure).max(WINDOW);
        assert_eq!(
            entry.next_attempt_at,
            Some(expected_next),
            "attempt {failure} waits out the larger of the window and the backoff"
        );
        assert!(entry.receipt_id.is_none());
        now.store(expected_next - 1, Ordering::SeqCst);
        assert_eq!(run(&a).await, 0, "not due yet");
    }
    assert!(
        (2..MAX_DELIVERY_ATTEMPTS).any(|n| backoff_secs(n) > WINDOW),
        "the backoff outgrows the window"
    );
    assert!(outbox(&a).await.1.is_empty());
    assert!(requesting_audit(&a).await.is_empty());

    // Past the budget: visibly failed, with one receipt naming B's rate
    // limit as the reason and B's last typed answer kept, never retried.
    now.store(expected_next, Ordering::SeqCst);
    assert_eq!(run(&a).await, 1);
    let failed = a.state.federation_outbox.get(&key).unwrap().unwrap();
    assert_eq!(failed.status, OutboxStatus::Failed);
    assert_eq!(failed.attempts.len() as u32, MAX_DELIVERY_ATTEMPTS);
    assert_eq!(failed.next_attempt_at, None);
    assert!(
        matches!(
            &failed.response,
            Some(IntentResponse::Denied {
                reason: DecisionReason::RateLimited,
                ..
            })
        ),
        "{:?}",
        failed.response
    );
    let (_, receipts) = outbox(&a).await;
    assert_eq!(receipts.len(), 1, "{receipts:?}");
    assert_eq!(receipts[0].id, failed.receipt_id.clone().unwrap());
    assert_eq!(receipts[0].outcome, IntentOutcome::Denied);
    assert_eq!(receipts[0].responder, b_id);
    assert_eq!(
        receipts[0].basis,
        ReceiptBasis::Policy {
            reason: DecisionReason::RateLimited,
            rule_id: None
        }
    );
    let lines = requesting_audit(&a).await;
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].decision.verdict, Verdict::Deny);
    assert_eq!(lines[0].decision.reason, DecisionReason::RateLimited);
    assert_eq!(
        broadcast_entries(&mut events)
            .last()
            .map(|entry| entry.status),
        Some(OutboxStatus::Failed)
    );
    now.store(expected_next + 24 * 60 * 60, Ordering::SeqCst);
    assert_eq!(run(&a).await, 0, "never retried");
    assert!(chat_messages(&b).is_empty(), "nothing reached B");
    let (records, _) = inbox(&b).await;
    assert!(
        records.is_empty(),
        "a rate-limited request is not settled on B"
    );
}

#[tokio::test]
async fn an_entry_that_expires_while_its_peers_owner_never_decided_says_so() {
    let now = Arc::new(AtomicU64::new(T0));
    let (a, b, _wire) = paired(&now).await;
    let mut events = a.state.events.subscribe();

    // No rule on B: B's owner has to answer, and denies once. B keeps
    // answering `needs_owner` (#219 keeps the owner's word off the wire),
    // so on A the request waits until its intent expires.
    let key = enqueue(&a, message(&b, "coffee on Saturday?"))
        .correlation_id()
        .to_owned();
    assert_eq!(run(&a).await, 1);
    let entry = a.state.federation_outbox.get(&key).unwrap().unwrap();
    assert_eq!(entry.status, OutboxStatus::WaitingOwner);
    let first_receipt = entry.receipt_id.clone().expect("a receipt for the wait");
    let (status, body) = b
        .owner(Method::GET, "/api/federation/approvals", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let approval_id = body["approvals"][0]["id"].as_str().unwrap().to_owned();
    let (status, body) = b
        .owner(
            Method::POST,
            &format!("/api/federation/approvals/{approval_id}/deny"),
            Some(json!({"scope": "once"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["approval"]["status"], "denied");
    now.store(T0 + OWNER_RETRY_SECS, Ordering::SeqCst);
    assert_eq!(run(&a).await, 1);
    let entry = a.state.federation_outbox.get(&key).unwrap().unwrap();
    assert_eq!(entry.status, OutboxStatus::WaitingOwner, "B says the same");
    assert_eq!(entry.failures(), 0);
    assert_eq!(entry.receipt_id.as_deref(), Some(first_receipt.as_str()));

    // Expired while waiting: settled once as such, with a receipt whose
    // reason is the one B gave for asking its owner, not `unreachable`,
    // and B's last typed answer kept on the entry.
    now.store(
        T0 + OUTBOX_INTENT_LIFETIME_SECS + MAX_INTENT_CLOCK_SKEW_SECS + 1,
        Ordering::SeqCst,
    );
    assert_eq!(run(&a).await, 1);
    let expired = a.state.federation_outbox.get(&key).unwrap().unwrap();
    assert_eq!(expired.status, OutboxStatus::Expired);
    assert_eq!(expired.attempts.len(), 2, "no attempt past the expiry");
    assert_eq!(expired.next_attempt_at, None);
    assert!(
        matches!(
            &expired.response,
            Some(IntentResponse::NeedsOwner {
                reason: DecisionReason::Default,
                ..
            })
        ),
        "{:?}",
        expired.response
    );
    let receipt_id = expired.receipt_id.clone().unwrap();
    assert_ne!(receipt_id, first_receipt);
    let (_, receipts) = outbox(&a).await;
    assert_eq!(receipts.len(), 2, "{receipts:?}");
    assert_eq!(receipts[0].id, receipt_id);
    assert_eq!(receipts[0].outcome, IntentOutcome::Denied);
    assert_eq!(receipts[0].responder, b.companion_id());
    assert_eq!(
        receipts[0].basis,
        ReceiptBasis::Policy {
            reason: DecisionReason::Default,
            rule_id: None
        },
        "the reason is the peer's own word, not unreachable"
    );
    assert_eq!(receipts[1].outcome, IntentOutcome::NeedsOwner);
    let lines = requesting_audit(&a).await;
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].decision.verdict, Verdict::Ask);
    assert_eq!(lines[1].decision.verdict, Verdict::Deny);
    assert_eq!(lines[1].decision.reason, DecisionReason::Default);
    assert_eq!(
        broadcast_entries(&mut events)
            .last()
            .map(|entry| entry.status),
        Some(OutboxStatus::Expired)
    );
    assert_eq!(run(&a).await, 0);
    assert!(chat_messages(&b).is_empty());
}

#[tokio::test]
async fn an_interrupted_delivery_finishes_once_after_a_restart_and_the_peer_never_sees_it_twice() {
    let now = Arc::new(AtomicU64::new(T0));
    let (a, b, wire) = paired(&now).await;
    let (a_id, b_id) = (a.companion_id(), b.companion_id());
    allow(&b, &a_id, IntentClass::Message, DisclosureClass::None);

    // B handles the intent in full, but its answer never reaches A.
    wire.lost_answers
        .lock()
        .unwrap()
        .insert(ORIGIN_B.to_owned());
    let key = enqueue(&a, message(&b, "the lake, Friday, bring the kite"))
        .correlation_id()
        .to_owned();
    assert_eq!(run(&a).await, 1);
    let entry = a.state.federation_outbox.get(&key).unwrap().unwrap();
    assert_eq!(entry.status, OutboxStatus::Queued);
    assert_eq!(outcomes(&entry), [AttemptOutcome::Unreachable {}]);
    assert_eq!(chat_messages(&b).len(), 1, "B delivered it");
    let (records, receipts) = inbox(&b).await;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, InboundStatus::Accepted);
    assert_eq!(records[0].correlation_id, key);
    assert_eq!(receipts.len(), 1);
    assert!(
        requesting_audit(&a).await.is_empty(),
        "nothing settled on A"
    );

    // The process dies during the next attempt: the file holds the
    // attempt as in flight. A fresh process over the same workspace marks
    // it interrupted and retries on the same correlation id.
    let path = a.state.federation_outbox.path();
    let file = std::fs::read_to_string(&path).unwrap();
    assert!(file.contains("\"unreachable\""), "{file}");
    std::fs::write(&path, file.replacen("\"unreachable\"", "\"in_flight\"", 1)).unwrap();
    let fresh = Outbox::with_transport_and_clock(a.workspace.path(), wire.clone(), clock(&now))
        .with_events(a.state.events.clone(), CANONICAL_SLUG);
    let mut events = a.state.events.subscribe();
    assert_eq!(fresh.recover_on_restart().unwrap(), 1);
    let recovered = fresh.get(&key).unwrap().unwrap();
    assert_eq!(recovered.status, OutboxStatus::Queued);
    assert_eq!(outcomes(&recovered), [AttemptOutcome::Interrupted {}]);
    assert_eq!(
        recovered.correlation_id(),
        key,
        "the same request, not a new one"
    );
    assert_eq!(fresh.recover_on_restart().unwrap(), 0, "idempotent");
    assert_eq!(
        broadcast_entries(&mut events).len(),
        1,
        "the recovered entry is broadcast"
    );
    assert_eq!(fresh.get(&key).unwrap().unwrap().attempts.len(), 1);

    // The wire is whole again; when the retry is due it lands on B's
    // record of the first delivery and is answered from it.
    wire.lost_answers.lock().unwrap().clear();
    now.store(recovered.next_attempt_at.unwrap(), Ordering::SeqCst);
    assert_eq!(
        fresh
            .run_due(&a.state.federation, &a.state.federation_gate)
            .await
            .unwrap(),
        1
    );
    let delivered = fresh.get(&key).unwrap().unwrap();
    assert_eq!(delivered.status, OutboxStatus::Delivered);
    assert_eq!(
        outcomes(&delivered),
        [
            AttemptOutcome::Interrupted {},
            AttemptOutcome::Answered {
                outcome: IntentOutcome::Accepted,
                reason: None
            }
        ]
    );
    assert_eq!(delivered.next_attempt_at, None);
    assert!(
        matches!(
            &delivered.response,
            Some(IntentResponse::Accepted { answer: IntentAnswer::Delivered {}, disclosure: DisclosureClass::None, responder, .. }) if responder == &b_id
        ),
        "{:?}",
        delivered.response
    );
    let view = fresh.view().unwrap();
    assert_eq!(view.receipts.len(), 1, "{:?}", view.receipts);
    assert_eq!(view.receipts[0].outcome, IntentOutcome::Accepted);
    assert_eq!(view.receipts[0].granted, DisclosureClass::None);
    assert_eq!(view.receipts[0].requested, DisclosureClass::None);
    assert_eq!(
        delivered.receipt_id.as_deref(),
        Some(view.receipts[0].id.as_str())
    );
    assert_eq!(
        broadcast_entries(&mut events)
            .last()
            .map(|entry| entry.status),
        Some(OutboxStatus::Delivered)
    );
    let lines = requesting_audit(&a).await;
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].decision.verdict, Verdict::Allow);

    // B: still one message, one record, one receipt.
    let messages = chat_messages(&b);
    assert_eq!(messages.len(), 1, "delivered exactly once: {messages:?}");
    assert_eq!(messages[0].role, ChatRole::User);
    assert!(messages[0].content.contains("bring the kite"));
    let (records, receipts) = inbox(&b).await;
    assert_eq!(records.len(), 1);
    assert_eq!(receipts.len(), 1);
    assert_eq!(
        audit(&b)
            .await
            .iter()
            .filter(|receipt| receipt.side == ReceiptSide::Answering && receipt.intent == "message")
            .count(),
        1,
        "judged once"
    );
    assert_eq!(
        fresh
            .run_due(&a.state.federation, &a.state.federation_gate)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn the_sending_tools_are_gated_by_this_owners_policy_and_queue_typed_intents() {
    let now = Arc::new(AtomicU64::new(T0));
    let (a, b, wire) = paired(&now).await;
    let (a_id, b_id) = (a.companion_id(), b.companion_id());
    // C accepted an invite from A, which A has not confirmed: pending on
    // A's side, with keys exchanged and an origin on record.
    let c = start_in(
        &wire,
        TOKEN_A,
        "http://c.test",
        tempfile::tempdir().unwrap(),
        clock(&now),
    )
    .await;
    let invite = mint_invite(&a).await;
    let (status, body) = c
        .owner(
            Method::POST,
            "/api/federation/accept",
            Some(accept_body(&invite)),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let c_id = c.companion_id();
    assert_eq!(
        a.state
            .federation
            .overview()
            .unwrap()
            .peers
            .iter()
            .find(|peer| peer.companion_id == c_id)
            .map(|peer| peer.state),
        Some(crate::domain::federation::PeerState::Pending)
    );
    let send = tool(&a, "send_peer_message");
    let names: Vec<String> =
        federation_tools(sending(&a), a.workspace.path(), CANONICAL_SLUG, "default")
            .into_iter()
            .map(|tool| tool.name())
            .collect();
    assert_eq!(
        names,
        [
            "send_peer_message",
            "ask_peer_availability",
            "propose_peer_reminder"
        ]
    );
    let path = a.state.federation_outbox.path();
    let args = |peer: &str| {
        json!({
            "peer": peer,
            "text": "are we still on for Friday?",
            "on_behalf_of": "Bob",
            "purpose": "plans for the weekend",
        })
        .to_string()
    };

    // A stranger, a prefix too short to name anyone, a pending peer (by
    // its id and by a prefix), and a peer the owner denied: refused with
    // a word on why, and nothing is queued, written, or broadcast. (A
    // revoked peer is refused the same way below, in
    // `each_answer_settles_or_holds_the_entry...`.)
    let mut events = a.state.events.subscribe();
    let stranger = "SN7Fvp7FYlYfvTUUW4vPFzy1jh9gtfDVlA7I3_QSj4U";
    for (peer, expect) in [
        (stranger.to_owned(), "unknown"),
        (b_id[..5].to_owned(), "unknown"),
        (c_id.clone(), "not paired"),
        (c_id[..8].to_owned(), "not paired"),
    ] {
        let refused = send.call(args(&peer)).await.unwrap_err().to_string();
        assert!(refused.to_lowercase().contains(expect), "{peer}: {refused}");
    }
    assert!(chat_messages(&c).is_empty());
    rule(
        &a,
        &b_id,
        IntentClass::Message,
        DisclosureClass::None,
        Access::Deny,
    );
    let refused = send.call(args(&b_id)).await.unwrap_err().to_string();
    assert!(refused.to_lowercase().contains("denied"), "{refused}");
    // An `ask` rule and the defaults do not refuse: the owner asked here.
    rule(
        &a,
        &b_id,
        IntentClass::Message,
        DisclosureClass::None,
        Access::Ask,
    );
    assert!(!path.exists(), "nothing was queued: {}", path.display());
    assert!(broadcast_entries(&mut events).is_empty());
    assert!(a.state.federation_outbox.view().unwrap().entries.is_empty());
    assert!(requesting_audit(&a).await.is_empty());

    // A valid call queues one typed intent for B, named by a prefix of
    // its id, and answers with the queued request and nothing else.
    allow(&b, &a_id, IntentClass::Message, DisclosureClass::None);
    let answer: serde_json::Value =
        serde_json::from_str(&send.call(args(&b_id[..8])).await.unwrap()).unwrap();
    assert_eq!(answer["status"], "queued", "{answer}");
    assert_eq!(answer["peer"], b_id);
    assert_eq!(answer["kind"], "message");
    let key = answer["request_id"]
        .as_str()
        .expect("the request id")
        .to_owned();
    assert_eq!(answer["expires_at"], T0 + OUTBOX_INTENT_LIFETIME_SECS);
    assert!(
        answer.get("text").is_none()
            && answer.get("body").is_none()
            && answer.get("response").is_none(),
        "{answer}"
    );
    let entry = a.state.federation_outbox.get(&key).unwrap().unwrap();
    assert_eq!(entry.status, OutboxStatus::Queued);
    assert_eq!(entry.recipient, b_id);
    assert_eq!(entry.intent.sender, a_id);
    assert_eq!(entry.intent.disclosure, DisclosureClass::None);
    assert_eq!(entry.intent.represented_owner.to_string(), "Bob");
    assert_eq!(entry.intent.purpose.to_string(), "plans for the weekend");
    assert!(matches!(entry.intent.intent, IntentPayload::Message { .. }));
    assert_eq!(entry.chat_id, "default");
    assert_eq!(broadcast_entries(&mut events).len(), 1);

    // Delivered on the next pass: B's conversation carries the text
    // inside the untrusted block from A's id, and A's receipt names what
    // was asked and what B disclosed.
    assert_eq!(run(&a).await, 1);
    let delivered = a.state.federation_outbox.get(&key).unwrap().unwrap();
    assert_eq!(delivered.status, OutboxStatus::Delivered);
    let messages = chat_messages(&b);
    assert_eq!(messages.len(), 1);
    assert!(messages[0].content.contains(&a_id));
    assert!(messages[0].content.contains("still on for Friday"));
    let (_, receipts) = outbox(&a).await;
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].outcome, IntentOutcome::Accepted);
    assert_eq!(receipts[0].requested, DisclosureClass::None);
    assert_eq!(receipts[0].granted, DisclosureClass::None);
    assert_eq!(receipts[0].requester, a_id);
    assert_eq!(receipts[0].responder, b_id);

    // The availability tool asks at the availability class with a window
    // in this companion's time; B has no calendar, so it answers with no
    // windows at `none`, which is what the receipt records as granted.
    allow(
        &b,
        &a_id,
        IntentClass::Availability,
        DisclosureClass::Availability,
    );
    let ask = tool(&a, "ask_peer_availability");
    let answer: serde_json::Value = serde_json::from_str(
        &ask.call(
            json!({
                "peer": b_id,
                "from": "2027-01-16T09:00:00Z",
                "to": "2027-01-16T11:00:00Z",
                "on_behalf_of": "Bob",
                "purpose": "find a slot",
            })
            .to_string(),
        )
        .await
        .unwrap(),
    )
    .unwrap();
    assert_eq!(answer["kind"], "availability", "{answer}");
    let key = answer["request_id"].as_str().unwrap().to_owned();
    let entry = a.state.federation_outbox.get(&key).unwrap().unwrap();
    assert_eq!(entry.intent.disclosure, DisclosureClass::Availability);
    assert!(
        matches!(
            entry.intent.intent,
            IntentPayload::Availability { window } if window.from == 1_800_090_000 && window.to == 1_800_097_200
        ),
        "{:?}",
        entry.intent.intent
    );
    assert_eq!(run(&a).await, 1);
    let entry = a.state.federation_outbox.get(&key).unwrap().unwrap();
    assert_eq!(entry.status, OutboxStatus::Delivered);
    assert!(
        matches!(
            &entry.response,
            Some(IntentResponse::Accepted { answer: IntentAnswer::Availability { windows }, disclosure: DisclosureClass::None, .. }) if windows.is_empty()
        ),
        "{:?}",
        entry.response
    );
    let (_, receipts) = outbox(&a).await;
    assert_eq!(receipts[0].correlation_id, key);
    assert_eq!(receipts[0].requested, DisclosureClass::Availability);
    assert_eq!(receipts[0].granted, DisclosureClass::None);

    // The reminder tool asks for a reminder at a time in the future; B
    // sets it as a commitment and answers `reminder_scheduled`.
    allow(&b, &a_id, IntentClass::Reminder, DisclosureClass::None);
    let propose = tool(&a, "propose_peer_reminder");
    let behind = propose
        .call(
            json!({
                "peer": b_id,
                "text": "water the plants",
                "at": "2027-01-15T07:00:00Z",
                "on_behalf_of": "Bob",
                "purpose": "a favour",
            })
            .to_string(),
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(behind.to_lowercase().contains("reminder"), "{behind}");
    let answer: serde_json::Value = serde_json::from_str(
        &propose
            .call(
                json!({
                    "peer": b_id,
                    "text": "water the plants",
                    "at": "2027-01-15T12:00:00Z",
                    "on_behalf_of": "Bob",
                    "purpose": "a favour",
                })
                .to_string(),
            )
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(answer["kind"], "reminder", "{answer}");
    let key = answer["request_id"].as_str().unwrap().to_owned();
    assert_eq!(run(&a).await, 1);
    let entry = a.state.federation_outbox.get(&key).unwrap().unwrap();
    assert_eq!(entry.status, OutboxStatus::Delivered);
    assert!(
        matches!(
            &entry.response,
            Some(IntentResponse::Accepted {
                answer: IntentAnswer::ReminderScheduled { at: 1_800_014_400 },
                ..
            })
        ),
        "{:?}",
        entry.response
    );
    let commitments = b.state.commitments.list(ListFilter::default(), T0 as i64);
    assert_eq!(commitments.len(), 1, "{commitments:?}");
    assert!(!commitments[0].promise.contains("water"));

    // Three requests, three receipts on A, all requesting side, and one
    // audit line each; B judged each once.
    let (entries, receipts) = outbox(&a).await;
    assert_eq!(entries.len(), 3);
    assert!(
        entries
            .iter()
            .all(|entry| entry.status == OutboxStatus::Delivered)
    );
    assert_eq!(receipts.len(), 3);
    assert!(
        receipts
            .iter()
            .all(|receipt| receipt.side == ReceiptSide::Requesting)
    );
    assert_eq!(requesting_audit(&a).await.len(), 3);
    assert_eq!(inbox(&b).await.0.len(), 3);
}

#[tokio::test]
async fn each_answer_settles_or_holds_the_entry_and_leaves_one_receipt_per_outcome() {
    let now = Arc::new(AtomicU64::new(T0));
    let (a, b, wire) = paired(&now).await;
    let a_id = a.companion_id();
    let mut events = a.state.events.subscribe();

    // No rule on B: B's owner has to answer. The entry waits, asks again
    // every OWNER_RETRY_SECS, and one receipt says so until the owner
    // approves once, after which it is delivered exactly once.
    let waiting = enqueue(&a, message(&b, "coffee on Saturday?"))
        .correlation_id()
        .to_owned();
    assert_eq!(run(&a).await, 1);
    let entry = a.state.federation_outbox.get(&waiting).unwrap().unwrap();
    assert_eq!(entry.status, OutboxStatus::WaitingOwner);
    assert_eq!(entry.next_attempt_at, Some(T0 + OWNER_RETRY_SECS));
    assert_eq!(
        outcomes(&entry),
        [AttemptOutcome::Answered {
            outcome: IntentOutcome::NeedsOwner,
            reason: Some(DecisionReason::Default)
        }]
    );
    assert!(
        matches!(
            &entry.response,
            Some(IntentResponse::NeedsOwner {
                reason: DecisionReason::Default,
                ..
            })
        ),
        "{:?}",
        entry.response
    );
    let receipt_id = entry.receipt_id.clone().expect("a receipt for needs_owner");
    let (_, receipts) = outbox(&a).await;
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].id, receipt_id);
    assert_eq!(receipts[0].outcome, IntentOutcome::NeedsOwner);
    assert_eq!(
        receipts[0].basis,
        ReceiptBasis::Policy {
            reason: DecisionReason::Default,
            rule_id: None
        }
    );
    let lines = requesting_audit(&a).await;
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].decision.verdict, Verdict::Ask);
    assert_eq!(
        broadcast_entries(&mut events)
            .last()
            .map(|entry| entry.status),
        Some(OutboxStatus::WaitingOwner)
    );
    assert!(chat_messages(&b).is_empty());

    // Asked again while the owner has not decided: still waiting, the
    // attempt recorded, no second receipt, no failure counted.
    now.store(T0 + OWNER_RETRY_SECS, Ordering::SeqCst);
    assert_eq!(run(&a).await, 1);
    let entry = a.state.federation_outbox.get(&waiting).unwrap().unwrap();
    assert_eq!(entry.status, OutboxStatus::WaitingOwner);
    assert_eq!(entry.attempts.len(), 2);
    assert_eq!(entry.failures(), 0);
    assert_eq!(entry.next_attempt_at, Some(T0 + 2 * OWNER_RETRY_SECS));
    assert_eq!(entry.receipt_id.as_deref(), Some(receipt_id.as_str()));
    assert_eq!(outbox(&a).await.1.len(), 1);
    assert_eq!(requesting_audit(&a).await.len(), 1);

    // B's owner approves once.
    let (status, body) = b
        .owner(Method::GET, "/api/federation/approvals", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let approval_id = body["approvals"][0]["id"]
        .as_str()
        .expect("A's request waits in B's queue")
        .to_owned();
    let (status, body) = b
        .owner(
            Method::POST,
            &format!("/api/federation/approvals/{approval_id}/approve"),
            Some(json!({"scope": "once"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    now.store(T0 + 2 * OWNER_RETRY_SECS, Ordering::SeqCst);
    assert_eq!(run(&a).await, 1);
    let entry = a.state.federation_outbox.get(&waiting).unwrap().unwrap();
    assert_eq!(entry.status, OutboxStatus::Delivered);
    assert_eq!(entry.attempts.len(), 3);
    assert_eq!(entry.next_attempt_at, None);
    assert_ne!(entry.receipt_id.as_deref(), Some(receipt_id.as_str()));
    let (_, receipts) = outbox(&a).await;
    assert_eq!(receipts.len(), 2, "{receipts:?}");
    assert_eq!(receipts[0].outcome, IntentOutcome::Accepted);
    assert_eq!(receipts[1].outcome, IntentOutcome::NeedsOwner);
    assert_eq!(chat_messages(&b).len(), 1);
    let lines = requesting_audit(&a).await;
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1].decision.verdict, Verdict::Allow);
    now.store(T0 + 3 * OWNER_RETRY_SECS, Ordering::SeqCst);
    assert_eq!(run(&a).await, 0, "settled");

    // A rule denying messages on B: settled as denied with B's reason.
    rule(
        &b,
        &a_id,
        IntentClass::Message,
        DisclosureClass::None,
        Access::Deny,
    );
    let denied = enqueue(&a, message(&b, "one more thing"))
        .correlation_id()
        .to_owned();
    assert_eq!(run(&a).await, 1);
    let entry = a.state.federation_outbox.get(&denied).unwrap().unwrap();
    assert_eq!(entry.status, OutboxStatus::Denied);
    assert_eq!(entry.next_attempt_at, None);
    assert!(
        matches!(
            &entry.response,
            Some(IntentResponse::Denied {
                reason: DecisionReason::Rule,
                ..
            })
        ),
        "{:?}",
        entry.response
    );
    let (_, receipts) = outbox(&a).await;
    assert_eq!(receipts[0].correlation_id, denied);
    assert_eq!(receipts[0].outcome, IntentOutcome::Denied);
    assert_eq!(
        receipts[0].basis,
        ReceiptBasis::Policy {
            reason: DecisionReason::Rule,
            rule_id: None
        }
    );
    assert_eq!(chat_messages(&b).len(), 1, "nothing more reached B");
    assert_eq!(
        requesting_audit(&a).await[2].decision.reason,
        DecisionReason::Rule
    );

    // A peer refusing with a code that will not change (here: A's intent
    // names a sender B does not know, because B revoked A) fails at once.
    let (status, body) = b
        .owner(
            Method::POST,
            &format!("/api/federation/peers/{a_id}/revoke"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // A's record of B was revoked by B's notice: the own gate refuses.
    let refused = a
        .state
        .federation_outbox
        .enqueue(
            &a.state.federation,
            &a.state.federation_gate,
            message(&b, "still there?"),
        )
        .unwrap_err();
    assert!(
        matches!(
            refused,
            crate::domain::federation::FederationError::PeerRevoked
        ),
        "{refused:?}"
    );

    // An intent that expires before it could be delivered (B was
    // unreachable the whole time) is settled as expired, once, with a
    // receipt, and never sent afterwards.
    let (a, b, wire2) = paired(&now).await;
    let _ = wire;
    let mut events = a.state.events.subscribe();
    allow(
        &b,
        &a.companion_id(),
        IntentClass::Message,
        DisclosureClass::None,
    );
    wire2.servers.lock().unwrap().remove(ORIGIN_B);
    let at = now.load(Ordering::SeqCst);
    let expiring = enqueue(&a, message(&b, "before the weekend"))
        .correlation_id()
        .to_owned();
    assert_eq!(run(&a).await, 1);
    now.store(
        at + OUTBOX_INTENT_LIFETIME_SECS + MAX_INTENT_CLOCK_SKEW_SECS + 1,
        Ordering::SeqCst,
    );
    wire2
        .servers
        .lock()
        .unwrap()
        .insert(ORIGIN_B.to_owned(), b.state.clone());
    assert_eq!(run(&a).await, 1);
    let entry = a.state.federation_outbox.get(&expiring).unwrap().unwrap();
    assert_eq!(entry.status, OutboxStatus::Expired);
    assert_eq!(entry.attempts.len(), 1, "no attempt past the expiry");
    assert_eq!(entry.next_attempt_at, None);
    let (_, receipts) = outbox(&a).await;
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].outcome, IntentOutcome::Denied);
    assert_eq!(
        receipts[0].basis,
        ReceiptBasis::Policy {
            reason: DecisionReason::Unreachable,
            rule_id: None
        }
    );
    assert_eq!(
        broadcast_entries(&mut events)
            .last()
            .map(|entry| entry.status),
        Some(OutboxStatus::Expired)
    );
    assert_eq!(run(&a).await, 0);
    assert!(chat_messages(&b).is_empty());
}

#[tokio::test]
async fn the_outbox_listing_needs_the_owner_and_lists_newest_first() {
    let now = Arc::new(AtomicU64::new(T0));
    let (a, b, _wire) = paired(&now).await;
    let (status, body) = a
        .anonymous(Method::GET, "/api/federation/outbox", None, &[])
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    let (status, body) = a
        .anonymous(
            Method::GET,
            "/api/federation/outbox",
            None,
            &[("authorization", &format!("Bearer {TOKEN_B}"))],
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");

    let first = enqueue(&a, message(&b, "first"))
        .correlation_id()
        .to_owned();
    now.store(T0 + 1, Ordering::SeqCst);
    let second = enqueue(&a, message(&b, "second"))
        .correlation_id()
        .to_owned();
    let (entries, receipts) = outbox(&a).await;
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.correlation_id().to_owned())
            .collect::<Vec<_>>(),
        [second, first]
    );
    assert!(receipts.is_empty());
    let (status, body) = a.owner(Method::GET, "/api/federation/outbox", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body["entries"][0]["intent"]["intent"]["body"]
            .as_str()
            .is_some_and(|text| text == "second"),
        "the owner sees their own words: {body}"
    );
}
