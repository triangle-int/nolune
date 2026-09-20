//! In-app server install (#128).
//!
//! Downloads the matching `nolune` release binary into `~/.nolune/bin`, runs
//! `nolune onboard --json`, spawns `nolune gateway` as a child the app owns, waits for
//! its ready line, and saves the connection. The gateway dies with the app; running it
//! as a service is a separate opt-in (#129).

use std::{
    collections::VecDeque,
    io::{self, BufRead, BufReader},
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{mpsc, Arc, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};

pub const REPO: &str = "triangle-int/nolune";
pub const READY_PREFIX: &str = "nolune: ready ";
pub const DEFAULT_PORT: u16 = 26559;
pub const SHUTDOWN_GRACE: Duration = Duration::from_secs(6);
const READY_TIMEOUT: Duration = Duration::from_secs(120);
const RECENT_LINES: usize = 20;

#[derive(Debug, Clone, Serialize)]
pub struct LocalStatus {
    pub supported: bool,
    pub target: Option<String>,
    pub home: String,
    pub binary_installed: bool,
    pub config_exists: bool,
    pub gateway_running: bool,
    pub port_in_use: bool,
    pub service_installed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstallOutcome {
    pub url: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct OnboardInfo {
    pub url: String,
    pub token: String,
}

/// A gateway process the app owns, plus the channel its ready line arrives on.
pub struct Gateway {
    child: Child,
    ready: mpsc::Receiver<String>,
    recent: Arc<Mutex<VecDeque<String>>>,
    readers: Vec<JoinHandle<()>>,
}

/// Tauri-managed slot for the one app-owned gateway.
pub struct LocalGateway(pub Mutex<Option<Gateway>>);

/// Release asset for a platform, matching the release workflow's matrix. `None` when no
/// binary is published for it.
pub fn asset_name(os: &str, arch: &str) -> Option<String> {
    let triple = match (os, arch) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        ("windows", "x86_64") => return Some("nolune-server-x86_64-pc-windows-msvc.exe".into()),
        _ => return None,
    };
    Some(format!("nolune-server-{triple}"))
}

/// Where the binary comes from. Stable uses the redirecting `latest` URL so no API call
/// (and no rate limit) is involved; nightly needs the tag.
pub fn download_url(channel: &str, asset: &str, nightly_tag: Option<&str>) -> String {
    if channel == "nightly" {
        let tag = nightly_tag.unwrap_or("nightly");
        format!("https://github.com/{REPO}/releases/download/{tag}/{asset}")
    } else {
        format!("https://github.com/{REPO}/releases/latest/download/{asset}")
    }
}

/// The workspace: `NOLUNE_HOME` if set, else `~/.nolune`, the same rule the server uses.
pub fn nolune_home() -> PathBuf {
    if let Some(home) = std::env::var_os("NOLUNE_HOME") {
        return PathBuf::from(home);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".nolune")
}

pub fn binary_path(home: &Path) -> PathBuf {
    home.join("bin").join(if cfg!(windows) {
        "nolune.exe"
    } else {
        "nolune"
    })
}

/// `onboard --json` prints one JSON line last; anything before it is progress text.
pub fn parse_onboard_output(stdout: &str) -> Result<OnboardInfo, String> {
    let last = stdout
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .ok_or("nolune onboard printed nothing")?;
    let info: OnboardInfo = serde_json::from_str(last.trim())
        .map_err(|error| format!("nolune onboard output is not JSON: {error}"))?;
    if info.url.is_empty() || info.token.is_empty() {
        return Err("nolune onboard output is missing url or token".into());
    }
    Ok(info)
}

/// The URL from a `nolune: ready <url>` line, or `None` for any other line.
pub fn ready_url(line: &str) -> Option<String> {
    line.trim()
        .strip_prefix(READY_PREFIX)
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(str::to_owned)
}

/// The port from config.toml, without a TOML parser: the file is one the binary wrote.
pub fn configured_port(home: &Path) -> u16 {
    std::fs::read_to_string(home.join("config.toml"))
        .ok()
        .and_then(|raw| {
            raw.lines().find_map(|line| {
                let (key, value) = line.split_once('=')?;
                if key.trim() != "port" {
                    return None;
                }
                value.split('#').next()?.trim().parse::<u16>().ok()
            })
        })
        .unwrap_or(DEFAULT_PORT)
}

/// Stream `url` to `dest` via a temporary file, then make it executable. `on_progress`
/// receives (downloaded, total) as chunks arrive.
pub async fn download_binary(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    mut on_progress: impl FnMut(u64, Option<u64>),
) -> Result<(), String> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("could not reach {url}: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("download failed: HTTP {status} for {url}"));
    }
    let total = response.content_length();
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| io_err("cannot create bin directory", error))?;
    }
    let tmp = dest.with_extension("tmp");
    let mut file = tokio::fs::File::create(&tmp)
        .await
        .map_err(|error| io_err("cannot create download file", error))?;
    let mut downloaded = 0u64;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(error) => {
                let _ = tokio::fs::remove_file(&tmp).await;
                return Err(format!("download interrupted: {error}"));
            }
        };
        if let Err(error) = file.write_all(&chunk).await {
            let _ = tokio::fs::remove_file(&tmp).await;
            return Err(io_err("cannot write binary", error));
        }
        downloaded += chunk.len() as u64;
        on_progress(downloaded, total);
    }
    file.flush()
        .await
        .map_err(|error| io_err("cannot finish binary", error))?;
    drop(file);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
            .await
            .map_err(|error| io_err("cannot mark binary executable", error))?;
    }
    tokio::fs::rename(&tmp, dest)
        .await
        .map_err(|error| io_err("cannot move binary into place", error))?;
    Ok(())
}

