//! Companion-to-companion tools (#110, PR 3): the chat's bounded way to
//! ask a paired companion for one of three things: deliver a message to
//! its owner, say whether its owner is free inside a window, or remind its
//! owner of something at a time. Each is a thin wrapper that turns its
//! arguments into one typed [`OutboxRequest`] and hands it to the outbox,
//! which asks this owner's own policy gate before anything is queued (an
//! unknown, pending, revoked, or denied peer is refused right there) and
//! builds the wire intent itself. Delivery is asynchronous: the tool
//! answers with the queued request's id and never sees the peer's
//! answer, let alone its text; the outcome shows on the activity page.
//! There is no tool for anything else: no free-form method, no remote
//! command, nothing that takes a peer's words back as an argument.
//!
//! They are chat tools only: the check-in and reflection routines never
//! carry them, so nothing the companion does on its own can reach another
//! owner.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use schemars::JsonSchema;
use serde::Deserialize;

use crate::domain::federation::FederationError;
use crate::services::federation::{
    gate::FederationGate,
    outbox::{Outbox, OutboxEntry, OutboxRequest, Outgoing},
    pairing::FederationState,
};
use crate::services::tool::{Tool, ToolDefinition, ToolDyn};

use super::{ToolExecError, commitments::parse_when, openai_schema};

/// The federation side a chat turn may send through: identity and peers,
/// this owner's policy gate, and the outbox. Built by the chat route from
/// the server state; the routines never get one.
#[derive(Clone)]
pub struct PeerSending {
    pub federation: Arc<FederationState>,
    pub gate: Arc<FederationGate>,
    pub outbox: Arc<Outbox>,
}

/// What every sending tool shares; the proposal tools of #111
/// (`super::peer_proposals`) share it too.
pub(super) struct Context {
    sending: PeerSending,
    instance_dir: PathBuf,
    chat_id: String,
}

impl Context {
    pub(super) fn new(
        sending: PeerSending,
        workspace_dir: &Path,
        instance_slug: &str,
        chat_id: &str,
    ) -> Self {
        Self {
            sending,
            instance_dir: workspace_dir.join("instances").join(instance_slug),
            chat_id: chat_id.to_owned(),
        }
    }

    fn tz(&self) -> chrono_tz::Tz {
        crate::services::commitment_evaluator::instance_timezone(&self.instance_dir)
    }

    pub(super) fn when(&self, text: &str) -> Result<u64, ToolExecError> {
        let at = parse_when(text, self.tz()).map_err(ToolExecError)?;
        u64::try_from(at).map_err(|_| ToolExecError("that moment is before 1970".into()))
    }

    /// Queues `request` for `peer` and answers with what was queued. The
    /// outbox asks this owner's own policy gate first; a refusal comes
    /// back as the error's own words, which name a state or a reason and
    /// never a text.
    pub(super) fn send(
        &self,
        peer: String,
        on_behalf_of: String,
        purpose: String,
        request: OutboxRequest,
    ) -> Result<serde_json::Value, ToolExecError> {
        let outgoing = Outgoing {
            peer,
            represented_owner: on_behalf_of,
            purpose,
            request,
            chat_id: self.chat_id.clone(),
        };
        let entry = self
            .sending
            .outbox
            .enqueue(&self.sending.federation, &self.sending.gate, outgoing)
            .map_err(refusal)?;
        Ok(queued(&entry))
    }
}

/// Why nothing was queued, in words the model can act on: the peer's
/// state, the owner's rule, or the wire's bound. Never a text.
fn refusal(error: FederationError) -> ToolExecError {
    ToolExecError(match error {
        FederationError::UnknownPeer => {
            "not queued: unknown companion; no paired companion is named by that id or prefix (see Settings › Connections › Companions)".into()
        }
        FederationError::PeerRevoked => {
            "not queued: that companion was revoked, so nothing can be sent to it".into()
        }
        FederationError::PeerNotPaired { state } => {
            format!("not queued: that companion is not paired yet ({state:?})")
        }
        FederationError::PolicyRefused(decision) => format!(
            "not queued: denied by this owner's own policy for that companion ({})",
            decision.reason.name()
        ),
        other => format!("not queued: {other}"),
    })
}

/// What the model gets back: the queued request, never the peer's answer.
fn queued(entry: &OutboxEntry) -> serde_json::Value {
    serde_json::json!({
        "status": "queued",
        "request_id": entry.intent.correlation_id,
        "peer": entry.recipient,
        "kind": entry.intent.class().name(),
        "expires_at": entry.intent.expires_at,
        "note": "Delivery is asynchronous: the other companion decides by its owner's policy, and the outcome is shown on the Activity page. Do not queue this request again.",
    })
}

