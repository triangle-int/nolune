//! Cua orchestration (#18): the loop policy the typed machine tools enforce
//! on every target, server-local or desktop alike.
//!
//! The `SnapshotLedger` remembers, per machine and window, which snapshot
//! `get_window_state` issued last and the element tokens it carried. An
//! element action is forwarded only with a token or index from that latest
//! snapshot: nothing observed yet, a token from a superseded snapshot, or a
//! snapshot the last action already acted on all fail closed. A point
//! address is forwarded only while the window's accessibility route is
//! unavailable or the last verification showed an action did not land.
//!
//! After an action the orchestrator reads the driver's `ActionOutcome` and,
//! when the caller supplied predicates, issues `verify_state` itself. Success
//! is reported only for a verified outcome: predicates satisfied, or the
//! driver's own readback confirming the effect. Refused, unverifiable,
//! suspected-noop and partial outcomes and unsatisfied or unknown
//! verifications are errors. Delivery is always background; a driver that
//! recommends foreground control is quoted in the refusal and never obeyed.

use std::{
    collections::{BTreeMap, HashSet},
    fmt,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
};

use cua_protocol::{
    ActionEffect, ActionEvidence, ActionOutcome, CheckedCuaAdapter, CuaAction, CuaActionKind,
    CuaActionResult, CuaRequestEnvelope, CuaResponse, CuaResponseEnvelope, CuaRuntimeError,
    ElementAddress, ElementRef, ExactWindowStatus, GetWindowStateArgs, MachineDescriptor,
    MachineId, Permission, PermissionKind, PredicateStatus, ProtocolVersion, RequestId, SnapshotId,
    VerificationResult, VerifyPredicate, VerifyStateArgs, WindowStateResult, WindowTarget,
};

use super::runtime::{CuaRuntime, ExecError, RunSession};

// ═══════════════════════════════════════════════════════════════════════════
// Snapshot ledger
// ═══════════════════════════════════════════════════════════════════════════

/// Why the ledger did not forward an address.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LedgerRefusal {
    /// No `get_window_state` has observed this window on this machine yet.
    NoSnapshot { target: WindowTarget },
    /// The latest observation of the window carried no accessibility tree
    /// (screenshot only, or the tree could not be resolved), so it issued no
    /// tokens.
    NoAccessibilityTree { target: WindowTarget },
    /// The address names a token or snapshot the latest snapshot did not
    /// issue: from a snapshot a newer one replaced, or unknown to this
    /// window altogether.
    StaleSnapshot {
        target: WindowTarget,
        latest: SnapshotId,
        superseded: Option<SnapshotId>,
    },
    /// The latest observation was taken before the last action on this
    /// window, which may have changed what it showed. `snapshot` is the
    /// id it issued, when it carried a tree.
    SnapshotConsumed {
        target: WindowTarget,
        snapshot: Option<SnapshotId>,
        action: CuaActionKind,
    },
    /// A point address while the window's accessibility route is usable and
    /// no verification has failed on it.
    PixelRefused { target: WindowTarget },
}

impl LedgerRefusal {
    /// Stable code the message starts with.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NoSnapshot { .. } => "snapshot_required",
            Self::NoAccessibilityTree { .. } => "no_accessibility_tree",
            Self::StaleSnapshot { .. } => "stale_snapshot",
            Self::SnapshotConsumed { .. } => "snapshot_consumed",
            Self::PixelRefused { .. } => "pixel_refused",
        }
    }
}

impl fmt::Display for LedgerRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.code())?;
        match self {
            Self::NoSnapshot { target } => write!(
                f,
                "window {} of pid {} has not been observed on this computer; call \
                 get_window_state for it first, then act with an element_token it returns",
                target.window_id, target.pid
            ),
            Self::NoAccessibilityTree { target } => write!(
                f,
                "the latest get_window_state of window {} (pid {}) issued no element tokens: no \
                 accessibility tree was requested, or the window's accessibility surface could \
                 not be resolved; observe it again with include_accessibility_tree, or use a \
                 point address if pixel_addresses allows it",
                target.window_id, target.pid
            ),
            Self::StaleSnapshot {
                target,
                latest,
                superseded,
            } => {
                write!(
                    f,
                    "the address is not from snapshot {}, the latest of window {} (pid {}); ",
                    latest.as_str(),
                    target.window_id,
                    target.pid
                )?;
                match superseded {
                    Some(old) => write!(
                        f,
                        "snapshot {} was replaced by it and its tokens are void. ",
                        old.as_str()
                    )?,
                    None => f.write_str("no snapshot of this window issued it. ")?,
                }
                f.write_str("Use only tokens from the latest get_window_state of the window")
            }
            Self::SnapshotConsumed {
                target,
                snapshot,
                action,
            } => write!(
                f,
                "{} of window {} (pid {}) was taken before the last action there ({}), which \
                 may have changed the window; call get_window_state again before the next \
                 action",
                match snapshot {
                    Some(id) => format!("snapshot {}", id.as_str()),
                    None => "the latest observation".to_owned(),
                },
                target.window_id,
                target.pid,
                kind_name(*action)
            ),
            Self::PixelRefused { target } => write!(
                f,
                "a point address on window {} (pid {}) is not allowed while its accessibility \
                 route works and no verification there has failed; act with an element_token \
                 from the latest get_window_state instead",
                target.window_id, target.pid
            ),
        }
    }
}

/// The snake_case name of a protocol enum value, as the wire spells it.
pub(crate) fn kind_name<T: serde::Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Whether a point address may be forwarded for a window, and why.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PixelPolicy {
    pub allowed: bool,
    pub reason: &'static str,
}

/// The latest snapshot of one window and what the loop did with it since.
#[derive(Clone, Debug, Default)]
struct WindowLedger {
    /// The snapshot the latest observation issued, when it carried a tree.
    snapshot: Option<Snapshot>,
    /// The snapshot before it, so a stale token can be named.
    superseded: Option<Snapshot>,
    /// The kind of the action taken since the latest observation, if any.
    acted: Option<CuaActionKind>,
    /// The latest observation could not resolve the window's accessibility
    /// surface (degraded, AX unresolved, or a tree with no elements).
    ax_unavailable: bool,
    /// The last verification on this window failed or an action did not
    /// land; cleared by the next verified action.
    fallback: bool,
}

impl WindowLedger {
    fn pixels_allowed(&self) -> bool {
        self.fallback || self.ax_unavailable
    }
}

#[derive(Clone, Debug)]
struct Snapshot {
    id: SnapshotId,
    tokens: HashSet<String>,
    indices: HashSet<u32>,
}

/// How one action addresses the window, as far as the ledger is concerned.
enum Address<'a> {
    Token(&'a str),
    Index {
        index: u32,
        snapshot: &'a SnapshotId,
    },
    Point,
}

impl<'a> From<&'a ElementAddress> for Address<'a> {
    fn from(address: &'a ElementAddress) -> Self {
        match address {
            ElementAddress::ElementToken { element_token } => Self::Token(element_token.as_str()),
            ElementAddress::ElementIndex {
                element_index,
                snapshot_id,
            } => Self::Index {
                index: *element_index,
                snapshot: snapshot_id,
            },
            ElementAddress::Point(_) => Self::Point,
        }
    }
}

impl<'a> From<&'a ElementRef> for Address<'a> {
    fn from(element: &'a ElementRef) -> Self {
        match element {
            ElementRef::ElementToken { element_token } => Self::Token(element_token.as_str()),
            ElementRef::ElementIndex {
                element_index,
                snapshot_id,
            } => Self::Index {
                index: *element_index,
                snapshot: snapshot_id,
            },
        }
    }
}

/// The window an action acts in and the addresses the ledger checks for
/// it. Menu and frame actions name a window but address nothing in it;
/// discovery, session and health calls name no window.
fn addressing(action: &CuaAction) -> Option<(WindowTarget, Vec<Address<'_>>)> {
    Some(match action {
        CuaAction::Click(args) => (args.target, vec![(&args.address).into()]),
        CuaAction::DoubleClick(args) => (args.target, vec![(&args.address).into()]),
        CuaAction::RightClick(args) => (args.target, vec![(&args.address).into()]),
        CuaAction::Scroll(args) => (args.target, vec![(&args.address).into()]),
        CuaAction::TypeText(args) => (args.target, vec![(&args.address).into()]),
        CuaAction::PressKey(args) => (args.target, vec![(&args.address).into()]),
        CuaAction::Hotkey(args) => (args.target, vec![(&args.address).into()]),
        CuaAction::MoveCursor(args) => (args.target, vec![Address::Point]),
        CuaAction::Drag(args) => (args.target, vec![Address::Point]),
        CuaAction::SetValue(args) => (args.target, vec![(&args.element).into()]),
        CuaAction::InvokeMenu(args) => (args.target, vec![]),
        CuaAction::SetWindowFrame(args) => (args.target, vec![]),
        CuaAction::ListApps(_)
        | CuaAction::LaunchApp(_)
        | CuaAction::ListWindows(_)
        | CuaAction::GetWindowState(_)
        | CuaAction::VerifyState(_)
        | CuaAction::StartSession(_)
        | CuaAction::GetSession(_)
        | CuaAction::ListSessions(_)
        | CuaAction::EndSession(_)
        | CuaAction::HealthReport(_) => return None,
    })
}

type Key = (MachineId, u32, u64);

fn key(machine: &MachineId, target: WindowTarget) -> Key {
    (machine.clone(), target.pid, target.window_id)
}

/// Per machine and window: which snapshot is current and whether pixels
/// may be used. One ledger serves one chat run.
#[derive(Default)]
pub struct SnapshotLedger {
    windows: Mutex<BTreeMap<Key, WindowLedger>>,
}

