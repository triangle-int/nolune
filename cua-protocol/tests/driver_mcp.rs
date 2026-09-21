//! The Cua Driver MCP wire mapping: every typed action encodes to the tool
//! call the driver's `list-tools` schemas accept, every driver payload decodes
//! into a correlated envelope, and a health report becomes the descriptor a
//! target advertises.

use cua_protocol::driver_mcp::*;
use cua_protocol::*;
use serde_json::{Map, Value, json};
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

fn token() -> ElementAddress {
    ElementAddress::ElementToken {
        element_token: ElementToken::try_from("tok/1").unwrap(),
    }
}

fn indexed() -> ElementAddress {
    ElementAddress::ElementIndex {
        element_index: 7,
        snapshot_id: SnapshotId::try_from("s0123abcd").unwrap(),
    }
}

fn text(value: &str) -> BoundedText {
    BoundedText::try_from(value).unwrap()
}

fn request(action: CuaAction) -> CuaRequestEnvelope {
    CuaRequestEnvelope {
        version: ProtocolVersion::V1,
        request_id: RequestId::try_from("req-1").unwrap(),
        machine_id: MachineId::try_from("server-local:studio").unwrap(),
        action,
    }
}

fn args(action: CuaAction) -> Map<String, Value> {
    tool_call(&action).unwrap().arguments
}

fn click(address: ElementAddress) -> CuaAction {
    CuaAction::Click(ClickArgs {
        target: target(),
        session: None,
        delivery_mode: DeliveryMode::Background,
        address,
        button: MouseButton::Left,
        action: ClickAction::Press,
        modifiers: vec![],
        count: None,
    })
}

/// One valid action per kind, in `CuaActionKind::ALL` order.
fn sample(kind: CuaActionKind) -> CuaAction {
    match kind {
        CuaActionKind::ListApps => CuaAction::ListApps(EmptyArgs {}),
        CuaActionKind::LaunchApp => CuaAction::LaunchApp(LaunchAppArgs {
            bundle_id: Some(AppBundleId::try_from("com.apple.Safari").unwrap()),
            name: None,
            creates_new_application_instance: false,
        }),
        CuaActionKind::ListWindows => CuaAction::ListWindows(ListWindowsArgs {
            pid: None,
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
        CuaActionKind::Click => click(point()),
        CuaActionKind::DoubleClick => CuaAction::DoubleClick(AddressedActionArgs {
            target: target(),
            session: None,
            delivery_mode: DeliveryMode::Background,
            address: token(),
        }),
        CuaActionKind::RightClick => CuaAction::RightClick(RightClickArgs {
            target: target(),
            session: None,
            delivery_mode: DeliveryMode::Background,
            address: indexed(),
            modifiers: vec![],
        }),
        CuaActionKind::MoveCursor => CuaAction::MoveCursor(MoveCursorArgs {
            target: target(),
            session: None,
            delivery_mode: DeliveryMode::Background,
            point: WindowPoint { x: 5.0, y: 6.0 },
        }),
        CuaActionKind::Drag => CuaAction::Drag(DragArgs {
            target: target(),
            session: None,
            delivery_mode: DeliveryMode::Background,
            from: WindowPoint { x: 1.0, y: 2.0 },
            to: WindowPoint { x: 3.0, y: 4.0 },
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
            by: ScrollGranularity::Page,
            amount: 3,
        }),
        CuaActionKind::TypeText => CuaAction::TypeText(TypeTextArgs {
            target: target(),
            session: None,
            delivery_mode: DeliveryMode::Background,
            address: token(),
            text: text("hello"),
            delay_ms: 3,
        }),
        CuaActionKind::PressKey => CuaAction::PressKey(PressKeyArgs {
            target: target(),
            session: None,
            delivery_mode: DeliveryMode::Background,
            address: token(),
            key: KeyName::try_from("return").unwrap(),
            modifiers: vec![Modifier::Command, Modifier::Shift],
        }),
        CuaActionKind::Hotkey => CuaAction::Hotkey(HotkeyArgs {
            target: target(),
            session: None,
            delivery_mode: DeliveryMode::Background,
            address: token(),
            keys: vec![
                HotkeyKey::try_from("cmd").unwrap(),
                HotkeyKey::try_from("c").unwrap(),
            ],
        }),
        CuaActionKind::SetValue => CuaAction::SetValue(SetValueArgs {
            target: target(),
            session: None,
            element: ElementRef::ElementIndex {
                element_index: 3,
                snapshot_id: SnapshotId::try_from("s0123abcd").unwrap(),
            },
            value: EmptyValueText::try_from("").unwrap(),
        }),
        CuaActionKind::InvokeMenu => CuaAction::InvokeMenu(InvokeMenuArgs {
            target: target(),
            session: None,
            path: vec![text("File"), text("New Window")],
        }),
        CuaActionKind::VerifyState => CuaAction::VerifyState(VerifyStateArgs {
            target: target(),
            session: None,
            expect: vec![VerifyPredicate::WindowExists(true)],
            include_screenshot: false,
            stable_samples: 1,
            timeout_ms: 0,
        }),
        CuaActionKind::StartSession => CuaAction::StartSession(StartSessionArgs { session: None }),
        CuaActionKind::GetSession => CuaAction::GetSession(SessionRefArgs { session: None }),
        CuaActionKind::ListSessions => CuaAction::ListSessions(ListSessionsArgs {
            cursor: None,
            limit: None,
        }),
        CuaActionKind::EndSession => CuaAction::EndSession(SessionRefArgs { session: None }),
        CuaActionKind::HealthReport => CuaAction::HealthReport(HealthReportArgs {
            include: vec![],
            skip: vec![],
        }),
    }
}

// The read-only report the installed 0.28.2 driver returns on a granted Mac;
// the same shape `semantic_tightening.rs` pins.
const HEALTHY_DARWIN: &str = r#"{
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

const ACCESSIBILITY_DENIED_DARWIN: &str = r#"{
    "schema_version":"1","platform":"darwin","driver_version":"0.28.2","overall":"degraded",
    "checks":[
        {"name":"binary_version","status":"pass","message":"cua-driver 0.28.2"},
        {"name":"platform_supported","status":"pass","message":"macOS 27.0 (arm64)","data":{"architecture":"arm64","os_version":"27.0"}},
        {"name":"session_active","status":"pass","message":"MCP session is active."},
        {"name":"bundle_identity","status":"pass","message":"Bundle is com.trycua.driver.","data":{"bundle_identifier":"com.trycua.driver","executable_path":"/Applications/CuaDriver.app/Contents/MacOS/cua-driver","identity_source":"current_process"}},
        {"name":"tcc_accessibility","status":"fail","message":"Accessibility is not granted.","hint":"Run cua-driver permissions grant.","data":{"bundle_identifier":"com.trycua.driver"}},
        {"name":"tcc_screen_recording","status":"pass","message":"Screen Recording is granted.","data":{"bundle_identifier":"com.trycua.driver"}},
        {"name":"ax_capability","status":"fail","message":"AX is not trusted.","hint":"Grant Accessibility."},
        {"name":"screen_capture_capability","status":"skip","message":"Direct capture was not probed."}
    ]
}"#;

const SESSION_FAILED_DARWIN: &str = r#"{
    "schema_version":"1","platform":"darwin","driver_version":"0.28.2","overall":"failed",
    "checks":[
        {"name":"binary_version","status":"pass","message":"cua-driver 0.28.2"},
        {"name":"platform_supported","status":"pass","message":"macOS 27.0 (arm64)","data":{"architecture":"arm64","os_version":"27.0"}},
        {"name":"session_active","status":"fail","message":"No MCP session.","hint":"Start the driver."},
        {"name":"bundle_identity","status":"pass","message":"Bundle is com.trycua.driver.","data":{"bundle_identifier":"com.trycua.driver","executable_path":"/Applications/CuaDriver.app/Contents/MacOS/cua-driver","identity_source":"current_process"}},
        {"name":"tcc_accessibility","status":"pass","message":"Accessibility is granted.","data":{"bundle_identifier":"com.trycua.driver"}},
        {"name":"tcc_screen_recording","status":"pass","message":"Screen Recording is granted.","data":{"bundle_identifier":"com.trycua.driver"}},
        {"name":"ax_capability","status":"pass","message":"AX is trusted and reachable."},
        {"name":"screen_capture_capability","status":"skip","message":"Direct capture was not probed."}
    ]
}"#;

