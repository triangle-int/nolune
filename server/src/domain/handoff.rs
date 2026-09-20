//! The reviewable handoff (#82): what a user sees when picking an unfinished
//! task up on a computer, derived from one continuity record and the known
//! machines. The card names the goal, the computer the task started on,
//! what already happened, the resources it links, its blockers, the proposed
//! next step, and what the destination must be able to do. It is built from
//! the persisted record and the persisted machine list, so it stays useful
//! while the origin computer is offline.
//!
//! Everything here is pure: nothing reads a store, and nothing drives a
//! computer. The checks say what would stop a continuation before any work
//! starts; the handoff service runs them again at acceptance.

use cua_protocol::{MachineHealth, Permission, PermissionKind, PermissionState, Platform};
use serde::Serialize;

use crate::domain::{
    continuity::{ContinuityRecord, ContinuityState, HandoffDecision, ResourceRef},
    machine::KnownMachine,
};

/// Capabilities every continuation on a computer needs: seeing the screen
/// and acting on it.
pub const COMPUTER_USE_CAPABILITIES: [&str; 4] = ["screenshot", "left_click", "type", "key"];
/// Capabilities a task that links files on a computer also needs.
pub const FILE_CAPABILITIES: [&str; 2] = ["file_read", "file_list"];
/// Desktop permissions computer use needs.
pub const REQUIRED_PERMISSIONS: [PermissionKind; 2] =
    [PermissionKind::ScreenCapture, PermissionKind::Accessibility];

/// A computer as the card shows it: the persisted record when the machine
/// is known, otherwise just its id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ComputerSummary {
    pub machine_id: String,
    /// The user's name, the hostname, or the id when the machine is unknown.
    pub display_name: String,
    /// Whether the machine list has a record for this id.
    pub known: bool,
    pub online: bool,
    pub health: MachineHealth,
    pub platform: Option<Platform>,
    /// Unix seconds of the last heartbeat or disconnect; `None` when unknown.
    pub last_seen: Option<i64>,
}

/// What the destination must be able to do for this task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Requirements {
    pub capabilities: Vec<String>,
    pub permissions: Vec<PermissionKind>,
}

/// One linked resource with what the reference check last said about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CardResource {
    pub resource: ResourceRef,
    pub label: String,
    /// `false` when the record carries a `resource_missing` blocker for it.
    pub available: bool,
}

/// The handoff card: everything a user needs to review the task and decide.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HandoffCard {
    pub record_id: String,
    pub goal: String,
    pub state: ContinuityState,
    /// The conversation the task came from; the continuation runs there.
    pub origin_chat_id: String,
    /// The computer the task started on: the first computer the record
    /// names. `None` for a task that has not touched a computer yet.
    pub origin: Option<ComputerSummary>,
    pub completed_steps: Vec<String>,
    pub resources: Vec<CardResource>,
    pub blockers: Vec<String>,
    pub next_step: Option<String>,
    pub required: Requirements,
    pub decision: Option<HandoffDecision>,
    /// The computer the task is bound to after acceptance.
    pub bound_to: Option<ComputerSummary>,
    /// Whether the card is offered right now (resumable, not kept or dismissed).
    pub offered: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// How a check affects continuation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Continuation is refused until this changes.
    Blocking,
    /// Continuation may go ahead; the user is told it will ask for this.
    Approval,
    /// Information only.
    Note,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CheckKind {
    /// The record is completed, dismissed, or failed.
    RecordClosed,
    /// The machine list has no record for the destination.
    MachineUnknown,
    MachineOffline,
    /// Connected, but its heartbeat is stale.
    MachineNotResponding,
    CapabilityMissing {
        capability: String,
    },
    PermissionDenied {
        permission: PermissionKind,
    },
    /// The desktop will ask for this permission when work starts.
    PermissionPrompt {
        permission: PermissionKind,
    },
    /// The desktop did not report its permissions.
    PermissionsUnknown,
    /// An upload, memory path, or file the reference check could not find.
    ResourceMissing {
        resource: ResourceRef,
    },
    /// A file on another computer than the destination.
    ResourceElsewhere {
        resource: ResourceRef,
        machine_id: String,
    },
    /// No chat model is configured, so no continuation can run.
    ModelUnavailable,
    /// Initiative is off, so the loop admits no run.
    InitiativeOff,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContinuationCheck {
    pub kind: CheckKind,
    pub severity: Severity,
    pub detail: String,
}

/// What the server knows besides the record and the machines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Environment {
    /// A chat model preset with a key is configured.
    pub model_ready: bool,
    /// The proactive policy's master switch.
    pub initiative_on: bool,
}

