//! Desktop Cua targets (#17): a desktop app that registers over the machine
//! WebSocket with a Cua descriptor becomes a target beside the server-local
//! one. Its checked adapter serialises every authorized request as one
//! `cua_request` frame on the desktop's socket and resolves the pending call
//! from the matching `cua_response` frame by request id; the desktop app
//! runs the driver. The link owns what the socket does not: the calls
//! waiting for an answer and the sessions the desktop opened for the
//! server, both dropped when the socket closes.
//!
//! The legacy `toolcall` / `action_result` messages (`remote_bash`,
//! `remote_files`, coordinate `computer_use`) travel on the same socket
//! unchanged; a desktop that registers without a descriptor never sees a
//! typed frame.

use std::{
    collections::{BTreeSet, HashMap},
    sync::{Arc, Mutex},
    time::Duration,
};

use cua_protocol::{
    CheckedCuaAdapter, CuaAction, CuaActionResult, CuaRegistrationEnvelope, CuaRequestEnvelope,
    CuaResponse, CuaResponseEnvelope, MachineDescriptor, MachineId, MachineLocation, RequestId,
    SessionLabel, ValidationError,
    driver_mcp::{DriverCallFailure, error_response},
};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

/// The frame the server sends a desktop for one typed request. The envelope
/// travels whole under `request`, so the desktop decodes it with
/// `CuaRequestEnvelope::from_json` (size, depth and shape bounded) and a
/// legacy toolcall, a flat object with `request_id` and `action`, stays
/// distinguishable from it.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DesktopFrame {
    CuaRequest { request: CuaRequestEnvelope },
}

/// The descriptor a registration's `cua` field carries, accepted only when
/// it names the machine the socket registered as, a desktop location, and
/// an id outside the server machine's reserved prefix: the descriptor is
/// bound to the authenticated socket, never the other way round, and a
/// desktop never registers as the server machine, nor under its id. The
/// prefix is refused whether or not the server-local target is registered
/// yet, because it registers in the background after the listener is up
/// and a desktop that took its id first would block it for the life of the
/// process.
pub fn accept_registration(machine_id: &str, cua: &Value) -> Result<MachineDescriptor, String> {
    let envelope =
        CuaRegistrationEnvelope::from_json(&cua.to_string()).map_err(|error| error.to_string())?;
    let descriptor = envelope.machine;
    if descriptor.machine_id.as_str() != machine_id {
        return Err(format!(
            "cua descriptor names machine '{}' but the socket registered as '{machine_id}'",
            descriptor.machine_id.as_str()
        ));
    }
    if descriptor.location != MachineLocation::Desktop {
        return Err("a desktop registers as a desktop target, never as the server machine".into());
    }
    if let Some(reason) = reserved_machine_id(&descriptor.machine_id) {
        return Err(reason);
    }
    Ok(descriptor)
}

/// Why a desktop may not register a target under `machine_id`: the
/// `server-local:` prefix names the machine the server runs on (#16).
pub fn reserved_machine_id(machine_id: &MachineId) -> Option<String> {
    machine_id
        .as_str()
        .starts_with(crate::services::cua::host::SERVER_LOCAL_PREFIX)
        .then(|| {
            format!(
                "machine id '{}' is reserved for the server-local target",
                machine_id.as_str()
            )
        })
}

/// What one `cua_response` frame did.
#[derive(Debug, Eq, PartialEq)]
pub enum Completion {
    /// The call waiting for this request id received the response.
    Resolved(RequestId),
    /// The frame names a waiting call but is not a response this protocol
    /// can read; the call was failed with `reason` instead of waiting for
    /// its deadline.
    Failed {
        request_id: RequestId,
        reason: String,
    },
    /// No call is waiting for this request id; the frame was dropped.
    Unmatched(String),
    /// The frame is unreadable and names no waiting call; it was dropped.
    Unreadable(String),
}

/// What a disconnect dropped: calls failed and sessions lost with the socket.
#[derive(Debug, Default, Eq, PartialEq)]
pub struct Dropped {
    pub pending: usize,
    pub sessions: Vec<SessionLabel>,
}

/// One desktop's end of the typed frames: the socket's sender, the calls
/// waiting for a `cua_response`, and the sessions the desktop holds open
/// for the server.
pub struct DesktopLink {
    machine_id: MachineId,
    sender: mpsc::UnboundedSender<String>,
    /// How long one call may wait for its `cua_response`.
    call_timeout: Duration,
    state: Mutex<LinkState>,
}