/// The release tag behind a download URL, from the `/releases/download/<tag>/` redirect.
pub async fn resolve_version(url: &str) -> Option<String> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .build()
        .ok()?;
    let response = client.head(url).send().await.ok()?;
    let location = response
        .headers()
        .get(reqwest::header::LOCATION)?
        .to_str()
        .ok()?;
    let rest = location.split("/releases/download/").nth(1)?;
    let tag = rest.split('/').next()?;
    (!tag.is_empty()).then(|| tag.to_string())
}

/// Run `nolune onboard --json` for `home` and parse its outcome.
pub fn run_onboard(binary: &Path, home: &Path) -> Result<OnboardInfo, String> {
    let output = Command::new(binary)
        .args(["onboard", "--json"])
        .env("NOLUNE_HOME", home)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| io_err("cannot run nolune onboard", error))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "nolune onboard failed ({}): {}",
            output.status,
            stderr.trim()
        ));
    }
    parse_onboard_output(&String::from_utf8_lossy(&output.stdout))
}

/// Spawn `nolune gateway` with `home`. Every stdout and stderr line goes to `on_line`;
/// the ready line is also delivered on the returned gateway's channel.
pub fn spawn_gateway(
    binary: &Path,
    home: &Path,
    on_line: impl Fn(String) + Send + Clone + 'static,
) -> Result<Gateway, String> {
    let mut child = Command::new(binary)
        .arg("gateway")
        .env("NOLUNE_HOME", home)
        .env("RUST_LOG", "info")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| io_err("cannot start nolune gateway", error))?;
    let stdout = child.stdout.take().ok_or("gateway stdout is unavailable")?;
    let stderr = child.stderr.take().ok_or("gateway stderr is unavailable")?;
    let (tx, rx) = mpsc::channel();
    let recent = Arc::new(Mutex::new(VecDeque::with_capacity(RECENT_LINES)));

    let remember = |recent: &Arc<Mutex<VecDeque<String>>>, line: &str| {
        if let Ok(mut lines) = recent.lock() {
            if lines.len() == RECENT_LINES {
                lines.pop_front();
            }
            lines.push_back(line.to_string());
        }
    };

    let (out_cb, out_recent) = (on_line.clone(), recent.clone());
    let out_reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(url) = ready_url(&line) {
                let _ = tx.send(url);
            }
            remember(&out_recent, &line);
            out_cb(line);
        }
    });
    let (err_cb, err_recent) = (on_line, recent.clone());
    let err_reader = thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            remember(&err_recent, &line);
            err_cb(line);
        }
    });

    Ok(Gateway {
        child,
        ready: rx,
        recent,
        readers: vec![out_reader, err_reader],
    })
}

