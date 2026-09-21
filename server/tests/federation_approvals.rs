//! Guards for #109 (PR 3): the pending-approval queue never sees a payload,
//! the gate consults it only after the engine asked and never for a ping,
//! the owner API is post/delete under the owner router with both revoke
//! paths dropping what a revoked peer had, a rotation carries the queue
//! along, the Companions section keeps nothing in the browser, and the
//! docs describe the controls.

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

fn read(relative: &str) -> String {
    fs::read_to_string(repo().join(relative)).unwrap_or_else(|_| panic!("{relative} is missing"))
}

fn production(relative: &str) -> String {
    without_cfg_test_items(&read(relative))
}

/// The body of `fn <name>(` up to the next method of the impl.
fn method<'a>(source: &'a str, name: &str) -> &'a str {
    source
        .split(&format!("fn {name}("))
        .nth(1)
        .and_then(|rest| rest.split("\n    pub ").next())
        .unwrap_or_else(|| panic!("{name} exists"))
}

#[test]
fn the_approval_queue_never_touches_a_payload_and_entries_have_no_room_for_one() {
    let store = production("server/src/services/federation/approvals.rs");
    for payload in [
        "PeerText",
        "render_untrusted",
        "TransportEnvelope",
        "TransportMessage",
        "SignedEnvelope",
        ".body",
        "body:",
        "text:",
        ".text",
        "payload:",
        "excerpt",
    ] {
        assert!(
            !store.contains(payload),
            "approvals.rs must never handle a payload; it mentions {payload:?}"
        );
    }
    for io in ["reqwest", "std::net", "tokio::net", "async fn"] {
        assert!(
            !store.contains(io),
            "approvals.rs is a store, not a transport; it mentions {io:?}"
        );
    }
    let domain = production("server/src/domain/federation_policy.rs");
    let entry = domain
        .split("pub struct PendingApproval {")
        .nth(1)
        .and_then(|rest| rest.split("\n}\n").next())
        .expect("PendingApproval is defined");
    let fields: Vec<&str> = entry
        .lines()
        .filter_map(|line| line.trim().strip_prefix("pub "))
        .map(|line| line.split(':').next().unwrap())
        .collect();
    assert_eq!(
        fields,
        [
            "version",
            "id",
            "pairing_id",
            "requester",
            "intent",
            "disclosure",
            "status",
            "requested_at",
            "decided_at",
            "expires_at",
            "summary",
        ]
    );
    assert!(
        domain
            .split("pub struct PendingApproval {")
            .next()
            .unwrap()
            .trim_end()
            .ends_with("#[serde(deny_unknown_fields)]"),
        "a pending approval refuses fields it does not know"
    );
    for closed in ["pub enum ApprovalScope", "pub enum ApprovalStatus"] {
        let attributes = domain.split(closed).next().unwrap();
        assert!(
            attributes
                .trim_end()
                .ends_with("rename_all = \"snake_case\")]"),
            "{closed} is not a closed, snake_case shape"
        );
    }
    // The scope is read through one strict wire shape: a deadline on a
    // scope that takes none is refused, not ignored.
    assert!(
        domain.contains("struct ApprovalScopeWire")
            && domain.contains("impl<'de> Deserialize<'de> for ApprovalScope"),
        "ApprovalScope must deserialize strictly"
    );
    assert!(
        store.contains("pub const MAX_APPROVALS: usize")
            && domain.contains("pub const PENDING_APPROVAL_TTL_SECS: u64")
            && domain.contains("pub const ONCE_APPROVAL_TTL_SECS: u64"),
        "the queue and its entries must stay bounded"
    );
}

