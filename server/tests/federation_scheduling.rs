//! Guards for #111: availability is planned by a pure module that answers
//! with free spans only; a meeting, a reminder, or a task handoff lands as
//! a reviewable proposal and nothing is written until the owner accepts,
//! on this owner's own server, from this server's own words; a handoff
//! carries references and provenance, never contents; no federation
//! module reaches a task or commitment store, and nothing a peer sends
//! can reach the file, email, or computer-use tools; the proposal tools
//! are thin typed wrappers registered for the chat only; the client shows
//! the cards through pure helpers; the docs say so.

#[path = "../test-support/source_scan.rs"]
mod source_scan;

use std::{
    fs,
    path::{Path, PathBuf},
};

use source_scan::without_cfg_test_items;

const SCHEDULING: &str = "server/src/services/federation/scheduling.rs";
const HANDOFFS: &str = "server/src/services/federation/handoffs.rs";
const PROPOSALS: &str = "server/src/services/federation/proposals.rs";
const PROPOSAL_DOMAIN: &str = "server/src/domain/federation_proposal.rs";
const INTENT_DOMAIN: &str = "server/src/domain/federation_intent.rs";
const DELIVERY: &str = "server/src/services/peer_delivery.rs";
const DECISIONS: &str = "server/src/services/peer_proposals.rs";
const TOOLS: &str = "server/src/services/tools/peer_proposals.rs";
const TOOLS_MOD: &str = "server/src/services/tools/mod.rs";
const OUTBOX: &str = "server/src/services/federation/outbox.rs";
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
    let root = repo();
    let mut paths = Vec::new();
    visit(&root.join(relative), &mut paths);
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let relative = path.strip_prefix(&root).unwrap().display().to_string();
            let source = fs::read_to_string(&path).unwrap();
            (relative, without_cfg_test_items(&source))
        })
        .collect()
}

/// The body of `fn <name>(` up to the end of that function.
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

/// The field names of `pub struct <name> {`.
fn fields(source: &str, name: &str) -> Vec<String> {
    source
        .split(&format!("pub struct {name} {{"))
        .nth(1)
        .and_then(|rest| rest.split("\n}\n").next())
        .unwrap_or_else(|| panic!("{name} is defined"))
        .lines()
        .filter_map(|line| line.trim().strip_prefix("pub "))
        .map(|line| line.split(':').next().unwrap().to_owned())
        .collect()
}

#[test]
fn availability_is_planned_purely_and_answers_with_free_spans_only() {
    let planner = production(SCHEDULING);
    for io in [
        "std::fs",
        "tokio",
        "reqwest",
        "std::net",
        "SystemTime",
        "Instant::",
        "getrandom",
        "std::env",
        "Mutex",
        "async fn",
        "AppState",
        "CommitmentStore",
        "ContinuityStore",
        "log::",
        "println",
    ] {
        assert!(
            !planner.contains(io),
            "{SCHEDULING} must stay pure; it mentions {io:?}"
        );
    }
    for required in [
        "pub fn free_windows(",
        "pub fn busy_spans(",
        "pub fn quiet_spans(",
        "pub fn granularity_secs(",
        "MAX_AVAILABILITY_WINDOWS",
        "AvailabilityState::Free",
    ] {
        assert!(planner.contains(required), "{SCHEDULING} lacks {required}");
    }
    assert!(
        !planner.contains("AvailabilityState::Busy"),
        "the planner never discloses a busy span"
    );
    // A commitment contributes its deadline and nothing else: no promise,
    // no id, no note ever enters the planner's output.
    for leak in [".promise", ".id", "note", "title", "summary"] {
        assert!(
            !planner.contains(leak),
            "{SCHEDULING} reads a commitment's {leak}"
        );
    }
    // Granularity follows the disclosure class and never goes below a
    // minute, so a moment is never disclosed exactly.
    let granularity = function(&planner, "granularity_secs");
    assert!(
        granularity.contains("DisclosureClass::Availability")
            && granularity.contains("DisclosureClass::Personal"),
        "granularity is gated by the disclosure class: {granularity}"
    );
    assert!(
        !granularity.contains("=> 0") && !granularity.contains("=> 1,"),
        "no class discloses to the second"
    );
    // The delivery plans through it and the answer's class is what the
    // delivery says it disclosed, clamped to what was asked.
    let delivery = production(DELIVERY);
    assert!(
        delivery.contains("scheduling::busy_spans(")
            && delivery.contains("scheduling::free_windows("),
        "{DELIVERY} answers availability through the planner"
    );
    let inbound = production("server/src/services/federation/inbound.rs");
    assert!(
        inbound.contains("disclosure: delivered.disclosure.min(intent.disclosure)")
            || inbound.contains(".min(intent.disclosure)"),
        "the answer never discloses above the class asked for"
    );
    assert!(
        fields(&inbound, "Delivered").contains(&"disclosure".to_owned()),
        "a delivery says what it disclosed"
    );
}