#[derive(Default)]
struct LinkState {
    pending: HashMap<RequestId, oneshot::Sender<Result<CuaResponseEnvelope, DriverCallFailure>>>,
    /// Labels of the sessions the desktop confirmed open and has not ended.
    open: BTreeSet<SessionLabel>,
    /// Set by `disconnect`; every later call fails at once.
    closed: bool,
}

impl DesktopLink {
    /// A link over the desktop's socket sender. `call_timeout` is
    /// `[cua].call_timeout_secs`: a desktop that does not answer within it
    /// fails the call as a retryable timeout.
    pub fn new(
        machine_id: MachineId,
        sender: mpsc::UnboundedSender<String>,
        call_timeout: Duration,
    ) -> Arc<Self> {
        Arc::new(Self {
            machine_id,
            sender,
            call_timeout,
            state: Mutex::new(LinkState::default()),
        })
    }

    pub fn machine_id(&self) -> &MachineId {
        &self.machine_id
    }

    /// A checked adapter that executes every authorized request over this
    /// link. The adapter validates and authorizes before the callback runs
    /// and checks the correlation after it, so the callback only frames,
    /// sends and waits; a desktop that disconnects, stalls or answers
    /// something else yields a typed runtime error, never a panic.
    pub fn checked_adapter(
        self: &Arc<Self>,
        descriptor: MachineDescriptor,
    ) -> Result<CheckedCuaAdapter, ValidationError> {
        let link = self.clone();
        CheckedCuaAdapter::new(descriptor, move |request| {
            let link = link.clone();
            Box::pin(async move { link.call(request).await })
        })
    }

    /// A `cua_response` frame arrived on the socket: decode it through the
    /// protocol's bounds and hand it to the call waiting for its request id.
    pub fn complete(&self, response: Value) -> Completion {
        match CuaResponseEnvelope::from_json(&response.to_string()) {
            Ok(envelope) => {
                let request_id = envelope.request_id.clone();
                match self.lock().pending.remove(&request_id) {
                    Some(waiting) => {
                        let _ = waiting.send(Ok(envelope));
                        Completion::Resolved(request_id)
                    }
                    None => Completion::Unmatched(request_id.as_str().to_owned()),
                }
            }
            Err(error) => {
                let reason = format!("unreadable cua_response: {error}");
                // The frame may still say which call it was for; that call
                // fails now rather than at its deadline.
                let named = response
                    .get("request_id")
                    .and_then(Value::as_str)
                    .and_then(|id| RequestId::try_from(id).ok());
                let Some(request_id) = named else {
                    return Completion::Unreadable(reason);
                };
                match self.lock().pending.remove(&request_id) {
                    Some(waiting) => {
                        let _ = waiting.send(Err(DriverCallFailure::Malformed(reason.clone())));
                        Completion::Failed { request_id, reason }
                    }
                    None => Completion::Unreadable(reason),
                }
            }
        }
    }

    /// The sessions the desktop confirmed open and has not ended, in label order.
    #[cfg(test)]
    pub fn open_sessions(&self) -> Vec<SessionLabel> {
        self.lock().open.iter().cloned().collect()
    }

    /// How many calls are waiting for a `cua_response`.
    #[cfg(test)]
    pub fn pending(&self) -> usize {
        self.lock().pending.len()
    }

    /// The socket is gone: every waiting call fails now as unavailable
    /// instead of at its deadline, and the sessions the desktop held are
    /// lost with it (there is nobody left to end them). Idempotent.
    pub fn disconnect(&self) -> Dropped {
        let mut state = self.lock();
        state.closed = true;
        let pending = state.pending.len();
        for (_, waiting) in state.pending.drain() {
            let _ = waiting.send(Err(DriverCallFailure::Transport(
                "desktop disconnected".to_owned(),
            )));
        }
        let sessions = std::mem::take(&mut state.open).into_iter().collect();
        Dropped { pending, sessions }
    }

