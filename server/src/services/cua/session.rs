//! Per-run driver sessions: every action a run executes carries the run's
//! session label, so the driver attributes it to one bounded session that the
//! runtime opened before the first action and closes when the run ends.
//! `OpenSessions` is the set of labels a target holds open for the server,
//! kept the same way by the server-local runtime (#192) and a desktop's
//! typed link (#17).

use std::{collections::BTreeSet, sync::Mutex};

use cua_protocol::{CuaAction, SessionLabel};

/// The labels of the sessions one target holds open for the server. The
/// runtime opens and ends its per-run sessions through it; a desktop link
/// records the ones the desktop confirmed over its socket. Either way, what
/// a driver exit or a closed socket loses is the set taken at that moment.
#[derive(Default)]
pub struct OpenSessions(Mutex<BTreeSet<SessionLabel>>);

impl OpenSessions {
    pub fn new() -> Self {
        Self::default()
    }

    /// Remember `label` as open; `false` when it already was.
    pub fn insert(&self, label: SessionLabel) -> bool {
        self.lock().insert(label)
    }

    /// Forget `label`; `false` when it was not open.
    pub fn remove(&self, label: &SessionLabel) -> bool {
        self.lock().remove(label)
    }

    /// Every open label in label order, leaving none: what is lost when
    /// the target goes away, or what a shutdown has to end.
    pub fn take(&self) -> Vec<SessionLabel> {
        std::mem::take(&mut *self.lock()).into_iter().collect()
    }

    /// The open labels, in label order.
    #[cfg(test)]
    pub fn labels(&self) -> Vec<SessionLabel> {
        self.lock().iter().cloned().collect()
    }