#[test]
fn proposals_are_reviewed_before_anything_is_written_and_only_on_this_owners_server() {
    // The delivery writes the conversation message and the proposal, and
    // nothing else: no commitment, no task, no schedule.
    let delivery = production(DELIVERY);
    assert!(
        delivery.contains("federation_proposals.receive("),
        "{DELIVERY} records a reviewable proposal"
    );
    for write in [
        "commitments.create(",
        "ContinuityStore",
        ".create(",
        "scheduled",
        "ScheduledTask",
        "proactive",
    ] {
        assert!(
            !delivery.contains(write),
            "{DELIVERY} writes before the owner accepts via {write:?}"
        );
    }
    let preface = function(&delivery, "preface");
    for hint in ["review", "accept"] {
        assert!(
            preface.to_lowercase().contains(hint),
            "the line above a proposal tells the owner to {hint} it"
        );
    }

    // The store keeps the details for the owner's review and nothing that
    // could act: typed, closed, bounded, private, fail-closed.
    let domain = production(PROPOSAL_DOMAIN);
    assert_eq!(
        fields(&domain, "PeerProposal"),
        [
            "version",
            "id",
            "sender",
            "pairing_id",
            "correlation_id",
            "represented_owner",
            "purpose",
            "intent",
            "details",
            "status",
            "message_id",
            "received_at",
            "expires_at",
            "decided_at",
            "outcome",
            "receipt_id",
        ]
    );
    for open_ended in ["serde_json::Value", "untagged", "flatten", "HashMap<String"] {
        assert!(
            !domain.contains(open_ended),
            "{PROPOSAL_DOMAIN} uses {open_ended}"
        );
    }
    assert!(
        domain.contains("#[serde(deny_unknown_fields)]\npub struct PeerProposal"),
        "a proposal refuses fields it does not know"
    );
    assert!(domain.contains("pub const HANDOFF_REVIEW_SECS: u64"));
    let store = production(PROPOSALS);
    for required in [
        "pub const MAX_PEER_PROPOSALS: usize",
        "pub const PROPOSAL_RETENTION_SECS: u64",
        "const MAX_PROPOSALS_FILE_BYTES: u64",
        "replace_private(",
        "unloadable",
        "ServerEvent::PeerProposalUpdated {",
    ] {
        assert!(store.contains(required), "{PROPOSALS} lacks {required}");
    }
    // Accepting: under one lock, the open-and-unexpired check, the owner's
    // receipt, the one write, then the record; a second accept finds the
    // record and writes nothing.
    let accept = function(&store, "accept");
    let locked_at = accept
        .find("decisions.lock()")
        .expect("accept holds the decision lock");
    let expired_at = accept
        .find("ProposalStatus::Expired")
        .expect("accept refuses a lapsed proposal");
    let already_at = accept
        .find("ProposalStatus::Accepted")
        .expect("accept recognises an accepted proposal");
    let receipt_at = accept
        .find("record_owner_decision(")
        .expect("accept records the owner's decision");
    let write_at = accept.find("write(").expect("accept runs the one write");
    let saved_at = accept.find(".save(").expect("accept persists the outcome");
    assert!(
        locked_at < expired_at && locked_at < already_at,
        "the checks run under the lock"
    );
    assert!(
        already_at < receipt_at && expired_at < receipt_at,
        "an accepted or lapsed proposal gets no second receipt"
    );
    assert!(
        receipt_at < write_at && write_at < saved_at,
        "receipt, then the write, then the record"
    );
    assert_eq!(
        accept.matches("write(").count(),
        1,
        "exactly one write per acceptance"
    );
    let dismiss = function(&store, "dismiss");
    assert!(
        dismiss.contains("record_owner_decision(") && !dismiss.contains("write("),
        "dismissing records the decision and writes nothing"
    );
    // The gate's owner receipt is an audit line on the owner side.
    let gate = production(GATE);
    let record = function(&gate, "record_owner_decision");
    assert!(record.contains("ReceiptSide::Owner"));

    // The decisions service writes on this server alone, from this
    // server's own ids: the sender's verified companion id and the
    // delivered message, never the peer's words.
    let decisions = production(DECISIONS);
    for (name, needed) in [
        ("meeting_promise", "message_id"),
        ("reminder_promise", "message_id"),
        ("handoff_goal", "message_id"),
    ] {
        let body = function(&decisions, name);
        assert!(
            body.contains(needed) && body.contains("sender"),
            "{name}: {body}"
        );
        for word in [
            "description",
            ".text",
            "goal",
            "task.",
            "record_id",
            "represented_owner",
            "purpose",
            "correlation_id",
            "render",
            "block",
        ] {
            assert!(
                !body.contains(word),
                "{name} carries the peer's {word}: {body}"
            );
        }
    }
    for leak in [
        "services::tools",
        "services::llm",
        "run_single_turn",
        "run_agent_loop",
        "proactive",
        "ScheduledTask",
        "scheduled",
        "render_untrusted_block",
        "PeerText::new(",
        "outbox",
        "enqueue(",
        "post_transport",
    ] {
        assert!(
            !decisions.contains(leak),
            "{DECISIONS} reaches a tool, a turn, a schedule, or the wire via {leak:?}"
        );
    }
    assert!(
        decisions.contains("federation_proposals")
            && decisions.contains(".accept(")
            && decisions.contains(".dismiss("),
        "the decisions go through the store"
    );

    // Routes: the owner alone, nothing public.
    let routes = production(ROUTES);
    for required in [
        ".route(\"/api/federation/proposals\", get(list_proposals))",
        "\"/api/federation/proposals/{id}/accept\"",
        "\"/api/federation/proposals/{id}/dismiss\"",
        "\"unknown_proposal\"",
        "\"proposal_not_open\"",
    ] {
        assert!(routes.contains(required), "{ROUTES} lacks {required}");
    }
    let public_router = routes
        .split("pub fn public_router()")
        .nth(1)
        .and_then(|rest| rest.split("\n}\n").next())
        .expect("the public router");
    assert!(
        !public_router.contains("proposal"),
        "nothing about proposals is public"
    );
    let state = production("server/src/app/state.rs");
    assert!(state.contains(
        "pub federation_proposals: Arc<crate::services::federation::proposals::ProposalStore>"
    ));
    let events = production("server/src/domain/events.rs");
    assert!(events.contains("PeerProposalUpdated {"));
}