impl SnapshotLedger {
    /// A poisoned lock only means a task panicked mid-update; the map
    /// itself is still consistent.
    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<Key, WindowLedger>> {
        self.windows
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Record what `get_window_state` returned for a window: the snapshot it
    /// issued (or that it issued none) and whether accessibility is usable
    /// there. Replaces the previous snapshot of the same machine and window
    /// and clears the action taken since; a failed verification stays on
    /// record, because the pixel action it allows needs this fresh
    /// screenshot.
    pub fn observed(
        &self,
        machine: &MachineId,
        args: &GetWindowStateArgs,
        state: &WindowStateResult,
    ) {
        let tree_requested = args.include_accessibility_tree;
        let snapshot = match &state.snapshot_id {
            Some(id) if tree_requested => Some(Snapshot {
                id: id.clone(),
                tokens: state
                    .elements
                    .iter()
                    .map(|element| element.element_token.as_str().to_owned())
                    .collect(),
                indices: state
                    .elements
                    .iter()
                    .map(|element| element.element_index)
                    .collect(),
            }),
            _ => None,
        };
        let ax_unresolved = state.background_input.as_ref().is_some_and(|background| {
            background.exact_window.status == ExactWindowStatus::AxUnresolved
        });
        let mut windows = self.lock();
        let entry = windows.entry(key(machine, state.target)).or_default();
        entry.ax_unavailable =
            state.degraded || ax_unresolved || (tree_requested && state.elements.is_empty());
        if let Some(previous) = entry.snapshot.take() {
            entry.superseded = Some(previous);
        }
        entry.snapshot = snapshot;
        entry.acted = None;
    }

    /// Check every address the action carries against the ledger. Actions
    /// without a window address (discovery, menus) pass.
    pub fn check(&self, machine: &MachineId, action: &CuaAction) -> Result<(), LedgerRefusal> {
        let Some((target, addresses)) = addressing(action) else {
            return Ok(());
        };
        if addresses.is_empty() {
            return Ok(());
        }
        let windows = self.lock();
        let entry = windows
            .get(&key(machine, target))
            .ok_or(LedgerRefusal::NoSnapshot { target })?;
        if let Some(action) = entry.acted {
            return Err(LedgerRefusal::SnapshotConsumed {
                target,
                snapshot: entry.snapshot.as_ref().map(|snapshot| snapshot.id.clone()),
                action,
            });
        }
        for address in addresses {
            match address {
                Address::Point => {
                    if !entry.pixels_allowed() {
                        return Err(LedgerRefusal::PixelRefused { target });
                    }
                }
                Address::Token(_) | Address::Index { .. } => {
                    let snapshot = entry
                        .snapshot
                        .as_ref()
                        .ok_or(LedgerRefusal::NoAccessibilityTree { target })?;
                    let issued = match address {
                        Address::Token(token) => snapshot.tokens.contains(token),
                        Address::Index {
                            index,
                            snapshot: id,
                        } => snapshot.id == *id && snapshot.indices.contains(&index),
                        Address::Point => unreachable!("matched above"),
                    };
                    if !issued {
                        return Err(LedgerRefusal::StaleSnapshot {
                            target,
                            latest: snapshot.id.clone(),
                            superseded: entry
                                .superseded
                                .as_ref()
                                .map(|snapshot| snapshot.id.clone()),
                        });
                    }
                }
            }
        }
        Ok(())
    }

    /// An action reached the driver: whatever it did to the window, the
    /// latest observation no longer describes it, so the next address
    /// there needs a fresh `get_window_state`.
    pub fn acted(&self, machine: &MachineId, target: WindowTarget, kind: CuaActionKind) {
        self.lock().entry(key(machine, target)).or_default().acted = Some(kind);
    }

    /// A verification of the window finished. Unsatisfied (or an action
    /// that did not land) opens the pixel route there; satisfied closes it.
    pub fn verified(&self, machine: &MachineId, target: WindowTarget, satisfied: bool) {
        self.lock()
            .entry(key(machine, target))
            .or_default()
            .fallback = !satisfied;
    }

    /// The driver itself called the window's snapshot stale: forget it, so
    /// the next address there is refused until a fresh observation.
    pub fn forget(&self, machine: &MachineId, target: WindowTarget) {
        self.lock().remove(&key(machine, target));
    }

    /// Whether a point address would be forwarded for the window right now.
    pub fn pixel_policy(&self, machine: &MachineId, target: WindowTarget) -> PixelPolicy {
        let windows = self.lock();
        let Some(entry) = windows.get(&key(machine, target)) else {
            return PixelPolicy {
                allowed: false,
                reason: "the window has not been observed; call get_window_state first",
            };
        };
        if entry.fallback {
            PixelPolicy {
                allowed: true,
                reason: "the last verification on this window failed or an action did not \
                         land; a point address is allowed until an action there is verified",
            }
        } else if entry.ax_unavailable {
            PixelPolicy {
                allowed: true,
                reason: "the window's accessibility surface is unavailable or empty; a point \
                         address read from the screenshot is the only route",
            }
        } else {
            PixelPolicy {
                allowed: false,
                reason: "the accessibility route works; address elements by element_token",
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Targets and executors
// ═══════════════════════════════════════════════════════════════════════════

/// One request's execution against a target, inside its session policy.
pub trait ActionExecutor: Send + Sync {
    fn execute<'a>(
        &'a self,
        action: CuaAction,
    ) -> Pin<Box<dyn Future<Output = Result<CuaResponseEnvelope, ExecError>> + Send + 'a>>;
}

impl ActionExecutor for RunSession {
    fn execute<'a>(
        &'a self,
        action: CuaAction,
    ) -> Pin<Box<dyn Future<Output = Result<CuaResponseEnvelope, ExecError>> + Send + 'a>> {
        Box::pin(RunSession::execute(self, action))
    }
}

/// Requests straight to a checked adapter: a desktop target (#17), whose
/// own driver keeps its implicit session, or a test fake.
pub struct AdapterExecutor {
    adapter: Arc<CheckedCuaAdapter>,
}

impl AdapterExecutor {
    pub fn new(adapter: Arc<CheckedCuaAdapter>) -> Self {
        Self { adapter }
    }

    fn envelope(&self, action: CuaAction) -> CuaRequestEnvelope {
        CuaRequestEnvelope {
            version: ProtocolVersion::V1,
            request_id: RequestId::try_from(format!("nolune-{}", uuid::Uuid::new_v4()))
                .expect("a prefix plus a uuid is an identifier"),
            machine_id: self.adapter.descriptor().machine_id.clone(),
            action,
        }
    }
}

impl ActionExecutor for AdapterExecutor {
    fn execute<'a>(
        &'a self,
        action: CuaAction,
    ) -> Pin<Box<dyn Future<Output = Result<CuaResponseEnvelope, ExecError>> + Send + 'a>> {
        Box::pin(async move {
            self.adapter
                .descriptor()
                .authorize(&action)
                .map_err(|error| ExecError::Refused(error.to_string()))?;
            let request = self.envelope(action);
            self.adapter
                .execute(&request)
                .await
                .map_err(ExecError::Protocol)
        })
    }
}

/// A machine the orchestrator drives. The server-local target executes each
/// operation as one run of its runtime (one driver session per tool call,
/// ended with it); a desktop target executes straight through its checked
/// adapter.
pub struct Target {
    adapter: Arc<CheckedCuaAdapter>,
    runs: Option<CuaRuntime>,
}

impl Target {
    /// A desktop target (#17) or a test fake.
    pub fn direct(adapter: Arc<CheckedCuaAdapter>) -> Self {
        Self {
            adapter,
            runs: None,
        }
    }

    /// The server-local target (#16): every operation is one run.
    pub fn server_local(adapter: Arc<CheckedCuaAdapter>, runtime: CuaRuntime) -> Self {
        Self {
            adapter,
            runs: Some(runtime),
        }
    }

    pub fn descriptor(&self) -> &MachineDescriptor {
        self.adapter.descriptor()
    }