    /// A poisoned lock only means a task panicked mid-update; the set
    /// itself is still consistent.
    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeSet<SessionLabel>> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// The action as the run sends it: labelled with the run's session wherever
/// the protocol carries one. Session-management and discovery calls have no
/// label and pass through unchanged.
pub fn with_session(mut action: CuaAction, session: &SessionLabel) -> CuaAction {
    let label = match &mut action {
        CuaAction::GetWindowState(args) => &mut args.session,
        CuaAction::SetWindowFrame(args) => &mut args.session,
        CuaAction::Click(args) => &mut args.session,
        CuaAction::DoubleClick(args) => &mut args.session,
        CuaAction::RightClick(args) => &mut args.session,
        CuaAction::MoveCursor(args) => &mut args.session,
        CuaAction::Drag(args) => &mut args.session,
        CuaAction::Scroll(args) => &mut args.session,
        CuaAction::TypeText(args) => &mut args.session,
        CuaAction::PressKey(args) => &mut args.session,
        CuaAction::Hotkey(args) => &mut args.session,
        CuaAction::SetValue(args) => &mut args.session,
        CuaAction::InvokeMenu(args) => &mut args.session,
        CuaAction::VerifyState(args) => &mut args.session,
        CuaAction::ListApps(_)
        | CuaAction::LaunchApp(_)
        | CuaAction::ListWindows(_)
        | CuaAction::StartSession(_)
        | CuaAction::GetSession(_)
        | CuaAction::ListSessions(_)
        | CuaAction::EndSession(_)
        | CuaAction::HealthReport(_) => return action,
    };
    *label = Some(session.clone());
    action
}

/// Whether a run may execute this action itself. Starting and ending sessions
/// is the runtime's job; a run that did it would leave the runtime's records
/// wrong and the cleanup non-deterministic.
pub fn is_run_managed(action: &CuaAction) -> bool {
    matches!(
        action,
        CuaAction::StartSession(_) | CuaAction::EndSession(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use cua_protocol::*;
    use std::num::NonZeroU32;

    fn target() -> WindowTarget {
        WindowTarget {
            pid: 42,
            window_id: 99,
        }
    }

    fn point() -> ElementAddress {
        ElementAddress::Point(WindowPoint { x: 1.0, y: 2.0 })
    }

    fn text(value: &str) -> BoundedText {
        BoundedText::try_from(value).unwrap()
    }

    /// One valid action per kind, none labelled, in `CuaActionKind::ALL` order.
    fn sample(kind: CuaActionKind) -> CuaAction {
        match kind {
            CuaActionKind::ListApps => CuaAction::ListApps(EmptyArgs {}),
            CuaActionKind::LaunchApp => CuaAction::LaunchApp(LaunchAppArgs {
                bundle_id: Some(AppBundleId::try_from("com.apple.Safari").unwrap()),
                name: None,
                creates_new_application_instance: false,
            }),
            CuaActionKind::ListWindows => CuaAction::ListWindows(ListWindowsArgs {
                pid: NonZeroU32::new(42),
                on_screen_only: true,
            }),
            CuaActionKind::GetWindowState => CuaAction::GetWindowState(GetWindowStateArgs {
                target: target(),
                session: None,
                include_accessibility_tree: true,
                include_screenshot: false,
                max_elements: None,
                max_depth: None,
                max_dimension: None,
                query: None,
            }),
            CuaActionKind::SetWindowFrame => CuaAction::SetWindowFrame(SetWindowFrameArgs {
                target: target(),
                session: None,
                frame: Rect::new(10.0, 20.0, 300.0, 200.0).unwrap(),
            }),
            CuaActionKind::Click => CuaAction::Click(ClickArgs {
                target: target(),
                session: None,
                delivery_mode: DeliveryMode::Background,
                address: point(),
                button: MouseButton::Left,
                action: ClickAction::Press,
                modifiers: vec![],
                count: None,
            }),
            CuaActionKind::DoubleClick => CuaAction::DoubleClick(AddressedActionArgs {
                target: target(),
                session: None,
                delivery_mode: DeliveryMode::Background,
                address: point(),
            }),
            CuaActionKind::RightClick => CuaAction::RightClick(RightClickArgs {
                target: target(),
                session: None,
                delivery_mode: DeliveryMode::Background,
                address: point(),
                modifiers: vec![],
            }),
            CuaActionKind::MoveCursor => CuaAction::MoveCursor(MoveCursorArgs {
                target: target(),
                session: None,
                delivery_mode: DeliveryMode::Background,
                point: WindowPoint { x: 3.0, y: 4.0 },
            }),
            CuaActionKind::Drag => CuaAction::Drag(DragArgs {
                target: target(),
                session: None,
                delivery_mode: DeliveryMode::Background,
                from: WindowPoint { x: 1.0, y: 1.0 },
                to: WindowPoint { x: 5.0, y: 5.0 },
                duration_ms: 100,
                steps: 4,
                button: MouseButton::Left,
                modifiers: vec![],
            }),
            CuaActionKind::Scroll => CuaAction::Scroll(ScrollArgs {
                target: target(),
                session: None,
                delivery_mode: DeliveryMode::Background,
                address: point(),
                direction: ScrollDirection::Down,
                by: ScrollGranularity::Line,
                amount: 3,
            }),
            CuaActionKind::TypeText => CuaAction::TypeText(TypeTextArgs {
                target: target(),
                session: None,
                delivery_mode: DeliveryMode::Background,
                address: point(),
                text: text("hello"),
                delay_ms: 0,
            }),
            CuaActionKind::PressKey => CuaAction::PressKey(PressKeyArgs {
                target: target(),
                session: None,
                delivery_mode: DeliveryMode::Background,
                address: point(),
                key: KeyName::try_from("return").unwrap(),
                modifiers: vec![],
            }),
            CuaActionKind::Hotkey => CuaAction::Hotkey(HotkeyArgs {
                target: target(),
                session: None,
                delivery_mode: DeliveryMode::Background,
                address: point(),
                keys: vec![
                    HotkeyKey::try_from("cmd").unwrap(),
                    HotkeyKey::try_from("s").unwrap(),
                ],
            }),
            CuaActionKind::SetValue => CuaAction::SetValue(SetValueArgs {
                target: target(),
                session: None,
                element: ElementRef::ElementToken {
                    element_token: ElementToken::try_from("tok/1").unwrap(),
                },
                value: EmptyValueText::try_from("42").unwrap(),
            }),
            CuaActionKind::InvokeMenu => CuaAction::InvokeMenu(InvokeMenuArgs {
                target: target(),
                session: None,
                path: vec![text("File"), text("Save")],
            }),
            CuaActionKind::VerifyState => CuaAction::VerifyState(VerifyStateArgs {
                target: target(),
                session: None,
                expect: vec![VerifyPredicate::WindowExists(true)],
                include_screenshot: false,
                stable_samples: 1,
                timeout_ms: 500,
            }),
            CuaActionKind::StartSession => CuaAction::StartSession(StartSessionArgs {
                session: Some(SessionLabel::try_from("other").unwrap()),
            }),
            CuaActionKind::GetSession => CuaAction::GetSession(SessionRefArgs {
                session: Some(SessionLabel::try_from("other").unwrap()),
            }),
            CuaActionKind::ListSessions => CuaAction::ListSessions(ListSessionsArgs {
                cursor: None,
                limit: None,
            }),
            CuaActionKind::EndSession => CuaAction::EndSession(SessionRefArgs {
                session: Some(SessionLabel::try_from("other").unwrap()),
            }),
            CuaActionKind::HealthReport => CuaAction::HealthReport(HealthReportArgs {
                include: vec![],
                skip: vec![],
            }),
        }
    }

    /// The kinds whose arguments carry a `session` label.
    const LABELLED: [CuaActionKind; 14] = [
        CuaActionKind::GetWindowState,
        CuaActionKind::SetWindowFrame,
        CuaActionKind::Click,
        CuaActionKind::DoubleClick,
        CuaActionKind::RightClick,
        CuaActionKind::MoveCursor,
        CuaActionKind::Drag,
        CuaActionKind::Scroll,
        CuaActionKind::TypeText,
        CuaActionKind::PressKey,
        CuaActionKind::Hotkey,
        CuaActionKind::SetValue,
        CuaActionKind::InvokeMenu,
        CuaActionKind::VerifyState,
    ];

    #[test]
    fn every_action_that_carries_a_session_gets_the_run_label() {
        let label = SessionLabel::try_from("nolune-run-7").unwrap();
        for kind in CuaActionKind::ALL {
            let labelled = with_session(sample(kind), &label);
            assert_eq!(labelled.kind(), kind, "the kind never changes");
            labelled.validate().unwrap();
            let call = cua_protocol::driver_mcp::tool_call(&labelled).unwrap();
            if LABELLED.contains(&kind) {
                assert_eq!(
                    call.arguments.get("session"),
                    Some(&serde_json::json!("nolune-run-7")),
                    "{kind:?} carries the run's session on the wire"
                );
            } else {
                // Session-management calls keep whatever they addressed; the
                // rest have no label at all.
                let original = cua_protocol::driver_mcp::tool_call(&sample(kind)).unwrap();
                assert_eq!(call, original, "{kind:?} is passed through unchanged");
            }
        }
    }

    /// The one open-session set the runtime and a desktop link share: a
    /// label is open once, forgotten once, and `take` empties it in order.
    #[test]
    fn open_sessions_are_counted_once_and_taken_in_label_order() {
        let label = |name: &str| SessionLabel::try_from(name).unwrap();
        let open = OpenSessions::new();
        assert!(open.labels().is_empty());
        assert!(open.insert(label("nolune-run-2")));
        assert!(open.insert(label("nolune-run-1")));
        assert!(!open.insert(label("nolune-run-1")), "already open");
        assert_eq!(
            open.labels(),
            vec![label("nolune-run-1"), label("nolune-run-2")]
        );
        assert!(open.remove(&label("nolune-run-2")));
        assert!(!open.remove(&label("nolune-run-2")), "already ended");
        assert!(open.insert(label("mine")));
        assert_eq!(
            open.take(),
            vec![label("mine"), label("nolune-run-1")],
            "what a driver exit or a closed socket loses"
        );
        assert!(open.labels().is_empty());
        assert!(open.take().is_empty(), "taking again finds nothing");
    }

    #[test]
    fn only_session_start_and_end_are_managed_by_the_run() {
        for kind in CuaActionKind::ALL {
            let managed = is_run_managed(&sample(kind));
            assert_eq!(
                managed,
                matches!(
                    kind,
                    CuaActionKind::StartSession | CuaActionKind::EndSession
                ),
                "{kind:?}"
            );
        }
    }
}
