use futures_util::{SinkExt, StreamExt};
use std::sync::Mutex;
use tauri::Emitter;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

use crate::computer_use;
use crate::cua_runtime;
use crate::overlay;

const MAX_QUEUED_REQUEST_BYTES: usize = 1024 * 1024;
const MAX_QUEUED_REQUESTS: usize = 128;
const MAX_COMMAND_OUTPUT_BYTES: usize = 8 * 1024 * 1024;

fn request_len(frame: &Message) -> Option<usize> {
    match frame {
        Message::Text(text) => Some(text.len()),
        Message::Binary(bytes) => Some(bytes.len()),
        _ => None,
    }
}

fn enqueue_request(
    queue: &mut std::collections::VecDeque<Message>,
    queued_bytes: &mut usize,
    frame: Message,
) -> Result<(), ()> {
    let size = request_len(&frame).ok_or(())?;
    if queue.len() >= MAX_QUEUED_REQUESTS
        || size > MAX_QUEUED_REQUEST_BYTES.saturating_sub(*queued_bytes)
    {
        return Err(());
    }
    *queued_bytes += size;
    queue.push_back(frame);
    Ok(())
}

fn request_text(frame: Message) -> Result<String, ()> {
    match frame {
        Message::Text(text) => Ok(text.to_string()),
        Message::Binary(bytes) => String::from_utf8(bytes.to_vec()).map_err(|_| ()),
        _ => Err(()),
    }
}