const CAPTURE_DENIED_LINUX: &str = r#"{
    "schema_version":"1","platform":"linux","driver_version":"0.28.2","overall":"degraded",
    "checks":[
        {"name":"binary_version","status":"pass","message":"cua-driver 0.28.2"},
        {"name":"platform_supported","status":"pass","message":"Linux 6.8 (x86_64)","data":{"architecture":"x86_64","os_version":"6.8"}},
        {"name":"session_active","status":"pass","message":"MCP session is active."},
        {"name":"ax_capability","status":"pass","message":"AT-SPI is reachable."},
        {"name":"screen_capture_capability","status":"fail","message":"No X11 display.","hint":"Set DISPLAY."}
    ]
}"#;

const UNPROBED_WINDOWS: &str = r#"{
    "schema_version":"1","platform":"win32","driver_version":"0.28.2","overall":"ok",
    "checks":[
        {"name":"binary_version","status":"pass","message":"cua-driver 0.28.2"},
        {"name":"platform_supported","status":"pass","message":"Windows 11 (x86_64)","data":{"architecture":"x86_64","os_version":"11"}},
        {"name":"session_active","status":"pass","message":"MCP session is active."},
        {"name":"ax_capability","status":"skip","message":"UIA was not probed."},
        {"name":"screen_capture_capability","status":"skip","message":"DXGI was not probed."}
    ]
}"#;

fn report(json: &str) -> HealthReportResult {
    serde_json::from_str(json).unwrap()
}

fn descriptor(json: &str) -> MachineDescriptor {
    descriptor_from_health(
        MachineId::try_from("server-local:studio").unwrap(),
        MachineLocation::ServerLocal,
        &report(json),
    )
}

#[test]
fn every_action_maps_to_the_driver_tool_of_the_same_name() {
    for kind in CuaActionKind::ALL {
        let call = tool_call(&sample(kind)).unwrap();
        let expected = serde_json::to_value(kind).unwrap();
        assert_eq!(
            Value::from(call.name),
            expected,
            "{kind:?} must call the driver tool of the same name"
        );
    }
    assert_eq!(
        tool_call(&sample(CuaActionKind::HealthReport))
            .unwrap()
            .name,
        HEALTH_REPORT_TOOL
    );
}

#[test]
fn invalid_actions_are_refused_before_encoding() {
    let modified_click = CuaAction::Click(ClickArgs {
        target: target(),
        session: None,
        delivery_mode: DeliveryMode::Background,
        address: point(),
        button: MouseButton::Left,
        action: ClickAction::Press,
        modifiers: vec![Modifier::Command],
        count: None,
    });
    assert!(modified_click.validate().is_err(), "premise");
    assert!(tool_call(&modified_click).is_err());
}