    /// One request over the socket: frame it, wait for the answer that
    /// names its request id, and check that the answer is for this request
    /// before handing it back. Every way the desktop can fail the call
    /// (gone, silent, answering something else or something unreadable) is
    /// a typed runtime error correlated to the request.
    async fn call(&self, request: CuaRequestEnvelope) -> CuaResponseEnvelope {
        let frame = match serde_json::to_string(&DesktopFrame::CuaRequest {
            request: request.clone(),
        }) {
            Ok(frame) => frame,
            Err(error) => {
                return error_response(
                    &request,
                    &DriverCallFailure::Malformed(format!("request could not be framed: {error}")),
                );
            }
        };
        let (respond, waiting) = oneshot::channel();
        {
            let mut state = self.lock();
            if state.closed {
                return error_response(
                    &request,
                    &DriverCallFailure::Transport("desktop disconnected".to_owned()),
                );
            }
            if state.pending.contains_key(&request.request_id) {
                return error_response(
                    &request,
                    &DriverCallFailure::Malformed(format!(
                        "request {} is already in flight",
                        request.request_id.as_str()
                    )),
                );
            }
            if self.sender.send(frame).is_err() {
                return error_response(
                    &request,
                    &DriverCallFailure::Transport(
                        "desktop disconnected (socket closed)".to_owned(),
                    ),
                );
            }
            state.pending.insert(request.request_id.clone(), respond);
        }

        let response = match tokio::time::timeout(self.call_timeout, waiting).await {
            Ok(Ok(Ok(response))) => response,
            Ok(Ok(Err(failure))) => return error_response(&request, &failure),
            Ok(Err(_dropped)) => {
                return error_response(
                    &request,
                    &DriverCallFailure::Transport(
                        "desktop disconnected before answering".to_owned(),
                    ),
                );
            }
            Err(_elapsed) => {
                self.lock().pending.remove(&request.request_id);
                return error_response(
                    &request,
                    &DriverCallFailure::Timeout(format!(
                        "desktop did not answer {:?} within {:?}",
                        request.action.kind(),
                        self.call_timeout
                    )),
                );
            }
        };
        if let Err(error) = response.validate_response_for(&request) {
            return error_response(
                &request,
                &DriverCallFailure::Malformed(format!(
                    "desktop answered request {} with a response that does not match it: {error}",
                    request.request_id.as_str()
                )),
            );
        }
        self.note_session(&request.action, &response.response);
        response
    }

    /// The bookkeeping for a confirmed answer: a `start_session` that came
    /// back active opens its label, an `end_session` closes it. Implicit
    /// sessions carry no label and are the desktop's own to end.
    fn note_session(&self, action: &CuaAction, response: &CuaResponse) {
        let CuaResponse::Success { result } = response else {
            return;
        };
        match (action, &**result) {
            (CuaAction::StartSession(args), CuaActionResult::StartSession(started)) => {
                if !started.active {
                    return;
                }
                if let Some(label) = started.session.clone().or_else(|| args.session.clone()) {
                    self.lock().open.insert(label);
                }
            }
            (CuaAction::EndSession(args), CuaActionResult::EndSession(ended)) => {
                if let Some(label) = ended.session.clone().or_else(|| args.session.clone()) {
                    self.lock().open.remove(&label);
                }
            }
            _ => {}
        }
    }

