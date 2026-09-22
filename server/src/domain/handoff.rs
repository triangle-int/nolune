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

use cua_protocol::{
    MachineHealth, MachineLocation, Permission, PermissionKind, PermissionState, Platform,
};
use serde::Serialize;

use crate::domain::{
    continuity::{BlockerKind, ContinuityRecord, ContinuityState, HandoffDecision, ResourceRef},
    machine::KnownMachine,
};

/// Toolcalls the destination's desktop app must execute for a task that
/// links files on a computer (the `remote_files` path the acceptance looks
/// for them through).
pub const FILE_CAPABILITIES: [&str; 2] = ["file_read", "file_list"];
/// The grants the destination's Cua driver must hold for every continuation
/// on a computer: seeing windows and acting in them. Since #19 nothing else
/// on a desktop does either, so requiring them is requiring the driver.
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
    /// Toolcalls the destination's desktop app must execute.
    pub capabilities: Vec<String>,
    /// Grants the destination's Cua driver must hold; a non-empty list is
    /// also the requirement that a driver runs there at all.
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
    /// The desktop app does not execute a toolcall the task needs.
    CapabilityMissing {
        capability: String,
    },
    /// No Cua driver runs on the destination, so nothing there can see or
    /// act in windows (#19).
    DriverMissing,
    /// The destination's Cua driver reports itself unavailable; the
    /// orchestrator refuses every action against it.
    DriverUnavailable,
    /// The destination's Cua driver reports degraded health.
    DriverDegraded,
    /// The Cua driver on the destination does not hold this grant.
    PermissionDenied {
        permission: PermissionKind,
    },
    /// The driver's health report did not cover this grant; the first action
    /// that needs it may prompt for it or be refused.
    PermissionPrompt {
        permission: PermissionKind,
    },
    /// A driver is registered but its grants were not reported.
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
    /// A file on the destination itself: whether it is still there is only
    /// known once the user confirms, when the service looks for it.
    ResourceUnverified {
        resource: ResourceRef,
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
    match machines.iter().find(|m| m.machine_id == machine_id) {
        Some(machine) => ComputerSummary {
            machine_id: machine.machine_id.clone(),
            display_name: machine.display_name.clone(),
            known: true,
            online: machine.online,
            health: machine.health,
            platform: machine.platform,
            last_seen: Some(machine.last_seen),
        },
        None => ComputerSummary {
            machine_id: machine_id.to_owned(),
            display_name: machine_id.to_owned(),
            known: false,
            online: false,
            health: MachineHealth::Unavailable,
            platform: None,
            last_seen: None,
        },
    }
}

/// What the destination must be able to do for this record: the file
/// toolcalls when it links a file on a computer, and the driver's grants
/// always.
pub fn requirements(record: &ContinuityRecord) -> Requirements {
    let links_files = record
        .resources
        .iter()
        .any(|link| matches!(link.resource, ResourceRef::MachinePath { .. }));
    Requirements {
        capabilities: if links_files {
            FILE_CAPABILITIES.iter().map(|s| (*s).to_owned()).collect()
        } else {
            Vec::new()
        },
        permissions: REQUIRED_PERMISSIONS.to_vec(),
    }
}

/// Whether the record's reference check found this resource missing.
fn resource_missing(record: &ContinuityRecord, resource: &ResourceRef) -> bool {
    record.blockers.iter().any(|blocker| {
        matches!(&blocker.kind, BlockerKind::ResourceMissing { resource: missing } if missing == resource)
    })
}

/// A resource as the card names it: a file on a computer is named by that
/// computer's display name, not its id.
fn resource_label(resource: &ResourceRef, machines: &[KnownMachine]) -> String {
    match resource {
        ResourceRef::MachinePath { machine_id, path } => {
            format!(
                "{path} on {}",
                computer_summary(machine_id, machines).display_name
            )
        }
        _ => resource.describe(),
    }
}