/// The three sending tools, in registration order.
pub fn federation_tools(
    sending: PeerSending,
    workspace_dir: &Path,
    instance_slug: &str,
    chat_id: &str,
) -> Vec<Box<dyn ToolDyn>> {
    let context = Arc::new(Context::new(sending, workspace_dir, instance_slug, chat_id));
    vec![
        Box::new(SendPeerMessageTool(context.clone())),
        Box::new(AskPeerAvailabilityTool(context.clone())),
        Box::new(ProposePeerReminderTool(context)),
    ]
}

/// What every sending tool says about the peer, the labels, and delivery.
pub(super) const SHARED_GUIDANCE: &str = "`peer` is the paired companion's id as Settings › Connections › Companions shows it: the full id, or its first 8 or more characters when that names one companion. `on_behalf_of` is the owner you speak for, by the name they go by; `purpose` is why you ask, in one line: both are shown to the other owner as your words. Only send what the user asked you to send in this conversation, never a secret, and never on your own initiative. The other companion decides by its own owner's policy; delivery is asynchronous and the outcome (delivered, waiting for their owner, refused, failed) is shown on the Activity page, so do not call this again for the same request.";

// ---------------------------------------------------------------------------
// send_peer_message
// ---------------------------------------------------------------------------

pub struct SendPeerMessageTool(Arc<Context>);

#[derive(Deserialize, JsonSchema)]
pub struct SendPeerMessageArgs {
    /// The paired companion to send to: its id, or a unique prefix of at least 8 characters.
    pub peer: String,
    /// The message for the other owner, in the user's words.
    pub text: String,
    /// The owner you speak for, by the name they go by.
    pub on_behalf_of: String,
    /// Why you send it, in one line.
    pub purpose: String,
}

impl Tool for SendPeerMessageTool {
    const NAME: &'static str = "send_peer_message";
    type Error = ToolExecError;
    type Args = SendPeerMessageArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.into(),
            description: format!(
                "Send a message to a paired companion for its owner to read. {SHARED_GUIDANCE}"
            ),
            parameters: openai_schema::<SendPeerMessageArgs>(),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        self.0.send(
            args.peer,
            args.on_behalf_of,
            args.purpose,
            OutboxRequest::Message { text: args.text },
        )
    }
}

// ---------------------------------------------------------------------------
// ask_peer_availability
// ---------------------------------------------------------------------------

pub struct AskPeerAvailabilityTool(Arc<Context>);

#[derive(Deserialize, JsonSchema)]
pub struct AskPeerAvailabilityArgs {
    /// The paired companion to ask: its id, or a unique prefix of at least 8 characters.
    pub peer: String,
    /// Start of the window to ask about: RFC 3339, or a local date and time like 2026-01-06T09:00, or a date.
    pub from: String,
    /// End of the window (same formats), after `from` and at most 31 days later.
    pub to: String,
    /// The owner you speak for, by the name they go by.
    pub on_behalf_of: String,
    /// Why you ask, in one line.
    pub purpose: String,
}

impl Tool for AskPeerAvailabilityTool {
    const NAME: &'static str = "ask_peer_availability";
    type Error = ToolExecError;
    type Args = AskPeerAvailabilityArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.into(),
            description: format!(
                "Ask a paired companion whether its owner is free inside a window. The answer, when it comes, is free or busy spans at most, never why. {SHARED_GUIDANCE}"
            ),
            parameters: openai_schema::<AskPeerAvailabilityArgs>(),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let from = self.0.when(&args.from)?;
        let to = self.0.when(&args.to)?;
        self.0.send(
            args.peer,
            args.on_behalf_of,
            args.purpose,
            OutboxRequest::Availability { from, to },
        )
    }
}

// ---------------------------------------------------------------------------
// propose_peer_reminder
// ---------------------------------------------------------------------------

pub struct ProposePeerReminderTool(Arc<Context>);

#[derive(Deserialize, JsonSchema)]
pub struct ProposePeerReminderArgs {
    /// The paired companion to ask: its id, or a unique prefix of at least 8 characters.
    pub peer: String,
    /// What its owner should be reminded of, in the user's words.
    pub text: String,
    /// When: RFC 3339, or a local date and time like 2026-01-06T09:00, or a date. In the future, at most a year ahead.
    pub at: String,
    /// The owner you speak for, by the name they go by.
    pub on_behalf_of: String,
    /// Why you ask, in one line.
    pub purpose: String,
}

impl Tool for ProposePeerReminderTool {
    const NAME: &'static str = "propose_peer_reminder";
    type Error = ToolExecError;
    type Args = ProposePeerReminderArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.into(),
            description: format!(
                "Ask a paired companion to remind its owner of something at a time. Its companion may set the reminder, ask its owner first, or refuse. {SHARED_GUIDANCE}"
            ),
            parameters: openai_schema::<ProposePeerReminderArgs>(),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let at = self.0.when(&args.at)?;
        self.0.send(
            args.peer,
            args.on_behalf_of,
            args.purpose,
            OutboxRequest::Reminder {
                text: args.text,
                at,
            },
        )
    }
}