#[test]
fn handoffs_carry_references_and_provenance_never_contents_and_no_peer_reaches_a_tool() {
    // The wire shape: a bounded goal, the most recent steps, the next
    // step, blockers, resource references, and provenance; nothing that
    // could hold a file.
    let intents = production(INTENT_DOMAIN);
    assert_eq!(
        fields(&intents, "TaskHandoff"),
        [
            "record_id",
            "goal",
            "completed_steps",
            "next_step",
            "blockers",
            "resources",
            "provenance",
        ]
    );
    assert_eq!(fields(&intents, "HandoffResource"), ["kind", "label"]);
    assert_eq!(
        fields(&intents, "HandoffProvenance"),
        ["source", "at", "note"]
    );
    for field in [
        "contents", "content", "bytes", "data", "body", "path:", "url",
    ] {
        assert!(
            !intents
                .split("pub struct TaskHandoff {")
                .nth(1)
                .unwrap()
                .split("\n}\n")
                .next()
                .unwrap()
                .contains(field),
            "TaskHandoff has room for {field:?}"
        );
    }
    for required in [
        "pub const MAX_HANDOFF_GOAL_CHARS: usize",
        "pub const MAX_HANDOFF_NOTE_CHARS: usize",
        "pub const MAX_HANDOFF_STEPS: usize",
        "pub const MAX_HANDOFF_BLOCKERS: usize",
        "pub const MAX_HANDOFF_RESOURCES: usize",
        "pub const MAX_HANDOFF_PROVENANCE: usize",
        "InvalidHandoff",
        "HandoffReceived {}",
        "\"handoff_received\"",
    ] {
        assert!(
            intents.contains(required),
            "{INTENT_DOMAIN} lacks {required}"
        );
    }
    let policy = production("server/src/domain/federation_policy.rs");
    assert!(
        policy.contains("Self::Handoff => \"handoff\","),
        "the handoff is its own intent class"
    );
    let engine = production("server/src/services/federation/policy.rs");
    assert!(
        engine.contains("IntentClass::Handoff, Nothing) => Access::Ask")
            || engine.contains("IntentClass::Handoff, Nothing) =>"),
        "a handoff asks the owner by default"
    );

    // The planner reads the record and never a file, an upload, or a
    // memory; the labels it writes are the references' own names.
    let planner = production(HANDOFFS);
    for io in [
        "std::fs",
        "read_to_string",
        "File::",
        "tokio",
        "reqwest",
        "MediaStore",
        "uploads",
        "memory::",
        "ContinuityStore",
        "AppState",
        "log::",
    ] {
        assert!(
            !planner.contains(io),
            "{HANDOFFS} must stay pure; it mentions {io:?}"
        );
    }
    assert!(planner.contains("pub fn handoff_from_record("));
    assert!(
        planner.contains("MAX_HANDOFF_STEPS") && planner.contains("MAX_HANDOFF_PROVENANCE"),
        "the planner bounds what travels"
    );

    // Nothing under the federation modules, the delivery, or the decisions
    // reaches a file, an email, or a computer: the peer's words can only
    // ever land in a conversation message, a proposal, a commitment's
    // deadline, or a task's origin.
    let mut sources = sources_under("server/src/services/federation");
    sources.push((DELIVERY.into(), production(DELIVERY)));
    sources.push((DECISIONS.into(), production(DECISIONS)));
    sources.push((TOOLS.into(), production(TOOLS)));
    for (path, source) in &sources {
        for reach in [
            "tools::files",
            "files::",
            "FileReadTool",
            "read_email",
            "communication::",
            "remote_files",
            "remote_bash",
            "services::cua",
            "cua::",
            "computer::",
            "MachineTarget",
            "CuaTools",
            "std::process",
            "Command::new",
            "run_single_turn",
            "run_agent_loop",
        ] {
            assert!(
                !source.contains(reach),
                "{path} reaches a file, an email, or a computer via {reach:?}"
            );
        }
    }
    // No shared task database: no federation module opens a task or a
    // commitment store; the owner's stores are written by the decisions
    // service alone, on the owner's own accept.
    for (path, source) in sources_under("server/src/services/federation") {
        for store in [
            "ContinuityStore",
            "CommitmentStore",
            "commitments.",
            "continuity::ContinuityStore",
        ] {
            assert!(
                !source.contains(store),
                "{path} reaches a task or commitment store via {store:?}"
            );
        }
    }
    let decisions = production(DECISIONS);
    assert!(
        decisions.contains("ContinuityStore::new(") && decisions.contains("commitments"),
        "the decisions service writes the owner's own stores"
    );

    // The proposal tools: two thin typed wrappers, one typed request
    // each, no free-form field, registered for the chat behind the same
    // federation side as the #110 tools.
    let tools = production(TOOLS);
    for name in ["\"propose_peer_meeting\"", "\"handoff_task_to_peer\""] {
        assert!(
            tools.contains(&format!("const NAME: &'static str = {name};")),
            "{TOOLS} lacks the tool {name}"
        );
    }
    assert_eq!(tools.matches("const NAME: &'static str").count(), 2);
    assert_eq!(tools.matches("OutboxRequest::").count(), 2);
    for args in ["ProposePeerMeetingArgs", "HandoffTaskToPeerArgs"] {
        let names = fields(&tools, args);
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
            "path",
            "file",
            "contents",
        ] {
            assert!(
                !names.contains(&forbidden.to_owned()),
                "{args} takes a free-form {forbidden:?}"
            );
        }
        assert!(
            names.contains(&"peer".to_owned())
                && names.contains(&"on_behalf_of".to_owned())
                && names.contains(&"purpose".to_owned()),
            "{args} names the peer and the two labels: {names:?}"
        );
    }
    assert!(
        fields(&tools, "HandoffTaskToPeerArgs").contains(&"record_id".to_owned()),
        "a handoff names a continuity record"
    );
    for leak in [
        "IntentResponse",
        ".response",
        ".answer",
        "inbound",
        "chat::",
        "proposals::",
        "ProposalStore",
        "federation_proposals",
    ] {
        assert!(!tools.contains(leak), "{TOOLS} mentions {leak:?}");
    }
    let tools_mod = production(TOOLS_MOD);
    assert!(tools_mod.contains("pub mod peer_proposals;"));
    let build = function(&tools_mod, "build_tools");
    assert!(
        build.contains("peer_proposals::proposal_tools("),
        "build_tools registers the proposal tools"
    );
    let registered_at = build.find("peer_proposals::proposal_tools(").unwrap();
    let gated_at = build.find("if let Some(sending) = peer_sending").unwrap();
    assert!(
        gated_at < registered_at,
        "the proposal tools are chat-only, like the sending tools"
    );
    for name in ["propose_peer_meeting", "handoff_task_to_peer"] {
        assert!(
            tools_mod.contains(&format!("\"{name}\" =>")),
            "tool_summary names {name}"
        );
    }
    // The outbox turns a record into the bounded wire shape itself and
    // asks the own gate first, as for every other request.
    let outbox = production(OUTBOX);
    let enqueue = function(&outbox, "enqueue");
    assert!(enqueue.contains("handoffs::handoff_from_record("));
    assert!(
        enqueue.find(".admit_outbound(").unwrap() < enqueue.find(".save(").unwrap(),
        "the gate comes before the write"
    );
    assert!(
        enqueue.contains("OutboxRequest::Proposal {")
            && enqueue.contains("OutboxRequest::Handoff {")
    );

    // The fixture pins the wire shape.
    let fixture: serde_json::Value = serde_json::from_str(&read(
        "server/tests/fixtures/federation/intents/handoff_v1.json",
    ))
    .unwrap();
    assert_eq!(fixture["intent"]["type"], "handoff");
    let task = fixture["intent"]["task"].as_object().expect("a task");
    let mut keys: Vec<&str> = task.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "blockers",
            "completed_steps",
            "goal",
            "next_step",
            "provenance",
            "record_id",
            "resources",
        ]
    );
}