impl Gateway {
    #[cfg(test)]
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    /// True while the child has not exited.
    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    fn recent_output(&mut self) -> String {
        // Once the process is gone the pipes close; let the readers drain first.
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
        self.recent
            .lock()
            .map(|lines| lines.iter().cloned().collect::<Vec<_>>().join("\n"))
            .unwrap_or_default()
    }

    /// Wait for the ready line (or a live `/healthz` on `port`) for up to `timeout`.
    /// Fails early if the process exits first.
    pub fn wait_ready(&mut self, port: u16, timeout: Duration) -> Result<String, String> {
        let deadline = Instant::now() + timeout;
        loop {
            match self.ready.recv_timeout(Duration::from_millis(250)) {
                Ok(url) => return Ok(url),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    thread::sleep(Duration::from_millis(250));
                }
            }
            if let Ok(Some(status)) = self.child.try_wait() {
                return Err(format!(
                    "nolune gateway exited ({status}) before it was ready:\n{}",
                    self.recent_output()
                ));
            }
            if port != 0 && port_is_listening(port) {
                return Ok(format!("http://localhost:{port}"));
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "nolune gateway did not become ready within {} seconds:\n{}",
                    timeout.as_secs(),
                    self.recent_output()
                ));
            }
        }
    }

    /// Ask the gateway to stop (SIGTERM on Unix), wait up to `grace`, then kill it.
    pub fn stop(mut self, grace: Duration) -> Result<(), String> {
        #[cfg(unix)]
        {
            // SAFETY: plain signal delivery to a pid this process spawned and still owns.
            unsafe {
                libc::kill(self.child.id() as libc::pid_t, libc::SIGTERM);
            }
        }
        #[cfg(not(unix))]
        {
            let _ = self.child.kill();
        }
        let deadline = Instant::now() + grace;
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return Ok(()),
                Ok(None) => {}
                Err(error) => return Err(io_err("cannot wait for nolune gateway", error)),
            }
            if Instant::now() >= deadline {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        self.child
            .kill()
            .map_err(|error| io_err("cannot kill nolune gateway", error))?;
        self.child
            .wait()
            .map_err(|error| io_err("cannot reap nolune gateway", error))?;
        Ok(())
    }
}

/// True when something answers on 127.0.0.1:`port`.
pub fn port_is_listening(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok()
}

fn io_err(context: &str, error: io::Error) -> String {
    format!("{context}: {error}")
}

fn port_of(url: &str) -> u16 {
    url::Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.port_or_known_default())
        .unwrap_or(DEFAULT_PORT)
}

// ── Tauri commands ──────────────────────────────────────────────────────────

fn take_gateway(app: &tauri::AppHandle) -> Result<Option<Gateway>, String> {
    Ok(app
        .state::<LocalGateway>()
        .0
        .lock()
        .map_err(|_| "gateway state is poisoned")?
        .take())
}

fn gateway_running(app: &tauri::AppHandle) -> bool {
    app.state::<LocalGateway>()
        .0
        .lock()
        .ok()
        .and_then(|mut slot| slot.as_mut().map(Gateway::is_running))
        .unwrap_or(false)
}

/// Stop the app-owned gateway if there is one. Used on reinstall and at app exit.
pub fn shutdown(app: &tauri::AppHandle) {
    if let Ok(Some(gateway)) = take_gateway(app) {
        let _ = gateway.stop(SHUTDOWN_GRACE);
    }
}