#[test]
fn window_tools_flatten_the_target_and_the_address() {
    assert_eq!(
        Value::Object(args(click(point()))),
        json!({
            "pid": 42, "window_id": 99, "delivery_mode": "background",
            "x": 1.0, "y": 2.0, "button": "left", "action": "press"
        })
    );
    assert_eq!(
        Value::Object(args(click(token()))),
        json!({
            "pid": 42, "window_id": 99, "delivery_mode": "background",
            "element_token": "tok/1", "button": "left", "action": "press"
        })
    );
    assert_eq!(
        Value::Object(args(click(indexed()))),
        json!({
            "pid": 42, "window_id": 99, "delivery_mode": "background",
            "element_index": 7, "snapshot_id": "s0123abcd", "button": "left", "action": "press"
        })
    );

    let mut counted = ClickArgs {
        target: target(),
        session: Some(SessionLabel::try_from("run-1").unwrap()),
        delivery_mode: DeliveryMode::Background,
        address: point(),
        button: MouseButton::Right,
        action: ClickAction::Press,
        modifiers: vec![],
        count: Some(2),
    };
    let encoded = args(CuaAction::Click(counted.clone()));
    assert_eq!(encoded["session"], "run-1");
    assert_eq!(encoded["count"], 2);
    assert_eq!(encoded["button"], "right");
    counted.count = None;
    counted.session = None;
    let encoded = args(CuaAction::Click(counted));
    assert!(!encoded.contains_key("count"), "absent options stay absent");
    assert!(!encoded.contains_key("session"));

    let double = args(sample(CuaActionKind::DoubleClick));
    assert_eq!(
        Value::Object(double),
        json!({"pid": 42, "window_id": 99, "delivery_mode": "background", "element_token": "tok/1"})
    );
    let right = args(sample(CuaActionKind::RightClick));
    assert_eq!(
        Value::Object(right),
        json!({
            "pid": 42, "window_id": 99, "delivery_mode": "background",
            "element_index": 7, "snapshot_id": "s0123abcd"
        })
    );
}

#[test]
fn move_cursor_addresses_the_window_through_target() {
    // `move_cursor` is the one window tool whose schema has no flat pid/window_id.
    assert_eq!(
        Value::Object(args(sample(CuaActionKind::MoveCursor))),
        json!({
            "target": {"kind": "window", "pid": 42, "window_id": 99},
            "x": 5.0, "y": 6.0
        })
    );
}

#[test]
fn pointer_and_keyboard_tools_use_the_driver_field_names() {
    assert_eq!(
        Value::Object(args(sample(CuaActionKind::Drag))),
        json!({
            "pid": 42, "window_id": 99, "delivery_mode": "background",
            "from_x": 1.0, "from_y": 2.0, "to_x": 3.0, "to_y": 4.0,
            "duration_ms": 100, "steps": 4, "button": "left"
        })
    );
    assert_eq!(
        Value::Object(args(sample(CuaActionKind::Scroll))),
        json!({
            "pid": 42, "window_id": 99, "delivery_mode": "background",
            "x": 1.0, "y": 2.0, "direction": "down", "by": "page", "amount": 3
        })
    );
    assert_eq!(
        Value::Object(args(sample(CuaActionKind::TypeText))),
        json!({
            "pid": 42, "window_id": 99, "delivery_mode": "background",
            "element_token": "tok/1", "text": "hello", "delay_ms": 3
        })
    );
    // press_key spells the modifier list `modifiers`; click/drag spell it `modifier`.
    assert_eq!(
        Value::Object(args(sample(CuaActionKind::PressKey))),
        json!({
            "pid": 42, "window_id": 99, "delivery_mode": "background",
            "element_token": "tok/1", "key": "return", "modifiers": ["cmd", "shift"]
        })
    );
    let plain_key = CuaAction::PressKey(PressKeyArgs {
        target: target(),
        session: None,
        delivery_mode: DeliveryMode::Background,
        address: token(),
        key: KeyName::try_from("a").unwrap(),
        modifiers: vec![],
    });
    assert!(
        !args(plain_key).contains_key("modifiers"),
        "an empty modifier list is omitted"
    );
    assert_eq!(
        Value::Object(args(sample(CuaActionKind::Hotkey))),
        json!({
            "pid": 42, "window_id": 99, "delivery_mode": "background",
            "element_token": "tok/1", "keys": ["cmd", "c"]
        })
    );
}

#[test]
fn value_menu_frame_launch_and_window_listing_map_their_fields() {
    assert_eq!(
        Value::Object(args(sample(CuaActionKind::SetValue))),
        json!({"pid": 42, "window_id": 99, "element_index": 3, "snapshot_id": "s0123abcd", "value": ""})
    );
    let by_token = CuaAction::SetValue(SetValueArgs {
        target: target(),
        session: None,
        element: ElementRef::ElementToken {
            element_token: ElementToken::try_from("tok/2").unwrap(),
        },
        value: EmptyValueText::try_from("42").unwrap(),
    });
    assert_eq!(
        Value::Object(args(by_token)),
        json!({"pid": 42, "window_id": 99, "element_token": "tok/2", "value": "42"})
    );
    assert_eq!(
        Value::Object(args(sample(CuaActionKind::InvokeMenu))),
        json!({"pid": 42, "window_id": 99, "path": ["File", "New Window"]})
    );
    assert_eq!(
        Value::Object(args(sample(CuaActionKind::SetWindowFrame))),
        json!({"pid": 42, "window_id": 99, "x": 10.0, "y": 20.0, "width": 300.0, "height": 200.0})
    );
    assert_eq!(
        Value::Object(args(sample(CuaActionKind::LaunchApp))),
        json!({"bundle_id": "com.apple.Safari", "creates_new_application_instance": false})
    );
    let by_name = CuaAction::LaunchApp(LaunchAppArgs {
        bundle_id: None,
        name: Some(AppDisplayName::try_from("Safari").unwrap()),
        creates_new_application_instance: true,
    });
    assert_eq!(
        Value::Object(args(by_name)),
        json!({"name": "Safari", "creates_new_application_instance": true})
    );
    assert_eq!(
        Value::Object(args(sample(CuaActionKind::ListApps))),
        json!({})
    );
    assert_eq!(
        Value::Object(args(sample(CuaActionKind::ListWindows))),
        json!({"on_screen_only": true})
    );
    let filtered = CuaAction::ListWindows(ListWindowsArgs {
        pid: NonZeroU32::new(42),
        on_screen_only: false,
    });
    assert_eq!(
        Value::Object(args(filtered)),
        json!({"pid": 42, "on_screen_only": false})
    );
}