/// The pre-continuation preview: the card, the destination, and every check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContinuationPreview {
    pub card: HandoffCard,
    pub destination: ComputerSummary,
    pub checks: Vec<ContinuationCheck>,
    /// No blocking check: the user may accept.
    pub ready: bool,
}

/// One computer for the card, from the machine list or just its id.
pub fn computer_summary(machine_id: &str, machines: &[KnownMachine]) -> ComputerSummary {
    let _ = (machine_id, machines);
    todo!("#82: summarize a computer for the card")
}

/// What the destination must be able to do for this record.
pub fn requirements(record: &ContinuityRecord) -> Requirements {
    let _ = record;
    todo!("#82: derive the required capabilities and permissions")
}

/// The card for one record against the current machine list.
pub fn build_card(record: &ContinuityRecord, machines: &[KnownMachine]) -> HandoffCard {
    let _ = (record, machines);
    todo!("#82: build the handoff card")
}

/// Every reason continuing this record on `machine_id` would stop, or
/// would ask the user for something, before any work starts.
pub fn continuation_checks(
    record: &ContinuityRecord,
    machine_id: &str,
    machines: &[KnownMachine],
    environment: Environment,
) -> Vec<ContinuationCheck> {
    let _ = (record, machine_id, machines, environment);
    todo!("#82: check the destination")
}

/// The preview for continuing `record` on `machine_id`.
pub fn preview(
    record: &ContinuityRecord,
    machine_id: &str,
    machines: &[KnownMachine],
    environment: Environment,
) -> ContinuationPreview {
    let _ = (record, machine_id, machines, environment);
    todo!("#82: build the preview")
}

/// The human name of a permission, for check details.
pub fn permission_label(permission: PermissionKind) -> &'static str {
    match permission {
        PermissionKind::Accessibility => "Accessibility",
        PermissionKind::ScreenCapture => "Screen Recording",
    }
}