/// Spawn the gateway, stream its lines to the webview, wait for readiness, and park it
/// in app state. On failure the process is stopped and nothing is kept.
async fn start_gateway(
    app: &tauri::AppHandle,
    binary: PathBuf,
    home: PathBuf,
    port: u16,
) -> Result<String, String> {
    if let Some(previous) = take_gateway(app)? {
        let _ = previous.stop(SHUTDOWN_GRACE);
    }
    let log_app = app.clone();
    let on_line = move |line: String| {
        let _ = log_app.emit("local-gateway-log", line);
    };
    let (gateway, url) = tokio::task::spawn_blocking(move || {
        let mut gateway = spawn_gateway(&binary, &home, on_line)?;
        match gateway.wait_ready(port, READY_TIMEOUT) {
            Ok(url) => Ok((gateway, url)),
            Err(error) => {
                let _ = gateway.stop(SHUTDOWN_GRACE);
                Err(error)
            }
        }
    })
    .await
    .map_err(|error| format!("gateway task failed: {error}"))??;
    *app.state::<LocalGateway>()
        .0
        .lock()
        .map_err(|_| "gateway state is poisoned")? = Some(gateway);
    Ok(url)
}

async fn save_local_connection(info: &OnboardInfo) -> Result<(), String> {
    crate::validate_connection(&info.url, &info.token).await?;
    let origin = crate::connection_url(&info.url)?
        .origin()
        .ascii_serialization();
    crate::credentials::store(crate::credentials::SavedConnection {
        origin,
        token: info.token.clone(),
    })
    .await
}

#[tauri::command]
pub async fn local_server_status(app: tauri::AppHandle) -> Result<LocalStatus, String> {
    let home = nolune_home();
    let target = asset_name(std::env::consts::OS, std::env::consts::ARCH);
    let port = configured_port(&home);
    Ok(LocalStatus {
        supported: target.is_some(),
        target,
        home: home.display().to_string(),
        binary_installed: binary_path(&home).exists(),
        config_exists: home.join("config.toml").exists(),
        gateway_running: gateway_running(&app),
        port_in_use: port_is_listening(port),
        service_installed: service_installed(),
    })
}

#[tauri::command]
pub async fn install_local_server(
    app: tauri::AppHandle,
    channel: String,
) -> Result<InstallOutcome, String> {
    let asset = asset_name(std::env::consts::OS, std::env::consts::ARCH)
        .ok_or("Nolune does not publish a server for this computer yet.")?;
    let home = nolune_home();
    let binary = binary_path(&home);

    // Reinstalling over our own gateway is an upgrade; anything else on the port is not ours.
    if let Some(previous) = take_gateway(&app)? {
        let _ = previous.stop(SHUTDOWN_GRACE);
    }
    let port = configured_port(&home);
    if port_is_listening(port) {
        return Err(format!(
            "Something is already listening on port {port}. Stop it, or connect to it as an existing server."
        ));
    }

    let _ = app.emit("local-install-step", "downloading");
    let client = reqwest::Client::builder()
        .user_agent(format!("nolune-desktop/{}", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(15))
        .build()
        .map_err(|error| format!("cannot create download client: {error}"))?;
    let url = download_url(&channel, &asset, Some("nightly"));
    let progress_app = app.clone();
    download_binary(&client, &url, &binary, move |downloaded, total| {
        let _ = progress_app.emit(
            "local-install-progress",
            serde_json::json!({ "downloaded": downloaded, "total": total }),
        );
    })
    .await
    .map_err(|error| format!("{error}. Check that github.com is reachable and try again."))?;
    let version = resolve_version(&url).await.unwrap_or_else(|| {
        if channel == "nightly" {
            "nightly".into()
        } else {
            "latest".into()
        }
    });
    let _ = std::fs::write(home.join("bin").join(".version"), &version);

    let _ = app.emit("local-install-step", "preparing");
    let info = {
        let (binary, home) = (binary.clone(), home.clone());
        tokio::task::spawn_blocking(move || run_onboard(&binary, &home))
            .await
            .map_err(|error| format!("onboard task failed: {error}"))??
    };

    let _ = app.emit("local-install-step", "starting");
    let ready_url = start_gateway(&app, binary, home, port_of(&info.url)).await?;
    if let Err(error) = save_local_connection(&info).await {
        shutdown(&app);
        return Err(format!(
            "Nolune started but the connection could not be saved: {error}"
        ));
    }
    Ok(InstallOutcome {
        url: ready_url,
        version,
    })
}

#[tauri::command]
pub async fn start_local_gateway(app: tauri::AppHandle) -> Result<String, String> {
    let home = nolune_home();
    let binary = binary_path(&home);
    if !binary.exists() || !home.join("config.toml").exists() {
        return Err("Nolune is not installed on this computer.".into());
    }
    let port = configured_port(&home);
    let url = format!("http://localhost:{port}");
    if gateway_running(&app) || port_is_listening(port) {
        // Ours from earlier, or a service / foreground gateway the user runs; nothing to start.
        return Ok(url);
    }
    // The connection may have been forgotten since install; onboard is idempotent and
    // returns the token so it can be saved again without the user reading config.toml.
    let info = {
        let (binary, home) = (binary.clone(), home.clone());
        tokio::task::spawn_blocking(move || run_onboard(&binary, &home))
            .await
            .map_err(|error| format!("onboard task failed: {error}"))??
    };
    let ready_url = start_gateway(&app, binary, home, port).await?;
    if crate::credentials::read().await?.is_none() {
        save_local_connection(&info).await?;
    }
    Ok(ready_url)
}

#[tauri::command]
pub async fn stop_local_gateway(app: tauri::AppHandle) -> Result<(), String> {
    match take_gateway(&app)? {
        Some(gateway) => gateway.stop(SHUTDOWN_GRACE),
        None => Ok(()),
    }
}

// ── Background service (#129) ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct BackgroundStatus {
    pub supported: bool,
    pub installed: bool,
    pub running: bool,
    pub managed_by_app: bool,
}