#[test]
fn the_gate_consults_the_queue_only_after_the_engine_asked_and_carries_it_through_a_rotation() {
    let gate = production("server/src/services/federation/gate.rs");
    let judge = method(&gate, "judge");
    let evaluate_at = judge
        .find("policy::evaluate(")
        .expect("judge runs the engine");
    let resolve_at = judge
        .find("self.approvals.resolve(")
        .expect("judge consults the queue");
    let record_at = judge
        .find("self.record_answering(")
        .expect("judge records the receipt");
    assert!(
        evaluate_at < resolve_at && resolve_at < record_at,
        "judge must evaluate, then consult the queue, then record"
    );
    let before_resolve = &judge[evaluate_at..resolve_at];
    assert!(
        before_resolve.contains("Verdict::Ask"),
        "the queue is consulted only when the engine said ask"
    );
    // The unknown-name branch (denied before the engine) never queues.
    let unknown = judge
        .split("Classified::Unknown {")
        .nth(1)
        .and_then(|rest| rest.split("self.record_answering(").next())
        .expect("judge handles unknown names");
    assert!(
        !unknown.contains("approvals"),
        "an unknown intent is denied, never queued"
    );
    // A key rotation moves the peer's queue with its rules, under the same
    // exclusive hold, and is refused while the queue cannot be loaded.
    let rotation = gate
        .split("pub fn receive_rotation(")
        .nth(1)
        .and_then(|rest| rest.split("\n    pub async fn ").next())
        .expect("receive_rotation exists");
    let loadable_at = rotation
        .find("self.approvals.ensure_loadable()")
        .expect("receive_rotation checks the queue first");
    let update_at = rotation
        .find("self.policy.update(")
        .expect("receive_rotation applies under the policy lock");
    let rekey_at = rotation
        .find("self.approvals.rekey(")
        .expect("receive_rotation moves the queue");
    assert!(
        loadable_at < update_at && update_at < rekey_at,
        "receive_rotation must check the queue, then apply, then move it"
    );
    // Every owner decision writes a receipt on the owner side, before the
    // change is applied.
    for name in ["approve", "deny"] {
        assert!(
            method(&gate, name).contains("self.decide("),
            "{name} must go through decide"
        );
    }
    for name in [
        "decide",
        "withdraw_approval",
        "set_rule",
        "revoke_rule",
        "forget_peer",
    ] {
        let body = method(&gate, name);
        let record_at = body
            .find("self.record(")
            .unwrap_or_else(|| panic!("{name} must record the owner's decision"));
        assert!(
            body[..record_at].contains("ReceiptSide::Owner")
                || body[record_at..].contains("ReceiptSide::Owner")
                || body[record_at..].contains("side,"),
            "{name} must record on the owner side"
        );
        for change in [
            "self.approvals.approve_once(",
            "self.approvals.deny_once(",
            "self.approvals.remove(",
            "self.approvals.forget_peer(",
            "self.approvals.drop_stale(",
            "self.write_rule(",
            "self.policy.update(",
        ] {
            if let Some(at) = body.find(change) {
                assert!(
                    record_at < at,
                    "{name} must record before it applies {change}"
                );
            }
        }
    }
}

#[test]
fn the_owner_api_is_post_and_delete_and_both_revoke_paths_drop_what_the_peer_had() {
    let routes = production("server/src/routes/federation.rs");
    let owner = routes
        .split("pub fn router()")
        .nth(1)
        .unwrap()
        .split("pub fn public_router")
        .next()
        .unwrap();
    for route in [
        "\"/api/federation/approvals\", get(",
        "\"/api/federation/approvals/{id}\", delete(",
        "\"/api/federation/approvals/{id}/approve\",",
        "\"/api/federation/approvals/{id}/deny\",",
        "\"/api/federation/peers/{companion_id}/rules\",",
        "\"/api/federation/peers/{companion_id}/rules/revoke\",",
    ] {
        assert!(owner.contains(route), "the owner router lacks {route}");
    }
    for verb in ["put(", "patch("] {
        assert!(
            !owner.contains(verb),
            "the owner routes are get, post, and delete only"
        );
    }
    assert!(
        !routes.contains("Query<") && !routes.contains("query::"),
        "nothing is taken from the query string"
    );
    assert!(
        !owner.contains("{intent}") && !owner.contains("{disclosure}"),
        "a rule's pair travels in a body; paths name a companion or an entry only"
    );
    for handler in ["async fn revoke_peer(", "async fn revoke_notice("] {
        let body = routes
            .split(handler)
            .nth(1)
            .and_then(|rest| rest.split("\n}\n").next())
            .unwrap_or_else(|| panic!("{handler} exists"));
        assert!(
            body.contains("forget_peer("),
            "{handler} must drop the revoked peer's rules and pending approvals"
        );
    }
    for code in ["\"unknown_approval\"", "\"unknown_rule\""] {
        assert!(routes.contains(code), "routes/federation.rs lacks {code}");
    }
}