#[test]
fn window_state_forwards_only_the_options_that_were_set() {
    assert_eq!(
        Value::Object(args(sample(CuaActionKind::GetWindowState))),
        json!({"pid": 42, "window_id": 99, "include_accessibility_tree": true, "include_screenshot": false})
    );
    let bounded = CuaAction::GetWindowState(GetWindowStateArgs {
        target: target(),
        session: Some(SessionLabel::try_from("run-1").unwrap()),
        include_accessibility_tree: true,
        include_screenshot: true,
        max_elements: Some(200),
        max_depth: Some(12),
        max_dimension: Some(1024),
        query: Some(text("Save")),
    });
    assert_eq!(
        Value::Object(args(bounded)),
        json!({
            "pid": 42, "window_id": 99, "session": "run-1",
            "include_accessibility_tree": true, "include_screenshot": true,
            "max_elements": 200, "max_depth": 12, "max_dimension": 1024, "query": "Save"
        })
    );
}

#[test]
fn verify_state_predicates_take_the_driver_shape() {
    let action = CuaAction::VerifyState(VerifyStateArgs {
        target: target(),
        session: None,
        expect: vec![
            VerifyPredicate::WindowExists(true),
            VerifyPredicate::WindowBounds {
                bounds: Rect::new(0.0, 0.0, 800.0, 600.0).unwrap(),
                tolerance_px: 2.5,
            },
            VerifyPredicate::Element(ElementPredicate {
                selector: ElementSelector {
                    role: Some(text("AXButton")),
                    label_contains: Some(text("Save")),
                },
                condition: ElementCondition::Exists,
            }),
            VerifyPredicate::Element(ElementPredicate {
                selector: ElementSelector {
                    role: Some(text("AXButton")),
                    label_contains: None,
                },
                condition: ElementCondition::Enabled(false),
            }),
            VerifyPredicate::Element(ElementPredicate {
                selector: ElementSelector {
                    role: None,
                    label_contains: Some(text("Tab")),
                },
                condition: ElementCondition::Selected(true),
            }),
            VerifyPredicate::Element(ElementPredicate {
                selector: ElementSelector {
                    role: Some(text("AXTextField")),
                    label_contains: None,
                },
                condition: ElementCondition::ValueEquals(EmptyValueText::try_from("").unwrap()),
            }),
        ],
        include_screenshot: true,
        stable_samples: 2,
        timeout_ms: 1500,
    });
    assert_eq!(
        Value::Object(args(action)),
        json!({
            "pid": 42, "window_id": 99,
            "expect": [
                {"window": {"exists": true}},
                {"window": {"bounds": {"x": 0.0, "y": 0.0, "width": 800.0, "height": 600.0, "tolerance_px": 2.5}}},
                {"element": {"selector": {"role": "AXButton", "label_contains": "Save"}, "exists": true}},
                {"element": {"selector": {"role": "AXButton"}, "enabled": false}},
                {"element": {"selector": {"label_contains": "Tab"}, "selected": true}},
                {"element": {"selector": {"role": "AXTextField"}, "value_equals": ""}}
            ],
            "include_screenshot": true, "stable_samples": 2, "timeout_ms": 1500
        })
    );
}

#[test]
fn session_and_health_calls_pass_optional_fields_only_when_set() {
    for kind in [
        CuaActionKind::StartSession,
        CuaActionKind::GetSession,
        CuaActionKind::EndSession,
        CuaActionKind::ListSessions,
        CuaActionKind::HealthReport,
    ] {
        assert_eq!(
            Value::Object(args(sample(kind))),
            json!({}),
            "{kind:?} sends nothing when nothing is set"
        );
    }
    let label = Some(SessionLabel::try_from("run-1").unwrap());
    assert_eq!(
        Value::Object(args(CuaAction::StartSession(StartSessionArgs {
            session: label.clone()
        }))),
        json!({"session": "run-1"})
    );
    assert_eq!(
        Value::Object(args(CuaAction::EndSession(SessionRefArgs {
            session: label
        }))),
        json!({"session": "run-1"})
    );
    assert_eq!(
        Value::Object(args(CuaAction::ListSessions(ListSessionsArgs {
            cursor: Some(PageCursor::try_from("next").unwrap()),
            limit: Some(10),
        }))),
        json!({"cursor": "next", "limit": 10})
    );
    // An empty `include` would read as "run nothing", so only set lists are sent.
    assert_eq!(
        Value::Object(args(CuaAction::HealthReport(HealthReportArgs {
            include: vec![],
            skip: vec![HealthCheckName::try_from("bundle_identity").unwrap()],
        }))),
        json!({"skip": ["bundle_identity"]})
    );
}

#[test]
fn driver_payloads_decode_into_correlated_envelopes() {
    let health = request(sample(CuaActionKind::HealthReport));
    let envelope = decode_response(&health, serde_json::from_str(HEALTHY_DARWIN).unwrap()).unwrap();
    assert_eq!(envelope.request_id, health.request_id);
    assert_eq!(envelope.machine_id, health.machine_id);
    assert_eq!(envelope.action, CuaActionKind::HealthReport);
    match envelope.response {
        CuaResponse::Success { result } => match *result {
            CuaActionResult::HealthReport(report) => {
                assert_eq!(report.overall, HealthOverall::Ok);
                assert_eq!(report.checks.len(), 8);
            }
            other => panic!("unexpected result {other:?}"),
        },
        CuaResponse::Error { error } => panic!("unexpected error {error:?}"),
    }

    // The live driver spells absent optionals as null.
    let sessions = request(sample(CuaActionKind::ListSessions));
    let payload = json!({
        "next_cursor": null,
        "sessions": [{
            "client_kind": "mcp", "cursor_visible": false, "expires_in_seconds": 299,
            "idle_seconds": 0, "implicit": true, "recording_active": false,
            "session": null, "state": "active", "transport": "mcp_stdio"
        }]
    });
    let envelope = decode_response(&sessions, payload).unwrap();
    assert!(matches!(envelope.response, CuaResponse::Success { .. }));

    // The degraded window-state fixture the protocol was typed from.
    let state = request(sample(CuaActionKind::GetWindowState));
    let fixture = include_str!("fixtures/degraded-window-state.json");
    let mut payload: Value = serde_json::from_str(fixture).unwrap();
    payload["target"] = json!({"pid": 42, "window_id": 99});
    payload["background_input"]["exact_window"]["pid"] = json!(42);
    payload["background_input"]["exact_window"]["window_id"] = json!(99);
    let envelope = decode_response(&state, payload).unwrap();
    match envelope.response {
        CuaResponse::Success { result } => match *result {
            CuaActionResult::GetWindowState(state) => assert!(state.degraded),
            other => panic!("unexpected result {other:?}"),
        },
        CuaResponse::Error { error } => panic!("unexpected error {error:?}"),
    }
}

