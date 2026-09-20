//! One commitment record (#85): a promise the companion tracks as
//! first-class state, so following through never collapses into a timer.
//! Bounded by construction: the promise, who owns it, its status, when it is
//! due and when it should next be looked at, what it waits on, where it came
//! from, and the evidence it was done. No model text.

use serde::{Deserialize, Serialize};

pub const COMMITMENT_FORMAT_VERSION: u32 = 1;

/// Longest promise kept on a record.
pub const MAX_PROMISE_CHARS: usize = 500;
/// Longest free-text note (evidence summary, waited-for event) kept on a record.
pub const MAX_NOTE_CHARS: usize = 200;
/// Most dependency and continuity links kept on a record.
pub const MAX_LINKS: usize = 32;

/// Who promised.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Owner {
    /// The companion promised the user something.
    #[default]
    Companion,
    /// The user promised themselves something and asked the companion to hold it.
    User,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommitmentStatus {
    /// Being followed through; looked at again at `next_check`.
    Active,
    /// Held until its waiting condition is met.
    Waiting,
    /// Held until every dependency is completed.
    Blocked,
    /// The deadline (or window) has arrived and it is not done.
    Due,
    Completed,
    /// Cancelled by the user.
    Dismissed,
    /// Could not be kept; stays inspectable.
    Failed,
}

impl CommitmentStatus {
    /// Open commitments can still change; closed ones are history.
    pub fn is_open(self) -> bool {
        matches!(
            self,
            Self::Active | Self::Waiting | Self::Blocked | Self::Due
        )
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Waiting => "waiting",
            Self::Blocked => "blocked",
            Self::Due => "due",
            Self::Completed => "completed",
            Self::Dismissed => "dismissed",
            Self::Failed => "failed",
        }
    }
}

/// When the promise falls due.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Deadline {
    /// Due at one moment.
    At { at: i64 },
    /// Due any time between `start` and `end`.
    Window { start: i64, end: i64 },
}

impl Deadline {
    /// The first moment the commitment counts as due.
    pub fn starts_at(&self) -> i64 {
        match self {
            Self::At { at } => *at,
            Self::Window { start, .. } => *start,
        }
    }

    pub fn has_started(&self, now: i64) -> bool {
        now >= self.starts_at()
    }
}

/// What a waiting commitment waits for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WaitCondition {
    /// The clock reaches `until`.
    Until { until: i64 },
    /// A named event is observed (for example `machine_connected:mac` or
    /// `email_reply:<thread>`); the evaluator names events, the record only holds one.
    Event { event: String },
    /// The user answers.
    UserReply,
}

impl WaitCondition {
    /// Only the clock condition settles itself; events and replies are
    /// observed by the evaluator or a tool, which then clears `waiting_on`.
    pub fn is_met(&self, now: i64) -> bool {
        match self {
            Self::Until { until } => now >= *until,
            Self::Event { .. } | Self::UserReply => false,
        }
    }
}

/// Where the commitment came from.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Provenance {
    /// Entered by the user through the client or API.
    #[default]
    Manual,
    /// Made in a conversation.
    Chat {
        chat_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message_id: Option<String>,
    },
    /// Made by a proactive run (`activity/{run_id}.json`).
    Run { run_id: String },
}

/// Why a commitment counts as done. At least one of `confirmed_by_user`, a
/// non-empty `summary`, or a `run_id` is required to complete it, and a
/// `run_id` only counts when the store finds that activity record.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionEvidence {
    /// The user said it was done.
    #[serde(default)]
    pub confirmed_by_user: bool,
    /// What was observed: a reply, a file, a receipt. At most `MAX_NOTE_CHARS`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// The proactive run whose receipts show the work
    /// (`activity/{run_id}.json`); it must exist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// When the evidence was recorded.
    #[serde(default)]
    pub at: i64,
}

impl CompletionEvidence {
    /// Explicit user confirmation or recorded evidence; never an empty claim.
    pub fn is_sufficient(&self) -> bool {
        self.confirmed_by_user
            || self
                .summary
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty())
            || self.run_id.as_deref().is_some_and(|s| !s.trim().is_empty())
    }
}

