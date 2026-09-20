//! The Resume my work ritual (#83), pure part: the policy the user sets, the
//! bounded suggestion the ritual persists, and the ranking that picks at
//! most one resumable task from the persisted continuity records and the
//! known machines. Nothing here reads a store, looks at a screen, or drives
//! a computer; the service decides when to rank and hands the pick to the
//! handoff card, where the user decides whether anything continues.

use serde::{Deserialize, Serialize};

use crate::domain::{
    continuity::{BlockerKind, ContinuityRecord, ContinuityState, Priority},
    handoff::{Environment, Severity, continuation_checks},
    machine::KnownMachine,
    proactive::MAX_REASON_CHARS,
};

pub const RESUME_FORMAT_VERSION: u32 = 1;
/// A record not updated for this long is stale and is never suggested.
pub const STALE_AFTER_SECS: i64 = 30 * 86_400;
/// Away for at least this long counts as a break.
pub const DEFAULT_BREAK_MINUTES: u32 = 120;
/// Minimum gap between two spontaneous suggestions, or after a refusal.
pub const DEFAULT_COOLDOWN_SECS: u64 = 3_600;
/// Most dismissed record ids kept; the oldest makes room.
pub const MAX_DISMISSED: usize = 200;

/// What asked the ritual to look for resumable work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RitualTrigger {
    /// The user pressed "Resume my work".
    Manual,
    /// The user opened Nolune after at least the configured break.
    OpenedAfterBreak { away_secs: i64 },
    /// A computer that waiting work names came back.
    MachineConnected { machine_id: String },
}

impl RitualTrigger {
    /// Spontaneous triggers wait for quiet hours, the cooldown, and a snooze
    /// to end; an explicit request does not.
    pub fn is_spontaneous(&self) -> bool {
        todo!("#83")
    }

    /// Why the suggestion appeared now, as one sentence naming the trigger.
    pub fn why_now(&self, machines: &[KnownMachine]) -> String {
        let _ = machines;
        todo!("#83")
    }
}

/// The user's controls, persisted beside the proactive policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ResumeRitualPolicy {
    /// Opt-in: off until the user turns the ritual on.
    pub enabled: bool,
    /// Away for at least this long counts as a break.
    pub break_minutes: u32,
    /// Minimum seconds between spontaneous suggestions, and after a refusal.
    pub cooldown_secs: u64,
    /// No spontaneous suggestion before this moment.
    pub snooze_until: Option<i64>,
    /// Records the user never wants suggested again.
    pub dismissed_record_ids: Vec<String>,
}

impl Default for ResumeRitualPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            break_minutes: DEFAULT_BREAK_MINUTES,
            cooldown_secs: DEFAULT_COOLDOWN_SECS,
            snooze_until: None,
            dismissed_record_ids: Vec::new(),
        }
    }
}

impl ResumeRitualPolicy {
    pub fn is_snoozed(&self, now: i64) -> bool {
        let _ = now;
        todo!("#83")
    }
}

/// One bounded suggestion: which record, why it was picked, and why now.
/// Persisted as the ritual's current offer; the card is derived on read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeSuggestion {
    /// `sug_<unix seconds>_<8 hex>`.
    pub id: String,
    pub record_id: String,
    pub goal: String,
    pub trigger: RitualTrigger,
    /// One sentence naming the trigger.
    pub why_now: String,
    /// The ranking's stated reason, at most 200 characters.
    pub why_this: String,
    /// The connected computer that can take the task right now.
    pub destination_id: String,
    pub suggested_at: i64,
}

/// What the ranking found: at most one record, with its reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub record_id: String,
    pub goal: String,
    /// Why this record over the others, at most 200 characters.
    pub why_this: String,
    /// A connected computer on which continuation would not be refused
    /// right now: one the record names when there is one.
    pub destination_id: String,
    pub score: i64,
}

/// Everything the ranking may look at besides the records.
#[derive(Debug, Clone, Copy)]
pub struct RankContext<'a> {
    pub now: i64,
    pub machines: &'a [KnownMachine],
    /// Record ids the user dismissed from the ritual.
    pub dismissed: &'a [String],
    /// Only records naming this computer are considered (the reconnect trigger).
    pub only_naming: Option<&'a str>,
}