#[test]
fn payloads_that_do_not_answer_the_request_are_refused() {
    let apps = request(sample(CuaActionKind::ListApps));
    assert!(decode_response(&apps, serde_json::from_str(HEALTHY_DARWIN).unwrap()).is_err());
    assert!(decode_response(&apps, json!("not an object")).is_err());
    assert!(decode_response(&apps, json!({"apps": [], "extra": 1})).is_err());

    // A window-state answer for a different window than the one asked about.
    let state = request(sample(CuaActionKind::GetWindowState));
    let mut other_window: Value =
        serde_json::from_str(include_str!("fixtures/degraded-window-state.json")).unwrap();
    other_window["target"] = json!({"pid": 42, "window_id": 100});
    other_window["background_input"]["exact_window"]["pid"] = json!(42);
    other_window["background_input"]["exact_window"]["window_id"] = json!(100);
    assert!(decode_response(&state, other_window).is_err());

    // Oversized payloads never reach serde.
    let huge = json!({"apps": [], "pad": "x".repeat(MAX_RESULT_BYTES)});
    assert!(decode_response(&apps, huge).is_err());
}

fn runtime_error(envelope: CuaResponseEnvelope) -> CuaRuntimeError {
    match envelope.response {
        CuaResponse::Error { error } => error,
        CuaResponse::Success { result } => panic!("expected an error, got {result:?}"),
    }
}

#[test]
fn driver_failures_become_typed_runtime_errors() {
    let req = request(sample(CuaActionKind::GetWindowState));

    let envelope = error_response(&req, &DriverCallFailure::Transport("broken pipe".into()));
    assert_eq!(envelope.request_id, req.request_id);
    assert_eq!(envelope.machine_id, req.machine_id);
    assert_eq!(envelope.action, CuaActionKind::GetWindowState);
    envelope.validate_response_for(&req).unwrap();
    let error = runtime_error(envelope);
    assert_eq!(error.code, RuntimeErrorCode::RuntimeUnavailable);
    assert!(error.retryable);
    assert_eq!(error.message.as_str(), "broken pipe");

    // The live driver's shape for a closed window: text plus a structured code.
    let not_found = DriverCallFailure::Tool {
        code: Some("window_id_not_found".into()),
        message: "window_id 1 is not a live window (closed, or the id is stale).".into(),
    };
    let error = runtime_error(error_response(&req, &not_found));
    assert_eq!(error.code, RuntimeErrorCode::TargetUnavailable);
    assert!(!error.retryable);
    assert!(
        error
            .message
            .as_str()
            .starts_with("window_id 1 is not a live window")
    );

    for (code, expected, retryable) in [
        ("stale_snapshot", RuntimeErrorCode::StaleSnapshot, false),
        (
            "permission_denied",
            RuntimeErrorCode::PermissionDenied,
            false,
        ),
        (
            "session_not_found",
            RuntimeErrorCode::SessionUnavailable,
            false,
        ),
        ("timeout", RuntimeErrorCode::Timeout, true),
        ("something_else", RuntimeErrorCode::DriverFailure, false),
    ] {
        let error = runtime_error(error_response(
            &req,
            &DriverCallFailure::Tool {
                code: Some(code.into()),
                message: "detail".into(),
            },
        ));
        assert_eq!(error.code, expected, "{code}");
        assert_eq!(error.retryable, retryable, "{code}");
    }

    // Without a code the message text decides.
    let error = runtime_error(error_response(
        &req,
        &DriverCallFailure::Tool {
            code: None,
            message: "Accessibility permission is not granted".into(),
        },
    ));
    assert_eq!(error.code, RuntimeErrorCode::PermissionDenied);

    let error = runtime_error(error_response(
        &req,
        &DriverCallFailure::Malformed("no structured payload".into()),
    ));
    assert_eq!(error.code, RuntimeErrorCode::DriverFailure);
    assert!(!error.retryable);

    // A call the transport cancelled at its deadline is a retryable timeout,
    // not an unavailable runtime: the driver is still there, it was slow.
    let envelope = error_response(
        &req,
        &DriverCallFailure::Timeout("get_window_state did not answer within 30s".into()),
    );
    envelope.validate_response_for(&req).unwrap();
    let error = runtime_error(envelope);
    assert_eq!(error.code, RuntimeErrorCode::Timeout);
    assert!(error.retryable);
    assert_eq!(
        error.message.as_str(),
        "get_window_state did not answer within 30s"
    );
}

#[test]
fn error_messages_are_bounded_and_never_empty() {
    let req = request(sample(CuaActionKind::ListApps));
    let noisy = DriverCallFailure::Tool {
        code: None,
        message: format!("bad\u{0}thing\u{7}\n{}", "x".repeat(MAX_TEXT_BYTES)),
    };
    let envelope = error_response(&req, &noisy);
    envelope.validate_response_for(&req).unwrap();
    let error = runtime_error(envelope);
    assert!(error.message.as_str().starts_with("badthing\n"));
    assert!(error.message.as_str().len() <= 1_024);

    let silent = DriverCallFailure::Tool {
        code: None,
        message: String::new(),
    };
    let error = runtime_error(error_response(&req, &silent));
    assert!(!error.message.as_str().is_empty());
}