fn permission(state: &PermissionState, kind: PermissionKind) -> Permission {
    match kind {
        PermissionKind::Accessibility => state.accessibility,
        PermissionKind::ScreenCapture => state.screen_capture,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::continuity::{
        ContinuityUpdate, HandoffOutcome, HandoffOutcomeStatus, Origin, Provenance,
        ProvenanceSource,
    };
    use cua_protocol::MachineLocation;

    const T0: i64 = 1_767_603_600;

    fn by(source: ProvenanceSource, note: &str) -> Provenance {
        Provenance {
            source,
            at: T0,
            note: note.into(),
        }
    }

    fn machine(id: &str, online: bool, health: MachineHealth) -> KnownMachine {
        KnownMachine {
            machine_id: id.into(),
            display_name: format!("{id} name"),
            custom_name: None,
            hostname: format!("{id}.local"),
            os: "macos".into(),
            platform: Some(Platform::Macos),
            location: MachineLocation::Desktop,
            screen_width: 1920,
            screen_height: 1080,
            permissions: Some(PermissionState {
                accessibility: Permission::Granted,
                screen_capture: Permission::Granted,
            }),
            capabilities: COMPUTER_USE_CAPABILITIES
                .iter()
                .chain(FILE_CAPABILITIES.iter())
                .chain(["bash"].iter())
                .map(|s| (*s).to_owned())
                .collect(),
            first_seen: T0 - 1000,
            last_seen: T0 - 5,
            instance_slug: Some("companion".into()),
            online,
            health,
            driver_version: None,
            cua_health: None,
        }
    }

    fn healthy(id: &str) -> KnownMachine {
        machine(id, true, MachineHealth::Healthy)
    }

    fn offline(id: &str) -> KnownMachine {
        machine(id, false, MachineHealth::Unavailable)
    }

    /// A task started on mac-a with one upload, one memory note, and one
    /// file on mac-a; the upload is missing per the reference check.
    fn record() -> ContinuityRecord {
        let mut record = ContinuityRecord::new(
            "task_1767603600_0badcafe".into(),
            "rename the trip photos",
            Origin {
                chat_id: "chat_1".into(),
                message_id: Some("msg_1".into()),
            },
            by(ProvenanceSource::Chat, "asked in chat"),
            T0,
        )
        .unwrap();
        record
            .apply(
                &ContinuityUpdate {
                    completed_step: Some("listed the folder".into()),
                    blocker: Some("needs the external drive".into()),
                    next_step: Some("rename IMG_* files".into()),
                    machine_ids: vec!["mac-a".into()],
                    resources: vec![
                        ResourceRef::Upload {
                            id: "upload_1".into(),
                        },
                        ResourceRef::Memory {
                            path: "notes/trip.md".into(),
                        },
                        ResourceRef::MachinePath {
                            machine_id: "mac-a".into(),
                            path: "/Volumes/Trip".into(),
                        },
                    ],
                    ..Default::default()
                },
                by(ProvenanceSource::Tool, "progress"),
                T0 + 1,
            )
            .unwrap();
        record.blockers.push(crate::domain::continuity::Blocker {
            kind: crate::domain::continuity::BlockerKind::ResourceMissing {
                resource: ResourceRef::Upload {
                    id: "upload_1".into(),
                },
            },
            detail: "upload upload_1 cannot be found".into(),
            provenance: by(ProvenanceSource::Server, "reference check"),
        });
        record
    }

    fn environment() -> Environment {
        Environment {
            model_ready: true,
            initiative_on: true,
        }
    }

    fn kinds(checks: &[ContinuationCheck]) -> Vec<&CheckKind> {
        checks.iter().map(|check| &check.kind).collect()
    }

    fn blocking(checks: &[ContinuationCheck]) -> Vec<&CheckKind> {
        checks
            .iter()
            .filter(|check| check.severity == Severity::Blocking)
            .map(|check| &check.kind)
            .collect()
    }

    #[test]
    fn the_card_is_derived_from_the_record_and_stays_useful_while_the_origin_is_offline() {
        let record = record();
        let card = build_card(&record, &[offline("mac-a"), healthy("mac-b")]);

        assert_eq!(card.record_id, record.id);
        assert_eq!(card.goal, "rename the trip photos");
        assert_eq!(card.state, ContinuityState::Active);
        assert_eq!(card.origin_chat_id, "chat_1");
        let origin = card.origin.as_ref().expect("the origin computer");
        assert_eq!(origin.machine_id, "mac-a");
        assert_eq!(origin.display_name, "mac-a name");
        assert!(origin.known);
        assert!(!origin.online);
        assert_eq!(origin.health, MachineHealth::Unavailable);
        assert_eq!(origin.last_seen, Some(T0 - 5));
        assert_eq!(card.completed_steps, vec!["listed the folder"]);
        assert_eq!(
            card.resources
                .iter()
                .map(|r| (r.label.as_str(), r.available))
                .collect::<Vec<_>>(),
            vec![
                ("upload upload_1", false),
                ("memory notes/trip.md", true),
                ("/Volumes/Trip on mac-a", true),
            ]
        );
        assert_eq!(
            card.blockers,
            vec![
                "needs the external drive",
                "upload upload_1 cannot be found"
            ]
        );
        assert_eq!(card.next_step.as_deref(), Some("rename IMG_* files"));
        assert_eq!(card.decision, None);
        assert_eq!(card.bound_to, None);
        assert!(card.offered);
        assert_eq!(card.created_at, T0);
        assert_eq!(card.updated_at, T0 + 1);

        // The origin is still named when the machine list has never heard of it.
        let card = build_card(&record, &[]);
        let origin = card.origin.unwrap();
        assert_eq!(origin.display_name, "mac-a");
        assert!(!origin.known && !origin.online);
        assert_eq!(origin.health, MachineHealth::Unavailable);
        assert_eq!(origin.last_seen, None);

        // A task that never touched a computer has no origin computer.
        let chat_only = ContinuityRecord::new(
            "task_1767603600_00000001".into(),
            "draft the newsletter".into(),
            Origin {
                chat_id: "default".into(),
                message_id: None,
            },
            by(ProvenanceSource::Chat, "asked"),
            T0,
        )
        .unwrap();
        assert_eq!(build_card(&chat_only, &[healthy("mac-b")]).origin, None);

        // After acceptance the card names the bound computer and its outcome.
        let mut accepted = self::record();
        accepted
            .decide_handoff(
                HandoffDecision::Accepted {
                    machine_id: "mac-b".into(),
                    run_id: "run_1767603700_0badcafe".into(),
                    at: T0 + 100,
                    outcome: Some(HandoffOutcome {
                        status: HandoffOutcomeStatus::Completed,
                        finished_at: T0 + 200,
                        summary: "2 actions".into(),
                    }),
                },
                by(ProvenanceSource::User, "continue on mac-b"),
                T0 + 100,
            )
            .unwrap();
        let card = build_card(&accepted, &[offline("mac-a"), healthy("mac-b")]);
        assert_eq!(card.bound_to.as_ref().unwrap().machine_id, "mac-b");
        assert!(card.bound_to.as_ref().unwrap().online);
        assert!(matches!(
            card.decision,
            Some(HandoffDecision::Accepted { .. })
        ));
        assert!(card.offered);

        let mut dismissed = self::record();
        dismissed
            .decide_handoff(
                HandoffDecision::Dismissed { at: T0 + 10 },
                by(ProvenanceSource::User, "dismissed"),
                T0 + 10,
            )
            .unwrap();
        assert!(!build_card(&dismissed, &[]).offered);
    }

    #[test]
    fn requirements_follow_what_the_record_links() {
        let required = requirements(&record());
        let mut expected: Vec<String> = COMPUTER_USE_CAPABILITIES
            .iter()
            .chain(FILE_CAPABILITIES.iter())
            .map(|s| (*s).to_owned())
            .collect();
        assert_eq!(required.capabilities, expected);
        assert_eq!(required.permissions, REQUIRED_PERMISSIONS.to_vec());

        let mut chat_only = record();
        chat_only
            .resources
            .retain(|link| !matches!(link.resource, ResourceRef::MachinePath { .. }));
        expected.truncate(COMPUTER_USE_CAPABILITIES.len());
        assert_eq!(requirements(&chat_only).capabilities, expected);
    }

    #[test]
    fn checks_name_every_reason_before_any_work_starts() {
        let record = record();
        let env = environment();

        // Unknown, offline, and stale destinations are refused.
        let checks = continuation_checks(&record, "ghost", &[healthy("mac-b")], env);
        assert!(blocking(&checks).contains(&&CheckKind::MachineUnknown));
        let checks = continuation_checks(&record, "mac-b", &[offline("mac-b")], env);
        assert!(blocking(&checks).contains(&&CheckKind::MachineOffline));
        let checks = continuation_checks(
            &record,
            "mac-b",
            &[machine("mac-b", true, MachineHealth::Degraded)],
            env,
        );
        assert!(blocking(&checks).contains(&&CheckKind::MachineNotResponding));

        // A missing capability is named.
        let mut limited = healthy("mac-b");
        limited.capabilities.retain(|c| c != "file_read");
        let checks = continuation_checks(&record, "mac-b", &[limited], env);
        assert!(blocking(&checks).contains(&&CheckKind::CapabilityMissing {
            capability: "file_read".into()
        }));

        // Denied permissions block; prompts and unreported ones are approvals.
        let mut denied = healthy("mac-b");
        denied.permissions = Some(PermissionState {
            accessibility: Permission::Granted,
            screen_capture: Permission::Denied,
        });
        let checks = continuation_checks(&record, "mac-b", &[denied], env);
        assert!(blocking(&checks).contains(&&CheckKind::PermissionDenied {
            permission: PermissionKind::ScreenCapture
        }));
        let mut prompting = healthy("mac-b");
        prompting.permissions = Some(PermissionState {
            accessibility: Permission::PromptRequired,
            screen_capture: Permission::Granted,
        });
        let checks = continuation_checks(&record, "mac-b", &[prompting], env);
        let prompt = checks
            .iter()
            .find(|c| {
                c.kind
                    == CheckKind::PermissionPrompt {
                        permission: PermissionKind::Accessibility,
                    }
            })
            .expect("a prompt check");
        assert_eq!(prompt.severity, Severity::Approval);
        assert!(prompt.detail.contains("Accessibility"), "{}", prompt.detail);
        let mut unreported = healthy("mac-b");
        unreported.permissions = None;
        let checks = continuation_checks(&record, "mac-b", &[unreported], env);
        let unknown = checks
            .iter()
            .find(|c| c.kind == CheckKind::PermissionsUnknown)
            .expect("an unknown-permissions check");
        assert_eq!(unknown.severity, Severity::Approval);

        // Resources: a missing upload blocks; a file on the offline origin
        // blocks; the same file is a note while the origin is online.
        let checks =
            continuation_checks(&record, "mac-b", &[offline("mac-a"), healthy("mac-b")], env);
        assert!(blocking(&checks).contains(&&CheckKind::ResourceMissing {
            resource: ResourceRef::Upload {
                id: "upload_1".into()
            }
        }));
        assert!(blocking(&checks).contains(&&CheckKind::ResourceElsewhere {
            resource: ResourceRef::MachinePath {
                machine_id: "mac-a".into(),
                path: "/Volumes/Trip".into(),
            },
            machine_id: "mac-a".into(),
        }));
        let checks =
            continuation_checks(&record, "mac-b", &[healthy("mac-a"), healthy("mac-b")], env);
        let elsewhere = checks
            .iter()
            .find(|c| matches!(c.kind, CheckKind::ResourceElsewhere { .. }))
            .expect("the file on mac-a is still mentioned");
        assert_eq!(elsewhere.severity, Severity::Note);
        assert!(
            elsewhere.detail.contains("mac-a name"),
            "{}",
            elsewhere.detail
        );

        // The environment: no model, initiative off, closed record.
        let checks = continuation_checks(
            &record,
            "mac-b",
            &[healthy("mac-b")],
            Environment {
                model_ready: false,
                initiative_on: false,
            },
        );
        assert!(blocking(&checks).contains(&&CheckKind::ModelUnavailable));
        assert!(blocking(&checks).contains(&&CheckKind::InitiativeOff));
        let mut closed = self::record();
        closed.state = ContinuityState::Completed;
        let checks = continuation_checks(&closed, "mac-b", &[healthy("mac-b")], env);
        assert!(blocking(&checks).contains(&&CheckKind::RecordClosed));

        // Every check carries a sentence the user can read.
        for check in &checks {
            assert!(!check.detail.trim().is_empty(), "{:?}", check.kind);
        }
    }

    #[test]
    fn the_preview_is_ready_only_without_blocking_checks() {
        let mut record = record();
        record.blockers.clear(); // the upload is back
        let machines = [healthy("mac-a"), healthy("mac-b")];
        let p = preview(&record, "mac-b", &machines, environment());
        assert!(p.ready, "{:?}", kinds(&p.checks));
        assert!(blocking(&p.checks).is_empty());
        assert_eq!(p.destination.machine_id, "mac-b");
        assert_eq!(p.destination.display_name, "mac-b name");
        assert_eq!(p.card.record_id, record.id);
        // The origin's file is mentioned as reachable, not as a stop.
        assert!(
            p.checks
                .iter()
                .any(|c| matches!(c.kind, CheckKind::ResourceElsewhere { .. }))
        );

        // Continuing on the origin itself needs no note about its own file.
        let p = preview(&record, "mac-a", &machines, environment());
        assert!(p.ready);
        assert!(
            !p.checks
                .iter()
                .any(|c| matches!(c.kind, CheckKind::ResourceElsewhere { .. }))
        );

        let p = preview(
            &record,
            "mac-b",
            &[offline("mac-a"), offline("mac-b")],
            environment(),
        );
        assert!(!p.ready);
        assert_eq!(p.destination.display_name, "mac-b name");
        assert!(!p.destination.online);
    }
}