#[test]
fn the_companions_section_keeps_no_approval_in_the_browser_and_is_documented() {
    let helpers = read("client/src/lib/federation/policy.js");
    for required in [
        "export function capabilityRows(",
        "export function approvalView(",
        "export function pendingApprovals(",
        "export function intentLabel(",
        "export function approvalScopes(",
    ] {
        assert!(
            helpers.contains(required),
            "policy.js is missing {required}"
        );
    }
    assert!(
        read("client/tests/federation-policy.test.mjs").contains("policy.js"),
        "the helpers need node tests"
    );
    for relative in [
        "client/src/lib/federation/policy.js",
        "client/src/lib/components/federation/Companions.svelte",
        "client/src/lib/components/federation/CompanionRow.svelte",
        "client/src/lib/components/federation/PeerCapabilities.svelte",
        "client/src/lib/components/federation/PendingApprovals.svelte",
    ] {
        let source = read(relative);
        for forbidden in [
            "localStorage",
            "sessionStorage",
            "console.",
            "document.cookie",
            "?approval",
            "?rule",
        ] {
            assert!(
                !source.contains(forbidden),
                "{relative} contains {forbidden:?}"
            );
        }
    }
    let client = read("client/src/lib/api/client.ts");
    for required in [
        "export function fetchFederationPolicy(",
        "export function fetchFederationApprovals(",
        "export function approveFederationRequest(",
        "export function denyFederationRequest(",
        "export function withdrawFederationApproval(",
        "export function setFederationRule(",
        "export function revokeFederationRule(",
    ] {
        assert!(client.contains(required), "client.ts is missing {required}");
    }
    assert!(
        !client.contains("?approval=") && !client.contains("?scope="),
        "the client never puts an approval or a scope in a URL"
    );
    let types = read("client/src/lib/api/types.ts");
    for required in [
        "export interface FederationApproval",
        "export interface FederationPolicyView",
        "export interface FederationPolicyRule",
        "export type FederationApprovalScope",
    ] {
        assert!(types.contains(required), "types.ts is missing {required}");
    }

    let settings = read("docs/settings.md");
    for required in [
        "approvals",
        "capabilit",
        "Once",
        "Allow",
        "Deny",
        "Withdraw",
    ] {
        assert!(
            settings.contains(required),
            "docs/settings.md must describe the Companions controls; missing {required:?}"
        );
    }
    let federation = read("docs/federation.md");
    for required in [
        "#109",
        "## Policy",
        "/api/federation/policy",
        "/api/federation/approvals",
        "/api/federation/receipts",
        "approval_required",
        "once",
        "until",
        "allow",
        "ask",
        "deny",
        "quiet hours",
        "rate limit",
        "revoke",
        "audit",
        "never",
        "approvals.json",
    ] {
        assert!(
            federation.contains(required),
            "docs/federation.md is missing {required:?}"
        );
    }
    let storage = read("docs/companion-storage.md");
    for required in ["federation/approvals.json", "/api/federation/approvals"] {
        assert!(
            storage.contains(required),
            "docs/companion-storage.md is missing {required:?}"
        );
    }
    let design = read("docs/design-system.md");
    assert!(
        design.contains("PendingApprovals") && design.contains("PeerCapabilities"),
        "docs/design-system.md must describe the approval and capability patterns"
    );
}