/// What the last evaluation concluded. Written by the evaluator, and by the
/// store when it observes a change that asks for a check; kept on the record
/// so failed evaluations and pending observations stay inspectable after a
/// restart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CheckOutcome {
    /// Nothing changed; no contact was warranted.
    Unchanged,
    /// A proactive run was started for it.
    Triggered,
    Failed {
        error: String,
        retryable: bool,
    },
    /// An event the record waited for (`machine_connected:<id>`) or the
    /// completion of a dependency (`commitment_completed:<id>`) was observed
    /// and a check was asked for; the next evaluation states it as the
    /// trigger condition.
    Observed {
        event: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    pub at: i64,
    pub outcome: CheckOutcome,
    /// The activity record the check produced, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Commitment {
    pub version: u32,
    /// `cmt_<unix seconds>_<8 hex>`.
    pub id: String,
    pub promise: String,
    pub owner: Owner,
    /// The status last written. The store re-derives an open one against
    /// the clock on every read, so a record can read `due` before a write
    /// says so.
    pub status: CommitmentStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<Deadline>,
    /// Ids of commitments that must complete first.
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting_on: Option<WaitCondition>,
    /// When the evaluator should next look at it; the one schedule it owns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_check: Option<i64>,
    /// Linked continuity record ids (#81), kept as plain strings.
    #[serde(default)]
    pub continuity_ids: Vec<String>,
    pub provenance: Provenance,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion: Option<CompletionEvidence>,
    /// Not surfaced before this moment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snoozed_until: Option<i64>,
    #[serde(default)]
    pub snooze_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_check: Option<Check>,
    pub created_at: i64,
    pub updated_at: i64,
    /// When a write last changed `status`.
    pub status_changed_at: i64,
}

impl Commitment {
    pub fn is_open(&self) -> bool {
        self.status.is_open()
    }

    pub fn is_snoozed(&self, now: i64) -> bool {
        self.snoozed_until.is_some_and(|until| now < until)
    }

    /// The open status the record's own fields imply right now. `unfinished`
    /// answers whether a dependency id is still not completed. Due wins (it is
    /// time to act), then blocked, then waiting, then active.
    pub fn derive_open_status(
        &self,
        now: i64,
        unfinished: impl Fn(&str) -> bool,
    ) -> CommitmentStatus {
        if self.deadline.is_some_and(|d| d.has_started(now)) && !self.is_snoozed(now) {
            CommitmentStatus::Due
        } else if self.dependencies.iter().any(|id| unfinished(id)) {
            CommitmentStatus::Blocked
        } else if self.waiting_on.as_ref().is_some_and(|w| !w.is_met(now)) {
            CommitmentStatus::Waiting
        } else {
            CommitmentStatus::Active
        }
    }

    /// The moment the record itself asks to be looked at: the earliest of a
    /// timed wait and the deadline start. Event waits have no clock.
    pub fn default_next_check(&self) -> Option<i64> {
        let until = match &self.waiting_on {
            Some(WaitCondition::Until { until }) => Some(*until),
            _ => None,
        };
        let deadline = self.deadline.map(|d| d.starts_at());
        match (until, deadline) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// Whether the evaluator should look at it now: open, not snoozed, and
    /// its `next_check` has passed. `next_check` is the one schedule the
    /// record owns; the evaluator moves it forward after each check.
    pub fn needs_check(&self, now: i64) -> bool {
        self.is_open() && !self.is_snoozed(now) && self.next_check.is_some_and(|at| at <= now)
    }

    /// The next moment after `now` the record itself asks to be looked at:
    /// the earliest future one of a snooze end, a timed wait, and the
    /// deadline start. None once every moment it names has passed, so a
    /// deadline is checked exactly once.
    pub fn next_check_after(&self, now: i64) -> Option<i64> {
        let until = match &self.waiting_on {
            Some(WaitCondition::Until { until }) => Some(*until),
            _ => None,
        };
        [
            self.snoozed_until,
            until,
            self.deadline.map(|d| d.starts_at()),
        ]
        .into_iter()
        .flatten()
        .filter(|at| *at > now)
        .min()
    }
}