/// Where the server's `nolune gateway install` writes its definition. This mirrors
/// `server/src/service.rs`; the app only ever reads it to know which mode is active.
pub fn service_definition_path(home_dir: &Path) -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        Some(home_dir.join("Library/LaunchAgents/dev.nolune.nolune.plist"))
    } else if cfg!(target_os = "linux") {
        Some(home_dir.join(".config/systemd/user/nolune.service"))
    } else {
        None
    }
}

pub fn service_installed() -> bool {
    dirs::home_dir()
        .and_then(|home| service_definition_path(&home))
        .is_some_and(|path| path.exists())
}

/// Run `nolune gateway <action>` for `home` and return its stdout. Failure carries stderr.
pub fn run_gateway_service(binary: &Path, home: &Path, action: &str) -> Result<String, String> {
    let output = Command::new(binary)
        .args(["gateway", action])
        .env("NOLUNE_HOME", home)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| io_err(&format!("cannot run nolune gateway {action}"), error))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        return Err(format!(
            "nolune gateway {action} failed ({}): {detail}",
            output.status
        ));
    }
    Ok(stdout)
}

fn background_status(app: &tauri::AppHandle) -> BackgroundStatus {
    let home = nolune_home();
    BackgroundStatus {
        supported: service_definition_path(Path::new("/")).is_some(),
        installed: service_installed(),
        running: port_is_listening(configured_port(&home)),
        managed_by_app: gateway_running(app),
    }
}

/// Poll until nothing listens on `port` or `timeout` passes.
fn wait_port_free(port: u16, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while port_is_listening(port) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(250));
    }
}

#[tauri::command]
pub async fn background_service_status(app: tauri::AppHandle) -> Result<BackgroundStatus, String> {
    Ok(background_status(&app))
}