/// The card for one record against the current machine list.
pub fn build_card(record: &ContinuityRecord, machines: &[KnownMachine]) -> HandoffCard {
    let summary = |machine_id: &str| computer_summary(machine_id, machines);
    HandoffCard {
        record_id: record.id.clone(),
        goal: record.goal.clone(),
        state: record.state,
        origin_chat_id: record.origin.chat_id.clone(),
        origin: record.machine_ids.first().map(|id| summary(id)),
        completed_steps: record
            .completed_steps
            .iter()
            .map(|step| step.summary.clone())
            .collect(),
        resources: record
            .resources
            .iter()
            .map(|link| CardResource {
                resource: link.resource.clone(),
                label: resource_label(&link.resource, machines),
                available: !resource_missing(record, &link.resource),
            })
            .collect(),
        blockers: record
            .blockers
            .iter()
            .map(|blocker| blocker.detail.clone())
            .collect(),
        next_step: record.next_step.clone(),
        required: requirements(record),
        decision: record.handoff.clone(),
        bound_to: record
            .handoff
            .as_ref()
            .and_then(HandoffDecision::bound_machine)
            .map(summary),
        offered: record.handoff_offered(),
        created_at: record.created_at,
        updated_at: record.updated_at,
    }
}

fn check(kind: CheckKind, severity: Severity, detail: impl Into<String>) -> ContinuationCheck {
    ContinuationCheck {
        kind,
        severity,
        detail: detail.into(),
    }
}

/// Every reason continuing this record on `machine_id` would stop, or
/// would ask the user for something, before any work starts.
pub fn continuation_checks(
    record: &ContinuityRecord,
    machine_id: &str,
    machines: &[KnownMachine],
    environment: Environment,
) -> Vec<ContinuationCheck> {
    use Severity::{Blocking, Note};
    let mut checks = Vec::new();
    let destination = computer_summary(machine_id, machines);
    let name = destination.display_name.as_str();

    if !record.state.is_resumable() {
        checks.push(check(
            CheckKind::RecordClosed,
            Blocking,
            "this task is closed and cannot be continued",
        ));
    }
    if !environment.model_ready {
        checks.push(check(
            CheckKind::ModelUnavailable,
            Blocking,
            "no chat model is configured; add one in Settings before continuing",
        ));
    }
    if !environment.initiative_on {
        checks.push(check(
            CheckKind::InitiativeOff,
            Blocking,
            "initiative is off; turn it on in Settings so the companion may continue work",
        ));
    }

    let machine = machines.iter().find(|m| m.machine_id == machine_id);
    match machine {
        None => checks.push(check(
            CheckKind::MachineUnknown,
            Blocking,
            format!("{name} has never connected to this companion"),
        )),
        Some(m) if !m.online => checks.push(check(
            CheckKind::MachineOffline,
            Blocking,
            format!("{name} is offline"),
        )),
        Some(m) if m.health == MachineHealth::Degraded => checks.push(check(
            CheckKind::MachineNotResponding,
            Blocking,
            format!("{name} is connected but not responding"),
        )),
        Some(_) => {}
    }

    if let Some(m) = machine {
        let required = requirements(record);
        for capability in required.capabilities {
            if !m.capabilities.contains(&capability) {
                checks.push(check(
                    CheckKind::CapabilityMissing {
                        capability: capability.clone(),
                    },
                    Blocking,
                    format!("{name} cannot {capability}; its desktop app does not offer it"),
                ));
            }
        }
        // Seeing and acting in windows is the Cua driver's alone (#19): the
        // grants the task needs are the driver's, so a destination without
        // one has nothing to hold them.
        if !required.permissions.is_empty() {
            checks.extend(driver_checks(m, name, &required.permissions));
        }
    }

    for link in &record.resources {
        let resource = &link.resource;
        let ResourceRef::MachinePath {
            machine_id: on,
            path,
        } = resource
        else {
            // An upload or memory note the reference check could not find.
            if resource_missing(record, resource) {
                checks.push(check(
                    CheckKind::ResourceMissing {
                        resource: resource.clone(),
                    },
                    Blocking,
                    format!("{} cannot be found", resource.describe()),
                ));
            }
            continue;
        };
        // Whether a file is reachable is a question about the destination,
        // which an unknown one has already answered.
        if machine.is_none() {
            continue;
        }
        // A file on the destination itself is only known to be there once
        // the user confirms and the service looks for it; the preview says
        // so rather than passing over it.
        if on == machine_id {
            checks.push(check(
                CheckKind::ResourceUnverified {
                    resource: resource.clone(),
                },
                Note,
                format!(
                    "{path} on {name} is looked for when you confirm; the continuation stops if it is not there"
                ),
            ));
            continue;
        }
        // The reference check marks a file on a computer that is not
        // connected as missing; the machine list says that more precisely.
        let holder = computer_summary(on, machines);
        let holder_name = holder.display_name.as_str();
        if holder.online && holder.health != MachineHealth::Degraded {
            checks.push(check(
                CheckKind::ResourceElsewhere {
                    resource: resource.clone(),
                    machine_id: on.clone(),
                },
                Note,
                format!("{path} is on {holder_name}; it stays reachable there while {holder_name} is connected"),
            ));
        } else {
            checks.push(check(
                CheckKind::ResourceElsewhere {
                    resource: resource.clone(),
                    machine_id: on.clone(),
                },
                Blocking,
                format!(
                    "{path} is on {holder_name}, which is not connected; {name} cannot reach it"
                ),
            ));
        }
    }

    checks
}