#[test]
fn the_client_reviews_proposals_through_pure_helpers_and_the_docs_say_so() {
    let helper = read("client/src/lib/federation/proposals.js");
    for required in [
        "export function proposalLabel(",
        "export function proposalStatusLabel(",
        "export function proposalNote(",
        "export function proposalActions(",
        "export function proposalWhen(",
        "export function handoffSections(",
        "export function upsertProposal(",
    ] {
        assert!(helper.contains(required), "proposals.js lacks {required}");
    }
    for forbidden in [
        "innerHTML",
        "eval(",
        "new Function",
        "localStorage",
        "console.",
    ] {
        assert!(!helper.contains(forbidden), "proposals.js uses {forbidden}");
    }
    assert!(
        read("client/tests/federation-proposals.test.mjs").contains("proposals.js"),
        "the helpers need node tests"
    );
    for relative in [
        "client/src/lib/components/activity/PeerProposals.svelte",
        "client/src/lib/components/activity/PeerProposalCard.svelte",
    ] {
        let source = read(relative);
        for forbidden in ["{@html", "localStorage", "sessionStorage", "console."] {
            assert!(!source.contains(forbidden), "{relative} uses {forbidden}");
        }
    }
    let section = read("client/src/lib/components/activity/PeerProposals.svelte");
    assert!(
        section.contains("fetchFederationProposals(") && section.contains("peer_proposal_updated"),
        "the section loads the listing and follows the event"
    );
    let card = read("client/src/lib/components/activity/PeerProposalCard.svelte");
    assert!(
        card.to_lowercase().contains("untrusted") || card.to_lowercase().contains("their words"),
        "the card says the details are the companion's own words"
    );
    let activity = read("client/src/lib/components/activity/ActivityView.svelte");
    assert!(activity.contains("<PeerProposals"));
    let client = read("client/src/lib/api/client.ts");
    for required in [
        "export function fetchFederationProposals(",
        "export function acceptFederationProposal(",
        "export function dismissFederationProposal(",
    ] {
        assert!(client.contains(required), "client.ts lacks {required}");
    }
    let types = read("client/src/lib/api/types.ts");
    for required in [
        "export interface FederationPeerProposal",
        "export interface FederationTaskHandoff",
        "type: \"peer_proposal_updated\"",
    ] {
        assert!(types.contains(required), "types.ts lacks {required}");
    }
    let receipts = read("client/src/lib/activity/receipts.js");
    for required in [
        "\"handoff_received\"",
        "case \"handoff\":",
        "case \"proposal\":",
    ] {
        assert!(receipts.contains(required), "receipts.js lacks {required}");
    }
    let policy = read("client/src/lib/federation/policy.js");
    assert!(
        policy.contains("\"handoff/none\":"),
        "policy.js labels the handoff class"
    );

    let federation = read("docs/federation.md");
    for required in [
        "## Scheduling and handoffs",
        "propose_peer_meeting",
        "handoff_task_to_peer",
        "GET /api/federation/proposals",
        "proposals.json",
        "free",
        "quiet hours",
        "granularity",
        "never a raw calendar",
        "idempotent",
    ] {
        assert!(
            federation.contains(required),
            "docs/federation.md lacks {required:?}"
        );
    }
    let storage = read("docs/companion-storage.md");
    for required in ["proposals.json", "expires_at", "handoff"] {
        assert!(
            storage.contains(required),
            "docs/companion-storage.md lacks {required:?}"
        );
    }
    let design = read("docs/design-system.md");
    assert!(
        design.contains("PeerProposals.svelte") && design.contains("PeerProposalCard.svelte"),
        "docs/design-system.md describes the cards"
    );
}

