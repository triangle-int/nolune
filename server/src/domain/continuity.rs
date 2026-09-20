//! Durable continuity records for explicit user tasks (#81).
//!
//! A record is the companion's bounded, inspectable memory of one unfinished
//! task: the goal, where it stands, which computers and resources it touches,
//! what already happened, what blocks it, and what to do next. Every write
//! carries provenance so the user can see where each step and decision came
//! from. Resources are kept as links (upload ids, memory paths, paths on a
//! computer), never copied. Records are written only by explicit task
//! activity, the `task_continuity_update` tool or the continuity API, and
//! never inferred from screenshots, check-ins, or other passive observation.

use serde::{Deserialize, Serialize};

pub const CONTINUITY_FORMAT_VERSION: u32 = 1;

/// Longest goal kept on a record.
pub const MAX_GOAL_CHARS: usize = 500;
/// Longest step, blocker, next step, or provenance note.
pub const MAX_NOTE_CHARS: usize = 300;
pub const MAX_STEPS: usize = 50;
pub const MAX_BLOCKERS: usize = 20;
pub const MAX_RESOURCES: usize = 40;
pub const MAX_MACHINES: usize = 16;
/// Provenance keeps the creating entry plus the most recent ones.
pub const MAX_PROVENANCE: usize = 100;
/// Largest file read back; bigger files are reported, never read. No record
/// built within the caps above can reach it, even with the most expensive
/// characters everywhere (`valid_content_never_reaches_the_file_cap`), so a
/// valid record can always be closed with one more provenance entry.
pub const MAX_RECORD_BYTES: usize = 1024 * 1024;
/// Longest path on a computer kept as a link.
pub const MAX_PATH_BYTES: usize = 1024;
/// Longest record id or machine id.
pub const MAX_ID_BYTES: usize = 64;
/// Longest chat id or message id kept as the origin.
pub const MAX_ORIGIN_BYTES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContinuityState {
    /// Being worked on right now.
    Active,
    /// Paused on something outside the companion's control (the user, a computer, a resource).
    Waiting,
    /// Everything needed is available again; the next step can start.
    ReadyToResume,
    Completed,
    Dismissed,
    Failed,
}

impl ContinuityState {
    pub const ALL: [Self; 6] = [
        Self::Active,
        Self::Waiting,
        Self::ReadyToResume,
        Self::Completed,
        Self::Dismissed,
        Self::Failed,
    ];

    /// Work the user may pick up again. Closed states never reappear as resumable.
    pub fn is_resumable(self) -> bool {
        matches!(self, Self::Active | Self::Waiting | Self::ReadyToResume)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProvenanceSource {
    /// The user, through the continuity API.
    User,
    /// A chat turn the task originated from.
    Chat,
    /// The `task_continuity_update` tool during explicit task work.
    Tool,
    /// The server itself: the reference check (a computer or resource went
    /// missing or came back) or the receipt of a finished continuation (#82).
    Server,
}

/// Who changed the record, when, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub source: ProvenanceSource,
    pub at: i64,
    pub note: String,
}

/// The conversation the task came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Origin {
    pub chat_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
}

/// A link to something the task needs. Contents are never copied here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResourceRef {
    /// A file in the companion's upload store.
    Upload { id: String },
    /// A path in the memory library.
    Memory { path: String },
    /// A path on a connected computer.
    MachinePath { machine_id: String, path: String },
}

impl ResourceRef {
    pub fn validate(&self) -> Result<(), ContinuityError> {
        use crate::services::resource_capability::CapabilityResource;
        match self {
            Self::Upload { id } => CapabilityResource::uploaded_file(id.as_str())
                .map(drop)
                .map_err(|_| invalid(format!("invalid upload id {id:?}"))),
            Self::Memory { path } => CapabilityResource::memory(path.as_str())
                .map(drop)
                .map_err(|_| invalid(format!("invalid memory path {path:?}"))),
            Self::MachinePath { machine_id, path } => {
                validate_machine_id(machine_id)?;
                if path.trim().is_empty()
                    || path.len() > MAX_PATH_BYTES
                    || path.chars().any(char::is_control)
                {
                    return Err(invalid(format!("invalid path {path:?} on {machine_id}")));
                }
                Ok(())
            }
        }
    }