#[test]
fn response_for_decodes_successes_and_wraps_every_failure() {
    let req = request(sample(CuaActionKind::ListApps));
    let ok = response_for(&req, Ok(json!({"apps": []})));
    ok.validate_response_for(&req).unwrap();
    assert!(matches!(ok.response, CuaResponse::Success { .. }));

    let undecodable = response_for(&req, Ok(json!({"windows": []})));
    undecodable.validate_response_for(&req).unwrap();
    let error = runtime_error(undecodable);
    assert_eq!(error.code, RuntimeErrorCode::DriverFailure);

    let failed = response_for(&req, Err(DriverCallFailure::Transport("gone".into())));
    assert_eq!(
        runtime_error(failed).code,
        RuntimeErrorCode::RuntimeUnavailable
    );

    let late = response_for(&req, Err(DriverCallFailure::Timeout("slow".into())));
    assert_eq!(runtime_error(late).code, RuntimeErrorCode::Timeout);
}

#[test]
fn a_granted_health_report_advertises_a_healthy_server_local_target() {
    let machine = descriptor(HEALTHY_DARWIN);
    machine.validate().unwrap();
    assert_eq!(machine.machine_id.as_str(), "server-local:studio");
    assert_eq!(machine.location, MachineLocation::ServerLocal);
    assert_eq!(machine.platform, Platform::Macos);
    assert_eq!(machine.driver_version.as_str(), "0.28.2");
    assert_eq!(machine.health, MachineHealth::Healthy);
    assert_eq!(
        machine.permissions,
        PermissionState {
            accessibility: Permission::Granted,
            screen_capture: Permission::Granted,
        }
    );
    let mut advertised = machine.capabilities.clone();
    advertised.sort_by_key(|c| format!("{c:?}"));
    let mut all = vec![
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
    ];
    all.sort_by_key(|c| format!("{c:?}"));
    assert_eq!(advertised, all, "every protocol capability is advertised");
    machine.authorize(&click(point())).unwrap();
}

#[test]
fn denied_accessibility_degrades_the_target_and_drops_input_capabilities() {
    let machine = descriptor(ACCESSIBILITY_DENIED_DARWIN);
    machine.validate().unwrap();
    assert_eq!(machine.health, MachineHealth::Degraded);
    assert_eq!(machine.permissions.accessibility, Permission::Denied);
    assert_eq!(machine.permissions.screen_capture, Permission::Granted);
    for dropped in [
        Capability::Pointer,
        Capability::Keyboard,
        Capability::WindowManagement,
        Capability::ElementValue,
        Capability::Menu,
        Capability::Verification,
    ] {
        assert!(
            !machine.capabilities.contains(&dropped),
            "{dropped:?} needs Accessibility and must not be advertised"
        );
    }
    for kept in [
        Capability::AppDiscovery,
        Capability::AppLaunch,
        Capability::WindowDiscovery,
        Capability::WindowObservation,
        Capability::SessionLifecycle,
        Capability::Health,
    ] {
        assert!(machine.capabilities.contains(&kept), "{kept:?} survives");
    }
    assert!(machine.authorize(&click(point())).is_err());
    machine.authorize(&sample(CuaActionKind::ListApps)).unwrap();
}

#[test]
fn a_failed_core_check_makes_the_target_unavailable() {
    let machine = descriptor(SESSION_FAILED_DARWIN);
    machine.validate().unwrap();
    assert_eq!(machine.health, MachineHealth::Unavailable);
    assert!(machine.authorize(&sample(CuaActionKind::ListApps)).is_err());
}

#[test]
fn other_platforms_read_the_capability_checks() {
    let linux = descriptor(CAPTURE_DENIED_LINUX);
    linux.validate().unwrap();
    assert_eq!(linux.platform, Platform::Linux);
    assert_eq!(linux.health, MachineHealth::Degraded);
    assert_eq!(linux.permissions.accessibility, Permission::Granted);
    assert_eq!(linux.permissions.screen_capture, Permission::Denied);
    assert!(linux.capabilities.contains(&Capability::Pointer));
    let screenshot = CuaAction::GetWindowState(GetWindowStateArgs {
        target: target(),
        session: None,
        include_accessibility_tree: false,
        include_screenshot: true,
        max_elements: None,
        max_depth: None,
        max_dimension: None,
        query: None,
    });
    assert!(linux.authorize(&screenshot).is_err());

    // Unprobed checks prove nothing, so nothing is granted and the target is
    // degraded rather than healthy.
    let windows = descriptor(UNPROBED_WINDOWS);
    windows.validate().unwrap();
    assert_eq!(windows.platform, Platform::Windows);
    assert_eq!(windows.health, MachineHealth::Degraded);
    assert_eq!(
        windows.permissions,
        PermissionState {
            accessibility: Permission::PromptRequired,
            screen_capture: Permission::PromptRequired,
        }
    );
    assert!(!windows.capabilities.contains(&Capability::Keyboard));
    assert!(
        !windows
            .capabilities
            .contains(&Capability::WindowObservation)
    );
    assert!(windows.capabilities.contains(&Capability::Health));
    assert_eq!(
        permissions_from_health(&report(UNPROBED_WINDOWS)),
        windows.permissions
    );
}