    /// A poisoned lock only means a task panicked mid-update; the maps are
    /// still consistent enough to drain.
    fn lock(&self) -> std::sync::MutexGuard<'_, LinkState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cua_protocol::{
        ActionDelivery, ActionEffect, ActionOutcome, ActionRoute, AppsResult, Capability,
        CaptureScope, ClickAction, ClickActionResult, ClickArgs, DeliveryMode, DriverVersion,
        ElementAddress, EmptyArgs, EndSessionResult, MachineHealth, MouseButton, Permission,
        PermissionState, Platform, ProtocolVersion, RuntimeErrorCode, SessionRefArgs,
        StartSessionArgs, StartSessionResult, WindowPoint, WindowTarget,
    };
    use serde_json::json;

    const STUDIO: &str = "4f3c1c2e-9b5e-4d2b-8f0a-1c2d3e4f5a6b";

    fn id(value: &str) -> MachineId {
        MachineId::try_from(value).unwrap()
    }

    fn descriptor(machine_id: &str, location: MachineLocation) -> MachineDescriptor {
        MachineDescriptor {
            machine_id: id(machine_id),
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
                Capability::Pointer,
                Capability::SessionLifecycle,
            ],
        }
    }

    fn registration(machine_id: &str, location: MachineLocation) -> Value {
        serde_json::to_value(CuaRegistrationEnvelope {
            version: ProtocolVersion::V1,
            machine: descriptor(machine_id, location),
        })
        .unwrap()
    }

    fn target() -> WindowTarget {
        WindowTarget {
            pid: 42,
            window_id: 99,
        }
    }

    fn click() -> CuaAction {
        CuaAction::Click(ClickArgs {
            target: target(),
            session: None,
            delivery_mode: DeliveryMode::Background,
            address: ElementAddress::Point(WindowPoint { x: 1.0, y: 2.0 }),
            button: MouseButton::Left,
            action: ClickAction::Press,
            modifiers: vec![],
            count: None,
        })
    }

    fn clicked() -> CuaActionResult {
        CuaActionResult::Click(ClickActionResult {
            target: target(),
            session: None,
            address: ElementAddress::Point(WindowPoint { x: 1.0, y: 2.0 }),
            button: MouseButton::Left,
            action: ClickAction::Press,
            count: None,
            outcome: ActionOutcome {
                effect: ActionEffect::Confirmed,
                route: ActionRoute::Accessibility,
                delivery: Some(ActionDelivery {
                    requested: DeliveryMode::Background,
                    delivered_count: Some(1),
                }),
                evidence: vec![],
                escalation: None,
            },
        })
    }

    fn request(request_id: &str, action: CuaAction) -> CuaRequestEnvelope {
        CuaRequestEnvelope {
            version: ProtocolVersion::V1,
            request_id: RequestId::try_from(request_id).unwrap(),
            machine_id: id(STUDIO),
            action,
        }
    }

    fn success(request: &CuaRequestEnvelope, result: CuaActionResult) -> Value {
        serde_json::to_value(CuaResponseEnvelope {
            version: request.version,
            request_id: request.request_id.clone(),
            machine_id: request.machine_id.clone(),
            action: result.kind(),
            response: CuaResponse::Success {
                result: Box::new(result),
            },
        })
        .unwrap()
    }

    fn link() -> (Arc<DesktopLink>, mpsc::UnboundedReceiver<String>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            DesktopLink::new(id(STUDIO), tx, Duration::from_secs(30)),
            rx,
        )
    }

    /// The next frame the desktop would read, as JSON.
    async fn next_frame(rx: &mut mpsc::UnboundedReceiver<String>) -> Value {
        let text = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("a frame within 5s")
            .expect("the link still holds the sender");
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn a_registration_is_accepted_only_for_the_socket_it_arrived_on_as_a_desktop() {
        let accepted =
            accept_registration(STUDIO, &registration(STUDIO, MachineLocation::Desktop)).unwrap();
        assert_eq!(accepted, descriptor(STUDIO, MachineLocation::Desktop));

        // Naming another machine: the descriptor is bound to the socket's identity.
        let other =
            accept_registration(STUDIO, &registration("elsewhere", MachineLocation::Desktop))
                .unwrap_err();
        assert!(
            other.to_string().contains("elsewhere") && other.to_string().contains(STUDIO),
            "{other}"
        );

        // A desktop is never the server machine.
        let local =
            accept_registration(STUDIO, &registration(STUDIO, MachineLocation::ServerLocal))
                .unwrap_err();
        assert!(local.to_string().contains("desktop"), "{local}");

        // The `server-local:` prefix is the server machine's own (#16), so a
        // desktop claiming it is refused whether or not that target is
        // registered yet: registering first must never block or shadow it.
        let reserved = accept_registration(
            "server-local:studio",
            &registration("server-local:studio", MachineLocation::Desktop),
        )
        .unwrap_err();
        assert!(
            reserved.contains("server-local:") && reserved.contains("reserved"),
            "{reserved}"
        );

        // The protocol's own checks still apply: shape, unknown fields, bounds.
        let mut unknown = registration(STUDIO, MachineLocation::Desktop);
        unknown["machine"]["extra"] = json!(true);
        assert!(accept_registration(STUDIO, &unknown).is_err());
        assert!(accept_registration(STUDIO, &json!({"version": "v1"})).is_err());
        assert!(accept_registration(STUDIO, &json!("not an envelope")).is_err());
    }

    #[tokio::test]
    async fn a_request_is_one_cua_request_frame_and_the_matching_response_resolves_it() {
        let (link, mut rx) = link();
        let adapter = link
            .checked_adapter(descriptor(STUDIO, MachineLocation::Desktop))
            .unwrap();
        let request = request("req-1", click());

        let call = {
            let adapter = Arc::new(adapter);
            let request = request.clone();
            tokio::spawn(async move { adapter.execute(&request).await })
        };

        // Exactly one frame: the envelope whole, under `request`.
        let frame = next_frame(&mut rx).await;
        assert_eq!(frame["type"], "cua_request");
        assert_eq!(
            CuaRequestEnvelope::from_json(&frame["request"].to_string()).unwrap(),
            request,
            "the desktop reads the envelope with the protocol's bounded decoder"
        );
        assert!(
            frame.get("request_id").is_none() && frame.get("action").is_none(),
            "a typed frame is never mistaken for a legacy toolcall: {frame}"
        );
        assert_eq!(link.pending(), 1);

        let answer = success(&request, clicked());
        assert_eq!(
            link.complete(answer.clone()),
            Completion::Resolved(RequestId::try_from("req-1").unwrap())
        );
        let response = call.await.unwrap().unwrap();
        assert_eq!(
            serde_json::to_value(&response).unwrap(),
            answer,
            "the structured result survives the proxy unchanged"
        );
        assert_eq!(link.pending(), 0);
        assert!(rx.try_recv().is_err(), "nothing else was sent");
    }

    #[tokio::test]
    async fn a_response_for_another_request_or_another_action_is_rejected() {
        let (link, mut rx) = link();
        let adapter = Arc::new(
            link.checked_adapter(descriptor(STUDIO, MachineLocation::Desktop))
                .unwrap(),
        );
        let request = request("req-7", click());
        let call = {
            let (adapter, request) = (adapter.clone(), request.clone());
            tokio::spawn(async move { adapter.execute(&request).await })
        };
        next_frame(&mut rx).await;

        // A well-formed response to a request nobody is waiting for is dropped
        // and the call keeps waiting.
        let stranger = success(&self::request("req-8", click()), clicked());
        assert_eq!(
            link.complete(stranger),
            Completion::Unmatched("req-8".into())
        );
        assert_eq!(link.pending(), 1);

        // The right id but another action's answer (self-consistent, so it
        // passes the envelope's own checks): the correlation check refuses
        // it and the call fails as a driver failure instead of surfacing a
        // result that does not belong to the request.
        let mismatched = success(
            &request,
            CuaActionResult::ListApps(AppsResult { apps: vec![] }),
        );
        assert_eq!(mismatched["action"], "list_apps");
        assert_eq!(
            link.complete(mismatched),
            Completion::Resolved(RequestId::try_from("req-7").unwrap())
        );
        let response = call.await.unwrap().unwrap();
        assert_eq!(response.request_id.as_str(), "req-7");
        assert_eq!(response.action, cua_protocol::CuaActionKind::Click);
        let CuaResponse::Error { error } = response.response else {
            panic!("a mismatched response must not become a success");
        };
        assert_eq!(error.code, RuntimeErrorCode::DriverFailure);
        assert!(!error.retryable);
        assert!(
            error.message.as_str().contains("does not match"),
            "{}",
            error.message.as_str()
        );
        assert_eq!(link.pending(), 0);
    }

    #[tokio::test]
    async fn an_unreadable_response_fails_its_call_without_waiting_for_the_deadline() {
        let (link, mut rx) = link();
        let adapter = Arc::new(
            link.checked_adapter(descriptor(STUDIO, MachineLocation::Desktop))
                .unwrap(),
        );
        let request = request("req-2", click());
        let call = {
            let (adapter, request) = (adapter.clone(), request.clone());
            tokio::spawn(async move { adapter.execute(&request).await })
        };
        next_frame(&mut rx).await;

        // Unreadable and anonymous: dropped, the call keeps waiting.
        assert!(matches!(
            link.complete(json!({"garbage": true})),
            Completion::Unreadable(_)
        ));
        assert_eq!(link.pending(), 1);

        // Unreadable but naming the request: the call fails now.
        let mut broken = success(&request, clicked());
        broken["response"]["result"]["result"]["unknown_field"] = json!(1);
        let completion = link.complete(broken);
        let Completion::Failed { request_id, reason } = completion else {
            panic!("expected Failed, got {completion:?}");
        };
        assert_eq!(request_id.as_str(), "req-2");
        assert!(reason.contains("unknown field"), "{reason}");
        let response = tokio::time::timeout(Duration::from_secs(5), call)
            .await
            .expect("the call fails at once, not at the 30s deadline")
            .unwrap()
            .unwrap();
        let CuaResponse::Error { error } = response.response else {
            panic!("expected an error");
        };
        assert_eq!(error.code, RuntimeErrorCode::DriverFailure);
        assert!(error.message.as_str().contains("unknown field"));
    }

    #[tokio::test]
    async fn start_and_end_session_answers_keep_the_open_set() {
        let (link, mut rx) = link();
        let adapter = Arc::new(
            link.checked_adapter(descriptor(STUDIO, MachineLocation::Desktop))
                .unwrap(),
        );
        let label = SessionLabel::try_from("nolune-run-1").unwrap();
        let start = request(
            "run-1-start",
            CuaAction::StartSession(StartSessionArgs {
                session: Some(label.clone()),
            }),
        );
        let started = CuaActionResult::StartSession(StartSessionResult {
            active: true,
            capture_scope: CaptureScope::Auto,
            effective_scope: CaptureScope::Window,
            desktop_capture_authorized: true,
            desktop_unlocked: true,
            escalation_detail: None,
            escalation_reason: None,
            revived: false,
            session: Some(label.clone()),
        });
        let call = {
            let (adapter, start) = (adapter.clone(), start.clone());
            tokio::spawn(async move { adapter.execute(&start).await })
        };
        next_frame(&mut rx).await;
        link.complete(success(&start, started));
        call.await.unwrap().unwrap();
        assert_eq!(link.open_sessions(), vec![label.clone()]);

        // A second start of the same label is one open session, not two.
        let again = request(
            "run-1-start-again",
            CuaAction::StartSession(StartSessionArgs {
                session: Some(label.clone()),
            }),
        );
        let revived = CuaActionResult::StartSession(StartSessionResult {
            active: true,
            capture_scope: CaptureScope::Auto,
            effective_scope: CaptureScope::Window,
            desktop_capture_authorized: true,
            desktop_unlocked: true,
            escalation_detail: None,
            escalation_reason: None,
            revived: true,
            session: Some(label.clone()),
        });
        let call = {
            let (adapter, again) = (adapter.clone(), again.clone());
            tokio::spawn(async move { adapter.execute(&again).await })
        };
        next_frame(&mut rx).await;
        link.complete(success(&again, revived));
        call.await.unwrap().unwrap();
        assert_eq!(link.open_sessions(), vec![label.clone()]);

        // An implicit session (no label) is the desktop's own; nothing to track.
        let implicit = request(
            "implicit",
            CuaAction::StartSession(StartSessionArgs { session: None }),
        );
        let started_implicit = CuaActionResult::StartSession(StartSessionResult {
            active: true,
            capture_scope: CaptureScope::Auto,
            effective_scope: CaptureScope::Window,
            desktop_capture_authorized: true,
            desktop_unlocked: true,
            escalation_detail: None,
            escalation_reason: None,
            revived: false,
            session: None,
        });
        let call = {
            let (adapter, implicit) = (adapter.clone(), implicit.clone());
            tokio::spawn(async move { adapter.execute(&implicit).await })
        };
        next_frame(&mut rx).await;
        link.complete(success(&implicit, started_implicit));
        call.await.unwrap().unwrap();
        assert_eq!(link.open_sessions(), vec![label.clone()]);

        let end = request(
            "run-1-end",
            CuaAction::EndSession(SessionRefArgs {
                session: Some(label.clone()),
            }),
        );
        let ended = CuaActionResult::EndSession(EndSessionResult {
            session: Some(label.clone()),
            active: false,
        });
        let call = {
            let (adapter, end) = (adapter.clone(), end.clone());
            tokio::spawn(async move { adapter.execute(&end).await })
        };
        next_frame(&mut rx).await;
        link.complete(success(&end, ended));
        call.await.unwrap().unwrap();
        assert!(link.open_sessions().is_empty());
    }

    #[tokio::test]
    async fn a_disconnect_fails_every_waiting_call_and_drops_the_sessions() {
        let (link, mut rx) = link();
        let adapter = Arc::new(
            link.checked_adapter(descriptor(STUDIO, MachineLocation::Desktop))
                .unwrap(),
        );
        let label = SessionLabel::try_from("nolune-run-3").unwrap();
        let start = request(
            "run-3-start",
            CuaAction::StartSession(StartSessionArgs {
                session: Some(label.clone()),
            }),
        );
        let started = CuaActionResult::StartSession(StartSessionResult {
            active: true,
            capture_scope: CaptureScope::Auto,
            effective_scope: CaptureScope::Window,
            desktop_capture_authorized: true,
            desktop_unlocked: true,
            escalation_detail: None,
            escalation_reason: None,
            revived: false,
            session: Some(label.clone()),
        });
        let call = {
            let (adapter, start) = (adapter.clone(), start.clone());
            tokio::spawn(async move { adapter.execute(&start).await })
        };
        next_frame(&mut rx).await;
        link.complete(success(&start, started));
        call.await.unwrap().unwrap();

        // Two calls the desktop never answers.
        let mut waiting = Vec::new();
        for n in [1, 2] {
            let request = request(&format!("run-3-{n}"), click());
            let adapter = adapter.clone();
            waiting.push(tokio::spawn(async move { adapter.execute(&request).await }));
            next_frame(&mut rx).await;
        }
        assert_eq!(link.pending(), 2);

        let dropped = link.disconnect();
        assert_eq!(
            dropped,
            Dropped {
                pending: 2,
                sessions: vec![label],
            }
        );
        for call in waiting {
            let response = tokio::time::timeout(Duration::from_secs(5), call)
                .await
                .expect("a waiting call fails at once, not at its deadline")
                .unwrap()
                .unwrap();
            let CuaResponse::Error { error } = response.response else {
                panic!("expected an error");
            };
            assert_eq!(error.code, RuntimeErrorCode::RuntimeUnavailable);
            assert!(error.retryable);
            assert!(error.message.as_str().contains("disconnected"));
        }
        assert!(link.open_sessions().is_empty());
        assert_eq!(link.pending(), 0);

        // Idempotent, and a call after the disconnect fails without a frame.
        assert_eq!(link.disconnect(), Dropped::default());
        let late = adapter.execute(&request("late", click())).await.unwrap();
        let CuaResponse::Error { error } = late.response else {
            panic!("expected an error");
        };
        assert_eq!(error.code, RuntimeErrorCode::RuntimeUnavailable);
        assert!(rx.try_recv().is_err(), "no frame for a closed link");
    }

    #[tokio::test(start_paused = true)]
    async fn a_call_the_desktop_never_answers_times_out_as_retryable() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let link = DesktopLink::new(id(STUDIO), tx, Duration::from_secs(3));
        let adapter = Arc::new(
            link.checked_adapter(descriptor(STUDIO, MachineLocation::Desktop))
                .unwrap(),
        );
        let request = request("slow", click());
        let call = {
            let (adapter, request) = (adapter.clone(), request.clone());
            tokio::spawn(async move { adapter.execute(&request).await })
        };
        next_frame(&mut rx).await;
        tokio::time::advance(Duration::from_secs(4)).await;
        let response = call.await.unwrap().unwrap();
        let CuaResponse::Error { error } = response.response else {
            panic!("expected an error");
        };
        assert_eq!(error.code, RuntimeErrorCode::Timeout);
        assert!(error.retryable);
        assert!(
            error.message.as_str().contains("3s"),
            "{}",
            error.message.as_str()
        );
        assert_eq!(link.pending(), 0, "the deadline cleans up after itself");

        // A late answer finds nobody waiting.
        assert_eq!(
            link.complete(success(&request, clicked())),
            Completion::Unmatched("slow".into())
        );
    }

    #[tokio::test]
    async fn the_adapter_refuses_before_a_frame_is_sent() {
        let (link, mut rx) = link();
        let mut degraded = descriptor(STUDIO, MachineLocation::Desktop);
        degraded.health = MachineHealth::Degraded;
        degraded.permissions.accessibility = Permission::Denied;
        degraded.capabilities = vec![Capability::AppDiscovery];
        let adapter = link.checked_adapter(degraded).unwrap();

        // Not advertised: refused inside the checked boundary, no frame.
        assert!(adapter.execute(&request("click", click())).await.is_err());
        // Another machine's request never travels on this desktop's socket.
        let foreign = CuaRequestEnvelope {
            machine_id: id("elsewhere"),
            ..request("foreign", CuaAction::ListApps(EmptyArgs {}))
        };
        assert!(adapter.execute(&foreign).await.is_err());
        assert!(rx.try_recv().is_err(), "nothing reached the desktop");
        assert_eq!(link.pending(), 0);
    }
}