// Each connection owns a distinct cancellation generation. Work holds a permit
// until its actual main-thread/blocking callback exits, not just its async waiter.
#[derive(Clone)]
struct Session {
    stop: tokio::sync::watch::Sender<bool>,
    work: std::sync::Arc<tokio::sync::Semaphore>,
    connection_stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl Session {
    fn new() -> Self {
        Self {
            stop: tokio::sync::watch::channel(false).0,
            work: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
            connection_stop: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
    fn cancelled(&self) -> bool {
        *self.stop.borrow()
            || self
                .connection_stop
                .load(std::sync::atomic::Ordering::Acquire)
    }
    fn cancel(&self) {
        self.stop.send_replace(true);
        self.cancel_connection();
    }
    fn begin_connection(&self) {
        self.connection_stop
            .store(false, std::sync::atomic::Ordering::Release);
    }
    fn cancel_connection(&self) {
        self.connection_stop
            .store(true, std::sync::atomic::Ordering::Release);
    }
    async fn stopped(&self) {
        let mut rx = self.stop.subscribe();
        if *rx.borrow_and_update() {
            return;
        }
        let _ = rx.changed().await;
    }
    async fn until_stopped<T>(&self, future: impl std::future::Future<Output = T>) -> Option<T> {
        tokio::select! {
            biased;
            _ = self.stopped() => None,
            result = future => Some(result),
        }
    }
    async fn drain(&self) {
        let _ = self.work.acquire().await;
    }
}
struct Work {
    _permit: tokio::sync::OwnedSemaphorePermit,
    session: Session,
}
impl Work {
    fn run<T>(self, action: impl FnOnce(&Session) -> Result<T, String>) -> Result<T, String> {
        if self.session.cancelled() {
            return Err("Session stopped".into());
        }
        action(&self.session)
    }
}
struct Bridge {
    session: Session,
    task: tokio::task::JoinHandle<()>,
}
static BRIDGE_TASK: Mutex<Option<Bridge>> = Mutex::new(None);

/// Instance slug this machine is bound to (set from URL or explicitly).
static INSTANCE_SLUG: Mutex<Option<String>> = Mutex::new(None);

/// Server URL for overlay (set on connect).
static SERVER_URL: Mutex<Option<String>> = Mutex::new(None);

/// Start the machine agent — connects to the server's machine WebSocket,
/// registers this machine, then listens for toolcalls and executes them.
pub async fn connect_computer_use(
    app: tauri::AppHandle,
    instance_url: String,
    auth_token: String,
) -> Result<(), String> {
    // Cancel the old socket and retry loop before starting another connection.
    disconnect_computer_use(app.clone()).await?;
    let mut task = BRIDGE_TASK.lock().map_err(|e| e.to_string())?;
    {
        let mut url = SERVER_URL.lock().map_err(|e| e.to_string())?;
        *url = Some(instance_url.clone());
    }

    let session = Session::new();
    let running = session.clone();
    let handle = tokio::spawn(async move {
        loop {
            if running
                .until_stopped(run_agent(&app, &instance_url, &auth_token, &running))
                .await
                .is_none()
            {
                break;
            }
            // A dropped network future may have dispatched a native callback.
            // Drain it before another socket/session can receive work.
            running.drain().await;
            tokio::select! {
                biased;
                _ = running.stopped() => break,
                _ = tokio::time::sleep(std::time::Duration::from_secs(3)) => {},
            }
        }
        running.drain().await;
    });
    *task = Some(Bridge {
        session,
        task: handle,
    });

    Ok(())
}

pub async fn disconnect_computer_use(app: tauri::AppHandle) -> Result<(), String> {
    let task = BRIDGE_TASK.lock().map_err(|e| e.to_string())?.take();
    if let Some(task) = task {
        task.session.cancel();
        overlay::hide(&app);
        let _ = task.task.await;
        task.session.drain().await;
        // The socket is gone for good: end the driver sessions it held (#17).
        cua_runtime::runtime().end_sessions().await;
    }
    *SERVER_URL.lock().map_err(|e| e.to_string())? = None;
    *INSTANCE_SLUG.lock().map_err(|e| e.to_string())? = None;
    overlay::hide(&app);
    Ok(())
}

#[tauri::command]
pub fn get_server_url() -> Result<String, String> {
    SERVER_URL
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or_else(|| "not connected".into())
}

/// Set the instance slug this machine is bound to.
#[tauri::command]
pub fn set_instance_slug(slug: String) -> Result<(), String> {
    let mut val = INSTANCE_SLUG.lock().map_err(|e| e.to_string())?;
    *val = if slug.is_empty() { None } else { Some(slug) };
    Ok(())
}

async fn run_agent(
    app: &tauri::AppHandle,
    instance_url: &str,
    auth_token: &str,
    session: &Session,
) -> Result<(), String> {
    let result = run_agent_connection(app, instance_url, auth_token, session).await;
    session.cancel_connection();
    overlay::hide(app);
    // The socket closed: the sessions the desktop held for the server end
    // now, and the driver stays up for the reconnect (#17).
    cua_runtime::runtime().end_sessions().await;
    result
}

async fn run_agent_connection(
    app: &tauri::AppHandle,
    instance_url: &str,
    auth_token: &str,
    session: &Session,
) -> Result<(), String> {
    session.begin_connection();
    // Register this machine under its stable id (#80); the hostname is for display.
    let machine_id = stable_machine_id(app)?;
    let host = hostname();
    let os = std::env::consts::OS.to_string();

    // The Cua driver (#17) is described before the socket opens, since the
    // server waits only briefly for the registration; without a driver this
    // desktop registers legacy-only and never sees a typed frame.
    let cua = match cua_runtime::runtime().start(&machine_id).await {
        Ok(descriptor) => Some(descriptor),
        Err(error) => {
            eprintln!("[cua] registering without a typed target: {error}");
            None
        }
    };

    let request = machine_request(instance_url, auth_token)?;
    let (ws, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|_| "Could not connect to machine WebSocket".to_string())?;

    let (mut write, mut read) = ws.split();

    // Get screen dimensions
    let screen = screenshots::Screen::all()
        .ok()
        .and_then(|s| s.into_iter().next());
    let (sw, sh) = screen
        .map(|s| {
            let info = s.display_info;
            (info.width, info.height)
        })
        .unwrap_or((1920, 1080));

    // Instance slug: only set explicitly via set_instance_slug.
    let instance_slug = INSTANCE_SLUG.lock().ok().and_then(|v| v.clone());

    let register = register_message(
        &machine_id,
        &os,
        &host,
        (sw, sh),
        instance_slug,
        &crate::permissions::check_permissions(),
        cua.as_ref(),
    );
    write
        .send(Message::Text(register.to_string().into()))
        .await
        .map_err(|e| format!("send register: {e}"))?;

    eprintln!(
        "[agent] registered as '{machine_id}' ({host}, {os}, {sw}x{sh}, cua driver: {})",
        cua.as_ref()
            .map(|descriptor| format!(
                "{} {:?}",
                descriptor.driver_version.as_str(),
                descriptor.health
            ))
            .unwrap_or_else(|| "none".to_owned())
    );

    // Emit server URL so overlay can build iframe src
    app.emit("server-url", instance_url.to_string()).ok();

    // Scale cache from last screenshot (shared with spawn_blocking tasks)
    let cached_scale = std::sync::Arc::new(std::sync::Mutex::new(1.0f64));

    let mut ping_interval = tokio::time::interval(std::time::Duration::from_secs(20));
    ping_interval.tick().await;
    let mut last_pong = std::time::Instant::now();

    let mut queued_requests = std::collections::VecDeque::new();
    let mut queued_request_bytes: usize = 0;

    // Main loop: receive toolcalls, execute, send results
    loop {
        if session.cancelled() {
            break;
        }

        let (frame, was_queued) = if let Some(frame) = queued_requests.pop_front() {
            (frame, true)
        } else {
            let frame = tokio::select! {
                msg = read.next() => {
                    match msg {
                        Some(Ok(frame @ (Message::Text(_) | Message::Binary(_)))) => frame,
                        Some(Ok(Message::Ping(data))) => {
                            if write.send(Message::Pong(data)).await.is_err() {
                                break;
                            }
                            continue;
                        }
                        Some(Ok(Message::Pong(_))) => {
                            last_pong = std::time::Instant::now();
                            continue;
                        }
                        Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                        Some(Ok(_)) => continue,
                    }
                }
                _ = ping_interval.tick() => {
                    if write.send(Message::Ping(vec![].into())).await.is_err()
                        || last_pong.elapsed() > std::time::Duration::from_secs(45)
                    {
                        break;
                    }
                    continue;
                }
            };
            (frame, false)
        };
        if was_queued {
            queued_request_bytes =
                queued_request_bytes.saturating_sub(request_len(&frame).unwrap_or_default());
        }
        let Ok(text) = request_text(frame) else {
            continue;
        };

        let call: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let Some(inbound) = Inbound::from_frame(&call) else {
            continue;
        };

        // Temporarily hide the action overlay so it does not appear in
        // explicit screenshots, nor in the window snapshot a typed
        // `get_window_state` may take.
        let hide_for_screenshot = inbound.hides_overlay();
        if hide_for_screenshot {
            overlay::set_visible(app, false);
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }

        let (request_id, action) = match &inbound {
            Inbound::Legacy { request_id, action } => (request_id.clone(), action.clone()),
            Inbound::Cua(_) => (String::new(), String::new()),
        };

        // Input actions (keyboard, mouse) must run on main thread on macOS
        // because enigo calls HIToolbox APIs that assert main queue.
        // Other actions (screenshot, bash, file I/O) use spawn_blocking.
        let is_input_action = matches!(
            action.as_str(),
            "key"
                | "type"
                | "left_click"
                | "right_click"
                | "middle_click"
                | "double_click"
                | "mouse_move"
                | "scroll"
                | "switch_desktop"
        );

        let action_future: std::pin::Pin<Box<dyn std::future::Future<Output = Executed> + Send>> =
            match inbound {
                // A typed request runs on the driver, not on the main
                // thread, and takes no work permit: the driver serializes
                // its own actions and a dropped call is cancelled at the
                // driver.
                Inbound::Cua(request) => {
                    let token = auth_token.to_owned();
                    Box::pin(async move {
                        Executed::Cua(answer_typed(cua_runtime::runtime(), &request, &token).await)
                    })
                }
                Inbound::Legacy { .. } => {
                    let permit = session
                        .work
                        .clone()
                        .acquire_owned()
                        .await
                        .map_err(|_| "Session stopped")?;
                    if session.cancelled() {
                        break;
                    }
                    let work = Work {
                        _permit: permit,
                        session: session.clone(),
                    };
                    let action_call = call.clone();
                    let action_name = action.clone();
                    let action_scale = cached_scale.clone();
                    let action_app = app.clone();
                    Box::pin(async move {
                        let result = if is_input_action {
                            let (tx, rx) = tokio::sync::oneshot::channel();
                            let _ = action_app.run_on_main_thread(move || {
                                let result = work.run(|session| {
                                    let mut scale = action_scale.lock().unwrap();
                                    execute_action(&action_call, &action_name, &mut scale, session)
                                });
                                let _ = tx.send(result);
                            });
                            rx.await
                                .unwrap_or_else(|error| Err(format!("main thread recv: {error}")))
                        } else {
                            tokio::task::spawn_blocking(move || {
                                work.run(|session| {
                                    let mut scale = action_scale.lock().unwrap();
                                    execute_action(&action_call, &action_name, &mut scale, session)
                                })
                            })
                            .await
                            .unwrap_or_else(|error| Err(format!("task panic: {error}")))
                        };
                        Executed::Legacy(result)
                    })
                }
            };
        tokio::pin!(action_future);
        let result = loop {
            tokio::select! {
                result = &mut action_future => break Some(result),
                frame = read.next() => match frame {
                    Some(Ok(Message::Ping(data))) => {
                        if write.send(Message::Pong(data)).await.is_err() {
                            session.cancel_connection();
                            overlay::hide(app);
                            let _ = action_future.await;
                            break None;
                        }
                    }
                    Some(Ok(Message::Pong(_))) => last_pong = std::time::Instant::now(),
                    Some(Ok(frame @ (Message::Text(_) | Message::Binary(_)))) => {
                        if enqueue_request(
                            &mut queued_requests,
                            &mut queued_request_bytes,
                            frame,
                        )
                        .is_err()
                        {
                            session.cancel_connection();
                            overlay::hide(app);
                            let _ = action_future.await;
                            break None;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => {
                        session.cancel_connection();
                        overlay::hide(app);
                        let _ = action_future.await;
                        break None;
                    }
                    Some(Ok(_)) => {}
                }
            }
        };
        let Some(result) = result else {
            break;
        };

        if session.cancelled() {
            break;
        }

        // Show overlay after any action (restore if hidden for screenshot)
        overlay::show(app);
        if hide_for_screenshot {
            overlay::set_visible(app, true);
        }

        let result = match result {
            Executed::Legacy(result) => result,
            Executed::Cua(answer) => {
                // The overlay names the kind and what it targeted; a frame
                // the protocol could not read was refused and shows nothing.
                if let Some((kind, detail)) = &answer.overlay {
                    overlay::emit_cua_action(app, *kind, detail);
                }
                if write
                    .send(Message::Text(answer.frame.to_string().into()))
                    .await
                    .is_err()
                {
                    break;
                }
                continue;
            }
        };

        // Build human-readable detail for the overlay
        let detail = match action.as_str() {
            "key" => call
                .get("key")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            "type" => {
                let t = call.get("text").and_then(|v| v.as_str()).unwrap_or("");
                let preview: String = t.chars().take(30).collect();
                if t.chars().count() > 30 {
                    format!("{preview}...")
                } else {
                    preview
                }
            }
            "left_click" | "right_click" | "double_click" | "middle_click" => {
                let (x, y) = parse_coordinate(&call);
                format!("{x}, {y}")
            }
            "scroll" => call
                .get("scroll_direction")
                .and_then(|v| v.as_str())
                .unwrap_or("down")
                .to_string(),
            "bash" => {
                let c = call.get("command").and_then(|v| v.as_str()).unwrap_or("");
                let preview: String = c.chars().take(40).collect();
                if c.chars().count() > 40 {
                    format!("{preview}...")
                } else {
                    preview
                }
            }
            _ => String::new(),
        };
        overlay::emit_action_detail(
            app,
            &crate::companion_relay::redact_secret(&action, auth_token),
            &crate::companion_relay::redact_secret(&detail, auth_token),
        );

        let response = match &result {
            Ok(AgentResult::Screenshot {
                image,
                width,
                height,
                scale,
            }) => serde_json::json!({
                "type": "action_result",
                "request_id": request_id,
                "result_type": "screenshot",
                "image": image,
                "width": width,
                "height": height,
                "scale": scale,
                "success": true,
            }),
            Ok(AgentResult::Action) => serde_json::json!({
                "type": "action_result",
                "request_id": request_id,
                "result_type": "action",
                "success": true,
            }),
            Ok(AgentResult::Output(text)) => serde_json::json!({
                "type": "action_result",
                "request_id": request_id,
                "result_type": "output",
                "success": true,
                "error": text, // reuse error field for output text
            }),
            Err(e) => serde_json::json!({
                "type": "action_result",
                "request_id": request_id,
                "result_type": "action",
                "success": false,
                "error": e,
            }),
        };

        if write
            .send(Message::Text(response.to_string().into()))
            .await
            .is_err()
        {
            break;
        }
    }

    // Connection lost — reset the action overlay.
    overlay::emit_idle(app);
    overlay::hide(app);

    Ok(())
}

/// One frame the server sent: a legacy toolcall (flat `request_id` and
/// `action`) or a typed Cua request (#17), told apart by its shape.
#[derive(Debug, PartialEq)]
enum Inbound {
    Legacy { request_id: String, action: String },
    Cua(serde_json::Value),
}

impl Inbound {
    /// The frame to execute, or `None` for one that carries no request:
    /// the `registered` ack, whose `cua` flag says whether the server took
    /// the descriptor, and anything else without a request id.
    fn from_frame(call: &serde_json::Value) -> Option<Self> {
        if let Some(request) = cua_runtime::typed_request(call) {
            return Some(Self::Cua(request.clone()));
        }
        match call.get("request_id").and_then(|v| v.as_str()) {
            Some(id) => Some(Self::Legacy {
                request_id: id.to_string(),
                action: call
                    .get("action")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            }),
            None => {
                if call.get("type").and_then(|v| v.as_str()) == Some("registered") {
                    eprintln!(
                        "[agent] registration acknowledged (typed cua frames: {})",
                        call.get("cua").and_then(|v| v.as_bool()).unwrap_or(false)
                    );
                }
                None
            }
        }
    }

    /// Whether the overlay hides while this runs, so it appears in neither
    /// an explicit screenshot nor the window snapshot a typed
    /// `get_window_state` may take.
    fn hides_overlay(&self) -> bool {
        match self {
            Self::Legacy { action, .. } => action == "screenshot",
            Self::Cua(request) => request["action"]["tool"] == "get_window_state",
        }
    }
}

/// What one typed request leaves behind once the runtime answered it: the
/// `cua_response` frame to write back and, for a request the protocol could
/// read, what the overlay is told (the kind's name and a redacted detail).
struct TypedAnswer {
    frame: serde_json::Value,
    overlay: Option<(cua_protocol::CuaActionKind, String)>,
}

/// A typed request's turn: answered on the runtime (refused locally, or
/// executed on the driver with the result forwarded unchanged).
async fn answer_typed(
    runtime: &cua_runtime::CuaRuntime,
    request: &serde_json::Value,
    auth_token: &str,
) -> TypedAnswer {
    let frame = runtime.handle(request).await;
    let overlay = cua_protocol::CuaRequestEnvelope::from_json(&request.to_string())
        .ok()
        .map(|typed| {
            (
                typed.action.kind(),
                crate::companion_relay::redact_secret(
                    &cua_runtime::action_detail(&typed.action),
                    auth_token,
                ),
            )
        });
    TypedAnswer { frame, overlay }
}

/// What executing one frame produced: the legacy result, or the typed
/// answer the runtime built.
enum Executed {
    Legacy(Result<AgentResult, String>),
    Cua(TypedAnswer),
}

enum AgentResult {
    Screenshot {
        image: String,
        width: u32,
        height: u32,
        scale: f64,
    },
    Action,
    /// Text output (bash stdout, file content, directory listing).
    Output(String),
}

fn execute_action(
    call: &serde_json::Value,
    action: &str,
    cached_scale: &mut f64,
    session: &Session,
) -> Result<AgentResult, String> {
    match action {
        "screenshot" => {
            let result = computer_use::computer_screenshot()?;
            *cached_scale = result.scale;
            Ok(AgentResult::Screenshot {
                image: result.image,
                width: result.width,
                height: result.height,
                scale: result.scale,
            })
        }
        "left_click" | "right_click" | "middle_click" => {
            let (x, y) = parse_coordinate(call);
            let button = action.trim_end_matches("_click").to_string();
            computer_use::computer_click(x, y, *cached_scale, button)?;
            Ok(AgentResult::Action)
        }
        "double_click" => {
            let (x, y) = parse_coordinate(call);
            computer_use::computer_double_click(x, y, *cached_scale)?;
            Ok(AgentResult::Action)
        }
        "mouse_move" => {
            let (x, y) = parse_coordinate(call);
            computer_use::computer_mouse_move(x, y, *cached_scale)?;
            Ok(AgentResult::Action)
        }
        "type" => {
            let text = call["text"].as_str().unwrap_or("").to_string();
            computer_use::computer_type(text)?;
            Ok(AgentResult::Action)
        }
        "key" => {
            let key = call["key"].as_str().unwrap_or("").to_string();
            computer_use::computer_key(key)?;
            Ok(AgentResult::Action)
        }
        "scroll" => {
            let (x, y) = parse_coordinate(call);
            let direction = call["scroll_direction"].as_str().unwrap_or("down");
            let amount = call["scroll_amount"].as_i64().unwrap_or(3) as i32;
            let (dx, dy) = match direction {
                "up" => (0, amount),
                "down" => (0, -amount),
                "left" => (-amount, 0),
                "right" => (amount, 0),
                _ => (0, -amount),
            };
            computer_use::computer_scroll(x, y, *cached_scale, dx, dy)?;
            Ok(AgentResult::Action)
        }
        // ── Switch desktop (macOS Spaces) ──
        "switch_desktop" => {
            let direction = call["scroll_direction"].as_str().unwrap_or("right");
            let key = match direction {
                "left" => "ctrl+left",
                "right" => "ctrl+right",
                _ => "ctrl+right",
            };
            computer_use::computer_key(key.to_string())?;
            // Wait for animation to complete
            std::thread::sleep(std::time::Duration::from_millis(700));
            Ok(AgentResult::Action)
        }
        // ── Bash ──
        "bash" => {
            let command = call["command"].as_str().unwrap_or("").to_string();
            let cwd = call["cwd"].as_str().map(|s| s.to_string());
            execute_bash(&command, cwd.as_deref(), session)
        }
        // ── File operations ──
        "file_read" => {
            let path = expand_path(call["path"].as_str().unwrap_or(""));
            match std::fs::read_to_string(&path) {
                Ok(content) => Ok(AgentResult::Output(content)),
                Err(e) => Err(format!("read {path}: {e}")),
            }
        }
        "upload_file" => {
            // Upload a local file to the server via HTTP POST multipart
            let path = expand_path(call["path"].as_str().unwrap_or(""));
            let upload_url = call["upload_url"].as_str().unwrap_or("").to_string();
            let auth_token = call["auth_token"].as_str().unwrap_or("").to_string();
            upload_file_to_server(&path, &upload_url, &auth_token, session)
        }
        "file_write" => {
            let path = expand_path(call["path"].as_str().unwrap_or(""));
            let content = call["content"].as_str().unwrap_or("");
            if let Some(parent) = std::path::Path::new(&path).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            match std::fs::write(&path, content) {
                Ok(_) => Ok(AgentResult::Output(format!(
                    "written {} bytes to {path}",
                    content.len()
                ))),
                Err(e) => Err(format!("write {path}: {e}")),
            }
        }
        "file_list" => {
            let path = expand_path(call["path"].as_str().unwrap_or("."));
            match std::fs::read_dir(&path) {
                Ok(entries) => {
                    let mut lines = Vec::new();
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        let meta = entry.metadata().ok();
                        let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
                        let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                        if is_dir {
                            lines.push(format!("{name}/"));
                        } else {
                            lines.push(format!("{name}  ({size} bytes)"));
                        }
                    }
                    lines.sort();
                    Ok(AgentResult::Output(lines.join("\n")))
                }
                Err(e) => Err(format!("list {path}: {e}")),
            }
        }
        _ => Err(format!("unknown action: {action}")),
    }
}

// Poll the child while independently draining nonblocking pipes. On Unix we
// never wait for EOF: a detached descendant may inherit a pipe after the shell
// exits, and joining an EOF reader would deadlock disconnect/reconnect.
#[cfg(unix)]
fn cancellable_output(
    cmd: &mut std::process::Command,
    session: &Session,
) -> std::io::Result<std::process::Output> {
    use std::io::Read;
    use std::os::fd::AsRawFd;
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;

    if session.cancelled() {
        return Err(std::io::ErrorKind::Interrupted.into());
    }
    cmd.process_group(0);
    let mut child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let mut stdout_pipe = child.stdout.take().unwrap();
    let mut stderr_pipe = child.stderr.take().unwrap();
    for fd in [stdout_pipe.as_raw_fd(), stderr_pipe.as_raw_fd()] {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags >= 0 {
            unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
        }
    }
    fn drain<R: Read>(
        reader: &mut R,
        output: &mut Vec<u8>,
        session: &Session,
    ) -> std::io::Result<()> {
        let mut buffer = [0_u8; 8192];
        // Bound each pass so a continuously writable pipe cannot starve the
        // cancellation/process checks in the outer loop.
        for _ in 0..8 {
            if session.cancelled() {
                return Err(std::io::ErrorKind::Interrupted.into());
            }
            let remaining = MAX_COMMAND_OUTPUT_BYTES.saturating_sub(output.len());
            if remaining == 0 {
                return Err(std::io::Error::other("command output limit exceeded"));
            }
            let read_len = remaining.min(buffer.len());
            match reader.read(&mut buffer[..read_len]) {
                Ok(0) => return Ok(()),
                Ok(count) => output.extend_from_slice(&buffer[..count]),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let kill = |child: &mut std::process::Child| {
        unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
        let _ = child.kill();
    };
    let status = loop {
        if let Err(error) = drain(&mut stdout_pipe, &mut stdout, session)
            .and_then(|_| drain(&mut stderr_pipe, &mut stderr, session))
        {
            kill(&mut child);
            let _ = child.wait();
            return Err(error);
        }
        if session.cancelled() {
            kill(&mut child);
            break child.wait()?;
        }
        if let Some(status) = child.try_wait()? {
            // Drain only a bounded amount already in the kernel, then close our
            // read ends even if an escaped descendant still owns a write end.
            let _ = drain(&mut stdout_pipe, &mut stdout, session);
            let _ = drain(&mut stderr_pipe, &mut stderr, session);
            kill(&mut child);
            break status;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    drop(stdout_pipe);
    drop(stderr_pipe);
    if session.cancelled() {
        return Err(std::io::ErrorKind::Interrupted.into());
    }
    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

#[cfg(not(unix))]
fn cancellable_output(
    cmd: &mut std::process::Command,
    session: &Session,
) -> std::io::Result<std::process::Output> {
    use std::io::Read;
    use std::process::Stdio;
    if session.cancelled() {
        return Err(std::io::ErrorKind::Interrupted.into());
    }
    let mut child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    fn reader<R: Read + Send + 'static>(
        mut pipe: R,
    ) -> std::sync::mpsc::Receiver<std::io::Result<Vec<u8>>> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut value = Vec::new();
            let mut buffer = [0_u8; 8192];
            let result = loop {
                if value.len() == MAX_COMMAND_OUTPUT_BYTES {
                    break Err(std::io::Error::other("command output limit exceeded"));
                }
                let remaining = MAX_COMMAND_OUTPUT_BYTES - value.len();
                let read_len = remaining.min(buffer.len());
                match pipe.read(&mut buffer[..read_len]) {
                    Ok(0) => break Ok(value),
                    Ok(count) => value.extend_from_slice(&buffer[..count]),
                    Err(error) => break Err(error),
                }
            };
            let _ = tx.send(result);
        });
        rx
    }
    let out = reader(stdout);
    let err = reader(stderr);
    let status = loop {
        if session.cancelled() {
            let _ = std::process::Command::new("taskkill")
                .args(["/F", "/T", "/PID", &child.id().to_string()])
                .status();
            let _ = child.kill();
            break child.wait()?;
        }
        if let Some(status) = child.try_wait()? {
            // Descendants may inherit the pipes. Best-effort tree termination and
            // bounded receives ensure reconnect never waits forever for EOF.
            let _ = std::process::Command::new("taskkill")
                .args(["/F", "/T", "/PID", &child.id().to_string()])
                .status();
            break status;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let wait = std::time::Duration::from_secs(1);
    let stdout = out.recv_timeout(wait).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::TimedOut, "stdout drain timed out")
    })??;
    let stderr = err.recv_timeout(wait).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::TimedOut, "stderr drain timed out")
    })??;
    if session.cancelled() {
        return Err(std::io::ErrorKind::Interrupted.into());
    }
    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

fn execute_bash(
    command: &str,
    cwd: Option<&str>,
    session: &Session,
) -> Result<AgentResult, String> {
    use std::process::Command;

    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(command);
        c
    } else {
        let mut c = Command::new("sh");
        c.arg("-c").arg(command);
        c
    };

    if let Some(dir) = cwd {
        cmd.current_dir(expand_path(dir));
    }

    match cancellable_output(&mut cmd, session) {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let mut result = String::new();
            if !stdout.is_empty() {
                result.push_str(&stdout);
            }
            if !stderr.is_empty() {
                if !result.is_empty() {
                    result.push('\n');
                }
                result.push_str("[stderr] ");
                result.push_str(&stderr);
            }
            if result.is_empty() {
                result = format!("(exit code: {})", output.status.code().unwrap_or(-1));
            }
            if output.status.success() {
                Ok(AgentResult::Output(result))
            } else {
                Err(result)
            }
        }
        Err(e) => Err(format!("failed to run command: {e}")),
    }
}

fn expand_path(path: &str) -> String {
    if path.starts_with('~') {
        if let Some(home) = dirs::home_dir() {
            return path.replacen('~', &home.to_string_lossy(), 1);
        }
    }
    path.to_string()
}

fn parse_coordinate(call: &serde_json::Value) -> (i32, i32) {
    let coord = &call["coordinate"];
    let x = coord.get(0).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let y = coord.get(1).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    (x, y)
}

fn hostname() -> String {
    gethostname::gethostname().to_string_lossy().to_string()
}

/// Settings store key for the id this computer registers under.
const MACHINE_ID_KEY: &str = "machine_id";

/// Every toolcall this agent executes (`execute_action`), reported at
/// registration so the server can show what the computer can do.
const CAPABILITIES: [&str; 15] = [
    "screenshot",
    "left_click",
    "right_click",
    "middle_click",
    "double_click",
    "mouse_move",
    "type",
    "key",
    "scroll",
    "switch_desktop",
    "bash",
    "file_read",
    "file_write",
    "file_list",
    "upload_file",
];

/// The id this computer registers under: a UUID persisted in the settings
/// store on first use, so reconnects, hostname changes, and reinstalls that
/// keep the store update the same server-side record (#80).
fn stable_machine_id(app: &tauri::AppHandle) -> Result<String, String> {
    use tauri_plugin_store::StoreExt;
    let store = app
        .store("settings.json")
        .map_err(|_| "Could not load settings".to_string())?;
    let (machine_id, fresh) = machine_id_from_store(store.get(MACHINE_ID_KEY));
    if fresh {
        store.set(
            MACHINE_ID_KEY,
            serde_json::Value::String(machine_id.clone()),
        );
        store
            .save()
            .map_err(|_| "Could not save the machine id".to_string())?;
    }
    Ok(machine_id)
}

/// The stored id when it is a UUID, otherwise a fresh one and `true`.
fn machine_id_from_store(stored: Option<serde_json::Value>) -> (String, bool) {
    let kept = stored
        .as_ref()
        .and_then(serde_json::Value::as_str)
        .filter(|id| uuid::Uuid::parse_str(id).is_ok())
        .map(str::to_owned);
    match kept {
        Some(id) => (id, false),
        None => (uuid::Uuid::new_v4().to_string(), true),
    }
}

/// The registration the server expects: stable id, hostname for display,
/// platform label, screen, the companion slug, permission state as the
/// protocol names it, what this agent can execute, and the Cua descriptor
/// of the driver this desktop runs (#17), absent when there is none.
fn register_message(
    machine_id: &str,
    os: &str,
    hostname: &str,
    screen: (u32, u32),
    instance_slug: Option<String>,
    permissions: &crate::permissions::PermissionStatus,
    cua: Option<&cua_protocol::MachineDescriptor>,
) -> serde_json::Value {
    let state = |granted: bool| if granted { "granted" } else { "denied" };
    let mut message = serde_json::json!({
        "type": "register",
        "machine_id": machine_id,
        "os": os,
        "hostname": hostname,
        "screen_width": screen.0,
        "screen_height": screen.1,
        "instance_slug": instance_slug,
        "permissions": {
            "accessibility": state(permissions.accessibility),
            "screen_capture": state(permissions.screen_recording),
        },
        "capabilities": CAPABILITIES,
    });
    if let Some(descriptor) = cua {
        message["cua"] = cua_runtime::registration_envelope(descriptor);
    }
    message
}

/// Upload a local file to the server via curl.
/// Returns the upload_id on success.
fn upload_file_to_server(
    path: &str,
    upload_url: &str,
    auth_token: &str,
    session: &Session,
) -> Result<AgentResult, String> {
    // Check file exists and has content
    let size = match std::fs::metadata(path) {
        Ok(m) => m.len(),
        Err(e) => return Err(format!("file not found: {path} ({e})")),
    };
    if size < 1000 {
        return Err(format!("file too small to upload ({size} bytes): {path}"));
    }

    let output = cancellable_output(
        std::process::Command::new("curl").args([
            "-s",
            "-w",
            "\n%{http_code}",
            "-X",
            "POST",
            "-H",
            &format!("Authorization: Bearer {auth_token}"),
            "-F",
            &format!("file=@{path}"),
            upload_url,
        ]),
        session,
    )
    .map_err(|_| "Upload failed")?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        return Err(format!(
            "curl exit {}: {stderr}",
            output.status.code().unwrap_or(-1)
        ));
    }