/// The checks on the destination's Cua driver: that one runs, that it does
/// not report itself unavailable, and that it holds each grant in
/// `permissions`. `machine.permissions` are the driver's grants while a
/// driver is registered (the registry records the descriptor's, never the
/// desktop app's own, which gate nothing since #19).
fn driver_checks(
    machine: &KnownMachine,
    name: &str,
    permissions: &[PermissionKind],
) -> Vec<ContinuationCheck> {
    use Severity::{Approval, Blocking, Note};
    let mut checks = Vec::new();
    if machine.driver_version.is_none() {
        checks.push(check(
            CheckKind::DriverMissing,
            Blocking,
            format!(
                "{name} runs no Cua driver, so it cannot see or act in windows; install one \
                 there from the Nolune app (Settings > Computer use > Install driver), then \
                 reconnect"
            ),
        ));
        return checks;
    }
    match machine.cua_health {
        Some(MachineHealth::Unavailable) => {
            checks.push(check(
                CheckKind::DriverUnavailable,
                Blocking,
                format!("the Cua driver on {name} reports itself unavailable; restart it there"),
            ));
            return checks;
        }
        Some(MachineHealth::Degraded) => checks.push(check(
            CheckKind::DriverDegraded,
            Note,
            format!(
                "the Cua driver on {name} reports degraded health; a window action there may fail"
            ),
        )),
        Some(MachineHealth::Healthy) | None => {}
    }
    let Some(state) = &machine.permissions else {
        checks.push(check(
            CheckKind::PermissionsUnknown,
            Blocking,
            format!(
                "the Cua driver on {name} did not report its grants; reconnect it and try again"
            ),
        ));
        return checks;
    };
    // Where the grant is made: the desktop app's Settings window runs the
    // driver's grant flow on a desktop; beside the server it is the
    // driver's own command.
    let grant = match machine.location {
        MachineLocation::Desktop => "grant it from the Nolune desktop app's Settings there",
        MachineLocation::ServerLocal => "run cua-driver permissions grant on the server",
    };
    for &kind in permissions {
        let label = permission_label(kind);
        match permission(state, kind) {
            Permission::Granted => {}
            Permission::PromptRequired => checks.push(check(
                CheckKind::PermissionPrompt { permission: kind },
                Approval,
                format!(
                    "the Cua driver on {name} has not been granted {label} yet; the first action that needs it may ask for it or be refused"
                ),
            )),
            Permission::Denied | Permission::Unavailable => checks.push(check(
                CheckKind::PermissionDenied { permission: kind },
                Blocking,
                format!("{label} is not granted to the Cua driver on {name}; {grant}, then reconnect"),
            )),
        }
    }
    checks
}