/// Pick at most one record to offer. Closed, kept, dismissed, snoozed-away,
/// stale, and unreachable work never qualifies; among the rest, explicit
/// priority, the deadline, the state, stated blockers, a ready destination
/// the record names, and recency decide, in that order of weight.
pub fn rank(records: &[ContinuityRecord], ctx: &RankContext<'_>) -> Option<Candidate> {
    let _ = (records, ctx);
    todo!("#83")
}

/// The computers on which continuing `record` would not be refused right
/// now, judged by the handoff checks against a ready environment: the ones
/// the record names first (its origin ahead), then the rest.
pub fn ready_destinations<'a>(
    record: &ContinuityRecord,
    machines: &'a [KnownMachine],
) -> Vec<&'a KnownMachine> {
    let _ = (
        record,
        machines,
        Environment {
            model_ready: true,
            initiative_on: true,
        },
    );
    let _ = (Severity::Blocking, continuation_checks);
    todo!("#83")
}

/// A duration in words: "45 minutes", "3 hours", "2 days".
pub fn humanize_secs(secs: i64) -> String {
    let _ = secs;
    todo!("#83")
}

#[allow(dead_code)]
fn unused(_: BlockerKind, _: ContinuityState, _: Priority, _: usize) -> usize {
    MAX_REASON_CHARS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::continuity::{ContinuityUpdate, Origin, Provenance, ProvenanceSource};
    use cua_protocol::{MachineHealth, MachineLocation, Permission, PermissionState, Platform};

    /// 2026-01-05 (Monday) 09:00:00 UTC
    const T0: i64 = 1_767_603_600;
    const STUDIO: &str = "studio-id";
    const LAPTOP: &str = "laptop-id";

    fn machine(id: &str, name: &str, online: bool) -> KnownMachine {
        KnownMachine {
            machine_id: id.into(),
            display_name: name.into(),
            custom_name: None,
            hostname: name.into(),
            os: "macos".into(),
            platform: Some(Platform::Macos),
            location: MachineLocation::Desktop,
            screen_width: 2560,
            screen_height: 1440,
            permissions: Some(PermissionState {
                accessibility: Permission::Granted,
                screen_capture: Permission::Granted,
            }),
            capabilities: crate::domain::machine::LEGACY_DESKTOP_CAPABILITIES
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
            first_seen: T0 - 86_400,
            last_seen: if online { T0 } else { T0 - 3_600 },
            instance_slug: None,
            online,
            health: if online {
                MachineHealth::Healthy
            } else {
                MachineHealth::Unavailable
            },
            driver_version: None,
            cua_health: None,
        }
    }

    fn by(note: &str, at: i64) -> Provenance {
        Provenance {
            source: ProvenanceSource::Chat,
            at,
            note: note.into(),
        }
    }

    /// A record updated at `updated_at`, naming `machine_ids`.
    fn record(id: &str, goal: &str, updated_at: i64, machine_ids: &[&str]) -> ContinuityRecord {
        let mut record = ContinuityRecord::new(
            format!("task_{}_{id}", T0 - 86_400),
            goal,
            Origin {
                chat_id: "default".into(),
                message_id: None,
            },
            by("asked in chat", T0 - 86_400),
            T0 - 86_400,
        )
        .unwrap();
        record
            .apply(
                &ContinuityUpdate {
                    machine_ids: machine_ids.iter().map(|s| (*s).to_owned()).collect(),
                    next_step: Some("carry on".into()),
                    ..Default::default()
                },
                by("progress", updated_at),
                updated_at,
            )
            .unwrap();
        record
    }

    fn with(record: &mut ContinuityRecord, update: ContinuityUpdate) {
        let at = record.updated_at;
        record.apply(&update, by("detail", at), at).unwrap();
    }

    fn ctx<'a>(machines: &'a [KnownMachine], dismissed: &'a [String]) -> RankContext<'a> {
        RankContext {
            now: T0,
            machines,
            dismissed,
            only_naming: None,
        }
    }

    #[test]
    fn nothing_valid_yields_no_candidate() {
        let machines = [machine(STUDIO, "studio", true)];
        assert_eq!(rank(&[], &ctx(&machines, &[])), None);

        // Closed states never come back.
        let mut closed = Vec::new();
        for (index, state) in [
            ContinuityState::Completed,
            ContinuityState::Dismissed,
            ContinuityState::Failed,
        ]
        .into_iter()
        .enumerate()
        {
            let mut record = record(
                &format!("0000000{index}"),
                "closed work",
                T0 - 60,
                &[STUDIO],
            );
            with(
                &mut record,
                ContinuityUpdate {
                    state: Some(state),
                    ..Default::default()
                },
            );
            closed.push(record);
        }
        assert_eq!(rank(&closed, &ctx(&machines, &[])), None);

        // Kept or dismissed on the handoff card: the user already answered.
        let mut kept = record("0000kept", "kept work", T0 - 60, &[STUDIO]);
        kept.handoff = Some(crate::domain::continuity::HandoffDecision::Kept {
            machine_id: Some(STUDIO.into()),
            at: T0 - 30,
        });
        let mut dismissed = record("00dismis", "dismissed work", T0 - 60, &[STUDIO]);
        dismissed.handoff =
            Some(crate::domain::continuity::HandoffDecision::Dismissed { at: T0 - 30 });
        assert_eq!(rank(&[kept, dismissed], &ctx(&machines, &[])), None);

        // Dismissed from the ritual itself.
        let ritual_dismissed = record("0ritdism", "dismissed from the ritual", T0 - 60, &[STUDIO]);
        let ids = [ritual_dismissed.id.clone()];
        assert_eq!(rank(&[ritual_dismissed], &ctx(&machines, &ids)), None);

        // Stale: untouched for longer than the window.
        let stale = record("000stale", "old work", T0 - STALE_AFTER_SECS - 1, &[STUDIO]);
        assert_eq!(rank(&[stale], &ctx(&machines, &[])), None);
    }

    #[test]
    fn an_offline_or_unknown_destination_disqualifies_a_record() {
        // The only computer the task could run on is offline.
        let offline = [machine(STUDIO, "studio", false)];
        let task = record("0offline", "photos on studio", T0 - 60, &[STUDIO]);
        assert_eq!(rank(std::slice::from_ref(&task), &ctx(&offline, &[])), None);
        assert!(ready_destinations(&task, &offline).is_empty());

        // No computer at all.
        assert_eq!(rank(std::slice::from_ref(&task), &ctx(&[], &[])), None);

        // A connected computer that lacks a required capability cannot take it.
        let mut blind = machine(LAPTOP, "laptop", true);
        blind.capabilities.retain(|c| c != "screenshot");
        assert_eq!(rank(std::slice::from_ref(&task), &ctx(&[blind], &[])), None);

        // A connected computer whose heartbeat is stale is not ready either.
        let mut silent = machine(LAPTOP, "laptop", true);
        silent.health = MachineHealth::Degraded;
        assert_eq!(
            rank(std::slice::from_ref(&task), &ctx(&[silent], &[])),
            None
        );
    }

    #[test]
    fn a_task_can_be_offered_on_another_ready_computer_than_the_one_it_names() {
        let machines = [
            machine(STUDIO, "studio", false),
            machine(LAPTOP, "laptop", true),
        ];
        let task = record("0another", "photos on studio", T0 - 60, &[STUDIO]);
        let ready: Vec<&str> = ready_destinations(&task, &machines)
            .iter()
            .map(|m| m.machine_id.as_str())
            .collect();
        assert_eq!(ready, vec![LAPTOP]);
        let candidate = rank(std::slice::from_ref(&task), &ctx(&machines, &[])).unwrap();
        assert_eq!(candidate.record_id, task.id);
        assert_eq!(candidate.destination_id, LAPTOP);
        assert!(
            candidate.why_this.contains("laptop"),
            "the reason names the destination: {}",
            candidate.why_this
        );
    }

    #[test]
    fn the_named_computer_is_preferred_and_the_reason_says_so() {
        let machines = [
            machine(STUDIO, "studio", true),
            machine(LAPTOP, "laptop", true),
        ];
        let task = record("0onstudio", "photos on studio", T0 - 60, &[STUDIO]);
        let ready: Vec<&str> = ready_destinations(&task, &machines)
            .iter()
            .map(|m| m.machine_id.as_str())
            .collect();
        assert_eq!(
            ready,
            vec![STUDIO, LAPTOP],
            "the origin first, then the rest"
        );
        let candidate = rank(std::slice::from_ref(&task), &ctx(&machines, &[])).unwrap();
        assert_eq!(candidate.destination_id, STUDIO);
        assert!(
            candidate.why_this.contains("studio"),
            "{}",
            candidate.why_this
        );
    }

    #[test]
    fn exactly_one_candidate_with_priority_deadline_blockers_and_recency_weighed() {
        let machines = [machine(STUDIO, "studio", true)];

        let recent = record("00recent", "the most recent task", T0 - 300, &[STUDIO]);
        let mut urgent = record("00urgent", "the urgent task", T0 - 20 * 3_600, &[STUDIO]);
        with(
            &mut urgent,
            ContinuityUpdate {
                priority: Some(Priority::High),
                due_at: Some(T0 + 2 * 3_600),
                ..Default::default()
            },
        );
        let mut blocked = record("0blocked", "the blocked task", T0 - 60, &[STUDIO]);
        with(
            &mut blocked,
            ContinuityUpdate {
                blocker: Some("needs the external drive".into()),
                ..Default::default()
            },
        );
        let mut low = record("00000low", "the low task", T0 - 30, &[STUDIO]);
        with(
            &mut low,
            ContinuityUpdate {
                priority: Some(Priority::Low),
                ..Default::default()
            },
        );

        // Recency alone: the newest wins.
        let candidate = rank(&[blocked.clone(), recent.clone()], &ctx(&machines, &[])).unwrap();
        assert_eq!(candidate.record_id, recent.id);

        // A stated blocker costs more than a few minutes of recency buys.
        let candidate = rank(&[blocked.clone(), recent.clone()], &ctx(&machines, &[])).unwrap();
        assert_ne!(candidate.record_id, blocked.id);

        // Explicit priority and a deadline beat recency.
        let all = [recent.clone(), blocked.clone(), low.clone(), urgent.clone()];
        let candidate = rank(&all, &ctx(&machines, &[])).unwrap();
        assert_eq!(candidate.record_id, urgent.id);
        assert_eq!(candidate.goal, "the urgent task");
        for expected in ["high priority", "due in 2 hours", "studio"] {
            assert!(
                candidate.why_this.contains(expected),
                "why_this {:?} lacks {expected:?}",
                candidate.why_this
            );
        }
        assert!(candidate.why_this.chars().count() <= MAX_REASON_CHARS);

        // Low priority loses to a normal one even when it is newer.
        let candidate = rank(&[low.clone(), recent.clone()], &ctx(&machines, &[])).unwrap();
        assert_eq!(candidate.record_id, recent.id);

        // Overdue work is stated as such.
        let mut overdue = record("0overdue", "the overdue task", T0 - 3 * 86_400, &[STUDIO]);
        with(
            &mut overdue,
            ContinuityUpdate {
                due_at: Some(T0 - 2 * 86_400),
                ..Default::default()
            },
        );
        let candidate = rank(&[recent.clone(), overdue.clone()], &ctx(&machines, &[])).unwrap();
        assert_eq!(candidate.record_id, overdue.id);
        assert!(
            candidate.why_this.contains("overdue by 2 days"),
            "{}",
            candidate.why_this
        );

        // Ready-to-resume work is preferred over merely active work of the same age.
        let mut ready = record("000ready", "the ready task", T0 - 300, &[STUDIO]);
        with(
            &mut ready,
            ContinuityUpdate {
                state: Some(ContinuityState::ReadyToResume),
                ..Default::default()
            },
        );
        let candidate = rank(&[recent.clone(), ready.clone()], &ctx(&machines, &[])).unwrap();
        assert_eq!(candidate.record_id, ready.id);
        assert!(
            candidate.why_this.contains("ready to resume"),
            "{}",
            candidate.why_this
        );
    }

    #[test]
    fn ranking_is_deterministic_and_ties_break_on_recency_then_id() {
        let machines = [machine(STUDIO, "studio", true)];
        let a = record("0000000a", "task a", T0 - 60, &[STUDIO]);
        let b = record("0000000b", "task b", T0 - 60, &[STUDIO]);
        let c = record("0000000c", "task c", T0 - 30, &[STUDIO]);
        let first = rank(&[a.clone(), b.clone(), c.clone()], &ctx(&machines, &[])).unwrap();
        let second = rank(&[c.clone(), b.clone(), a.clone()], &ctx(&machines, &[])).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.record_id, c.id, "the newer one wins");
        let tie = rank(&[b.clone(), a.clone()], &ctx(&machines, &[])).unwrap();
        assert_eq!(tie.record_id, a.id, "then the smaller id");
    }

    #[test]
    fn the_reconnect_trigger_considers_only_work_naming_that_computer() {
        let machines = [
            machine(STUDIO, "studio", true),
            machine(LAPTOP, "laptop", true),
        ];
        let on_studio = record("0onstudi", "work on studio", T0 - 3_600, &[STUDIO]);
        let on_laptop = record("0onlapto", "work on laptop", T0 - 60, &[LAPTOP]);
        let records = [on_studio.clone(), on_laptop.clone()];
        let context = RankContext {
            only_naming: Some(STUDIO),
            ..ctx(&machines, &[])
        };
        let candidate = rank(&records, &context).unwrap();
        assert_eq!(
            candidate.record_id, on_studio.id,
            "newer work elsewhere does not count"
        );
        let none = RankContext {
            only_naming: Some("never-seen"),
            ..ctx(&machines, &[])
        };
        assert_eq!(rank(&records, &none), None);
    }

    #[test]
    fn triggers_explain_why_now_and_only_the_manual_one_is_explicit() {
        let machines = [machine(STUDIO, "Studio Mac", true)];
        assert!(!RitualTrigger::Manual.is_spontaneous());
        assert!(RitualTrigger::OpenedAfterBreak { away_secs: 7_200 }.is_spontaneous());
        assert!(
            RitualTrigger::MachineConnected {
                machine_id: STUDIO.into()
            }
            .is_spontaneous()
        );
        assert_eq!(
            RitualTrigger::Manual.why_now(&machines),
            "You asked to resume your work."
        );
        assert_eq!(
            RitualTrigger::OpenedAfterBreak {
                away_secs: 3 * 3_600
            }
            .why_now(&machines),
            "You opened Nolune after 3 hours away."
        );
        assert_eq!(
            RitualTrigger::MachineConnected {
                machine_id: STUDIO.into()
            }
            .why_now(&machines),
            "Studio Mac reconnected, and this task names it."
        );
        assert_eq!(
            RitualTrigger::MachineConnected {
                machine_id: "unknown".into()
            }
            .why_now(&machines),
            "unknown reconnected, and this task names it."
        );
        assert_eq!(humanize_secs(45 * 60), "45 minutes");
        assert_eq!(humanize_secs(3_600), "1 hour");
        assert_eq!(humanize_secs(2 * 86_400 + 3_600), "2 days");
        assert_eq!(humanize_secs(30), "less than a minute");
    }

    #[test]
    fn the_policy_is_opt_in_with_documented_defaults_and_a_snooze_that_ends() {
        let policy = ResumeRitualPolicy::default();
        assert!(!policy.enabled, "the ritual is opt-in");
        assert_eq!(policy.break_minutes, DEFAULT_BREAK_MINUTES);
        assert_eq!(policy.cooldown_secs, DEFAULT_COOLDOWN_SECS);
        assert!(!policy.is_snoozed(T0));
        let snoozed = ResumeRitualPolicy {
            snooze_until: Some(T0 + 60),
            ..policy
        };
        assert!(snoozed.is_snoozed(T0));
        assert!(!snoozed.is_snoozed(T0 + 60));
        let json = serde_json::to_string(&snoozed).unwrap();
        let back: ResumeRitualPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(back, snoozed);
        // A file written with fewer fields reads back with defaults.
        let back: ResumeRitualPolicy = serde_json::from_str(r#"{"enabled":true}"#).unwrap();
        assert!(back.enabled);
        assert_eq!(back.break_minutes, DEFAULT_BREAK_MINUTES);
    }
}