    // stdout ends with \nHTTP_CODE, split it
    let (body, _status) = stdout.rsplit_once('\n').unwrap_or((&stdout, ""));

    if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
        let upload_id = json["id"].as_str().unwrap_or("").to_string();
        if upload_id.is_empty() {
            return Err(format!("server returned no id: {body}"));
        }

        Ok(AgentResult::Output(upload_id))
    } else {
        Err(format!("unexpected response: {body}"))
    }
}

fn machine_request(
    instance_url: &str,
    auth_token: &str,
) -> Result<tokio_tungstenite::tungstenite::http::Request<()>, String> {
    let mut url = crate::connection_url(instance_url)?;
    url.set_path(&format!(
        "{}/api/agents/ws/machine",
        url.path().trim_end_matches('/')
    ));
    let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
    url.set_scheme(scheme)
        .map_err(|_| "Invalid WebSocket URL")?;
    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|_| "Invalid WebSocket request")?;
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {auth_token}")
            .parse()
            .map_err(|_| "Invalid auth token")?,
    );
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stored_uuid_is_reused_and_anything_else_is_replaced() {
        let stored = "4f3c1c2e-9b5e-4d2b-8f0a-1c2d3e4f5a6b";
        assert_eq!(
            machine_id_from_store(Some(serde_json::Value::String(stored.into()))),
            (stored.to_owned(), false)
        );

        let (fresh, minted) = machine_id_from_store(None);
        assert!(minted);
        assert!(uuid::Uuid::parse_str(&fresh).is_ok(), "{fresh}");
        let (other, _) = machine_id_from_store(None);
        assert_ne!(fresh, other, "every mint is a new id");

        for bad in [
            serde_json::Value::String("".into()),
            serde_json::Value::String("not a uuid".into()),
            serde_json::Value::String("../escape".into()),
            serde_json::json!(42),
            serde_json::json!({"machine_id": stored}),
        ] {
            let (replaced, minted) = machine_id_from_store(Some(bad.clone()));
            assert!(minted, "{bad}");
            assert!(uuid::Uuid::parse_str(&replaced).is_ok(), "{bad}");
        }
    }

    #[test]
    fn registration_carries_the_stable_id_permissions_and_capabilities() {
        let message = register_message(
            "4f3c1c2e-9b5e-4d2b-8f0a-1c2d3e4f5a6b",
            "macos",
            "studio.local",
            (2560, 1440),
            None,
            &crate::permissions::PermissionStatus {
                screen_recording: false,
                accessibility: true,
            },
            None,
        );
        assert_eq!(message["type"], "register");
        assert_eq!(
            message["machine_id"],
            "4f3c1c2e-9b5e-4d2b-8f0a-1c2d3e4f5a6b"
        );
        assert_eq!(
            message["hostname"], "studio.local",
            "hostname stays for display"
        );
        assert_eq!(message["os"], "macos");
        assert_eq!(message["screen_width"], 2560);
        assert_eq!(message["screen_height"], 1440);
        assert_eq!(message["instance_slug"], serde_json::Value::Null);
        assert_eq!(
            message["permissions"],
            serde_json::json!({"accessibility": "granted", "screen_capture": "denied"}),
            "permissions use the protocol's names and states"
        );
        let capabilities: Vec<&str> = message["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(capabilities, CAPABILITIES);
        for capability in CAPABILITIES {
            assert!(
                capability
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'_'),
                "{capability} must be a plain identifier"
            );
        }

        let bound = register_message(
            "id",
            "linux",
            "box",
            (1, 1),
            Some("companion".into()),
            &crate::permissions::PermissionStatus {
                screen_recording: true,
                accessibility: false,
            },
            None,
        );
        assert_eq!(bound["instance_slug"], "companion");
        assert_eq!(bound["permissions"]["accessibility"], "denied");
        assert_eq!(bound["permissions"]["screen_capture"], "granted");
    }

    /// Every action `execute_action` understands is advertised, and nothing else.
    #[test]
    fn advertised_capabilities_match_the_executed_actions() {
        let source = include_str!("computer_use_bridge.rs");
        let body = &source[source.find("fn execute_action(").unwrap()..];
        let body = &body[..body.find("\nfn ").unwrap_or(body.len())];
        let mut handled = std::collections::BTreeSet::new();
        for line in body.lines() {
            let line = line.trim();
            if !line.ends_with("=> {") {
                continue;
            }
            for arm in line.trim_end_matches("=> {").split('|') {
                let arm = arm.trim();
                if arm.starts_with('"') && arm.ends_with('"') {
                    handled.insert(arm.trim_matches('"').to_owned());
                }
            }
        }
        let advertised: std::collections::BTreeSet<String> =
            CAPABILITIES.iter().map(|s| (*s).to_owned()).collect();
        assert_eq!(advertised, handled);
    }

    /// The descriptor rides on the register message under the same stable
    /// id the socket registers as, so the server binds it to this socket; a
    /// desktop without a driver sends no `cua` field and stays legacy-only.
    #[test]
    fn registration_carries_the_cua_descriptor_under_the_same_stable_id() {
        use cua_protocol::*;

        // The id survives a second run against the same store.
        let (first, minted) = machine_id_from_store(None);
        assert!(minted);
        let (again, minted) = machine_id_from_store(Some(serde_json::Value::String(first.clone())));
        assert!(!minted);
        assert_eq!(again, first, "the persisted id is what every run registers");

        let descriptor = MachineDescriptor {
            machine_id: MachineId::try_from(first.as_str()).unwrap(),
            location: MachineLocation::Desktop,
            platform: Platform::Macos,
            driver_version: DriverVersion::try_from("0.28.2").unwrap(),
            health: MachineHealth::Healthy,
            permissions: PermissionState {
                accessibility: Permission::Granted,
                screen_capture: Permission::Granted,
            },
            capabilities: vec![Capability::AppDiscovery, Capability::Pointer],
        };
        let permissions = crate::permissions::PermissionStatus {
            screen_recording: true,
            accessibility: true,
        };
        let message = register_message(
            &first,
            "macos",
            "studio.local",
            (2560, 1440),
            None,
            &permissions,
            Some(&descriptor),
        );
        let cua = CuaRegistrationEnvelope::from_json(&message["cua"].to_string())
            .expect("the cua field is the registration envelope the server decodes");
        assert_eq!(cua.version, ProtocolVersion::V1);
        assert_eq!(cua.machine, descriptor);
        assert_eq!(message["machine_id"], first);
        assert_eq!(
            cua.machine.machine_id.as_str(),
            message["machine_id"].as_str().unwrap()
        );
        assert_eq!(cua.machine.location, MachineLocation::Desktop);
        // The legacy fields are untouched beside it.
        assert_eq!(
            message["capabilities"].as_array().unwrap().len(),
            CAPABILITIES.len()
        );
        assert_eq!(message["permissions"]["accessibility"], "granted");

        let legacy = register_message(
            &first,
            "macos",
            "studio.local",
            (2560, 1440),
            None,
            &permissions,
            None,
        );
        assert!(
            legacy.get("cua").is_none(),
            "no driver, no cua field: {legacy}"
        );
    }

    /// The socket loop's typed-frame turn, against the fake driver: the
    /// server's frames are told apart by shape (the `registered` ack
    /// executes nothing, a legacy toolcall keeps its path), a typed
    /// request is answered on the runtime with the driver's result inside
    /// the `cua_response` frame unchanged and the overlay told the kind and
    /// a redacted detail, a frame the protocol cannot read is refused and
    /// shows nothing, and the overlay hides for a window snapshot as it
    /// does for a legacy screenshot.
    #[tokio::test]
    async fn typed_frames_are_answered_on_the_runtime_and_labelled_for_the_overlay() {
        use crate::cua_runtime::fake::{FakeTransport, HEALTHY};
        use crate::cua_runtime::{CuaRuntime, DriverTransport};
        use cua_protocol::*;
        use serde_json::json;
        use std::sync::Arc;

        const STUDIO: &str = "4f3c1c2e-9b5e-4d2b-8f0a-1c2d3e4f5a6b";
        const TOKEN: &str = "machine-token-17";
        let target = WindowTarget {
            pid: 42,
            window_id: 99,
        };
        let address = ElementAddress::Point(WindowPoint { x: 1.0, y: 2.0 });
        let text = BoundedText::try_from(format!("paste {TOKEN} here")).unwrap();
        let type_text = CuaAction::TypeText(TypeTextArgs {
            target,
            session: None,
            delivery_mode: DeliveryMode::Background,
            address: address.clone(),
            text: text.clone(),
            delay_ms: 0,
        });
        let typed_result = serde_json::to_value(TypeTextActionResult {
            target,
            session: None,
            address,
            text,
            delay_ms: 0,
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
        .unwrap();
        let fake = FakeTransport::answering([
            Ok(serde_json::from_str(HEALTHY).unwrap()),
            Ok(typed_result.clone()),
        ]);
        let runtime = CuaRuntime::with_spawner(Arc::new({
            let fake = fake.clone();
            move || {
                let fake = fake.clone();
                Box::pin(async move { Ok(fake as Arc<dyn DriverTransport>) })
            }
        }));
        runtime.start(STUDIO).await.unwrap();

        // The frames as the server sends them (#196).
        let typed = |request_id: &str, action: CuaAction| {
            json!({
                "type": "cua_request",
                "request": CuaRequestEnvelope {
                    version: ProtocolVersion::V1,
                    request_id: RequestId::try_from(request_id).unwrap(),
                    machine_id: MachineId::try_from(STUDIO).unwrap(),
                    action,
                },
            })
        };
        assert_eq!(
            Inbound::from_frame(&json!({"type": "registered", "machine_id": STUDIO, "cua": true})),
            None,
            "the ack executes nothing"
        );
        let screenshot = Inbound::from_frame(&json!({"request_id": "abc", "action": "screenshot"}))
            .expect("a legacy toolcall");
        assert_eq!(
            screenshot,
            Inbound::Legacy {
                request_id: "abc".into(),
                action: "screenshot".into()
            }
        );
        assert!(screenshot.hides_overlay());
        let snapshot = Inbound::from_frame(&typed(
            "obs-1",
            CuaAction::GetWindowState(GetWindowStateArgs {
                target,
                session: None,
                include_accessibility_tree: true,
                include_screenshot: true,
                max_elements: None,
                max_depth: None,
                max_dimension: None,
                query: None,
            }),
        ))
        .expect("a typed frame");
        assert!(matches!(snapshot, Inbound::Cua(_)));
        assert!(
            snapshot.hides_overlay(),
            "a window snapshot hides the overlay like a screenshot"
        );

        // The typed request's turn: the driver's result rides back inside
        // the response frame, the overlay is told the kind and a detail
        // with the secret redacted.
        let Some(Inbound::Cua(request)) = Inbound::from_frame(&typed("req-1", type_text)) else {
            panic!("a typed frame")
        };
        assert!(!Inbound::Cua(request.clone()).hides_overlay());
        let answer = answer_typed(&runtime, &request, TOKEN).await;
        assert_eq!(answer.frame["type"], "cua_response", "{}", answer.frame);
        assert_eq!(answer.frame["response"]["request_id"], "req-1");
        assert_eq!(answer.frame["response"]["machine_id"], STUDIO);
        assert_eq!(answer.frame["response"]["action"], "type_text");
        assert_eq!(answer.frame["response"]["response"]["status"], "success");
        assert_eq!(
            answer.frame["response"]["response"]["result"]["result"],
            typed_result
        );
        let sent = CuaRequestEnvelope::from_json(&request.to_string()).unwrap();
        CuaResponseEnvelope::from_json(&answer.frame["response"].to_string())
            .unwrap()
            .validate_response_for(&sent)
            .unwrap();
        let (kind, detail) = answer.overlay.expect("the overlay is told");
        assert_eq!(kind, CuaActionKind::TypeText);
        assert_eq!(detail, "paste [redacted] here");
        assert_eq!(fake.tools_called(), ["health_report", "type_text"]);

        // A forged frame is refused locally and shows nothing.
        let forged = json!({"type": "cua_request", "request": {
            "version": "v1", "request_id": "forged-1", "machine_id": STUDIO,
            "action": {"tool": "clipboard_read", "args": {}},
        }});
        let Some(Inbound::Cua(request)) = Inbound::from_frame(&forged) else {
            panic!("a typed frame by shape")
        };
        let refused = answer_typed(&runtime, &request, TOKEN).await;
        assert_eq!(refused.frame["type"], "cua_response");
        assert_eq!(refused.frame["response"]["request_id"], "forged-1");
        assert_eq!(
            refused.frame["response"]["response"]["error"]["code"],
            "capability_denied"
        );
        assert!(refused.overlay.is_none(), "nothing to show for a refusal");
        assert_eq!(fake.tools_called().len(), 2, "the driver never saw it");
    }

    #[test]
    fn concurrent_request_queue_preserves_text_binary_order_and_bounds() {
        let mut queue = std::collections::VecDeque::new();
        let mut bytes = 0;
        enqueue_request(
            &mut queue,
            &mut bytes,
            Message::Text(r#"{"request_id":"one"}"#.into()),
        )
        .unwrap();
        enqueue_request(
            &mut queue,
            &mut bytes,
            Message::Binary(br#"{"request_id":"two"}"#.to_vec().into()),
        )
        .unwrap();
        let ids: Vec<_> = queue
            .drain(..)
            .map(|frame| {
                serde_json::from_str::<serde_json::Value>(&request_text(frame).unwrap()).unwrap()
                    ["request_id"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect();
        assert_eq!(ids, ["one", "two"]);
        let oversized = Message::Binary(vec![0; MAX_QUEUED_REQUEST_BYTES + 1].into());
        assert!(enqueue_request(&mut queue, &mut bytes, oversized).is_err());
    }

    async fn work(session: &Session) -> Work {
        Work {
            _permit: session.work.clone().acquire_owned().await.unwrap(),
            session: session.clone(),
        }
    }

    #[tokio::test]
    async fn queued_callback_is_cancelled_and_drain_waits_for_callback_exit() {
        let session = Session::new();
        let queued = work(&session).await;
        session.cancel();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), session.drain())
                .await
                .is_err()
        );
        assert!(queued
            .run::<()>(|_| panic!("cancelled queued action executed"))
            .is_err());
        tokio::time::timeout(std::time::Duration::from_secs(1), session.drain())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn in_flight_blocking_action_must_finish_before_disconnect_and_new_generation() {
        let session = Session::new();
        let active = work(&session).await;
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let task = tokio::task::spawn_blocking(move || {
            active.run(|_| {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok("old result")
            })
        });
        started_rx.await.unwrap();
        session.cancel();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), session.drain())
                .await
                .is_err()
        );
        release_tx.send(()).unwrap();
        let result = task.await.unwrap().unwrap();
        session.drain().await;
        let next = Session::new();
        assert!(session.cancelled(), "old result {result} must be discarded");
        assert!(!next.cancelled());
        assert_eq!(
            work(&next).await.run(|_| Ok("new result")).unwrap(),
            "new result"
        );
        session.cancel();
        assert!(
            !next.cancelled(),
            "old cancellation must not affect reconnect"
        );
    }

    #[tokio::test]
    async fn stopped_generation_cannot_publish_an_in_flight_result_to_reconnect() {
        let old = Session::new();
        let active = work(&old).await;
        let running = old.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let task = tokio::spawn(async move {
            let result = running
                .until_stopped(async move {
                    tokio::task::spawn_blocking(move || {
                        active.run(|_| {
                            started_tx.send(()).unwrap();
                            release_rx.recv().unwrap();
                            Ok("stale")
                        })
                    })
                    .await
                    .unwrap()
                })
                .await;
            running.drain().await;
            result
        });
        started_rx.await.unwrap();
        old.cancel();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), old.drain())
                .await
                .is_err()
        );
        release_tx.send(()).unwrap();
        assert!(task.await.unwrap().is_none());
        let new = Session::new();
        assert_eq!(new.until_stopped(async { "fresh" }).await, Some("fresh"));
        assert!(old
            .until_stopped(async { panic!("stale socket polled") })
            .await
            .is_none());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cancellation_kills_subprocess_and_descendants_and_drains_work() {
        let session = Session::new();
        let active = work(&session).await;
        let marker = std::env::temp_dir().join(format!("nolune-cancel-{}", uuid::Uuid::new_v4()));
        let command = format!("touch '{}'; sleep 30 & wait", marker.display());
        let task = tokio::task::spawn_blocking(move || {
            active.run(|session| execute_bash(&command, None, session))
        });
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while !marker.exists() {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        session.cancel();
        tokio::time::timeout(std::time::Duration::from_secs(3), session.drain())
            .await
            .unwrap();
        assert!(task.await.unwrap().is_err());
        std::fs::remove_file(marker).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn network_generation_cancellation_stops_subprocess_and_allows_reconnect() {
        let session = Session::new();
        session.begin_connection();
        let active = work(&session).await;
        let task = tokio::task::spawn_blocking(move || {
            active.run(|session| execute_bash("sleep 30", None, session))
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        session.cancel_connection();
        tokio::time::timeout(std::time::Duration::from_secs(2), session.drain())
            .await
            .expect("WebSocket EOF must cancel and drain the active subprocess");
        assert!(task.await.unwrap().is_err());
        assert!(session.cancelled());
        session.begin_connection();
        assert!(
            !session.cancelled(),
            "network EOF must not cancel the reconnect loop"
        );
        assert_eq!(
            work(&session).await.run(|_| Ok("reconnected")).unwrap(),
            "reconnected"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn detached_pipe_holder_cannot_block_command_completion_or_drain() {
        let session = Session::new();
        let active = work(&session).await;
        let task = tokio::task::spawn_blocking(move || {
            active.run(|session| {
                execute_bash(
                    "python3 -c 'import os,time; os.setsid(); time.sleep(2)' &",
                    None,
                    session,
                )
            })
        });
        tokio::time::timeout(std::time::Duration::from_millis(750), task)
            .await
            .expect("detached descendants holding pipes must not block completion")
            .unwrap()
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_millis(100), session.drain())
            .await
            .unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn subprocess_output_is_bounded() {
        let session = Session::new();
        let result = execute_bash(
            "python3 -c 'import sys; sys.stdout.write(\"x\" * 9000000)'",
            None,
            &session,
        );
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("unbounded command output unexpectedly succeeded"),
        };
        assert!(error.contains("output limit"));
    }

    #[test]
    fn machine_auth_uses_header_and_rejects_base_paths() {
        assert!(machine_request("https://example.org:8443/nolune/", "secret").is_err());
        let request = machine_request("https://example.org:8443/", "secret").unwrap();
        assert_eq!(
            request.uri().to_string(),
            "wss://example.org:8443/api/agents/ws/machine"
        );
        assert_eq!(request.headers()["Authorization"], "Bearer secret");
        assert!(request.uri().query().is_none());
        assert_eq!(
            machine_request("http://localhost:3000", "secret")
                .unwrap()
                .uri()
                .scheme_str(),
            Some("ws")
        );
    }

    #[test]
    fn machine_auth_rejects_header_injection() {
        assert!(machine_request("http://localhost:3000", "secret\r\nInjected: yes").is_err());
    }
}