    /// Short user-readable name, used in blocker details.
    pub fn describe(&self) -> String {
        match self {
            Self::Upload { id } => format!("upload {id}"),
            Self::Memory { path } => format!("memory {path}"),
            Self::MachinePath { machine_id, path } => format!("{path} on {machine_id}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceLink {
    pub resource: ResourceRef,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub summary: String,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BlockerKind {
    /// A computer the task needs is not connected.
    MachineUnavailable { machine_id: String },
    /// A linked resource cannot be found.
    ResourceMissing { resource: ResourceRef },
    /// Anything stated by the user or the tool.
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Blocker {
    pub kind: BlockerKind,
    pub detail: String,
    pub provenance: Provenance,
}

impl Blocker {
    /// Blockers the server adds and clears itself as references come and go.
    pub fn is_reference_check(&self) -> bool {
        !matches!(self.kind, BlockerKind::Other)
    }
}

/// What the user decided about picking the task up on a computer (#82).
/// `kept` and `dismissed` stop the handoff card from being offered until new
/// explicit work updates the record (`apply` clears them); `accepted` binds
/// the task to one stable machine id and names the activity run that
/// continues it. Only the handoff API writes this.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HandoffDecision {
    /// Continue on `machine_id`; `run_id` is the activity record of the continuation.
    Accepted {
        machine_id: String,
        run_id: String,
        at: i64,
        /// What the continuation did, once it finished.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outcome: Option<HandoffOutcome>,
    },
    /// Leave the task where it is; `machine_id` names the origin computer when there is one.
    Kept {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        machine_id: Option<String>,
        at: i64,
    },
    /// Stop offering the task until explicit work updates the record.
    Dismissed { at: i64 },
}

impl HandoffDecision {
    /// The computer the task is bound to after acceptance.
    pub fn bound_machine(&self) -> Option<&str> {
        match self {
            Self::Accepted { machine_id, .. } => Some(machine_id),
            Self::Kept { .. } | Self::Dismissed { .. } => None,
        }
    }

    /// Decisions that hide the card until explicit work updates the record.
    pub fn hides_card(&self) -> bool {
        matches!(self, Self::Kept { .. } | Self::Dismissed { .. })
    }
}

/// How the continuation run ended, as the activity record says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffOutcomeStatus {
    Completed,
    Failed,
    Cancelled,
}

/// The receipt of a finished continuation, appended to the accepted handoff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffOutcome {
    pub status: HandoffOutcomeStatus,
    pub finished_at: i64,
    /// Short user-readable summary: actions taken or why it stopped.
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContinuityRecord {
    pub version: u32,
    pub id: String,
    pub goal: String,
    pub state: ContinuityState,
    pub origin: Origin,
    #[serde(default)]
    pub machine_ids: Vec<String>,
    #[serde(default)]
    pub resources: Vec<ResourceLink>,
    #[serde(default)]
    pub completed_steps: Vec<Step>,
    #[serde(default)]
    pub blockers: Vec<Blocker>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_step: Option<String>,
    /// The user's handoff decision (#82), if any. Absent on records written
    /// before handoff cards existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handoff: Option<HandoffDecision>,
    pub created_at: i64,
    pub updated_at: i64,
    pub provenance: Vec<Provenance>,
}

/// One explicit change to a record. Lists are added to, never replaced.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ContinuityUpdate {
    pub goal: Option<String>,
    pub state: Option<ContinuityState>,
    pub completed_step: Option<String>,
    pub blocker: Option<String>,
    /// Drop stated blockers; reference-check blockers stay until the reference returns.
    pub clear_blockers: bool,
    /// `Some("")` clears the next step.
    pub next_step: Option<String>,
    pub machine_ids: Vec<String>,
    pub resources: Vec<ResourceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContinuityError {
    Invalid(String),
    TooLarge { bytes: usize, max: usize },
    NotFound,
    Io(String),
}

impl std::fmt::Display for ContinuityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) => f.write_str(message),
            Self::TooLarge { bytes, max } => {
                write!(f, "record is {bytes} bytes; the limit is {max}")
            }
            Self::NotFound => f.write_str("unknown continuity record"),
            Self::Io(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ContinuityError {}

impl ContinuityRecord {
    /// A fresh active record. `provenance` says who started it and why.
    pub fn new(
        id: String,
        goal: &str,
        origin: Origin,
        provenance: Provenance,
        now: i64,
    ) -> Result<Self, ContinuityError> {
        let goal = required(goal, MAX_GOAL_CHARS, "goal")?;
        let provenance = checked_provenance(provenance)?;
        let record = Self {
            version: CONTINUITY_FORMAT_VERSION,
            id,
            goal,
            state: ContinuityState::Active,
            origin,
            machine_ids: Vec::new(),
            resources: Vec::new(),
            completed_steps: Vec::new(),
            blockers: Vec::new(),
            next_step: None,
            handoff: None,
            created_at: now,
            updated_at: now,
            provenance: vec![provenance],
        };
        record.validate()?;
        Ok(record)
    }

    /// Apply one explicit change. Fails without touching `self` when the
    /// change is invalid or would exceed a cap.
    pub fn apply(
        &mut self,
        update: &ContinuityUpdate,
        provenance: Provenance,
        now: i64,
    ) -> Result<(), ContinuityError> {
        let provenance = checked_provenance(provenance)?;
        let mut next = self.clone();

        if let Some(goal) = &update.goal {
            next.goal = required(goal, MAX_GOAL_CHARS, "goal")?;
        }
        if let Some(state) = update.state {
            next.state = state;
        }
        if let Some(step) = &update.completed_step {
            next.completed_steps.push(Step {
                summary: required(step, MAX_NOTE_CHARS, "completed step")?,
                provenance: provenance.clone(),
            });
        }
        if update.clear_blockers {
            next.blockers.retain(Blocker::is_reference_check);
        }
        if let Some(blocker) = &update.blocker {
            next.blockers.push(Blocker {
                kind: BlockerKind::Other,
                detail: required(blocker, MAX_NOTE_CHARS, "blocker")?,
                provenance: provenance.clone(),
            });
        }
        if let Some(next_step) = &update.next_step {
            let next_step = bounded(next_step, MAX_NOTE_CHARS);
            next.next_step = (!next_step.is_empty()).then_some(next_step);
        }
        for machine_id in &update.machine_ids {
            validate_machine_id(machine_id)?;
            if !next.machine_ids.contains(machine_id) {
                next.machine_ids.push(machine_id.clone());
            }
        }
        for resource in &update.resources {
            resource.validate()?;
            if !next.resources.iter().any(|link| &link.resource == resource) {
                next.resources.push(ResourceLink {
                    resource: resource.clone(),
                    provenance: provenance.clone(),
                });
            }
        }
        // Explicit work brings a kept or dismissed handoff card back (#82);
        // an acceptance stays bound through the progress it records.
        if next
            .handoff
            .as_ref()
            .is_some_and(HandoffDecision::hides_card)
        {
            next.handoff = None;
        }
        next.touch(provenance, now);

        next.validate()?;
        *self = next;
        Ok(())
    }

    /// Stamp one write: the time and its provenance, bounded.
    fn touch(&mut self, provenance: Provenance, now: i64) {
        self.updated_at = now;
        self.provenance.push(provenance);
        if self.provenance.len() > MAX_PROVENANCE {
            // Keep the creating entry and the most recent ones.
            let excess = self.provenance.len() - MAX_PROVENANCE;
            self.provenance.drain(1..1 + excess);
        }
    }

    /// Every invariant a stored record must hold.
    pub fn validate(&self) -> Result<(), ContinuityError> {
        if self.version != CONTINUITY_FORMAT_VERSION {
            return Err(invalid(format!(
                "unsupported continuity format version {} (this server writes {CONTINUITY_FORMAT_VERSION})",
                self.version
            )));
        }
        if !is_valid_id(&self.id) {
            return Err(invalid(format!("invalid record id {:?}", self.id)));
        }
        if self.goal.trim().is_empty() {
            return Err(invalid("goal is required"));
        }
        if self.provenance.is_empty() {
            return Err(invalid("provenance is required"));
        }
        for entry in &self.provenance {
            if entry.note.trim().is_empty() {
                return Err(invalid("provenance note is required"));
            }
        }
        if self.origin.chat_id.trim().is_empty() {
            return Err(invalid("origin chat_id is required"));
        }
        if self.origin.chat_id.len() > MAX_ORIGIN_BYTES
            || self
                .origin
                .message_id
                .as_ref()
                .is_some_and(|id| id.len() > MAX_ORIGIN_BYTES)
        {
            return Err(invalid(format!(
                "origin ids exceed the limit of {MAX_ORIGIN_BYTES} bytes"
            )));
        }
        for (label, len, max) in [
            ("goal", self.goal.chars().count(), MAX_GOAL_CHARS),
            ("completed steps", self.completed_steps.len(), MAX_STEPS),
            ("blockers", self.blockers.len(), MAX_BLOCKERS),
            ("resources", self.resources.len(), MAX_RESOURCES),
            ("computers", self.machine_ids.len(), MAX_MACHINES),
            ("provenance", self.provenance.len(), MAX_PROVENANCE),
        ] {
            if len > max {
                return Err(invalid(format!("{label} exceed the limit of {max}")));
            }
        }
        for text in self
            .completed_steps
            .iter()
            .map(|step| &step.summary)
            .chain(self.blockers.iter().map(|blocker| &blocker.detail))
            .chain(self.provenance.iter().map(|entry| &entry.note))
            .chain(self.next_step.iter())
        {
            if text.chars().count() > MAX_NOTE_CHARS {
                return Err(invalid(format!(
                    "text exceeds the limit of {MAX_NOTE_CHARS} characters"
                )));
            }
        }
        for machine_id in &self.machine_ids {
            validate_machine_id(machine_id)?;
        }
        for link in &self.resources {
            link.resource.validate()?;
        }
        if let Some(handoff) = &self.handoff {
            handoff.validate()?;
        }
        Ok(())
    }

    // ── handoff (#82) ──────────────────────────────────────────────────────

    /// Record the user's handoff decision. Only a resumable record can be
    /// handed off. `accepted` also binds the computer (added to
    /// `machine_ids`) and makes the task active again; `kept` and
    /// `dismissed` hide the card until explicit work updates the record.
    /// Fails without touching `self`.
    pub fn decide_handoff(
        &mut self,
        decision: HandoffDecision,
        provenance: Provenance,
        now: i64,
    ) -> Result<(), ContinuityError> {
        let provenance = checked_provenance(provenance)?;
        if !self.state.is_resumable() {
            return Err(invalid("only a resumable task can be handed off"));
        }
        decision.validate()?;
        let mut next = self.clone();
        if let HandoffDecision::Accepted { machine_id, .. } = &decision {
            if !next.machine_ids.contains(machine_id) {
                next.machine_ids.push(machine_id.clone());
            }
            next.state = ContinuityState::Active;
        }
        next.handoff = Some(decision);
        next.touch(provenance, now);
        next.validate()?;
        *self = next;
        Ok(())
    }

    /// Append what the continuation `run_id` did. Only the accepted handoff
    /// bound to that run, and without an outcome yet, takes one, so a run is
    /// recorded on the record exactly once and never on a later acceptance.
    pub fn record_handoff_outcome(
        &mut self,
        run_id: &str,
        outcome: HandoffOutcome,
        provenance: Provenance,
        now: i64,
    ) -> Result<(), ContinuityError> {
        let provenance = checked_provenance(provenance)?;
        let outcome = HandoffOutcome {
            summary: required(&outcome.summary, MAX_NOTE_CHARS, "outcome summary")?,
            ..outcome
        };
        let mut next = self.clone();
        match &mut next.handoff {
            Some(HandoffDecision::Accepted {
                run_id: bound,
                outcome: slot @ None,
                ..
            }) if bound == run_id => *slot = Some(outcome),
            Some(HandoffDecision::Accepted { run_id: bound, .. }) if bound == run_id => {
                return Err(invalid("the continuation's outcome is already recorded"));
            }
            Some(HandoffDecision::Accepted { .. }) => {
                return Err(invalid(format!(
                    "the task is bound to another continuation than {run_id}"
                )));
            }
            _ => return Err(invalid("no accepted handoff to record an outcome for")),
        }
        next.touch(provenance, now);
        next.validate()?;
        *self = next;
        Ok(())
    }

    /// Whether a handoff card is offered for this record: resumable and not
    /// kept or dismissed since the last explicit update.
    pub fn handoff_offered(&self) -> bool {
        self.state.is_resumable()
            && !self
                .handoff
                .as_ref()
                .is_some_and(HandoffDecision::hides_card)
    }
}

impl HandoffDecision {
    fn validate(&self) -> Result<(), ContinuityError> {
        match self {
            Self::Accepted {
                machine_id, run_id, ..
            } => {
                validate_machine_id(machine_id)?;
                if !is_valid_id(run_id) {
                    return Err(invalid(format!("invalid run id {run_id:?}")));
                }
            }
            Self::Kept {
                machine_id: Some(machine_id),
                ..
            } => validate_machine_id(machine_id)?,
            Self::Kept {
                machine_id: None, ..
            }
            | Self::Dismissed { .. } => {}
        }
        if let Self::Accepted {
            outcome: Some(outcome),
            ..
        } = self
            && outcome.summary.chars().count() > MAX_NOTE_CHARS
        {
            return Err(invalid(format!(
                "outcome summary exceeds the limit of {MAX_NOTE_CHARS} characters"
            )));
        }
        Ok(())
    }
}

/// Record ids are one path component: `task_<unix seconds>_<8 hex>`.
pub fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_BYTES
        && !id.starts_with('.')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

fn validate_machine_id(machine_id: &str) -> Result<(), ContinuityError> {
    if machine_id.is_empty()
        || machine_id.len() > MAX_ID_BYTES
        || machine_id.starts_with('.')
        || machine_id.contains(['/', '\\', '\0'])
        || machine_id.chars().any(char::is_control)
    {
        return Err(invalid(format!("invalid machine id {machine_id:?}")));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> ContinuityError {
    ContinuityError::Invalid(message.into())
}

fn bounded(text: &str, max: usize) -> String {
    text.trim().chars().take(max).collect()
}

fn required(text: &str, max: usize, label: &str) -> Result<String, ContinuityError> {
    let text = bounded(text, max);
    if text.is_empty() {
        return Err(invalid(format!("{label} is required")));
    }
    Ok(text)
}

fn checked_provenance(provenance: Provenance) -> Result<Provenance, ContinuityError> {
    Ok(Provenance {
        note: required(&provenance.note, MAX_NOTE_CHARS, "provenance note")?,
        ..provenance
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_767_603_600;

    fn by(source: ProvenanceSource, note: &str) -> Provenance {
        Provenance {
            source,
            at: T0,
            note: note.into(),
        }
    }

    fn origin() -> Origin {
        Origin {
            chat_id: "default".into(),
            message_id: Some("msg_1".into()),
        }
    }

    fn record() -> ContinuityRecord {
        ContinuityRecord::new(
            "task_1767603600_0badcafe".into(),
            "rename the photos from the trip",
            origin(),
            by(ProvenanceSource::Chat, "user asked in chat"),
            T0,
        )
        .unwrap()
    }

    #[test]
    fn every_state_round_trips_losslessly_through_the_versioned_format() {
        for state in ContinuityState::ALL {
            let mut record = record();
            record
                .apply(
                    &ContinuityUpdate {
                        state: Some(state),
                        completed_step: Some("listed the folder".into()),
                        blocker: Some("needs the external drive".into()),
                        next_step: Some("rename IMG_* files".into()),
                        machine_ids: vec!["mac-mini".into()],
                        resources: vec![
                            ResourceRef::Upload {
                                id: "upload_1".into(),
                            },
                            ResourceRef::Memory {
                                path: "notes/trip.md".into(),
                            },
                            ResourceRef::MachinePath {
                                machine_id: "mac-mini".into(),
                                path: "/Volumes/Trip".into(),
                            },
                        ],
                        ..Default::default()
                    },
                    by(ProvenanceSource::Tool, "progress"),
                    T0 + 1,
                )
                .unwrap();
            let json = serde_json::to_string_pretty(&record).unwrap();
            let back: ContinuityRecord = serde_json::from_str(&json).unwrap();
            assert_eq!(back, record, "{state:?}");
            assert_eq!(back.version, CONTINUITY_FORMAT_VERSION);
            assert!(json.contains("\"version\": 1"));
            let name = serde_json::to_value(state).unwrap();
            assert!(json.contains(&format!("\"state\": {name}")), "{json}");
            assert_eq!(
                state.is_resumable(),
                matches!(
                    state,
                    ContinuityState::Active
                        | ContinuityState::Waiting
                        | ContinuityState::ReadyToResume
                )
            );
        }
    }

    #[test]
    fn a_new_record_is_active_and_carries_its_origin_and_provenance() {
        let record = record();
        assert_eq!(record.state, ContinuityState::Active);
        assert_eq!(record.origin, origin());
        assert_eq!(record.created_at, T0);
        assert_eq!(record.updated_at, T0);
        assert_eq!(record.provenance.len(), 1);
        assert_eq!(record.provenance[0].source, ProvenanceSource::Chat);
        assert!(record.machine_ids.is_empty());
        assert!(record.blockers.is_empty());
        assert!(record.next_step.is_none());
        record.validate().unwrap();

        let long_goal = "g".repeat(MAX_GOAL_CHARS + 20);
        let bounded = ContinuityRecord::new(
            "task_1767603600_00000001".into(),
            &long_goal,
            origin(),
            by(ProvenanceSource::User, "typed"),
            T0,
        )
        .unwrap();
        assert_eq!(bounded.goal.chars().count(), MAX_GOAL_CHARS);
    }

    #[test]
    fn every_write_needs_a_goal_and_a_provenance_note() {
        for goal in ["", "   "] {
            assert!(matches!(
                ContinuityRecord::new(
                    "task_1767603600_00000001".into(),
                    goal,
                    origin(),
                    by(ProvenanceSource::User, "typed"),
                    T0,
                ),
                Err(ContinuityError::Invalid(_))
            ));
        }
        assert!(matches!(
            ContinuityRecord::new(
                "task_1767603600_00000001".into(),
                "goal",
                origin(),
                by(ProvenanceSource::User, "  "),
                T0,
            ),
            Err(ContinuityError::Invalid(_))
        ));

        let mut unchanged = record();
        let before = unchanged.clone();
        let result = unchanged.apply(
            &ContinuityUpdate {
                state: Some(ContinuityState::Waiting),
                ..Default::default()
            },
            by(ProvenanceSource::Tool, ""),
            T0 + 5,
        );
        assert!(matches!(result, Err(ContinuityError::Invalid(_))));
        assert_eq!(unchanged, before, "a rejected update changes nothing");

        let mut stripped = record();
        stripped.provenance.clear();
        assert!(matches!(
            stripped.validate(),
            Err(ContinuityError::Invalid(_))
        ));
        let mut wrong_version = record();
        wrong_version.version = CONTINUITY_FORMAT_VERSION + 1;
        assert!(matches!(
            wrong_version.validate(),
            Err(ContinuityError::Invalid(_))
        ));
    }

    #[test]
    fn updates_append_steps_blockers_links_and_provenance_with_bounds() {
        let mut record = record();
        let update = ContinuityUpdate {
            goal: Some("  rename the photos  ".into()),
            state: Some(ContinuityState::Waiting),
            completed_step: Some("x".repeat(MAX_NOTE_CHARS + 10)),
            blocker: Some("drive not mounted".into()),
            next_step: Some("mount the drive".into()),
            machine_ids: vec!["mac-mini".into(), "mac-mini".into()],
            resources: vec![
                ResourceRef::Upload {
                    id: "upload_1".into(),
                },
                ResourceRef::Upload {
                    id: "upload_1".into(),
                },
            ],
            ..Default::default()
        };
        record
            .apply(&update, by(ProvenanceSource::Tool, "first pass"), T0 + 10)
            .unwrap();
        assert_eq!(record.goal, "rename the photos");
        assert_eq!(record.state, ContinuityState::Waiting);
        assert_eq!(record.completed_steps.len(), 1);
        assert_eq!(
            record.completed_steps[0].summary.chars().count(),
            MAX_NOTE_CHARS
        );
        assert_eq!(record.completed_steps[0].provenance.note, "first pass");
        assert_eq!(record.blockers.len(), 1);
        assert_eq!(record.blockers[0].kind, BlockerKind::Other);
        assert_eq!(record.blockers[0].detail, "drive not mounted");
        assert_eq!(record.next_step.as_deref(), Some("mount the drive"));
        assert_eq!(record.machine_ids, vec!["mac-mini"], "deduplicated");
        assert_eq!(record.resources.len(), 1, "deduplicated");
        assert_eq!(
            record.resources[0].provenance.source,
            ProvenanceSource::Tool
        );
        assert_eq!(record.updated_at, T0 + 10);
        assert_eq!(record.created_at, T0);
        assert_eq!(record.provenance.len(), 2);

        // Clearing: stated blockers go, the next step can be emptied.
        record
            .apply(
                &ContinuityUpdate {
                    clear_blockers: true,
                    next_step: Some(String::new()),
                    ..Default::default()
                },
                by(ProvenanceSource::User, "mounted it"),
                T0 + 20,
            )
            .unwrap();
        assert!(record.blockers.is_empty());
        assert!(record.next_step.is_none());

        // A reference-check blocker survives clear_blockers.
        record.blockers.push(Blocker {
            kind: BlockerKind::MachineUnavailable {
                machine_id: "mac-mini".into(),
            },
            detail: "computer mac-mini is not connected".into(),
            provenance: by(ProvenanceSource::Server, "reference check"),
        });
        record
            .apply(
                &ContinuityUpdate {
                    clear_blockers: true,
                    ..Default::default()
                },
                by(ProvenanceSource::User, "again"),
                T0 + 30,
            )
            .unwrap();
        assert_eq!(record.blockers.len(), 1);
        assert!(record.blockers[0].is_reference_check());

        // Caps are enforced without partial writes.
        for i in 0..(MAX_STEPS - 1) {
            record
                .apply(
                    &ContinuityUpdate {
                        completed_step: Some(format!("step {i}")),
                        ..Default::default()
                    },
                    by(ProvenanceSource::Tool, "step"),
                    T0 + 100 + i as i64,
                )
                .unwrap();
        }
        assert_eq!(record.completed_steps.len(), MAX_STEPS);
        let before = record.clone();
        assert!(matches!(
            record.apply(
                &ContinuityUpdate {
                    completed_step: Some("one too many".into()),
                    ..Default::default()
                },
                by(ProvenanceSource::Tool, "step"),
                T0 + 999,
            ),
            Err(ContinuityError::Invalid(_))
        ));
        assert_eq!(record, before);
        assert!(record.provenance.len() <= MAX_PROVENANCE);
        assert_eq!(
            record.provenance[0].note, "user asked in chat",
            "the creating entry is never trimmed"
        );
        assert_eq!(record.provenance.last().unwrap().note, "step");
    }

    /// Every field at its cap, filled with the characters that take the most
    /// bytes once serialized: a character-capped text of control characters
    /// escapes to six bytes per character, a byte-capped path of quotes
    /// doubles, and the origin ids escape to six bytes per byte.
    fn maximal_record() -> ContinuityRecord {
        let worst = || "\u{1}".repeat(MAX_NOTE_CHARS);
        let prov = |_: usize| Provenance {
            source: ProvenanceSource::Server,
            at: i64::MAX,
            note: worst(),
        };
        let machine_id = |i: usize| format!("{i:02}{}", "\"".repeat(MAX_ID_BYTES - 2));
        let path = |i: usize| format!("{i:02}{}", "\"".repeat(MAX_PATH_BYTES - 2));
        let mut record = ContinuityRecord::new(
            "x".repeat(MAX_ID_BYTES),
            &"\u{1}".repeat(MAX_GOAL_CHARS),
            Origin {
                chat_id: "\u{1}".repeat(MAX_ORIGIN_BYTES),
                message_id: Some("\u{1}".repeat(MAX_ORIGIN_BYTES)),
            },
            prov(0),
            i64::MAX,
        )
        .unwrap();
        record.state = ContinuityState::ReadyToResume;
        record.machine_ids = (0..MAX_MACHINES).map(machine_id).collect();
        record.resources = (0..MAX_RESOURCES)
            .map(|i| ResourceLink {
                resource: ResourceRef::MachinePath {
                    machine_id: machine_id(i),
                    path: path(i),
                },
                provenance: prov(i),
            })
            .collect();
        record.completed_steps = (0..MAX_STEPS)
            .map(|i| Step {
                summary: worst(),
                provenance: prov(i + 1),
            })
            .collect();
        record.blockers = (0..MAX_BLOCKERS)
            .map(|i| Blocker {
                kind: BlockerKind::ResourceMissing {
                    resource: ResourceRef::MachinePath {
                        machine_id: machine_id(i),
                        path: path(i),
                    },
                },
                detail: worst(),
                provenance: prov(i + 2),
            })
            .collect();
        record.next_step = Some(worst());
        record.handoff = Some(HandoffDecision::Accepted {
            machine_id: machine_id(0),
            run_id: "x".repeat(MAX_ID_BYTES),
            at: i64::MAX,
            outcome: Some(HandoffOutcome {
                status: HandoffOutcomeStatus::Completed,
                finished_at: i64::MAX,
                summary: worst(),
            }),
        });
        record.updated_at = i64::MAX;
        record.provenance = (0..MAX_PROVENANCE).map(prov).collect();
        record
    }

    #[test]
    fn valid_content_never_reaches_the_file_cap() {
        let record = maximal_record();
        record.validate().unwrap();
        let json = serde_json::to_string_pretty(&record).unwrap();
        assert!(
            json.len() <= MAX_RECORD_BYTES,
            "a record built within every cap is {} bytes, over the {MAX_RECORD_BYTES}-byte file cap",
            json.len()
        );
        let back: ContinuityRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back, record);
    }

    #[test]
    fn resource_links_and_ids_are_validated_as_single_path_components() {
        for bad in [
            ResourceRef::Upload { id: "../x".into() },
            ResourceRef::Upload { id: String::new() },
            ResourceRef::Memory {
                path: "/etc/passwd".into(),
            },
            ResourceRef::Memory {
                path: "notes/../../x".into(),
            },
            ResourceRef::MachinePath {
                machine_id: String::new(),
                path: "/tmp".into(),
            },
            ResourceRef::MachinePath {
                machine_id: "mac/mini".into(),
                path: "/tmp".into(),
            },
            ResourceRef::MachinePath {
                machine_id: "mac-mini".into(),
                path: String::new(),
            },
            ResourceRef::MachinePath {
                machine_id: "mac-mini".into(),
                path: "/tmp/\u{1}".into(),
            },
            ResourceRef::MachinePath {
                machine_id: "mac-mini".into(),
                path: "/".repeat(MAX_PATH_BYTES + 1),
            },
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
            let mut record = record();
            let before = record.clone();
            assert!(
                record
                    .apply(
                        &ContinuityUpdate {
                            resources: vec![bad.clone()],
                            ..Default::default()
                        },
                        by(ProvenanceSource::Tool, "link"),
                        T0 + 1,
                    )
                    .is_err()
            );
            assert_eq!(record, before);
        }
        assert!(
            ResourceRef::Memory {
                path: "notes/trip.md".into()
            }
            .validate()
            .is_ok()
        );
        assert_eq!(
            ResourceRef::MachinePath {
                machine_id: "mac-mini".into(),
                path: "/Volumes/Trip".into(),
            }
            .describe(),
            "/Volumes/Trip on mac-mini"
        );

        let mut record = record();
        assert!(
            record
                .apply(
                    &ContinuityUpdate {
                        machine_ids: vec!["a/b".into()],
                        ..Default::default()
                    },
                    by(ProvenanceSource::Tool, "link"),
                    T0 + 1,
                )
                .is_err()
        );

        assert!(is_valid_id("task_1767603600_0badcafe"));
        for bad in ["", "../x", "a/b", ".hidden", "task_1\\2", &"x".repeat(65)] {
            assert!(!is_valid_id(bad), "{bad:?}");
        }

        // The origin is bounded too, so nothing on a record is open-ended.
        let mut wide = self::record();
        wide.origin.chat_id = "c".repeat(MAX_ORIGIN_BYTES + 1);
        assert!(matches!(wide.validate(), Err(ContinuityError::Invalid(_))));
        let mut wide = self::record();
        wide.origin.message_id = Some("m".repeat(MAX_ORIGIN_BYTES + 1));
        assert!(matches!(wide.validate(), Err(ContinuityError::Invalid(_))));
    }

    // ── handoff (#82) ──────────────────────────────────────────────────────

    fn accepted(machine_id: &str) -> HandoffDecision {
        HandoffDecision::Accepted {
            machine_id: machine_id.into(),
            run_id: "run_1767603700_0badcafe".into(),
            at: T0 + 100,
            outcome: None,
        }
    }

    #[test]
    fn accepting_a_handoff_binds_the_computer_and_reactivates_the_task() {
        let mut record = record();
        record
            .apply(
                &ContinuityUpdate {
                    state: Some(ContinuityState::Waiting),
                    machine_ids: vec!["mac-a".into()],
                    ..Default::default()
                },
                by(ProvenanceSource::Tool, "paused"),
                T0 + 1,
            )
            .unwrap();

        record
            .decide_handoff(
                accepted("mac-b"),
                by(ProvenanceSource::User, "continue on Studio Mac"),
                T0 + 100,
            )
            .unwrap();
        assert_eq!(record.state, ContinuityState::Active);
        assert_eq!(record.machine_ids, vec!["mac-a", "mac-b"]);
        assert_eq!(record.handoff, Some(accepted("mac-b")));
        assert_eq!(
            record.handoff.as_ref().unwrap().bound_machine(),
            Some("mac-b")
        );
        assert_eq!(record.updated_at, T0 + 100);
        assert_eq!(
            record.provenance.last().unwrap().note,
            "continue on Studio Mac"
        );
        assert_eq!(
            record.provenance.last().unwrap().source,
            ProvenanceSource::User
        );
        assert!(
            record.handoff_offered(),
            "an accepted task is still offered"
        );

        // An invalid computer changes nothing.
        let before = record.clone();
        assert!(
            record
                .decide_handoff(
                    accepted("a/b"),
                    by(ProvenanceSource::User, "continue"),
                    T0 + 101
                )
                .is_err()
        );
        assert_eq!(record, before);

        // A closed task cannot be handed off.
        let mut done = self::record();
        done.apply(
            &ContinuityUpdate {
                state: Some(ContinuityState::Completed),
                ..Default::default()
            },
            by(ProvenanceSource::User, "done"),
            T0 + 1,
        )
        .unwrap();
        let before = done.clone();
        assert!(matches!(
            done.decide_handoff(
                accepted("mac-b"),
                by(ProvenanceSource::User, "continue"),
                T0 + 2
            ),
            Err(ContinuityError::Invalid(_))
        ));
        assert_eq!(done, before);
        assert!(!done.handoff_offered());
    }

    #[test]
    fn kept_and_dismissed_handoffs_hide_the_card_until_explicit_work() {
        let mut record = record();
        assert!(record.handoff_offered());

        record
            .decide_handoff(
                HandoffDecision::Dismissed { at: T0 + 10 },
                by(ProvenanceSource::User, "dismissed"),
                T0 + 10,
            )
            .unwrap();
        assert!(!record.handoff_offered());
        assert_eq!(
            record.state,
            ContinuityState::Active,
            "the record itself stays resumable"
        );
        assert_eq!(record.provenance.last().unwrap().note, "dismissed");

        // A server reference check never counts as explicit work: it edits
        // blockers directly, without `apply`, so the decision stays.
        record.blockers.push(Blocker {
            kind: BlockerKind::MachineUnavailable {
                machine_id: "mac-a".into(),
            },
            detail: "computer mac-a is not connected".into(),
            provenance: by(ProvenanceSource::Server, "reference check"),
        });
        assert!(!record.handoff_offered());

        // Explicit work (the tool or a PUT) clears the dismissal.
        record
            .apply(
                &ContinuityUpdate {
                    completed_step: Some("found the folder".into()),
                    ..Default::default()
                },
                by(ProvenanceSource::Tool, "progress"),
                T0 + 20,
            )
            .unwrap();
        assert_eq!(record.handoff, None);
        assert!(record.handoff_offered());

        record
            .decide_handoff(
                HandoffDecision::Kept {
                    machine_id: Some("mac-a".into()),
                    at: T0 + 30,
                },
                by(ProvenanceSource::User, "kept on mac-a"),
                T0 + 30,
            )
            .unwrap();
        assert!(!record.handoff_offered());
        assert!(record.handoff.as_ref().unwrap().hides_card());
        record
            .apply(
                &ContinuityUpdate {
                    next_step: Some("rename the files".into()),
                    ..Default::default()
                },
                by(ProvenanceSource::User, "edited"),
                T0 + 40,
            )
            .unwrap();
        assert!(record.handoff_offered());

        // An acceptance survives explicit work: progress recorded during the
        // continuation must not unbind it.
        record
            .decide_handoff(
                accepted("mac-b"),
                by(ProvenanceSource::User, "continue on mac-b"),
                T0 + 50,
            )
            .unwrap();
        record
            .apply(
                &ContinuityUpdate {
                    completed_step: Some("renamed 3 files".into()),
                    ..Default::default()
                },
                by(ProvenanceSource::Tool, "progress"),
                T0 + 60,
            )
            .unwrap();
        assert_eq!(record.handoff, Some(accepted("mac-b")));
    }

    #[test]
    fn a_continuation_outcome_is_recorded_exactly_once_on_the_accepted_handoff() {
        let outcome = HandoffOutcome {
            status: HandoffOutcomeStatus::Completed,
            finished_at: T0 + 200,
            summary: "2 actions".into(),
        };
        let mut record = record();
        let before = record.clone();
        assert!(matches!(
            record.record_handoff_outcome(
                "run_1767603700_0badcafe",
                outcome.clone(),
                by(ProvenanceSource::Server, "finished"),
                T0 + 200
            ),
            Err(ContinuityError::Invalid(_))
        ));
        assert_eq!(record, before, "no outcome without an acceptance");

        record
            .decide_handoff(
                accepted("mac-b"),
                by(ProvenanceSource::User, "continue on mac-b"),
                T0 + 100,
            )
            .unwrap();
        let before = record.clone();
        assert!(matches!(
            record.record_handoff_outcome(
                "run_1767603700_00000000",
                outcome.clone(),
                by(ProvenanceSource::Server, "finished"),
                T0 + 200
            ),
            Err(ContinuityError::Invalid(_))
        ));
        assert_eq!(record, before, "an outcome from another run never lands");
        record
            .record_handoff_outcome(
                "run_1767603700_0badcafe",
                outcome.clone(),
                by(
                    ProvenanceSource::Server,
                    "continuation on mac-b completed: 2 actions",
                ),
                T0 + 200,
            )
            .unwrap();
        assert_eq!(
            record.handoff,
            Some(HandoffDecision::Accepted {
                machine_id: "mac-b".into(),
                run_id: "run_1767603700_0badcafe".into(),
                at: T0 + 100,
                outcome: Some(outcome.clone()),
            })
        );
        assert_eq!(record.updated_at, T0 + 200);
        assert_eq!(
            record.provenance.last().unwrap().note,
            "continuation on mac-b completed: 2 actions"
        );

        // A second outcome for the same acceptance is refused unchanged.
        let before = record.clone();
        assert!(
            record
                .record_handoff_outcome(
                    "run_1767603700_0badcafe",
                    HandoffOutcome {
                        status: HandoffOutcomeStatus::Failed,
                        finished_at: T0 + 300,
                        summary: "again".into(),
                    },
                    by(ProvenanceSource::Server, "finished again"),
                    T0 + 300,
                )
                .is_err()
        );
        assert_eq!(record, before);

        // A summary is bounded like every other note, and never empty.
        let mut fresh = self::record();
        fresh
            .decide_handoff(
                accepted("mac-b"),
                by(ProvenanceSource::User, "continue"),
                T0 + 100,
            )
            .unwrap();
        assert!(
            fresh
                .record_handoff_outcome(
                    "run_1767603700_0badcafe",
                    HandoffOutcome {
                        summary: "   ".into(),
                        ..outcome.clone()
                    },
                    by(ProvenanceSource::Server, "finished"),
                    T0 + 200,
                )
                .is_err()
        );
        fresh
            .record_handoff_outcome(
                "run_1767603700_0badcafe",
                HandoffOutcome {
                    summary: "x".repeat(MAX_NOTE_CHARS + 1),
                    ..outcome
                },
                by(ProvenanceSource::Server, "finished"),
                T0 + 200,
            )
            .unwrap();
        let Some(HandoffDecision::Accepted {
            outcome: Some(stored),
            ..
        }) = &fresh.handoff
        else {
            panic!("outcome recorded");
        };
        assert_eq!(stored.summary.chars().count(), MAX_NOTE_CHARS);
        fresh.validate().unwrap();
    }

    #[test]
    fn handoff_decisions_round_trip_through_the_versioned_format() {
        for decision in [
            accepted("mac-b"),
            HandoffDecision::Accepted {
                machine_id: "mac-b".into(),
                run_id: "run_1767603700_0badcafe".into(),
                at: T0 + 100,
                outcome: Some(HandoffOutcome {
                    status: HandoffOutcomeStatus::Failed,
                    finished_at: T0 + 200,
                    summary: "no model turn ran".into(),
                }),
            },
            HandoffDecision::Kept {
                machine_id: Some("mac-a".into()),
                at: T0 + 10,
            },
            HandoffDecision::Kept {
                machine_id: None,
                at: T0 + 10,
            },
            HandoffDecision::Dismissed { at: T0 + 10 },
        ] {
            let mut record = record();
            record.handoff = Some(decision.clone());
            record.validate().unwrap();
            let json = serde_json::to_string_pretty(&record).unwrap();
            let back: ContinuityRecord = serde_json::from_str(&json).unwrap();
            assert_eq!(back.handoff, Some(decision));
        }
        // A record written before handoff cards existed reads back without one.
        let mut json = serde_json::to_value(record()).unwrap();
        json.as_object_mut().unwrap().remove("handoff");
        let back: ContinuityRecord = serde_json::from_value(json).unwrap();
        assert_eq!(back.handoff, None);
        assert!(back.handoff_offered());
    }
}