/// #18: the installed driver (0.28.2) spells a window flat (`pid` and
/// `window_id` beside the record's fields, no `target`), carries advisory
/// `_note` text on a window state and omits `truncated`. `decode_response`
/// folds those into the protocol's shape before validation; a canonical
/// payload passes through unchanged, and a record that carries both
/// spellings is still refused as unknown fields.
#[test]
fn live_window_spellings_fold_into_the_protocol_shape() {
    let live_window = json!({
        "app_name": "Claude", "bounds": {"height": 800.0, "width": 1658.0, "x": 679.0, "y": 256.0},
        "current_space_id": 1, "is_on_screen": true, "layer": 0, "on_current_space": true,
        "pid": 42, "space_ids": [1], "title": "Claude", "window_id": 8361, "z_index": 18
    });

    let windows = request(sample(CuaActionKind::ListWindows));
    let payload = json!({"current_space_id": 1, "windows": [live_window.clone()]});
    let envelope = decode_response(&windows, payload).unwrap();
    let CuaResponse::Success { result } = envelope.response else {
        panic!("live windows decode");
    };
    let CuaActionResult::ListWindows(listed) = *result else {
        panic!("a windows result");
    };
    assert_eq!(listed.windows[0].target.pid, 42);
    assert_eq!(listed.windows[0].target.window_id, 8361);
    assert_eq!(listed.windows[0].app_name.as_str(), "Claude");

    // Windows nested under apps and under a launch result fold the same way.
    let apps = request(sample(CuaActionKind::ListApps));
    let payload = json!({"apps": [{
        "active": false, "bundle_id": "com.anthropic.claudefordesktop", "kind": "desktop",
        "name": "Claude", "pid": 42, "running": true, "windows": [live_window.clone()]
    }]});
    let envelope = decode_response(&apps, payload).unwrap();
    assert!(matches!(envelope.response, CuaResponse::Success { .. }));
    let launch = request(CuaAction::LaunchApp(LaunchAppArgs {
        bundle_id: Some(AppBundleId::try_from("com.anthropic.claudefordesktop").unwrap()),
        name: None,
        creates_new_application_instance: false,
    }));
    let payload = json!({
        "pid": 42, "bundle_id": "com.anthropic.claudefordesktop", "name": "Claude",
        "launch_state": "window_ready", "windows": [live_window.clone()],
        "self_activation_suppressed": true
    });
    let envelope = decode_response(&launch, payload).unwrap();
    assert!(matches!(envelope.response, CuaResponse::Success { .. }));

    // The live degraded window state: flat target, advisory note, no
    // `truncated`.
    let state = request(sample(CuaActionKind::GetWindowState));
    let payload = json!({
        "_note": "AX unresolved; see escalation",
        "app_name": "Claude", "window_title": "Claude",
        "pid": 42, "window_id": 99,
        "degraded": true, "degraded_reason": "ax_window_unresolved: exact window accessibility surface unavailable",
        "element_count": 0, "elements": [], "elements_complete": false,
        "returned_element_count": 0, "total_element_count": 0, "tree_markdown": "",
        "background_input": {
            "exact_window": {"pid": 42, "status": "ax_unresolved", "window_id": 99},
            "observation": {"frame_freshness": "unknown", "one_shot_capture": "unavailable"},
            "routes": [{"reason": "off_space_or_ax_unresolved", "route": "accessibility", "status": "refused"}]
        },
        "escalation": {"reason": "observation-only", "recommended": "foreground"}
    });
    let envelope = decode_response(&state, payload).unwrap();
    let CuaResponse::Success { result } = envelope.response else {
        panic!("live window state decodes");
    };
    let CuaActionResult::GetWindowState(observed) = *result else {
        panic!("a window state");
    };
    assert_eq!(observed.target.pid, 42);
    assert_eq!(observed.target.window_id, 99);
    assert!(observed.degraded);
    assert!(!observed.truncated, "absent means the walk was not cut");

    // Live elements spell their frame `{x, y, w, h}` and their tokens as
    // `<snapshot>:<index>`; the frame folds to the protocol's rectangle.
    let payload = json!({
        "_note": "ok", "app_name": "Finder", "window_title": "Applications",
        "pid": 42, "window_id": 99, "snapshot_id": "s00000001",
        "element_count": 3, "returned_element_count": 3, "total_element_count": 3,
        "elements_complete": true, "tree_markdown": "- [element_index 0] AXWindow",
        "background_input": {
            "exact_window": {"pid": 42, "status": "matched", "window_id": 99},
            "observation": {"frame_freshness": "unknown", "one_shot_capture": "unavailable"},
            "routes": [
                {"route": "accessibility", "status": "available"},
                {"route": "window_pointer", "status": "available"},
                {"reason": "same_pid_keyboard_ambiguity", "route": "pid_keyboard", "status": "refused"}
            ]
        },
        "elements": [
            {"actions": ["AXRaise"], "depth": 0, "element_index": 0, "element_token": "s00000001:0",
             "frame": {"h": 436.0, "w": 920.0, "x": 820.0, "y": 521.0}, "label": "Applications", "role": "AXWindow"},
            {"actions": ["AXPress"], "depth": 1, "element_index": 1, "element_token": "s00000001:1",
             "frame": {"h": 20.0, "w": 60.0, "x": 830.0, "y": 530.0}, "label": "Back", "role": "AXButton",
             "parent_index": 0},
            {"actions": ["AXPress"], "depth": 3, "element_index": 2, "element_token": "s00000001:2",
             "frame": {"h": 20.0, "w": 60.0, "x": 900.0, "y": 530.0}, "label": "Forward", "role": "AXButton",
             "parent_index": 0, "in_web_content": true}
        ]
    });
    let envelope = decode_response(&state, payload).unwrap();
    let CuaResponse::Success { result } = envelope.response else {
        panic!("live elements decode");
    };
    let CuaActionResult::GetWindowState(observed) = *result else {
        panic!("a window state");
    };
    // `depth` counts every node of the tree while `parent_index` names the
    // nearest actionable ancestor, so a child is deeper than its parent by
    // any amount.
    assert_eq!(observed.elements.len(), 3);
    assert_eq!(observed.elements[2].depth, 3);
    assert_eq!(observed.elements[2].in_web_content, Some(true));
    assert_eq!(observed.elements[1].in_web_content, None);
    let frame = observed.elements[1].frame.unwrap();
    assert_eq!(
        (frame.x, frame.y, frame.width, frame.height),
        (830.0, 530.0, 60.0, 20.0)
    );
    assert_eq!(observed.elements[1].element_token.as_str(), "s00000001:1");
    assert!(!observed.truncated);
    // A resolved window reads `matched` for the protocol's `available`, and
    // an available route carries no reason.
    let background = observed.background_input.unwrap();
    assert_eq!(background.exact_window.status, ExactWindowStatus::Available);
    assert_eq!(background.routes[0].reason, None);
    assert_eq!(
        background.routes[2]
            .reason
            .as_ref()
            .map(BoundedText::as_str),
        Some("same_pid_keyboard_ambiguity")
    );

    // A walk the driver cut reads as truncated.
    let payload = json!({
        "pid": 42, "window_id": 99, "elements": [], "snapshot_id": "s00000001",
        "element_count": 300, "returned_element_count": 0, "total_element_count": 300
    });
    let envelope = decode_response(&state, payload).unwrap();
    let CuaResponse::Success { result } = envelope.response else {
        panic!("cut walk decodes");
    };
    let CuaActionResult::GetWindowState(observed) = *result else {
        panic!("a window state");
    };
    assert!(observed.truncated);

    // The live verification: `status` for `overall`, `index` for
    // `predicate_index`, and timing plus observation text the protocol does
    // not carry.
    let verify = request(sample(CuaActionKind::VerifyState));
    let payload = json!({
        "elapsed_ms": 10,
        "predicates": [{
            "index": 0, "observed_json": "{\"exists\":true}", "status": "satisfied",
            "unknown_reason": null
        }],
        "samples": 1, "stable": true, "status": "satisfied"
    });
    let envelope = decode_response(&verify, payload).unwrap();
    let CuaResponse::Success { result } = envelope.response else {
        panic!("live verification decodes");
    };
    let CuaActionResult::VerifyState(verification) = *result else {
        panic!("a verification");
    };
    assert_eq!(verification.overall, PredicateStatus::Satisfied);
    assert_eq!(verification.predicates[0].predicate_index, 0);
    assert_eq!(
        verification.predicates[0].status,
        PredicateStatus::Satisfied
    );
    // The canonical spelling still decodes as it did.
    let payload = json!({
        "overall": "unknown",
        "predicates": [{"predicate_index": 0, "status": "unknown"}]
    });
    assert!(decode_response(&verify, payload).is_ok());

    // A capture-only answer: the screenshot spelled out in `screenshot_*`
    // fields, and an empty `tree_markdown` that is no tree at all.
    let capture = request(CuaAction::GetWindowState(GetWindowStateArgs {
        target: WindowTarget {
            pid: 42,
            window_id: 99,
        },
        session: None,
        include_accessibility_tree: false,
        include_screenshot: true,
        max_elements: None,
        max_depth: None,
        max_dimension: Some(200),
        query: None,
    }));
    let png = {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(include_bytes!("fixtures/tiny.png"))
    };
    let live_capture = json!({
        "_note": "Prefer element tokens", "app_name": "Finder", "window_title": "Applications",
        "pid": 42, "window_id": 99,
        "element_count": 0, "elements": [], "elements_complete": false,
        "returned_element_count": 0, "total_element_count": 0, "tree_markdown": "",
        "screenshot_frame_valid": true, "screenshot_height": 1, "screenshot_width": 1,
        "screenshot_mime_type": "image/png", "screenshot_png_b64": png, "screenshot_scale": 1.0,
        "window_bounds": {"height": 436.0, "width": 920.0, "x": 820.0, "y": 521.0},
        "background_input": {
            "exact_window": {"pid": 42, "status": "matched", "window_id": 99},
            "observation": {"frame_freshness": "unknown", "one_shot_capture": "available"},
            "routes": [{"route": "accessibility", "status": "available"}]
        }
    });
    let envelope = decode_response(&capture, live_capture.clone()).unwrap();
    let CuaResponse::Success { result } = envelope.response else {
        panic!("live capture decodes");
    };
    let CuaActionResult::GetWindowState(observed) = *result else {
        panic!("a window state");
    };
    assert_eq!(
        observed
            .background_input
            .as_ref()
            .map(|b| b.observation.one_shot_capture),
        Some(ObservationStatus::Available)
    );
    let screenshot = observed.screenshot.expect("the capture is carried");
    assert_eq!(screenshot.media_type, ImageMediaType::Png);
    assert_eq!((screenshot.width, screenshot.height), (1, 1));
    assert_eq!(observed.screenshot_scale, Some(1.0));
    assert_eq!(observed.tree_markdown, None);
    assert_eq!(observed.window_bounds.map(|b| b.width), Some(920.0));
    // A frame the driver could not prove is not handed on as a screenshot.
    let mut unprovable = live_capture;
    unprovable["screenshot_frame_valid"] = json!(false);
    unprovable["degraded"] = json!(true);
    unprovable["degraded_reason"] = json!("px_frame_mismatch");
    let envelope = decode_response(&capture, unprovable).unwrap();
    let CuaResponse::Success { result } = envelope.response else {
        panic!("an unprovable capture still decodes");
    };
    let CuaActionResult::GetWindowState(observed) = *result else {
        panic!("a window state");
    };
    assert!(observed.screenshot.is_none() && observed.screenshot_scale.is_none());

    // An image beside an action result (screenshot evidence the driver may
    // attach to a click, folded in by the transport as `screenshot_*`) is
    // dropped: the protocol's action results carry no capture.
    let clicked = request(click(token()));
    let mut with_capture = json!({
        "target": {"pid": 42, "window_id": 99},
        "address": {"kind": "element_token", "element_token": "tok/1"},
        "button": "left", "action": "press",
        "outcome": {
            "effect": "confirmed", "route": "accessibility",
            "delivery": {"requested": "background", "delivered_count": 1},
            "evidence": ["accessibility_readback", "screenshot"]
        },
        "screenshot_png_b64": png, "screenshot_mime_type": "image/png",
        "screenshot_width": 1, "screenshot_height": 1, "screenshot_scale": 1.0,
        "screenshot_frame_valid": true
    });
    let envelope = decode_response(&clicked, with_capture.clone()).unwrap();
    let CuaResponse::Success { result } = envelope.response else {
        panic!("a click with an image beside it decodes");
    };
    let CuaActionResult::Click(clicked_result) = *result else {
        panic!("a click result");
    };
    assert_eq!(clicked_result.outcome.effect, ActionEffect::Confirmed);
    // Any other unknown field on an action result is still refused.
    with_capture["surprise"] = json!(1);
    assert!(decode_response(&clicked, with_capture).is_err());

    // Both spellings at once is not a shape the driver emits: refused.
    let mut both = live_window.clone();
    both["target"] = json!({"pid": 42, "window_id": 8361});
    assert!(decode_response(&windows, json!({"windows": [both]})).is_err());
    // Advisory keys are dropped only on the window state, not elsewhere.
    assert!(decode_response(&apps, json!({"apps": [], "_note": "x"})).is_err());
}