#[test]
fn the_proposing_owner_is_told_the_decision_through_a_typed_notice_gated_like_any_intent() {
    // The wire: one more intent class, `decision`, whose payload names
    // the request it answers and the decision, nothing else; its answer
    // carries nothing.
    let intents = production(INTENT_DOMAIN);
    let payload = intents
        .split("pub enum IntentPayload {")
        .nth(1)
        .and_then(|rest| rest.split("\n}\n").next())
        .expect("IntentPayload is defined");
    assert!(
        payload.contains("Decision {")
            && payload.contains("correlation_id: String")
            && payload.contains("decision: ProposalDecision"),
        "the decision payload names the request and the decision:\n{payload}"
    );
    let decision = intents
        .split("pub enum ProposalDecision {")
        .nth(1)
        .and_then(|rest| rest.split("\n}\n").next())
        .expect("ProposalDecision is defined");
    assert_eq!(
        decision
            .lines()
            .filter_map(|line| line.trim().strip_suffix(','))
            .filter(|line| line.starts_with(|c: char| c.is_ascii_uppercase()))
            .collect::<Vec<_>>(),
        ["Accepted", "Dismissed"],
        "a decision is accepted or dismissed, nothing else"
    );
    assert!(
        !payload
            .split("Decision {")
            .nth(1)
            .unwrap()
            .split("},")
            .next()
            .unwrap()
            .contains("PeerText"),
        "a decision carries no text"
    );
    assert!(
        intents.contains("DecisionNoted {}")
            && intents.contains("\"decision_noted\" => Some(IntentClass::Decision)"),
        "the answer to a decision carries nothing"
    );
    let policy = production("server/src/domain/federation_policy.rs");
    assert!(
        policy.contains("Self::Decision => \"decision\""),
        "the policy names the class"
    );
    // The engine: a decision answers this owner's own request, so it is
    // allowed by default at `none` and never discloses at any other class;
    // the owner's rule can still deny it.
    let engine = production("server/src/services/federation/policy.rs");
    assert!(
        engine.contains("(IntentClass::Decision, Nothing) => Access::Allow")
            && engine.contains("(IntentClass::Decision, _) => return None"),
        "the engine's defaults for a decision: {engine}"
    );
    assert!(
        fs::read_to_string(
            repo().join("server/tests/fixtures/federation/intents/decision_v1.json")
        )
        .is_ok(),
        "the decision fixture pins the wire shape"
    );

    // The deciding side: the store tells the peer after the decision is
    // saved, through the outbox and this owner's own outbound gate, with
    // the request's correlation id and the decision, never the details.
    let store = production(PROPOSALS);
    for name in ["accept", "dismiss"] {
        let body = function(&store, name);
        let saved_at = body.find(".save(").expect("the decision is saved");
        let told_at = body
            .find("tell_peer(")
            .unwrap_or_else(|| panic!("{name} tells the peer"));
        assert!(saved_at < told_at, "{name}: the decision is saved first");
    }
    let tell = function(&store, "tell_peer");
    assert!(
        tell.contains("enqueue(") && tell.contains("OutboxRequest::Decision"),
        "the notice goes through the outbox: {tell}"
    );
    for leak in [
        "description",
        ".text",
        "goal",
        "task.",
        "details",
        "render",
        "post_transport",
    ] {
        assert!(!tell.contains(leak), "the notice carries {leak}: {tell}");
    }
    assert!(
        tell.contains("log::warn!") || tell.contains("log::info!"),
        "a notice that cannot be queued is logged, and the decision stands"
    );
    let outbox = production(OUTBOX);
    assert!(
        outbox.contains("OutboxRequest::Decision {")
            && outbox.contains("IntentPayload::Decision {"),
        "the outbox builds the decision intent"
    );
    // The decisions service itself still never touches the wire.
    let decisions = production(DECISIONS);
    for leak in ["outbox", "enqueue(", "post_transport", "tell_peer"] {
        assert!(
            !decisions.contains(leak),
            "{DECISIONS} reaches the wire via {leak:?}"
        );
    }

    // The proposing side: the delivery notes the decision on the entry it
    // sent (a delivered proposal, reminder, or handoff to that peer, and
    // nothing else), tells the owner in this server's words, and writes
    // nothing else; a notice for a request never sent is refused, typed.
    assert!(
        fields(&outbox, "OutboxEntry").contains(&"decision".to_owned()),
        "an entry keeps the peer's decision"
    );
    assert_eq!(
        fields(&outbox, "PeerDecision"),
        ["decision", "at"],
        "what is noted: the decision and when"
    );
    let noted = function(&outbox, "note_decision");
    assert!(
        noted.contains("OutboxStatus::Delivered")
            && noted.contains("IntentPayload::Reminder")
            && noted.contains("IntentPayload::Proposal")
            && noted.contains("IntentPayload::Handoff")
            && noted.contains("FederationError::UnknownRequest"),
        "only a delivered proposal of this owner's is noted: {noted}"
    );
    let delivery = production(DELIVERY);
    assert!(
        delivery.contains("note_decision(") && delivery.contains("IntentAnswer::DecisionNoted {}"),
        "{DELIVERY} notes the decision on the outbox entry"
    );
    let line = function(&delivery, "decision_line");
    for leak in [
        "render_untrusted_block",
        "PeerText",
        "represented_owner",
        "purpose",
        ".body",
        ".text",
        "description",
        "goal",
    ] {
        assert!(
            !line.contains(leak),
            "the decision line carries {leak}: {line}"
        );
    }
    let routes = production(ROUTES);
    assert!(
        routes.contains("\"unknown_request\""),
        "a notice for a request never sent is refused with a typed code"
    );
    let inbox_test = read("server/tests/federation_intents.rs");
    assert!(
        inbox_test.contains("\"decision_v1.json\""),
        "the fixture list names the decision"
    );

    // The client and the docs say so.
    let receipts = read("client/src/lib/activity/receipts.js");
    for required in [
        "\"decision_noted\"",
        "case \"decision\":",
        "Accepted by their owner",
        "Declined by their owner",
    ] {
        assert!(receipts.contains(required), "receipts.js lacks {required}");
    }
    let policy_js = read("client/src/lib/federation/policy.js");
    assert!(
        policy_js.contains("\"decision/none\":"),
        "policy.js labels the decision class"
    );
    let types = read("client/src/lib/api/types.ts");
    assert!(
        types.contains("export interface FederationPeerDecision"),
        "types.ts carries the noted decision"
    );
    let federation = read("docs/federation.md");
    for required in ["`decision`", "declined", "unknown_request"] {
        assert!(
            federation.contains(required),
            "docs/federation.md lacks {required:?}"
        );
    }
    assert!(
        !federation.contains("not that anything was done"),
        "the docs no longer describe the delivery answer as the last word"
    );
    let storage = read("docs/companion-storage.md");
    assert!(
        storage.contains("`decision`"),
        "docs/companion-storage.md names the class and the entry's field"
    );
}