/// Hand the gateway to the user-level service (`nolune gateway install`) or take it back
/// (`nolune gateway uninstall`, then an app-managed child again). Both are user-level; no
/// elevated privileges are involved.
#[tauri::command]
pub async fn set_background_service(
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<BackgroundStatus, String> {
    if service_definition_path(Path::new("/")).is_none() {
        return Err("Running in the background is not available on this platform yet.".into());
    }
    let home = nolune_home();
    let binary = binary_path(&home);
    if !binary.exists() || !home.join("config.toml").exists() {
        return Err("Nolune is not installed on this computer.".into());
    }
    let port = configured_port(&home);
    let run = |action: &'static str| {
        let (binary, home) = (binary.clone(), home.clone());
        async move {
            tokio::task::spawn_blocking(move || run_gateway_service(&binary, &home, action))
                .await
                .map_err(|error| format!("gateway {action} task failed: {error}"))?
        }
    };
    if enabled {
        // The service cannot bind the port while our child holds it.
        if let Some(previous) = take_gateway(&app)? {
            let _ = previous.stop(SHUTDOWN_GRACE);
        }
        if let Err(error) = run("install").await {
            // Do not leave the user without a server: resume the app-managed gateway.
            let _ = start_gateway(&app, binary, home, port).await;
            return Err(error);
        }
    } else {
        run("uninstall").await?;
        let free_port = port;
        tokio::task::spawn_blocking(move || wait_port_free(free_port, Duration::from_secs(10)))
            .await
            .map_err(|error| format!("wait task failed: {error}"))?;
        start_gateway(&app, binary, home, port).await?;
    }
    Ok(background_status(&app))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, net::TcpListener};

    #[test]
    fn asset_names_match_the_release_matrix() {
        assert_eq!(
            asset_name("macos", "aarch64").as_deref(),
            Some("nolune-server-aarch64-apple-darwin")
        );
        assert_eq!(
            asset_name("macos", "x86_64").as_deref(),
            Some("nolune-server-x86_64-apple-darwin")
        );
        assert_eq!(
            asset_name("linux", "x86_64").as_deref(),
            Some("nolune-server-x86_64-unknown-linux-gnu")
        );
        assert_eq!(
            asset_name("linux", "aarch64").as_deref(),
            Some("nolune-server-aarch64-unknown-linux-gnu")
        );
        assert_eq!(
            asset_name("windows", "x86_64").as_deref(),
            Some("nolune-server-x86_64-pc-windows-msvc.exe")
        );
        assert_eq!(asset_name("windows", "aarch64"), None);
        assert_eq!(asset_name("freebsd", "x86_64"), None);
    }

    #[test]
    fn download_urls_follow_the_installer_contract() {
        assert_eq!(
            download_url("stable", "nolune-server-aarch64-apple-darwin", None),
            "https://github.com/triangle-int/nolune/releases/latest/download/nolune-server-aarch64-apple-darwin"
        );
        assert_eq!(
            download_url("nightly", "nolune-server-aarch64-apple-darwin", Some("nightly")),
            "https://github.com/triangle-int/nolune/releases/download/nightly/nolune-server-aarch64-apple-darwin"
        );
    }

    #[test]
    fn binary_lives_in_bin_under_home() {
        let path = binary_path(Path::new("/Users/me/.nolune"));
        let expected = if cfg!(windows) {
            "nolune.exe"
        } else {
            "nolune"
        };
        assert_eq!(path, Path::new("/Users/me/.nolune/bin").join(expected));
    }

    #[test]
    fn configured_port_reads_the_binary_written_config() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(configured_port(tmp.path()), DEFAULT_PORT);
        fs::write(
            tmp.path().join("config.toml"),
            "host = \"0.0.0.0\"\nport = 4242 # custom\n[llm]\nport = 9\n",
        )
        .unwrap();
        assert_eq!(configured_port(tmp.path()), 4242);
    }

    #[test]
    fn onboard_output_takes_the_last_json_line() {
        let out = "workspace ready at /x\n{\"dir\":\"/x\",\"config_path\":\"/x/config.toml\",\"url\":\"http://localhost:26559\",\"token\":\"abc\",\"created_config\":true,\"generated_token\":true}\n";
        assert_eq!(
            parse_onboard_output(out).unwrap(),
            OnboardInfo {
                url: "http://localhost:26559".into(),
                token: "abc".into()
            }
        );
        assert!(parse_onboard_output("").is_err());
        assert!(parse_onboard_output("not json\n").is_err());
        assert!(
            parse_onboard_output("{\"url\":\"http://localhost:26559\"}\n").is_err(),
            "token is required"
        );
    }

    #[test]
    fn ready_line_yields_its_url() {
        assert_eq!(
            ready_url("nolune: ready http://localhost:26559").as_deref(),
            Some("http://localhost:26559")
        );
        assert_eq!(
            ready_url("  nolune: ready http://localhost:1234  ").as_deref(),
            Some("http://localhost:1234")
        );
        assert_eq!(ready_url("INFO starting server"), None);
        assert_eq!(ready_url("nolune: ready"), None);
    }

    #[test]
    fn port_probe_distinguishes_bound_from_free() {
        // Other tests in this binary bind 127.0.0.1:0 concurrently, so a
        // just-released ephemeral port can be taken again before the probe
        // runs. Retry with a fresh port instead of asserting on one sample.
        for _ in 0..5 {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            assert!(port_is_listening(port));
            drop(listener);
            if !port_is_listening(port) {
                return;
            }
        }
        panic!("a released port still reported as listening after five attempts");
    }

    #[tokio::test]
    async fn download_streams_to_an_executable_file_with_progress() {
        use axum::{routing::get, Router};
        let body: Vec<u8> = (0..(64 * 1024)).map(|i| (i % 251) as u8).collect();
        let served = body.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Router::new()
            .route(
                "/ok",
                get(move || {
                    let b = served.clone();
                    async move { b }
                }),
            )
            .route(
                "/missing",
                get(|| async { (axum::http::StatusCode::NOT_FOUND, "nope") }),
            );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("bin/nolune");
        let client = reqwest::Client::new();
        let mut seen: Vec<(u64, Option<u64>)> = Vec::new();
        download_binary(&client, &format!("http://{addr}/ok"), &dest, |d, t| {
            seen.push((d, t))
        })
        .await
        .unwrap();
        assert_eq!(fs::read(&dest).unwrap(), body);
        assert_eq!(seen.last().unwrap().0, body.len() as u64);
        assert_eq!(seen.last().unwrap().1, Some(body.len() as u64));
        assert!(
            !tmp.path().join("bin/nolune.tmp").exists(),
            "temp file must be renamed away"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_ne!(
                fs::metadata(&dest).unwrap().permissions().mode() & 0o111,
                0,
                "must be executable"
            );
        }

        let missing = tmp.path().join("bin/missing");
        let err = download_binary(
            &client,
            &format!("http://{addr}/missing"),
            &missing,
            |_, _| {},
        )
        .await
        .unwrap_err();
        assert!(err.contains("404"), "{err}");
        assert!(!missing.exists() && !tmp.path().join("bin/missing.tmp").exists());
        server.abort();
    }

    #[cfg(unix)]
    fn fake_binary(dir: &Path, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        fs::create_dir_all(dir).unwrap();
        let path = dir.join("nolune");
        fs::write(&path, format!("#!/bin/bash\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[cfg(unix)]
    #[test]
    fn onboard_runs_with_home_and_parses_json() {
        let tmp = tempfile::tempdir().unwrap();
        let binary = fake_binary(
            tmp.path(),
            r#"[[ "$1" = onboard && "$2" = --json ]] || exit 9
echo "progress line"
printf '{"dir":"%s","config_path":"%s/config.toml","url":"http://localhost:4242","token":"tok","created_config":true,"generated_token":true}\n' "$NOLUNE_HOME" "$NOLUNE_HOME""#,
        );
        let home = tmp.path().join("home");
        let info = run_onboard(&binary, &home).unwrap();
        assert_eq!(info.url, "http://localhost:4242");
        assert_eq!(info.token, "tok");

        let failing = fake_binary(
            &tmp.path().join("f"),
            "echo 'cannot create dir' >&2; exit 1",
        );
        let err = run_onboard(&failing, &home).unwrap_err();
        assert!(err.contains("cannot create dir"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn gateway_reports_ready_streams_lines_and_stops_on_request() {
        let tmp = tempfile::tempdir().unwrap();
        let binary = fake_binary(
            tmp.path(),
            r#"[[ "$1" = gateway ]] || exit 9
echo "INFO starting on $NOLUNE_HOME" >&2
echo "nolune: ready http://localhost:1"
trap 'exit 0' TERM
while true; do sleep 0.1; done"#,
        );
        let (tx, rx) = mpsc::channel::<String>();
        let mut gateway = spawn_gateway(&binary, &tmp.path().join("home"), move |l| {
            tx.send(l).ok();
        })
        .unwrap();
        // Port 1 is never listening, so readiness must come from the line.
        let url = gateway.wait_ready(1, Duration::from_secs(10)).unwrap();
        assert_eq!(url, "http://localhost:1");
        assert!(gateway.is_running());
        thread::sleep(Duration::from_millis(100));
        let lines: Vec<String> = rx.try_iter().collect();
        assert!(
            lines.iter().any(|l| l.contains("INFO starting on")),
            "{lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|l| l == "nolune: ready http://localhost:1"),
            "{lines:?}"
        );

        let started = std::time::Instant::now();
        gateway.stop(Duration::from_secs(5)).unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "SIGTERM should be honoured before the kill fallback"
        );
    }

    #[cfg(unix)]
    #[test]
    fn gateway_that_exits_early_fails_wait_with_its_last_output() {
        let tmp = tempfile::tempdir().unwrap();
        let binary = fake_binary(
            tmp.path(),
            "echo 'port 26559 is already in use' >&2; exit 1",
        );
        let mut gateway = spawn_gateway(&binary, &tmp.path().join("home"), |_| {}).unwrap();
        let err = gateway.wait_ready(1, Duration::from_secs(10)).unwrap_err();
        assert!(err.contains("already in use"), "{err}");
        assert!(!gateway.is_running());
    }

    #[cfg(unix)]
    #[test]
    fn stop_kills_a_gateway_that_ignores_sigterm() {
        let tmp = tempfile::tempdir().unwrap();
        let binary = fake_binary(
            tmp.path(),
            "trap '' TERM\necho 'nolune: ready http://localhost:1'\nwhile true; do sleep 0.1; done",
        );
        let mut gateway = spawn_gateway(&binary, &tmp.path().join("home"), |_| {}).unwrap();
        gateway.wait_ready(1, Duration::from_secs(10)).unwrap();
        let pid = gateway.id();
        gateway.stop(Duration::from_millis(300)).unwrap();
        // The process must be gone after the kill fallback.
        let alive = unsafe { libc::kill(pid as i32, 0) } == 0;
        assert!(!alive, "process {pid} still alive after stop");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn service_definition_is_the_launch_agent() {
        assert_eq!(
            service_definition_path(Path::new("/Users/me")),
            Some(PathBuf::from(
                "/Users/me/Library/LaunchAgents/dev.nolune.nolune.plist"
            ))
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn service_definition_is_the_user_unit() {
        assert_eq!(
            service_definition_path(Path::new("/home/me")),
            Some(PathBuf::from(
                "/home/me/.config/systemd/user/nolune.service"
            ))
        );
    }

    #[cfg(unix)]
    #[test]
    fn gateway_service_actions_run_the_binary_with_home() {
        let tmp = tempfile::tempdir().unwrap();
        let binary = fake_binary(
            tmp.path(),
            r#"[[ "$1" = gateway ]] || exit 9
echo "gateway $2 home=$NOLUNE_HOME"
if [[ "$2" = uninstall ]]; then echo "launchctl bootout failed" >&2; exit 1; fi"#,
        );
        let home = tmp.path().join("home");
        let out = run_gateway_service(&binary, &home, "install").unwrap();
        assert!(
            out.contains(&format!("gateway install home={}", home.display())),
            "{out}"
        );
        let err = run_gateway_service(&binary, &home, "uninstall").unwrap_err();
        assert!(err.contains("launchctl bootout failed"), "{err}");
    }
}