    pub fn machine_id(&self) -> &MachineId {
        &self.adapter.descriptor().machine_id
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Failures
// ═══════════════════════════════════════════════════════════════════════════

/// Why an operation did not succeed, in words the model relays. Every
/// message starts with its code.
#[derive(Clone, Debug, PartialEq)]
pub enum Failure {
    /// The ledger refused an address.
    Ledger(LedgerRefusal),
    /// The target's descriptor refuses the action: capability not
    /// advertised, machine unavailable, or an action the run manages.
    Refused(String),
    /// A permission the action needs is not granted on the target.
    PermissionDenied {
        permission: PermissionKind,
        state: Permission,
    },
    /// The driver answered with a typed runtime error.
    Driver(CuaRuntimeError),
    /// The driver refused the action on its background route. The
    /// escalation it recommends is reported, never applied.
    ActionRefused { outcome: ActionOutcome },
    /// The action reached the driver but its effect was not confirmed, and
    /// no predicates were given to verify it.
    Unconfirmed { outcome: ActionOutcome },
    /// The predicates were evaluated and not satisfied.
    Unverified { verification: VerificationResult },
    /// The run or the transport failed.
    Exec(String),
    /// The driver's answer did not pass the protocol boundary.
    Protocol(String),
    /// The orchestrator was handed an action it does not orchestrate.
    Unsupported(CuaActionKind),
}

impl Failure {
    /// Stable code the message starts with.
    pub fn code(&self) -> String {
        match self {
            Self::Ledger(refusal) => refusal.code().to_owned(),
            Self::Refused(_) => "action_refused".to_owned(),
            Self::PermissionDenied { .. } => "permission_denied".to_owned(),
            Self::Driver(error) => serde_json::to_value(error.code)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_else(|| "driver_error".to_owned()),
            Self::ActionRefused { .. } => "action_refused".to_owned(),
            Self::Unconfirmed { .. } => "unverified".to_owned(),
            Self::Unverified { verification } => match verification.overall {
                PredicateStatus::Unsatisfied => "verification_failed".to_owned(),
                _ => "unverified".to_owned(),
            },
            Self::Exec(_) => "run_failed".to_owned(),
            Self::Protocol(_) => "protocol_error".to_owned(),
            Self::Unsupported(_) => "unsupported_action".to_owned(),
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ledger(refusal) => return refusal.fmt(f),
            _ => write!(f, "{}: ", self.code())?,
        }
        match self {
            Self::Ledger(_) => unreachable!("written above"),
            Self::Refused(reason) => write!(f, "the target refuses the action: {reason}"),
            Self::PermissionDenied { permission, state } => {
                let permission = match permission {
                    PermissionKind::Accessibility => "Accessibility",
                    PermissionKind::ScreenCapture => "Screen recording",
                };
                let state = match state {
                    Permission::Denied => "denied",
                    Permission::PromptRequired => "not been allowed yet",
                    Permission::Unavailable => "unavailable",
                    Permission::Granted => "granted",
                };
                write!(
                    f,
                    "{permission} is {state} for the driver on this computer, so the action \
                     cannot be delivered; ask the user to grant it to the driver there \
                     (`nolune cua status` on the server machine, the Computers page for a \
                     desktop)"
                )
            }
            Self::Driver(error) => {
                write!(f, "the driver answered: {}", error.message.as_str())?;
                if error.retryable {
                    f.write_str(" (retryable)")?;
                }
                if error.code == cua_protocol::RuntimeErrorCode::StaleSnapshot {
                    f.write_str("; call get_window_state again and use its tokens")?;
                }
                Ok(())
            }
            Self::ActionRefused { outcome } => {
                write!(
                    f,
                    "the driver refused the action on its {} route",
                    kind_name(outcome.route)
                )?;
                if let Some(escalation) = &outcome.escalation {
                    write!(
                        f,
                        " (reason: {}); it recommends escalating to {} control, which Nolune \
                         never does: delivery stays background",
                        kind_name(escalation.reason),
                        kind_name(escalation.target)
                    )?;
                }
                f.write_str(
                    "; observe the window again and act with an element_token, or a point \
                     address if pixel_addresses allows it",
                )
            }
            Self::Unconfirmed { outcome } => {
                if outcome.effect == ActionEffect::Confirmed {
                    write!(
                        f,
                        "the driver reports the action delivered (evidence: {}) but read \
                         nothing back that confirms its effect",
                        evidence_names(outcome)
                    )?;
                } else {
                    write!(
                        f,
                        "the driver reports the effect as {} (evidence: {})",
                        kind_name(outcome.effect),
                        evidence_names(outcome)
                    )?;
                    if let Some(escalation) = &outcome.escalation {
                        write!(
                            f,
                            "; it recommends escalating to {} control ({}), which Nolune never \
                             does",
                            kind_name(escalation.target),
                            kind_name(escalation.reason)
                        )?;
                    }
                }
                f.write_str(
                    "; not reported as done. Pass verify.expect predicates for what the action \
                     must change, or observe the window again to see what happened",
                )
            }
            Self::Unverified { verification } => {
                let unsatisfied: Vec<String> = verification
                    .predicates
                    .iter()
                    .filter(|predicate| predicate.status == PredicateStatus::Unsatisfied)
                    .map(|predicate| predicate.predicate_index.to_string())
                    .collect();
                let unknown: Vec<String> = verification
                    .predicates
                    .iter()
                    .filter(|predicate| predicate.status == PredicateStatus::Unknown)
                    .map(|predicate| predicate.predicate_index.to_string())
                    .collect();
                if !unsatisfied.is_empty() {
                    write!(
                        f,
                        "predicate(s) {} of {} unsatisfied: the action did not land as \
                         expected",
                        unsatisfied.join(", "),
                        verification.predicates.len()
                    )?;
                } else {
                    write!(
                        f,
                        "predicate(s) {} of {} could not be evaluated: the effect is unknown",
                        unknown.join(", "),
                        verification.predicates.len()
                    )?;
                }
                f.write_str(
                    "; not reported as done. Observe the window again; a point address is \
                     allowed there now if the accessibility route failed you",
                )
            }
            Self::Exec(message) => f.write_str(message),
            Self::Protocol(message) => f.write_str(message),
            Self::Unsupported(kind) => write!(
                f,
                "{} is not something the typed machine tools perform",
                kind_name(*kind)
            ),
        }
    }
}

fn evidence_names(outcome: &ActionOutcome) -> String {
    if outcome.evidence.is_empty() {
        return "none".to_owned();
    }
    outcome
        .evidence
        .iter()
        .map(|evidence| kind_name(*evidence))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The driver confirmed the effect by reading it back, not merely by
/// delivering the input.
fn confirmed_by_readback(outcome: &ActionOutcome) -> bool {
    outcome.effect == ActionEffect::Confirmed
        && outcome.evidence.iter().any(|evidence| {
            matches!(
                evidence,
                ActionEvidence::AccessibilityReadback
                    | ActionEvidence::WindowReadback
                    | ActionEvidence::Snapshot
                    | ActionEvidence::Screenshot
            )
        })
}

/// The outcome an action result carries.
fn outcome_of(result: &CuaActionResult) -> Option<&ActionOutcome> {
    Some(match result {
        CuaActionResult::Click(result) => &result.outcome,
        CuaActionResult::DoubleClick(result) | CuaActionResult::RightClick(result) => {
            &result.outcome
        }
        CuaActionResult::MoveCursor(result) => &result.outcome,
        CuaActionResult::Drag(result) => &result.outcome,
        CuaActionResult::Scroll(result) => &result.outcome,
        CuaActionResult::TypeText(result) => &result.outcome,
        CuaActionResult::PressKey(result) => &result.outcome,
        CuaActionResult::Hotkey(result) => &result.outcome,
        CuaActionResult::SetValue(result) => &result.outcome,
        CuaActionResult::InvokeMenu(result) => &result.outcome,
        CuaActionResult::SetWindowFrame(result) => &result.outcome,
        _ => return None,
    })
}

impl std::error::Error for Failure {}

impl From<LedgerRefusal> for Failure {
    fn from(refusal: LedgerRefusal) -> Self {
        Self::Ledger(refusal)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Orchestrator
// ═══════════════════════════════════════════════════════════════════════════

/// Predicates to verify after an action, with the driver's timing bounds.
#[derive(Clone, Debug, PartialEq)]
pub struct VerifySpec {
    pub expect: Vec<VerifyPredicate>,
    pub timeout_ms: u16,
    pub stable_samples: u8,
}

/// A verified action: the driver's outcome and, when predicates were
/// given, their evaluation.
#[derive(Clone, Debug, PartialEq)]
pub struct ActReport {
    pub outcome: ActionOutcome,
    pub verification: Option<VerificationResult>,
}

/// One chat run's orchestration state: the ledger the typed tools share.
#[derive(Default)]
pub struct Orchestrator {
    ledger: SnapshotLedger,
}

impl Orchestrator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn ledger(&self) -> &SnapshotLedger {
        &self.ledger
    }

    /// `list_apps`, `list_windows` or `launch_app` on the target.
    pub async fn discover(
        &self,
        target: &Target,
        action: CuaAction,
    ) -> Result<CuaActionResult, Failure> {
        if !matches!(
            action,
            CuaAction::ListApps(_) | CuaAction::ListWindows(_) | CuaAction::LaunchApp(_)
        ) {
            return Err(Failure::Unsupported(action.kind()));
        }
        authorize(target.descriptor(), &action)?;
        let purpose = format!("discover_windows ({})", kind_name(action.kind()));
        self.with_executor(target, &purpose, |executor| async move {
            execute(executor.as_ref(), action).await
        })
        .await
    }

    /// `get_window_state`: the observation is recorded in the ledger before
    /// it is returned.
    pub async fn window_state(
        &self,
        target: &Target,
        args: GetWindowStateArgs,
    ) -> Result<Box<WindowStateResult>, Failure> {
        let action = CuaAction::GetWindowState(args.clone());
        authorize(target.descriptor(), &action)?;
        let result = self
            .with_executor(target, "get_window_state", |executor| async move {
                execute(executor.as_ref(), action).await
            })
            .await?;
        let CuaActionResult::GetWindowState(state) = result else {
            return Err(Failure::Protocol(
                "get_window_state answered with another kind of result".into(),
            ));
        };
        self.ledger.observed(target.machine_id(), &args, &state);
        Ok(state)
    }

    /// One action, checked against the ledger, delivered in the background,
    /// and reported only when verified.
    pub async fn act(
        &self,
        target: &Target,
        action: CuaAction,
        verify: Option<VerifySpec>,
    ) -> Result<ActReport, Failure> {
        let kind = action.kind();
        let Some((window, _)) = addressing(&action) else {
            return Err(Failure::Unsupported(kind));
        };
        let machine = target.machine_id().clone();
        authorize(target.descriptor(), &action)?;
        self.ledger.check(&machine, &action)?;
        // The verification is built and authorized before the action goes
        // out, so a predicate the protocol or the target refuses stops the
        // action instead of leaving it unverified.
        let verification = verify
            .map(|spec| {
                let action = CuaAction::VerifyState(VerifyStateArgs {
                    target: window,
                    session: None,
                    expect: spec.expect,
                    include_screenshot: false,
                    stable_samples: spec.stable_samples,
                    timeout_ms: spec.timeout_ms,
                });
                action
                    .validate()
                    .map_err(|error| Failure::Refused(format!("verification refused: {error}")))?;
                authorize(target.descriptor(), &action)?;
                Ok::<_, Failure>(action)
            })
            .transpose()?;

        let purpose = format!("act ({})", kind_name(kind));
        let outcome = self
            .with_executor(target, &purpose, |executor| async move {
                let result = execute(executor.as_ref(), action).await?;
                let outcome = outcome_of(&result)
                    .ok_or_else(|| {
                        Failure::Protocol("the action's result carries no outcome".into())
                    })?
                    .clone();
                if outcome.effect == ActionEffect::Refused {
                    return Ok((outcome, None));
                }
                let verification = match verification {
                    Some(action) => {
                        let CuaActionResult::VerifyState(verification) =
                            execute(executor.as_ref(), action).await?
                        else {
                            return Err(Failure::Protocol(
                                "verify_state answered with another kind of result".into(),
                            ));
                        };
                        Some(verification)
                    }
                    None => None,
                };
                Ok((outcome, verification))
            })
            .await;

        // The action was sent, so the latest observation no longer describes
        // the window, whatever the driver answered. A driver that calls the
        // snapshot stale is believed: the window is forgotten outright.
        match &outcome {
            Err(Failure::Driver(error))
                if error.code == cua_protocol::RuntimeErrorCode::StaleSnapshot =>
            {
                self.ledger.forget(&machine, window);
            }
            _ => self.ledger.acted(&machine, window, kind),
        }
        let (outcome, verification) = outcome?;

        if outcome.effect == ActionEffect::Refused {
            self.ledger.verified(&machine, window, false);
            return Err(Failure::ActionRefused { outcome });
        }
        match verification {
            Some(verification) => {
                let satisfied = verification.overall == PredicateStatus::Satisfied;
                self.ledger.verified(&machine, window, satisfied);
                if satisfied {
                    Ok(ActReport {
                        outcome,
                        verification: Some(verification),
                    })
                } else {
                    Err(Failure::Unverified { verification })
                }
            }
            None if confirmed_by_readback(&outcome) => {
                self.ledger.verified(&machine, window, true);
                Ok(ActReport {
                    outcome,
                    verification: None,
                })
            }
            None => {
                self.ledger.verified(&machine, window, false);
                Err(Failure::Unconfirmed { outcome })
            }
        }
    }

    /// `verify_state` on its own; an unsatisfied or unknown result is an
    /// error and allows pixels on that window next.
    pub async fn verify(
        &self,
        target: &Target,
        args: VerifyStateArgs,
    ) -> Result<VerificationResult, Failure> {
        let window = args.target;
        let action = CuaAction::VerifyState(args);
        authorize(target.descriptor(), &action)?;
        let result = self
            .with_executor(target, "verify_state", |executor| async move {
                execute(executor.as_ref(), action).await
            })
            .await?;
        let CuaActionResult::VerifyState(verification) = result else {
            return Err(Failure::Protocol(
                "verify_state answered with another kind of result".into(),
            ));
        };
        let satisfied = verification.overall == PredicateStatus::Satisfied;
        self.ledger.verified(target.machine_id(), window, satisfied);
        if satisfied {
            Ok(verification)
        } else {
            Err(Failure::Unverified { verification })
        }
    }

    /// Run `body` with the executor the target's kind calls for: one run
    /// of the runtime on the server machine, the adapter itself on a
    /// desktop.
    async fn with_executor<T, F, Fut>(
        &self,
        target: &Target,
        purpose: &str,
        body: F,
    ) -> Result<T, Failure>
    where
        F: FnOnce(Arc<dyn ActionExecutor>) -> Fut,
        Fut: Future<Output = Result<T, Failure>>,
    {
        match &target.runs {
            Some(runtime) => {
                let outcome = runtime
                    .run(purpose, |session| async move {
                        let executor: Arc<dyn ActionExecutor> = session;
                        Ok(body(executor).await)
                    })
                    .await;
                match outcome {
                    Ok(result) => result,
                    Err(error) => Err(Failure::Exec(error.to_string())),
                }
            }
            None => body(Arc::new(AdapterExecutor::new(target.adapter.clone()))).await,
        }
    }
}

/// The target's own say before anything is sent: an unavailable machine, a
/// permission the action needs but the driver does not hold, a capability
/// it does not advertise.
fn authorize(descriptor: &MachineDescriptor, action: &CuaAction) -> Result<(), Failure> {
    if descriptor.health == cua_protocol::MachineHealth::Unavailable {
        return Err(Failure::Refused(
            "the driver reports the machine unavailable".into(),
        ));
    }
    for permission in action.required_permissions() {
        let state = match permission {
            PermissionKind::Accessibility => descriptor.permissions.accessibility,
            PermissionKind::ScreenCapture => descriptor.permissions.screen_capture,
        };
        if state != Permission::Granted {
            return Err(Failure::PermissionDenied { permission, state });
        }
    }
    descriptor
        .authorize(action)
        .map_err(|error| Failure::Refused(error.to_string()))
}

/// One request through the executor: a driver error envelope is the typed
/// failure, a successful envelope its result.
async fn execute(
    executor: &dyn ActionExecutor,
    action: CuaAction,
) -> Result<CuaActionResult, Failure> {
    let envelope = executor
        .execute(action)
        .await
        .map_err(|error| match error {
            ExecError::Refused(reason) => Failure::Refused(reason),
            ExecError::SessionFailed(reason) => {
                Failure::Exec(format!("the driver session could not start: {reason}"))
            }
            ExecError::Protocol(error) => Failure::Protocol(error.to_string()),
        })?;
    match envelope.response {
        CuaResponse::Success { result } => Ok(*result),
        CuaResponse::Error { error } => Err(Failure::Driver(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::cua::transport::fake::FakeTransport;
    use crate::services::machine_registry::MachineRegistry;
    use cua_protocol::{
        AddressedActionArgs, Capability, ClickAction, ClickArgs, DeliveryMode, DriverVersion,
        ElementCondition, ElementPredicate, ElementSelector, ElementToken, EmptyArgs,
        EmptyValueText, ListWindowsArgs, MachineHealth, MachineLocation, MouseButton,
        PermissionState, Platform, RuntimeErrorCode, SetValueArgs, TypeTextArgs, WindowPoint,
        driver_mcp::{DriverCallFailure, response_for},
    };
    use serde_json::{Value, json};
    use std::{collections::VecDeque, sync::Mutex};

    const STUDIO: &str = "server-local:studio";
    const LAPTOP: &str = "9a8b7c6d-5e4f-4a3b-9c2d-1e0f9a8b7c6d";

    fn descriptor(machine_id: &str, location: MachineLocation) -> MachineDescriptor {
        MachineDescriptor {
            machine_id: MachineId::try_from(machine_id).unwrap(),
            location,
            platform: Platform::Macos,
            driver_version: DriverVersion::try_from("0.28.2").unwrap(),
            health: MachineHealth::Healthy,
            permissions: PermissionState {
                accessibility: Permission::Granted,
                screen_capture: Permission::Granted,
            },
            capabilities: vec![
                Capability::AppDiscovery,
                Capability::AppLaunch,
                Capability::WindowDiscovery,
                Capability::WindowObservation,
                Capability::WindowManagement,
                Capability::Pointer,
                Capability::Keyboard,
                Capability::ElementValue,
                Capability::Menu,
                Capability::Verification,
                Capability::SessionLifecycle,
                Capability::Health,
            ],
        }
    }

    fn desktop() -> MachineDescriptor {
        descriptor(LAPTOP, MachineLocation::Desktop)
    }

    /// What the fake answers one request with.
    #[derive(Clone)]
    enum Answer {
        /// A raw driver payload (a window state, an apps list, ...).
        Payload(Value),
        /// An action result echoing the request's own arguments with this
        /// `outcome` object.
        Outcome(Value),
        /// A verification result with this overall status and one
        /// predicate per requested predicate.
        Verification(&'static str),
        /// A driver failure.
        Fail(DriverCallFailure),
    }

    type Log = Arc<Mutex<Vec<CuaRequestEnvelope>>>;

    /// A fake target: a checked adapter over a closure that answers from a
    /// script and records every request it received.
    fn fake(descriptor: MachineDescriptor, script: Vec<Answer>) -> (Target, Log) {
        let script = Arc::new(Mutex::new(VecDeque::from(script)));
        let log: Log = Arc::new(Mutex::new(Vec::new()));
        let seen = log.clone();
        let adapter = CheckedCuaAdapter::new(descriptor, move |request| {
            seen.lock().unwrap().push(request.clone());
            let answer = script.lock().unwrap().pop_front().unwrap_or(Answer::Fail(
                DriverCallFailure::Transport("no scripted answer".into()),
            ));
            Box::pin(async move { answer_to(&request, answer) })
        })
        .unwrap();
        (Target::direct(Arc::new(adapter)), log)
    }

    fn answer_to(request: &CuaRequestEnvelope, answer: Answer) -> CuaResponseEnvelope {
        let outcome = match answer {
            Answer::Payload(payload) => Ok(payload),
            Answer::Outcome(outcome) => Ok(echo(&request.action, outcome)),
            Answer::Verification(overall) => {
                let CuaAction::VerifyState(args) = &request.action else {
                    panic!("a verification answers verify_state");
                };
                let predicates: Vec<Value> = (0..args.expect.len())
                    .map(|index| json!({"predicate_index": index, "status": overall}))
                    .collect();
                Ok(json!({"overall": overall, "predicates": predicates}))
            }
            Answer::Fail(failure) => Err(failure),
        };
        response_for(request, outcome)
    }

    /// The action's own arguments as its result, plus the outcome: what a
    /// driver echoes back for a click, a typed text, a set value.
    fn echo(action: &CuaAction, outcome: Value) -> Value {
        let mut value = serde_json::to_value(action).unwrap();
        let mut args = value["args"].take();
        let object = args.as_object_mut().unwrap();
        object.remove("delivery_mode");
        object.remove("modifiers");
        object.insert("outcome".into(), outcome);
        match action {
            CuaAction::PressKey(args) => {
                object.insert("modifiers".into(), json!(args.modifiers));
            }
            CuaAction::Drag(_) => {
                object.remove("modifiers");
            }
            _ => {}
        }
        args
    }

    fn confirmed(evidence: &[&str]) -> Value {
        json!({
            "effect": "confirmed", "route": "accessibility",
            "delivery": {"requested": "background", "delivered_count": 1},
            "evidence": evidence,
        })
    }

    fn outcome(effect: &str, escalation: Option<Value>) -> Value {
        let mut value = json!({
            "effect": effect, "route": "accessibility",
            "delivery": {"requested": "background", "delivered_count": 1},
            "evidence": ["delivery_receipt"],
        });
        if let Some(escalation) = escalation {
            value["escalation"] = escalation;
        }
        value
    }

    fn window() -> WindowTarget {
        WindowTarget {
            pid: 42,
            window_id: 99,
        }
    }

    fn element(index: u32, token: &str, role: &str, label: &str) -> Value {
        json!({
            "element_index": index, "element_token": token, "role": role, "label": label,
            "actions": ["AXPress"], "enabled": true,
            "frame": {"x": 10.0, "y": 20.0, "width": 80.0, "height": 24.0}, "depth": 0
        })
    }

    /// A healthy window state carrying `snapshot` with the given elements.
    fn state(snapshot: &str, elements: Vec<Value>) -> Value {
        json!({
            "target": {"pid": 42, "window_id": 99},
            "snapshot_id": snapshot,
            "elements": elements,
            "truncated": false,
            "app_name": "Notes",
            "window_title": "Untitled",
        })
    }

    fn degraded_state() -> Value {
        let fixture =
            include_str!("../../../../cua-protocol/tests/fixtures/degraded-window-state.json");
        let mut snapshot: Value = serde_json::from_str(fixture).unwrap();
        snapshot["target"] = json!({"pid": 42, "window_id": 99});
        snapshot["background_input"]["exact_window"]["pid"] = json!(42);
        snapshot["background_input"]["exact_window"]["window_id"] = json!(99);
        snapshot
    }

    fn observe_args(tree: bool) -> GetWindowStateArgs {
        GetWindowStateArgs {
            target: window(),
            session: None,
            include_accessibility_tree: tree,
            include_screenshot: !tree,
            max_elements: None,
            max_depth: None,
            max_dimension: None,
            query: None,
        }
    }

    fn click_token(token: &str) -> CuaAction {
        CuaAction::Click(ClickArgs {
            target: window(),
            session: None,
            delivery_mode: DeliveryMode::Background,
            address: ElementAddress::ElementToken {
                element_token: ElementToken::try_from(token).unwrap(),
            },
            button: MouseButton::Left,
            action: ClickAction::Press,
            modifiers: vec![],
            count: None,
        })
    }

    fn click_index(index: u32, snapshot: &str) -> CuaAction {
        CuaAction::Click(ClickArgs {
            target: window(),
            session: None,
            delivery_mode: DeliveryMode::Background,
            address: ElementAddress::ElementIndex {
                element_index: index,
                snapshot_id: SnapshotId::try_from(snapshot).unwrap(),
            },
            button: MouseButton::Left,
            action: ClickAction::Press,
            modifiers: vec![],
            count: None,
        })
    }

    fn click_point() -> CuaAction {
        CuaAction::Click(ClickArgs {
            target: window(),
            session: None,
            delivery_mode: DeliveryMode::Background,
            address: ElementAddress::Point(WindowPoint { x: 12.0, y: 34.0 }),
            button: MouseButton::Left,
            action: ClickAction::Press,
            modifiers: vec![],
            count: None,
        })
    }

    fn type_text(token: &str) -> CuaAction {
        CuaAction::TypeText(TypeTextArgs {
            target: window(),
            session: None,
            delivery_mode: DeliveryMode::Background,
            address: ElementAddress::ElementToken {
                element_token: ElementToken::try_from(token).unwrap(),
            },
            text: cua_protocol::BoundedText::try_from("hello").unwrap(),
            delay_ms: 0,
        })
    }

    fn set_value(token: &str) -> CuaAction {
        CuaAction::SetValue(SetValueArgs {
            target: window(),
            session: None,
            element: ElementRef::ElementToken {
                element_token: ElementToken::try_from(token).unwrap(),
            },
            value: EmptyValueText::try_from("hello").unwrap(),
        })
    }

    fn value_equals(label: &str, value: &str) -> VerifySpec {
        VerifySpec {
            expect: vec![VerifyPredicate::Element(ElementPredicate {
                selector: ElementSelector {
                    role: None,
                    label_contains: Some(cua_protocol::BoundedText::try_from(label).unwrap()),
                },
                condition: ElementCondition::ValueEquals(EmptyValueText::try_from(value).unwrap()),
            })],
            timeout_ms: 500,
            stable_samples: 1,
        }
    }

    fn kinds(log: &Log) -> Vec<CuaActionKind> {
        log.lock()
            .unwrap()
            .iter()
            .map(|request| request.action.kind())
            .collect()
    }

    async fn observe(orchestrator: &Orchestrator, target: &Target, tree: bool) {
        orchestrator
            .window_state(target, observe_args(tree))
            .await
            .unwrap();
    }

    // ── Slice 1: the ledger ────────────────────────────────────────────

    #[tokio::test]
    async fn an_element_action_before_any_window_state_is_refused() {
        let orchestrator = Orchestrator::new();
        let (target, log) = fake(desktop(), vec![Answer::Outcome(confirmed(&["snapshot"]))]);

        for action in [
            click_token("tok/1"),
            click_index(0, "s00000001"),
            set_value("tok/1"),
        ] {
            let error = orchestrator.act(&target, action, None).await.unwrap_err();
            assert_eq!(
                error,
                Failure::Ledger(LedgerRefusal::NoSnapshot { target: window() })
            );
            let text = error.to_string();
            assert!(
                text.starts_with("snapshot_required: ") && text.contains("get_window_state"),
                "{text}"
            );
        }
        assert!(kinds(&log).is_empty(), "nothing reached the driver");

        // A point address needs the screenshot it is read from too.
        let error = orchestrator
            .act(&target, click_point(), None)
            .await
            .unwrap_err();
        assert_eq!(
            error,
            Failure::Ledger(LedgerRefusal::NoSnapshot { target: window() })
        );
        assert!(kinds(&log).is_empty());
    }

    #[tokio::test]
    async fn a_token_from_a_superseded_snapshot_fails_closed() {
        let orchestrator = Orchestrator::new();
        let (target, log) = fake(
            desktop(),
            vec![
                Answer::Payload(state(
                    "s00000001",
                    vec![element(0, "tok/a", "AXButton", "Save")],
                )),
                Answer::Payload(state(
                    "s00000002",
                    vec![element(0, "tok/b", "AXButton", "Save")],
                )),
            ],
        );
        observe(&orchestrator, &target, true).await;
        observe(&orchestrator, &target, true).await;
        let machine = target.machine_id().clone();
        let s1 = SnapshotId::try_from("s00000001").unwrap();
        let s2 = SnapshotId::try_from("s00000002").unwrap();

        // The token s1 issued is stale now that s2 replaced it.
        let error = orchestrator
            .act(&target, click_token("tok/a"), None)
            .await
            .unwrap_err();
        assert_eq!(
            error,
            Failure::Ledger(LedgerRefusal::StaleSnapshot {
                target: window(),
                latest: s2.clone(),
                superseded: Some(s1.clone()),
            })
        );
        let text = error.to_string();
        assert!(
            text.starts_with("stale_snapshot: ")
                && text.contains("s00000001")
                && text.contains("s00000002"),
            "{text}"
        );

        // An index with the superseded snapshot id, and an index the latest
        // snapshot never issued, are stale too. A token nobody issued is
        // stale rather than unknown: the ledger never guesses.
        assert_eq!(
            orchestrator
                .ledger()
                .check(&machine, &click_index(0, "s00000001")),
            Err(LedgerRefusal::StaleSnapshot {
                target: window(),
                latest: s2.clone(),
                superseded: Some(s1.clone()),
            })
        );
        assert_eq!(
            orchestrator
                .ledger()
                .check(&machine, &click_index(7, "s00000002")),
            Err(LedgerRefusal::StaleSnapshot {
                target: window(),
                latest: s2.clone(),
                superseded: Some(s1.clone()),
            })
        );
        assert_eq!(
            orchestrator
                .ledger()
                .check(&machine, &set_value("tok/never")),
            Err(LedgerRefusal::StaleSnapshot {
                target: window(),
                latest: s2,
                superseded: Some(s1),
            })
        );
        assert_eq!(
            kinds(&log),
            [CuaActionKind::GetWindowState, CuaActionKind::GetWindowState],
            "no stale action reached the driver"
        );
    }

    #[tokio::test]
    async fn the_latest_snapshots_token_is_forwarded_in_the_background() {
        let orchestrator = Orchestrator::new();
        let (target, log) = fake(
            desktop(),
            vec![
                Answer::Payload(state(
                    "s00000001",
                    vec![element(0, "tok/a", "AXButton", "Save")],
                )),
                Answer::Payload(state(
                    "s00000002",
                    vec![
                        element(0, "tok/b", "AXButton", "Save"),
                        element(1, "tok/c", "AXTextField", "Name"),
                    ],
                )),
                Answer::Outcome(confirmed(&["accessibility_readback"])),
                Answer::Payload(state(
                    "s00000003",
                    vec![element(1, "tok/d", "AXTextField", "Name")],
                )),
                Answer::Outcome(confirmed(&["accessibility_readback"])),
            ],
        );
        observe(&orchestrator, &target, true).await;
        observe(&orchestrator, &target, true).await;

        let report = orchestrator
            .act(&target, click_token("tok/b"), None)
            .await
            .unwrap();
        assert_eq!(report.outcome.effect, ActionEffect::Confirmed);
        assert_eq!(report.verification, None);

        // An index from the latest snapshot is forwarded as well.
        observe(&orchestrator, &target, true).await;
        orchestrator
            .act(&target, click_index(1, "s00000003"), None)
            .await
            .unwrap();

        let requests = log.lock().unwrap().clone();
        let CuaAction::Click(click) = &requests[2].action else {
            panic!("the click was forwarded: {requests:?}");
        };
        assert_eq!(
            click.address,
            ElementAddress::ElementToken {
                element_token: ElementToken::try_from("tok/b").unwrap()
            }
        );
        assert_eq!(click.delivery_mode, DeliveryMode::Background);
        assert_eq!(
            click.session, None,
            "a desktop target keeps its own session"
        );
        assert_eq!(requests[2].machine_id.as_str(), LAPTOP);
        assert_eq!(
            requests
                .iter()
                .map(|request| request.request_id.as_str().to_owned())
                .collect::<HashSet<_>>()
                .len(),
            requests.len(),
            "every request has its own id"
        );
    }

    #[tokio::test]
    async fn tokens_are_scoped_per_machine_and_window() {
        let orchestrator = Orchestrator::new();
        let (laptop, _) = fake(
            desktop(),
            vec![Answer::Payload(state(
                "s00000001",
                vec![element(0, "tok/a", "AXButton", "Save")],
            ))],
        );
        let (studio, studio_log) = fake(descriptor(STUDIO, MachineLocation::ServerLocal), vec![]);
        observe(&orchestrator, &laptop, true).await;

        // The same token on another machine: nothing observed there.
        let error = orchestrator
            .act(&studio, click_token("tok/a"), None)
            .await
            .unwrap_err();
        assert_eq!(
            error,
            Failure::Ledger(LedgerRefusal::NoSnapshot { target: window() })
        );
        assert!(kinds(&studio_log).is_empty());

        // Another window of the same machine: nothing observed there either.
        let other = WindowTarget {
            pid: 42,
            window_id: 100,
        };
        let mut action = click_token("tok/a");
        if let CuaAction::Click(args) = &mut action {
            args.target = other;
        }
        assert_eq!(
            orchestrator.ledger().check(laptop.machine_id(), &action),
            Err(LedgerRefusal::NoSnapshot { target: other })
        );
        // The window it was issued for still accepts it.
        assert_eq!(
            orchestrator
                .ledger()
                .check(laptop.machine_id(), &click_token("tok/a")),
            Ok(())
        );
    }

    #[tokio::test]
    async fn an_observation_without_a_tree_clears_the_tokens() {
        let orchestrator = Orchestrator::new();
        let (target, _) = fake(
            desktop(),
            vec![
                Answer::Payload(state(
                    "s00000001",
                    vec![element(0, "tok/a", "AXButton", "Save")],
                )),
                Answer::Payload(json!({
                    "target": {"pid": 42, "window_id": 99},
                    "elements": [],
                    "screenshot": {"media_type": "png", "base64": tiny_png(), "width": 1, "height": 1},
                    "truncated": false,
                })),
            ],
        );
        observe(&orchestrator, &target, true).await;
        observe(&orchestrator, &target, false).await;
        let error = orchestrator
            .act(&target, click_token("tok/a"), None)
            .await
            .unwrap_err();
        assert_eq!(
            error,
            Failure::Ledger(LedgerRefusal::NoAccessibilityTree { target: window() })
        );
        assert!(
            error.to_string().contains("include_accessibility_tree"),
            "{error}"
        );
    }

    fn tiny_png() -> String {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(include_bytes!(
            "../../../../cua-protocol/tests/fixtures/tiny.png"
        ))
    }

    #[tokio::test]
    async fn a_snapshot_is_consumed_by_the_action_taken_on_it() {
        let orchestrator = Orchestrator::new();
        let (target, log) = fake(
            desktop(),
            vec![
                Answer::Payload(state(
                    "s00000001",
                    vec![
                        element(0, "tok/a", "AXButton", "Save"),
                        element(1, "tok/b", "AXTextField", "Name"),
                    ],
                )),
                Answer::Outcome(confirmed(&["accessibility_readback"])),
                Answer::Payload(state(
                    "s00000002",
                    vec![element(1, "tok/c", "AXTextField", "Name")],
                )),
                Answer::Outcome(confirmed(&["accessibility_readback"])),
            ],
        );
        observe(&orchestrator, &target, true).await;
        orchestrator
            .act(&target, click_token("tok/a"), None)
            .await
            .unwrap();

        // The window may have changed: the other token of the same snapshot
        // is not forwarded until the window is observed again.
        let error = orchestrator
            .act(&target, type_text("tok/b"), None)
            .await
            .unwrap_err();
        assert_eq!(
            error,
            Failure::Ledger(LedgerRefusal::SnapshotConsumed {
                target: window(),
                snapshot: Some(SnapshotId::try_from("s00000001").unwrap()),
                action: CuaActionKind::Click,
            })
        );
        assert!(error.to_string().starts_with("snapshot_consumed: "));

        observe(&orchestrator, &target, true).await;
        orchestrator
            .act(&target, type_text("tok/c"), None)
            .await
            .unwrap();
        assert_eq!(
            kinds(&log),
            [
                CuaActionKind::GetWindowState,
                CuaActionKind::Click,
                CuaActionKind::GetWindowState,
                CuaActionKind::TypeText,
            ]
        );
    }

    #[tokio::test]
    async fn discovery_and_menus_need_no_snapshot() {
        let orchestrator = Orchestrator::new();
        let (target, log) = fake(
            desktop(),
            vec![
                Answer::Payload(json!({"apps": []})),
                Answer::Payload(json!({"windows": [], "current_space_id": 3})),
                Answer::Outcome(confirmed(&["accessibility_readback"])),
            ],
        );
        let apps = orchestrator
            .discover(&target, CuaAction::ListApps(EmptyArgs {}))
            .await
            .unwrap();
        assert!(matches!(apps, CuaActionResult::ListApps(_)));
        let windows = orchestrator
            .discover(
                &target,
                CuaAction::ListWindows(ListWindowsArgs {
                    pid: None,
                    on_screen_only: true,
                }),
            )
            .await
            .unwrap();
        assert!(matches!(windows, CuaActionResult::ListWindows(_)));
        assert_eq!(
            kinds(&log),
            [CuaActionKind::ListApps, CuaActionKind::ListWindows]
        );

        // Discovery never forwards an action.
        let error = orchestrator
            .discover(&target, click_token("tok/a"))
            .await
            .unwrap_err();
        assert_eq!(error, Failure::Unsupported(CuaActionKind::Click));
        assert_eq!(kinds(&log).len(), 2);

        // A menu path addresses nothing inside the window, so it needs no
        // snapshot; it still consumes whatever the window's next action
        // would have relied on.
        let menu = CuaAction::InvokeMenu(cua_protocol::InvokeMenuArgs {
            target: window(),
            session: None,
            path: vec![
                cua_protocol::BoundedText::try_from("File").unwrap(),
                cua_protocol::BoundedText::try_from("Save").unwrap(),
            ],
        });
        let report = orchestrator.act(&target, menu, None).await.unwrap();
        assert_eq!(report.outcome.effect, ActionEffect::Confirmed);
        assert_eq!(kinds(&log).len(), 3);
        assert!(matches!(
            orchestrator
                .ledger()
                .check(target.machine_id(), &click_token("tok/a")),
            Err(LedgerRefusal::SnapshotConsumed { .. })
        ));
    }

    // ── Slice 2: the verification gate ─────────────────────────────────

    #[tokio::test]
    async fn a_confirmed_action_whose_verification_is_not_satisfied_is_an_error() {
        for (overall, code) in [
            ("unknown", "unverified"),
            ("unsatisfied", "verification_failed"),
        ] {
            let orchestrator = Orchestrator::new();
            let (target, log) = fake(
                desktop(),
                vec![
                    Answer::Payload(state(
                        "s00000001",
                        vec![element(1, "tok/b", "AXTextField", "Name")],
                    )),
                    Answer::Outcome(confirmed(&["accessibility_readback"])),
                    Answer::Verification(overall),
                ],
            );
            observe(&orchestrator, &target, true).await;
            let error = orchestrator
                .act(
                    &target,
                    type_text("tok/b"),
                    Some(value_equals("Name", "hello")),
                )
                .await
                .unwrap_err();
            let Failure::Unverified { verification } = &error else {
                panic!("{overall}: expected an unverified failure, got {error:?}");
            };
            assert_eq!(serde_json::to_value(verification.overall).unwrap(), overall);
            assert_eq!(error.code(), code);
            assert!(
                error.to_string().starts_with(&format!("{code}: ")),
                "{error}"
            );
            assert_eq!(
                kinds(&log),
                [
                    CuaActionKind::GetWindowState,
                    CuaActionKind::TypeText,
                    CuaActionKind::VerifyState
                ],
                "{overall}: the orchestrator issued verify_state itself"
            );
            let requests = log.lock().unwrap().clone();
            let CuaAction::VerifyState(verify) = &requests[2].action else {
                panic!("verify_state was sent");
            };
            assert_eq!(verify.target, window());
            assert_eq!(verify.expect.len(), 1);
            assert!(!verify.include_screenshot);
        }
    }

    #[tokio::test]
    async fn a_refused_outcome_is_an_error_and_a_foreground_escalation_is_never_applied() {
        let orchestrator = Orchestrator::new();
        let (target, log) = fake(
            desktop(),
            vec![
                Answer::Payload(state(
                    "s00000001",
                    vec![element(0, "tok/a", "AXButton", "Save")],
                )),
                Answer::Outcome(json!({
                    "effect": "refused", "route": "accessibility",
                    "delivery": {"requested": "background"},
                    "evidence": [],
                    "escalation": {"target": "foreground", "reason": "route_unavailable"},
                })),
            ],
        );
        observe(&orchestrator, &target, true).await;
        let error = orchestrator
            .act(
                &target,
                click_token("tok/a"),
                Some(value_equals("Save", "done")),
            )
            .await
            .unwrap_err();
        let Failure::ActionRefused { outcome } = &error else {
            panic!("expected the refusal, got {error:?}");
        };
        assert_eq!(outcome.effect, ActionEffect::Refused);
        let text = error.to_string();
        assert!(text.starts_with("action_refused: "), "{text}");
        assert!(
            text.contains("foreground") && text.contains("never"),
            "the recommendation is quoted as something Nolune does not do: {text}"
        );
        assert!(text.contains("route_unavailable"), "{text}");
        assert_eq!(
            kinds(&log),
            [CuaActionKind::GetWindowState, CuaActionKind::Click],
            "no verification and no second delivery follow a refusal"
        );
        let requests = log.lock().unwrap().clone();
        let CuaAction::Click(click) = &requests[1].action else {
            panic!("the click was forwarded");
        };
        assert_eq!(click.delivery_mode, DeliveryMode::Background);
    }

    #[tokio::test]
    async fn an_unconfirmed_outcome_without_predicates_is_not_success() {
        for effect in ["unverifiable", "suspected_noop", "partial"] {
            let orchestrator = Orchestrator::new();
            let (target, log) = fake(
                desktop(),
                vec![
                    Answer::Payload(state(
                        "s00000001",
                        vec![element(0, "tok/a", "AXButton", "Save")],
                    )),
                    Answer::Outcome(outcome(effect, None)),
                ],
            );
            observe(&orchestrator, &target, true).await;
            let error = orchestrator
                .act(&target, click_token("tok/a"), None)
                .await
                .unwrap_err();
            let Failure::Unconfirmed { outcome } = &error else {
                panic!("{effect}: expected an unconfirmed failure, got {error:?}");
            };
            assert_eq!(serde_json::to_value(outcome.effect).unwrap(), effect);
            let text = error.to_string();
            assert!(
                text.starts_with("unverified: ") && text.contains("verify"),
                "{effect}: {text}"
            );
            assert_eq!(
                kinds(&log),
                [CuaActionKind::GetWindowState, CuaActionKind::Click]
            );
        }

        // Confirmed by a delivery receipt alone is delivered, not verified.
        let orchestrator = Orchestrator::new();
        let (target, _) = fake(
            desktop(),
            vec![
                Answer::Payload(state(
                    "s00000001",
                    vec![element(0, "tok/a", "AXButton", "Save")],
                )),
                Answer::Outcome(confirmed(&["delivery_receipt"])),
            ],
        );
        observe(&orchestrator, &target, true).await;
        let error = orchestrator
            .act(&target, click_token("tok/a"), None)
            .await
            .unwrap_err();
        assert!(matches!(error, Failure::Unconfirmed { .. }), "{error:?}");
        assert!(error.to_string().contains("delivery"), "{error}");
    }

    #[tokio::test]
    async fn only_a_verified_outcome_succeeds() {
        let orchestrator = Orchestrator::new();
        let (target, log) = fake(
            desktop(),
            vec![
                Answer::Payload(state(
                    "s00000001",
                    vec![element(1, "tok/b", "AXTextField", "Name")],
                )),
                // Delivered but not read back: the predicates decide.
                Answer::Outcome(outcome("unverifiable", None)),
                Answer::Verification("satisfied"),
                Answer::Payload(state(
                    "s00000002",
                    vec![element(0, "tok/a", "AXButton", "Save")],
                )),
                // Read back by the driver: verified without predicates.
                Answer::Outcome(confirmed(&["accessibility_readback", "delivery_receipt"])),
            ],
        );
        observe(&orchestrator, &target, true).await;
        let report = orchestrator
            .act(
                &target,
                type_text("tok/b"),
                Some(value_equals("Name", "hello")),
            )
            .await
            .unwrap();
        assert_eq!(report.outcome.effect, ActionEffect::Unverifiable);
        assert_eq!(
            report.verification.as_ref().map(|v| v.overall),
            Some(PredicateStatus::Satisfied)
        );

        observe(&orchestrator, &target, true).await;
        let report = orchestrator
            .act(&target, click_token("tok/a"), None)
            .await
            .unwrap();
        assert_eq!(report.outcome.effect, ActionEffect::Confirmed);
        assert!(
            report
                .outcome
                .evidence
                .contains(&ActionEvidence::AccessibilityReadback)
        );
        assert_eq!(report.verification, None);
        assert_eq!(
            kinds(&log),
            [
                CuaActionKind::GetWindowState,
                CuaActionKind::TypeText,
                CuaActionKind::VerifyState,
                CuaActionKind::GetWindowState,
                CuaActionKind::Click,
            ]
        );
    }

    #[tokio::test]
    async fn a_point_address_is_refused_until_accessibility_is_unavailable_or_verification_failed()
    {
        let orchestrator = Orchestrator::new();
        let (target, log) = fake(
            desktop(),
            vec![
                Answer::Payload(state(
                    "s00000001",
                    vec![element(0, "tok/a", "AXButton", "Save")],
                )),
                Answer::Payload(degraded_state()),
                Answer::Outcome(confirmed(&["screenshot"])),
                Answer::Payload(state(
                    "s00000002",
                    vec![element(1, "tok/b", "AXTextField", "Name")],
                )),
                Answer::Outcome(confirmed(&["accessibility_readback"])),
                Answer::Verification("unsatisfied"),
                Answer::Payload(state(
                    "s00000003",
                    vec![element(1, "tok/c", "AXTextField", "Name")],
                )),
                Answer::Outcome(confirmed(&["screenshot"])),
                Answer::Verification("satisfied"),
                Answer::Payload(state(
                    "s00000004",
                    vec![element(1, "tok/d", "AXTextField", "Name")],
                )),
            ],
        );
        let machine = target.machine_id().clone();

        // A healthy snapshot: the accessibility route is the one to use.
        observe(&orchestrator, &target, true).await;
        assert!(
            !orchestrator
                .ledger()
                .pixel_policy(&machine, window())
                .allowed
        );
        let error = orchestrator
            .act(&target, click_point(), None)
            .await
            .unwrap_err();
        assert_eq!(
            error,
            Failure::Ledger(LedgerRefusal::PixelRefused { target: window() })
        );
        assert!(
            error.to_string().starts_with("pixel_refused: ")
                && error.to_string().contains("element_token"),
            "{error}"
        );
        assert_eq!(kinds(&log), [CuaActionKind::GetWindowState]);

        // The exact window's accessibility surface is unresolved: pixels are
        // the only route left, and the driver's foreground recommendation is
        // not what happens.
        observe(&orchestrator, &target, true).await;
        let policy = orchestrator.ledger().pixel_policy(&machine, window());
        assert!(policy.allowed, "{policy:?}");
        let report = orchestrator
            .act(&target, click_point(), None)
            .await
            .unwrap();
        assert_eq!(report.outcome.effect, ActionEffect::Confirmed);
        let requests = log.lock().unwrap().clone();
        let CuaAction::Click(click) = &requests[2].action else {
            panic!("the pixel click was forwarded");
        };
        assert!(matches!(click.address, ElementAddress::Point(_)));
        assert_eq!(click.delivery_mode, DeliveryMode::Background);

        // Healthy again: pixels refused; then a verification that fails
        // opens the pixel route for this window.
        observe(&orchestrator, &target, true).await;
        assert!(
            !orchestrator
                .ledger()
                .pixel_policy(&machine, window())
                .allowed
        );
        let error = orchestrator
            .act(
                &target,
                type_text("tok/b"),
                Some(value_equals("Name", "hello")),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, Failure::Unverified { .. }), "{error:?}");
        let policy = orchestrator.ledger().pixel_policy(&machine, window());
        assert!(policy.allowed, "{policy:?}");
        assert!(policy.reason.contains("verification"), "{policy:?}");

        // The fresh snapshot the pixel click needs does not close it again;
        // a verified action does.
        observe(&orchestrator, &target, true).await;
        assert!(
            orchestrator
                .ledger()
                .pixel_policy(&machine, window())
                .allowed
        );
        orchestrator
            .act(&target, click_point(), Some(value_equals("Name", "hello")))
            .await
            .unwrap();
        observe(&orchestrator, &target, true).await;
        assert!(
            !orchestrator
                .ledger()
                .pixel_policy(&machine, window())
                .allowed
        );
        let error = orchestrator
            .act(&target, click_point(), None)
            .await
            .unwrap_err();
        assert_eq!(
            error,
            Failure::Ledger(LedgerRefusal::PixelRefused { target: window() })
        );
    }

    #[tokio::test]
    async fn a_tree_without_elements_counts_as_accessibility_unavailable() {
        let orchestrator = Orchestrator::new();
        let (target, _) = fake(
            desktop(),
            vec![Answer::Payload(json!({
                "target": {"pid": 42, "window_id": 99},
                "snapshot_id": "s00000009",
                "elements": [],
                "truncated": false,
            }))],
        );
        observe(&orchestrator, &target, true).await;
        let policy = orchestrator
            .ledger()
            .pixel_policy(target.machine_id(), window());
        assert!(policy.allowed, "{policy:?}");
    }

    #[tokio::test]
    async fn driver_errors_are_typed_and_a_stale_verdict_from_the_driver_drops_the_ledger_entry() {
        let orchestrator = Orchestrator::new();
        let (target, log) = fake(
            desktop(),
            vec![
                Answer::Payload(state(
                    "s00000001",
                    vec![element(0, "tok/a", "AXButton", "Save")],
                )),
                Answer::Fail(DriverCallFailure::Tool {
                    code: Some("stale_snapshot".into()),
                    message: "snapshot s00000001 was superseded".into(),
                }),
                Answer::Payload(state(
                    "s00000002",
                    vec![element(0, "tok/b", "AXButton", "Save")],
                )),
                Answer::Fail(DriverCallFailure::Timeout(
                    "click timed out after 30s".into(),
                )),
            ],
        );
        observe(&orchestrator, &target, true).await;
        let error = orchestrator
            .act(&target, click_token("tok/a"), None)
            .await
            .unwrap_err();
        let Failure::Driver(driver) = &error else {
            panic!("expected the driver's error, got {error:?}");
        };
        assert_eq!(driver.code, RuntimeErrorCode::StaleSnapshot);
        assert_eq!(error.code(), "stale_snapshot");
        assert!(error.to_string().starts_with("stale_snapshot: "), "{error}");

        // The ledger no longer trusts what it recorded for the window.
        let error = orchestrator
            .act(&target, click_token("tok/a"), None)
            .await
            .unwrap_err();
        assert_eq!(
            error,
            Failure::Ledger(LedgerRefusal::NoSnapshot { target: window() })
        );

        observe(&orchestrator, &target, true).await;
        let error = orchestrator
            .act(&target, click_token("tok/b"), None)
            .await
            .unwrap_err();
        let Failure::Driver(driver) = &error else {
            panic!("expected the driver's error, got {error:?}");
        };
        assert_eq!(driver.code, RuntimeErrorCode::Timeout);
        assert!(driver.retryable);
        assert_eq!(error.code(), "timeout");
        assert_eq!(
            kinds(&log),
            [
                CuaActionKind::GetWindowState,
                CuaActionKind::Click,
                CuaActionKind::GetWindowState,
                CuaActionKind::Click,
            ]
        );
    }

    #[tokio::test]
    async fn permission_and_capability_refusals_never_reach_the_adapter() {
        let orchestrator = Orchestrator::new();
        let mut denied = desktop();
        denied.permissions.accessibility = Permission::Denied;
        denied.capabilities.retain(|capability| {
            !matches!(
                capability,
                Capability::Pointer
                    | Capability::Keyboard
                    | Capability::ElementValue
                    | Capability::Menu
                    | Capability::Verification
                    | Capability::WindowManagement
            )
        });
        let (target, log) = fake(
            denied,
            vec![Answer::Payload(json!({
                "target": {"pid": 42, "window_id": 99},
                "elements": [],
                "screenshot": {"media_type": "png", "base64": tiny_png(), "width": 1, "height": 1},
                "truncated": false,
            }))],
        );
        observe(&orchestrator, &target, false).await;

        let error = orchestrator
            .act(&target, click_point(), None)
            .await
            .unwrap_err();
        assert_eq!(
            error,
            Failure::PermissionDenied {
                permission: PermissionKind::Accessibility,
                state: Permission::Denied,
            }
        );
        let text = error.to_string();
        assert!(
            text.starts_with("permission_denied: ") && text.contains("Accessibility"),
            "{text}"
        );

        // A tree needs Accessibility too; the observation itself is refused.
        let error = orchestrator
            .window_state(&target, observe_args(true))
            .await
            .unwrap_err();
        assert!(
            matches!(error, Failure::PermissionDenied { .. }),
            "{error:?}"
        );
        assert_eq!(kinds(&log), [CuaActionKind::GetWindowState]);
    }

    #[tokio::test]
    async fn verify_alone_reports_the_predicates_and_marks_the_window() {
        let orchestrator = Orchestrator::new();
        let (target, log) = fake(
            desktop(),
            vec![
                Answer::Payload(state(
                    "s00000001",
                    vec![element(0, "tok/a", "AXButton", "Save")],
                )),
                Answer::Verification("satisfied"),
                Answer::Verification("unsatisfied"),
            ],
        );
        observe(&orchestrator, &target, true).await;
        let args = VerifyStateArgs {
            target: window(),
            session: None,
            expect: vec![VerifyPredicate::WindowExists(true)],
            include_screenshot: false,
            stable_samples: 1,
            timeout_ms: 500,
        };
        let verification = orchestrator.verify(&target, args.clone()).await.unwrap();
        assert_eq!(verification.overall, PredicateStatus::Satisfied);
        assert!(
            !orchestrator
                .ledger()
                .pixel_policy(target.machine_id(), window())
                .allowed
        );

        let error = orchestrator.verify(&target, args).await.unwrap_err();
        assert!(matches!(error, Failure::Unverified { .. }), "{error:?}");
        assert!(
            orchestrator
                .ledger()
                .pixel_policy(target.machine_id(), window())
                .allowed,
            "a failed verification opens the pixel route"
        );
        // Verifying does not consume the snapshot: its tokens still stand.
        assert_eq!(
            orchestrator
                .ledger()
                .check(target.machine_id(), &click_token("tok/a")),
            Ok(())
        );
        assert_eq!(
            kinds(&log),
            [
                CuaActionKind::GetWindowState,
                CuaActionKind::VerifyState,
                CuaActionKind::VerifyState
            ]
        );
    }

    #[tokio::test]
    async fn a_foreground_recommendation_in_a_window_state_is_reported_not_applied() {
        let orchestrator = Orchestrator::new();
        let (target, log) = fake(desktop(), vec![Answer::Payload(degraded_state())]);
        let state = orchestrator
            .window_state(&target, observe_args(true))
            .await
            .unwrap();
        assert!(state.degraded);
        assert_eq!(
            state.escalation.as_ref().map(|e| e.recommended),
            Some(cua_protocol::ObservationEscalationTarget::Foreground)
        );
        // Nothing but the observation reached the driver; the orchestrator
        // never acts on a recommendation.
        assert_eq!(kinds(&log), [CuaActionKind::GetWindowState]);
    }

    // ── Parity: one orchestration for both kinds of target ─────────────

    const HEALTHY: &str = r#"{
        "schema_version":"1","platform":"darwin","driver_version":"0.28.2","overall":"ok",
        "checks":[
            {"name":"binary_version","status":"pass","message":"cua-driver 0.28.2"},
            {"name":"platform_supported","status":"pass","message":"macOS 27.0 (arm64)","data":{"architecture":"arm64","os_version":"27.0"}},
            {"name":"session_active","status":"pass","message":"MCP session is active."},
            {"name":"bundle_identity","status":"pass","message":"Bundle is com.trycua.driver.","data":{"bundle_identifier":"com.trycua.driver","executable_path":"/Applications/CuaDriver.app/Contents/MacOS/cua-driver","identity_source":"current_process"}},
            {"name":"tcc_accessibility","status":"pass","message":"Accessibility is granted.","data":{"bundle_identifier":"com.trycua.driver"}},
            {"name":"tcc_screen_recording","status":"pass","message":"Screen Recording is granted.","data":{"bundle_identifier":"com.trycua.driver"}},
            {"name":"ax_capability","status":"pass","message":"AX is trusted and reachable."},
            {"name":"screen_capture_capability","status":"skip","message":"Direct capture was not probed."}
        ]
    }"#;

    fn started(n: u64) -> Value {
        json!({
            "active": true, "capture_scope": "auto", "effective_scope": "window",
            "desktop_capture_authorized": true, "desktop_unlocked": true,
            "revived": false, "session": format!("nolune-run-{n}")
        })
    }

    fn ended(n: u64) -> Value {
        json!({"session": format!("nolune-run-{n}"), "active": false})
    }

    /// A click result the server-local driver would return inside run `n`.
    fn clicked_in_run(n: u64, token: &str) -> Value {
        json!({
            "target": {"pid": 42, "window_id": 99},
            "session": format!("nolune-run-{n}"),
            "address": {"kind": "element_token", "element_token": token},
            "button": "left", "action": "press",
            "outcome": confirmed(&["accessibility_readback"]),
        })
    }

    #[tokio::test]
    async fn the_same_orchestration_serves_server_local_and_desktop_targets() {
        // The desktop: straight through its checked adapter.
        let orchestrator = Orchestrator::new();
        let (laptop, laptop_log) = fake(
            desktop(),
            vec![
                Answer::Payload(state(
                    "s00000001",
                    vec![element(0, "tok/a", "AXButton", "Save")],
                )),
                Answer::Outcome(confirmed(&["accessibility_readback"])),
                Answer::Payload(state(
                    "s00000002",
                    vec![element(0, "tok/b", "AXButton", "Save")],
                )),
            ],
        );

        // The server machine: each operation is one run of the runtime,
        // sessioned and ended with it.
        let registry = MachineRegistry::new();
        let transport = Arc::new(FakeTransport::answering(vec![
            Ok(serde_json::from_str(HEALTHY).unwrap()),
            Ok(started(1)),
            Ok(state(
                "s00000001",
                vec![element(0, "tok/a", "AXButton", "Save")],
            )),
            Ok(ended(1)),
            Ok(started(2)),
            Ok(clicked_in_run(2, "tok/a")),
            Ok(ended(2)),
            Ok(started(3)),
            Ok(state(
                "s00000002",
                vec![element(0, "tok/b", "AXButton", "Save")],
            )),
            Ok(ended(3)),
        ]));
        let runtime = CuaRuntime::new(
            crate::config::CuaConfig::default(),
            registry.cua().clone(),
            std::path::PathBuf::new(),
        );
        runtime
            .attach(
                transport.clone(),
                MachineId::try_from(STUDIO).unwrap(),
                "studio",
                None,
            )
            .await
            .unwrap();
        let adapter = registry
            .cua()
            .select(Some(&MachineId::try_from(STUDIO).unwrap()))
            .await
            .unwrap();
        let studio = Target::server_local(adapter, runtime.clone());
        assert_eq!(studio.descriptor().location, MachineLocation::ServerLocal);
        assert_eq!(laptop.descriptor().location, MachineLocation::Desktop);

        for target in [&laptop, &studio] {
            let name = target.machine_id().as_str().to_owned();
            // Nothing observed yet: refused before anything is sent.
            let error = orchestrator
                .act(target, click_token("tok/a"), None)
                .await
                .unwrap_err();
            assert_eq!(
                error,
                Failure::Ledger(LedgerRefusal::NoSnapshot { target: window() }),
                "{name}"
            );

            let state = orchestrator
                .window_state(target, observe_args(true))
                .await
                .unwrap();
            assert_eq!(
                state.snapshot_id.as_ref().map(SnapshotId::as_str),
                Some("s00000001"),
                "{name}"
            );

            let report = orchestrator
                .act(target, click_token("tok/a"), None)
                .await
                .unwrap();
            assert_eq!(report.outcome.effect, ActionEffect::Confirmed, "{name}");

            // A newer snapshot makes the first token stale on both.
            observe(&orchestrator, target, true).await;
            let error = orchestrator
                .act(target, click_token("tok/a"), None)
                .await
                .unwrap_err();
            assert!(
                matches!(error, Failure::Ledger(LedgerRefusal::StaleSnapshot { .. })),
                "{name}: {error:?}"
            );
        }

        assert_eq!(
            kinds(&laptop_log),
            [
                CuaActionKind::GetWindowState,
                CuaActionKind::Click,
                CuaActionKind::GetWindowState
            ]
        );
        assert_eq!(
            transport.tools_called(),
            vec![
                "health_report",
                "start_session",
                "get_window_state",
                "end_session",
                "start_session",
                "click",
                "end_session",
                "start_session",
                "get_window_state",
                "end_session",
            ],
            "on the server machine every operation is one sessioned run"
        );
        let calls = transport.calls();
        assert_eq!(calls[5].1["session"], "nolune-run-2");
        assert_eq!(calls[5].1["element_token"], "tok/a");
        assert_eq!(calls[5].1["delivery_mode"], "background");
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn a_run_that_cannot_start_is_a_run_failure() {
        let orchestrator = Orchestrator::new();
        let registry = MachineRegistry::new();
        let runtime = CuaRuntime::new(
            crate::config::CuaConfig::default(),
            registry.cua().clone(),
            std::path::PathBuf::new(),
        );
        let (fake_target, _) = fake(descriptor(STUDIO, MachineLocation::ServerLocal), vec![]);
        let target = Target::server_local(fake_target.adapter.clone(), runtime);
        let error = orchestrator
            .discover(&target, CuaAction::ListApps(EmptyArgs {}))
            .await
            .unwrap_err();
        assert!(matches!(error, Failure::Exec(_)), "{error:?}");
        assert!(error.to_string().starts_with("run_failed: "), "{error}");
    }

    #[test]
    fn move_cursor_and_double_click_are_addressed_like_clicks() {
        let ledger = SnapshotLedger::default();
        let machine = MachineId::try_from(LAPTOP).unwrap();
        let double = CuaAction::DoubleClick(AddressedActionArgs {
            target: window(),
            session: None,
            delivery_mode: DeliveryMode::Background,
            address: ElementAddress::ElementToken {
                element_token: ElementToken::try_from("tok/a").unwrap(),
            },
        });
        assert_eq!(
            ledger.check(&machine, &double),
            Err(LedgerRefusal::NoSnapshot { target: window() })
        );
        let drag = CuaAction::Drag(cua_protocol::DragArgs {
            target: window(),
            session: None,
            delivery_mode: DeliveryMode::Background,
            from: WindowPoint { x: 1.0, y: 1.0 },
            to: WindowPoint { x: 5.0, y: 5.0 },
            duration_ms: 100,
            steps: 5,
            button: MouseButton::Left,
            modifiers: vec![],
        });
        assert_eq!(
            ledger.check(&machine, &drag),
            Err(LedgerRefusal::NoSnapshot { target: window() }),
            "a drag is a pixel action and needs the screenshot it is read from"
        );
    }
}