/// The preview for continuing `record` on `machine_id`.
pub fn preview(
    record: &ContinuityRecord,
    machine_id: &str,
    machines: &[KnownMachine],
    environment: Environment,
) -> ContinuationPreview {
    let checks = continuation_checks(record, machine_id, machines, environment);
    ContinuationPreview {
        card: build_card(record, machines),
        destination: computer_summary(machine_id, machines),
        ready: !checks
            .iter()
            .any(|check| check.severity == Severity::Blocking),
        checks,
    }
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

    const T0: i64 = 1_767_603_600;

    fn by(source: ProvenanceSource, note: &str) -> Provenance {
        Provenance {
            source,
            at: T0,
            note: note.into(),
        }
    }

    /// A desktop from this release: the five toolcalls the app executes and
    /// a healthy Cua driver holding both grants.
    fn machine(id: &str, online: bool, health: MachineHealth) -> KnownMachine {
        KnownMachine {
            machine_id: id.into(),
            display_name: format!("{id} name"),
            custom_name: None,
            hostname: format!("{id}.local"),
            os: "macos".into(),
            platform: Some(Platform::Macos),
            location: MachineLocation::Desktop,
            permissions: Some(PermissionState {
                accessibility: Permission::Granted,
                screen_capture: Permission::Granted,
            }),
            capabilities: crate::domain::machine::DESKTOP_TOOLCALLS
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
            first_seen: T0 - 1000,
            last_seen: T0 - 5,
            instance_slug: Some("companion".into()),
            online,
            health,
            driver_version: online.then(|| "0.28.2".to_owned()),
            cua_health: online.then_some(MachineHealth::Healthy),
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
            kind: BlockerKind::ResourceMissing {
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
                ("/Volumes/Trip on mac-a name", true),
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

        // The origin is still named when the machine list has never heard of
        // it, and so is the computer holding a file.
        let card = build_card(&record, &[]);
        assert_eq!(card.resources[2].label, "/Volumes/Trip on mac-a");
        let origin = card.origin.unwrap();
        assert_eq!(origin.display_name, "mac-a");
        assert!(!origin.known && !origin.online);
        assert_eq!(origin.health, MachineHealth::Unavailable);
        assert_eq!(origin.last_seen, None);

        // A task that never touched a computer has no origin computer.
        let chat_only = ContinuityRecord::new(
            "task_1767603600_00000001".into(),
            "draft the newsletter",
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
        // A file on a computer needs the desktop app's file toolcalls; the
        // driver's grants are needed either way, and no coordinate action
        // name is ever required (#19: the desktop app executes none).
        let required = requirements(&record());
        assert_eq!(required.capabilities, FILE_CAPABILITIES.to_vec());
        assert_eq!(required.permissions, REQUIRED_PERMISSIONS.to_vec());
        for name in ["screenshot", "left_click", "type", "key"] {
            assert!(!required.capabilities.iter().any(|c| c == name));
        }

        let mut chat_only = record();
        chat_only
            .resources
            .retain(|link| !matches!(link.resource, ResourceRef::MachinePath { .. }));
        let required = requirements(&chat_only);
        assert!(required.capabilities.is_empty());
        assert_eq!(required.permissions, REQUIRED_PERMISSIONS.to_vec());
    }

    #[test]
    fn a_desktop_from_this_release_is_ready_and_one_without_a_driver_is_refused() {
        let mut record = record();
        record.blockers.clear();
        let env = environment();

        // Exactly what the desktop app registers with: its five toolcalls
        // and the driver's descriptor.
        let current = healthy("mac-b");
        assert_eq!(
            current.capabilities,
            crate::domain::machine::DESKTOP_TOOLCALLS
                .iter()
                .map(|s| (*s).to_owned())
                .collect::<Vec<_>>()
        );
        let checks = continuation_checks(&record, "mac-b", &[healthy("mac-a"), current], env);
        assert!(blocking(&checks).is_empty(), "{:?}", kinds(&checks));

        // The same desktop without a driver cannot see or act in windows,
        // whatever toolcalls it executes, and the check says what to install.
        let mut driverless = healthy("mac-b");
        driverless.driver_version = None;
        driverless.cua_health = None;
        driverless.permissions = None;
        let checks = continuation_checks(&record, "mac-b", &[healthy("mac-a"), driverless], env);
        assert_eq!(blocking(&checks), vec![&CheckKind::DriverMissing]);
        let missing = checks
            .iter()
            .find(|c| c.kind == CheckKind::DriverMissing)
            .unwrap();
        assert!(
            missing.detail.contains("mac-b name")
                && missing.detail.contains("Cua driver")
                // The remedy is a button in the destination's own app, never
                // a command: a desktop user has no `nolune` on their PATH.
                && missing.detail.contains("Settings > Computer use > Install driver")
                && !missing.detail.contains("nolune cua"),
            "{}",
            missing.detail
        );
        assert!(
            !checks
                .iter()
                .any(|c| matches!(c.kind, CheckKind::PermissionsUnknown)),
            "no grants are asked about when there is no driver to hold them"
        );

        // An older desktop advertising the coordinate names the deleted
        // executor answered is no better off: those names satisfy nothing.
        let mut legacy = healthy("mac-b");
        legacy.capabilities = crate::domain::machine::LEGACY_DESKTOP_CAPABILITIES
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        legacy.driver_version = None;
        legacy.cua_health = None;
        let checks = continuation_checks(&record, "mac-b", &[healthy("mac-a"), legacy], env);
        assert_eq!(blocking(&checks), vec![&CheckKind::DriverMissing]);

        // The driver's own health: unavailable stops, degraded is noted.
        let mut down = healthy("mac-b");
        down.cua_health = Some(MachineHealth::Unavailable);
        let checks = continuation_checks(&record, "mac-b", &[healthy("mac-a"), down], env);
        assert_eq!(blocking(&checks), vec![&CheckKind::DriverUnavailable]);
        let mut shaky = healthy("mac-b");
        shaky.cua_health = Some(MachineHealth::Degraded);
        let checks = continuation_checks(&record, "mac-b", &[healthy("mac-a"), shaky], env);
        assert!(blocking(&checks).is_empty(), "{:?}", kinds(&checks));
        let note = checks
            .iter()
            .find(|c| c.kind == CheckKind::DriverDegraded)
            .expect("a degraded driver is mentioned");
        assert_eq!(note.severity, Severity::Note);

        // A driver whose grants were not reported is not assumed to hold them.
        let mut silent = healthy("mac-b");
        silent.permissions = None;
        let checks = continuation_checks(&record, "mac-b", &[healthy("mac-a"), silent], env);
        assert_eq!(blocking(&checks), vec![&CheckKind::PermissionsUnknown]);
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

        // A missing file toolcall is named.
        let mut limited = healthy("mac-b");
        limited.capabilities.retain(|c| c != "file_read");
        let checks = continuation_checks(&record, "mac-b", &[limited], env);
        assert!(blocking(&checks).contains(&&CheckKind::CapabilityMissing {
            capability: "file_read".into()
        }));

        // The driver's grants: a denied one blocks and says where to grant
        // it; one its report did not cover is an approval.
        let mut denied = healthy("mac-b");
        denied.permissions = Some(PermissionState {
            accessibility: Permission::Granted,
            screen_capture: Permission::Denied,
        });
        let checks = continuation_checks(&record, "mac-b", &[denied.clone()], env);
        let refused = checks
            .iter()
            .find(|c| {
                c.kind
                    == CheckKind::PermissionDenied {
                        permission: PermissionKind::ScreenCapture,
                    }
            })
            .expect("a denied check");
        assert_eq!(refused.severity, Severity::Blocking);
        assert!(
            refused.detail.contains("Screen Recording")
                && refused.detail.contains("Cua driver on mac-b name")
                && refused.detail.contains("desktop app's Settings"),
            "{}",
            refused.detail
        );
        let mut beside_server = denied.clone();
        beside_server.location = MachineLocation::ServerLocal;
        let checks = continuation_checks(&record, "mac-b", &[beside_server], env);
        let refused = checks
            .iter()
            .find(|c| matches!(c.kind, CheckKind::PermissionDenied { .. }))
            .unwrap();
        assert!(
            refused
                .detail
                .contains("cua-driver permissions grant on the server"),
            "{}",
            refused.detail
        );
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
        assert!(
            prompt.detail.contains("Accessibility") && prompt.detail.contains("Cua driver"),
            "{}",
            prompt.detail
        );

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

        // Continuing on the origin itself: its own file is not elsewhere, but
        // whether it is still there is only known once the user confirms, and
        // the preview says so instead of skipping it.
        let p = preview(&record, "mac-a", &machines, environment());
        assert!(p.ready);
        assert!(
            !p.checks
                .iter()
                .any(|c| matches!(c.kind, CheckKind::ResourceElsewhere { .. }))
        );
        let unverified = p
            .checks
            .iter()
            .find(|c| {
                c.kind
                    == CheckKind::ResourceUnverified {
                        resource: ResourceRef::MachinePath {
                            machine_id: "mac-a".into(),
                            path: "/Volumes/Trip".into(),
                        },
                    }
            })
            .expect("the destination's own file is named");
        assert_eq!(unverified.severity, Severity::Note);
        assert!(
            unverified.detail.contains("/Volumes/Trip") && unverified.detail.contains("mac-a name"),
            "{}",
            unverified.detail
        );
        // An unknown destination has already been refused; nothing is said
        // about its files, and the file of a known destination is never a stop
        // before acceptance.
        let p = preview(&record, "ghost", &machines, environment());
        assert!(
            !p.checks
                .iter()
                .any(|c| matches!(c.kind, CheckKind::ResourceUnverified { .. }))
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
