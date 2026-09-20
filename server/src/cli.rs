use std::{
    io::{self, IsTerminal},
    path::PathBuf,
    process::Command,
};

use clap::{Parser, Subcommand};

use crate::{config, onboard, service, uninstall};

#[derive(Parser)]
#[command(
    name = "nolune",
    about = "Nolune — AI companion",
    version = env!("CARGO_PKG_VERSION"),
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<CliCommand>,
}

#[derive(Subcommand)]
pub enum CliCommand {
    /// Alias for `gateway start` (hidden; kept for existing scripts)
    #[command(hide = true)]
    Start,
    /// Alias for `gateway stop`
    #[command(hide = true)]
    Stop,
    /// Alias for `gateway restart`
    #[command(hide = true)]
    Restart,
    /// Alias for `gateway status`
    #[command(hide = true)]
    Status,
    /// Alias for `gateway logs`
    #[command(hide = true)]
    Logs,
    /// Print version
    Version,
    /// Create a one-time code so a browser can sign in to this server
    Pair,
    /// Prepare ~/.nolune (config, data directories, auth token) without starting anything
    Onboard {
        /// Print the outcome as one JSON line instead of human-readable progress
        #[arg(long)]
        json: bool,
    },
    /// Run the server in the foreground, or manage the optional background service
    Gateway {
        #[command(subcommand)]
        action: Option<GatewayAction>,
    },
    /// Remove the background service and installed files; keeps data only with --keep-data
    Uninstall {
        /// Keep ~/.nolune (config, memory, chats); remove only the service, binary, and log
        #[arg(long)]
        keep_data: bool,
        /// Delete data without asking (required when not running in a terminal)
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

#[derive(Subcommand)]
pub enum GatewayAction {
    /// Register the gateway as a user-level background service and start it
    Install,
    /// Stop and remove the background service (data is untouched)
    Uninstall,
    /// Start the background service
    Start,
    /// Stop the background service
    Stop,
    /// Restart the background service
    Restart,
    /// Show whether the service is installed and running
    Status,
    /// Stream service logs
    Logs,
}

pub fn run(cmd: CliCommand) -> i32 {
    match cmd {
        CliCommand::Start => gateway(GatewayAction::Start),
        CliCommand::Stop => gateway(GatewayAction::Stop),
        CliCommand::Restart => gateway(GatewayAction::Restart),
        CliCommand::Status => gateway(GatewayAction::Status),
        CliCommand::Logs => gateway(GatewayAction::Logs),
        CliCommand::Gateway {
            action: Some(action),
        } => gateway(action),
        // main runs the server for this variant before ever reaching here.
        CliCommand::Gateway { action: None } => unreachable!("gateway is run by main"),
        CliCommand::Uninstall { keep_data, yes } => uninstall_cmd(keep_data, yes),
        CliCommand::Version => {
            println!("nolune {}", env!("CARGO_PKG_VERSION"));
            0
        }
        CliCommand::Pair => pair(),
        CliCommand::Onboard { json } => onboard_cmd(json),
    }
}

fn gateway(action: GatewayAction) -> i32 {
    if !cfg!(any(target_os = "macos", target_os = "linux")) {
        eprintln!(
            "background service management is not supported on this platform yet; run `nolune gateway` in the foreground"
        );
        return 1;
    }
    match action {
        GatewayAction::Install => gateway_install(),
        GatewayAction::Uninstall => gateway_uninstall(),
        GatewayAction::Start => svc_start(),
        GatewayAction::Stop => svc_stop(),
        GatewayAction::Restart => {
            svc_stop();
            svc_start()
        }
        GatewayAction::Status => svc_status(),
        GatewayAction::Logs => svc_logs(),
    }
}

// ── Background service (#125) ───────────────────────────────────────────

fn home_dir() -> PathBuf {
    dirs::home_dir().expect("cannot resolve home directory")
}

fn definition_path() -> PathBuf {
    service::definition_path(&home_dir(), config::DEFAULT_PROFILE)
}

/// The port from config.toml, without loading the whole config (which would create one).
fn configured_port(home: &std::path::Path) -> u16 {
    std::fs::read_to_string(home.join("config.toml"))
        .ok()
        .and_then(|raw| toml::from_str::<toml::Value>(&raw).ok())
        .and_then(|doc| doc.get("port").and_then(toml::Value::as_integer))
        .and_then(|port| u16::try_from(port).ok())
        .unwrap_or(onboard::DEFAULT_PORT)
}

fn gateway_install() -> i32 {
    let home = config::workspace_root();
    if !home.join("config.toml").exists() {
        eprintln!(
            "no config at {}; run `nolune onboard` first",
            home.join("config.toml").display()
        );
        return 1;
    }
    let definition = definition_path();
    let port = configured_port(&home);
    let upgrade = definition.exists();
    if !upgrade && service::port_is_listening(port) {
        eprintln!(
            "something is already listening on port {port}, probably a foreground `nolune gateway`. \
             Stop it, then run `nolune gateway install` again."
        );
        return 1;
    }
    let binary = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("cannot determine the nolune binary path: {error}");
            return 1;
        }
    };
    let spec = service::ServiceSpec {
        binary,
        home: home.clone(),
        profile: config::DEFAULT_PROFILE.to_owned(),
    };
    if upgrade {
        // Reinstall in place: stop the old definition before overwriting it.
        platform_stop_quietly();
    }
    let contents = if cfg!(target_os = "macos") {
        service::render_launchd_plist(&spec)
    } else {
        service::render_systemd_unit(&spec)
    };
    if let Err(error) = service::write_definition(&definition, &contents) {
        eprintln!("cannot write {}: {error}", definition.display());
        return 1;
    }
    let code = platform_install_start(&definition);
    if code != 0 {
        return code;
    }
    println!(
        "background service {} and started (definition: {})",
        if upgrade { "updated" } else { "installed" },
        definition.display()
    );
    println!("  status: nolune gateway status");
    println!("  logs:   nolune gateway logs");
    println!("  remove: nolune gateway uninstall");
    0
}

fn gateway_uninstall() -> i32 {
    let definition = definition_path();
    if !definition.exists() {
        println!("no background service is installed");
        return 0;
    }
    platform_stop_quietly();
    if let Err(error) = std::fs::remove_file(&definition) {
        eprintln!("cannot remove {}: {error}", definition.display());
        return 1;
    }
    platform_after_remove();
    println!(
        "background service removed; data at {} is untouched",
        config::workspace_root().display()
    );
    0
}

// ── Uninstall (#126) ────────────────────────────────────────────────────

fn uninstall_cmd(keep_data: bool, yes: bool) -> i32 {
    let home = config::workspace_root();
    let definition = definition_path();
    if !home.exists() && !definition.exists() {
        println!("nothing to uninstall: {} does not exist", home.display());
        return 0;
    }
    if !keep_data && !yes && !confirm_delete(&home) {
        return 1;
    }
    if definition.exists() {
        let code = gateway_uninstall();
        if code != 0 {
            return code;
        }
    }
    let report = match uninstall::remove_files(&home, keep_data) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("uninstall failed: {error}");
            return 1;
        }
    };
    for path in &report.removed {
        println!("removed {}", path.display());
    }
    if let Some(kept) = &report.kept {
        println!("kept {} (config, memory, chats)", kept.display());
        println!("  to remove it later: nolune uninstall --yes (or delete the directory)");
    }
    if service::port_is_listening(configured_port(&home)) {
        eprintln!(
            "note: something is still listening on port {}; a foreground `nolune gateway` may still be running",
            configured_port(&home)
        );
    }
    0
}

/// Ask before deleting data. Non-interactive callers must pass `--yes` explicitly.
fn confirm_delete(home: &std::path::Path) -> bool {
    if !io::stdin().is_terminal() {
        eprintln!(
            "refusing to delete {} without confirmation: pass --yes to delete data, or --keep-data to keep it",
            home.display()
        );
        return false;
    }
    eprint!(
        "This deletes everything in {} (config, memory, chats). Continue? [y/N] ",
        home.display()
    );
    let mut answer = String::new();
    if io::stdin().read_line(&mut answer).is_err() {
        return false;
    }
    if matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        true
    } else {
        eprintln!("aborted; nothing was removed");
        false
    }
}

fn onboard_cmd(json: bool) -> i32 {
    let dir = config::workspace_root();
    let outcome = match crate::onboard::onboard(&dir, config::DEFAULT_PROFILE, None) {
        Ok(outcome) => outcome,
        Err(error) => {
            eprintln!("onboard failed: {error:#}");
            return 1;
        }
    };
    if json {
        // One machine-readable line; the token appears here and nowhere else.
        return match serde_json::to_string(&outcome) {
            Ok(line) => {
                println!("{line}");
                0
            }
            Err(error) => {
                eprintln!("cannot encode onboard outcome: {error}");
                1
            }
        };
    }
    println!("workspace ready at {}", outcome.dir.display());
    if outcome.created_config {
        println!("created {}", outcome.config_path.display());
    } else {
        println!("kept existing {}", outcome.config_path.display());
    }
    if outcome.generated_token {
        println!("generated an authentication token (saved in config.toml)");
    }
    println!(
        "next: run `nolune gateway` to start the server, then open {}",
        outcome.url
    );
    0
}

// ── Browser pairing ─────────────────────────────────────────────────────

/// Ask the running server for a pairing code and print it. This is the local
/// owner surface for #112: the code is short-lived and single-use, and the
/// API token itself never leaves this process.
fn pair() -> i32 {
    let config = match config::load_config() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("cannot read {}: {error}", config::config_path().display());
            return 1;
        }
    };
    if config.auth_token.is_empty() {
        println!(
            "Authentication is disabled (auth_token is empty in {}), so browsers can open Nolune without pairing.",
            config::config_path().display()
        );
        return 0;
    }

    let connect_host = match config.host.as_str() {
        "" | "0.0.0.0" => "127.0.0.1".to_string(),
        "::" | "[::]" => "[::1]".to_string(),
        other => other.to_string(),
    };
    let url = format!("http://{connect_host}:{}/api/session/pairing", config.port);
    let open_url = if config.public_url.is_empty() {
        format!("http://localhost:{}", config.port)
    } else {
        config.public_url.clone()
    };

    // `main` is already inside the tokio runtime, so drive this request on a
    // fresh thread with its own runtime.
    let auth_token = config.auth_token.clone();
    let request_url = url.clone();
    let response = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        runtime.block_on(async {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .map_err(|e| e.to_string())?;
            let response = client
                .post(&request_url)
                .bearer_auth(&auth_token)
                .json(&serde_json::json!({ "source": "cli" }))
                .send()
                .await
                .map_err(|e| e.to_string())?;
            let status = response.status();
            let body: serde_json::Value = response.json().await.unwrap_or_default();
            Ok::<_, String>((status, body))
        })
    })
    .join()
    .unwrap_or_else(|_| Err("pairing thread panicked".to_string()));

    match response {
        Err(error) => {
            eprintln!(
                "Nolune is not reachable at {url} ({error}).
Start it with `nolune gateway` and try again."
            );
            1
        }
        Ok((status, _)) if status == reqwest::StatusCode::UNAUTHORIZED => {
            eprintln!(
                "The running server rejected the token from {}.
If the service was started with NOLUNE_AUTH_TOKEN, run `nolune pair` with the same value.",
                config::config_path().display()
            );
            1
        }
        Ok((status, body)) if !status.is_success() => {
            let message = body["message"].as_str().unwrap_or("");
            eprintln!("pairing request failed: HTTP {status} {message}");
            1
        }
        Ok((_, body)) => {
            let Some(code) = body["code"].as_str() else {
                eprintln!("the server did not return a pairing code");
                return 1;
            };
            let minutes = body["expires_in_secs"].as_u64().unwrap_or(300) / 60;
            println!();
            println!("  Pairing code:  {code}");
            println!();
            println!("  Open {open_url} in the browser you want to connect and enter this code.");
            println!(
                "  It works once and expires in {minutes} minutes. Run `nolune pair` again for another browser."
            );
            println!();
            0
        }
    }
}

// ── macOS (launchd) ─────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn uid() -> String {
    let output = Command::new("id")
        .arg("-u")
        .output()
        .expect("failed to run `id`");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[cfg(target_os = "macos")]
fn launchd_target() -> String {
    format!("gui/{}/{}", uid(), service::LABEL)
}

#[cfg(target_os = "macos")]
fn platform_stop_quietly() {
    let _ = Command::new("launchctl")
        .args(["bootout", &launchd_target()])
        .output();
}

#[cfg(target_os = "macos")]
fn platform_after_remove() {}

#[cfg(target_os = "macos")]
fn platform_install_start(definition: &std::path::Path) -> i32 {
    let domain = format!("gui/{}", uid());
    let plist = definition.to_string_lossy();
    match Command::new("launchctl")
        .args(["bootstrap", &domain, &plist])
        .status()
    {
        Ok(status) if status.success() || status.code() == Some(37) => {}
        Ok(status) => {
            eprintln!(
                "launchctl bootstrap failed (exit {})",
                status.code().unwrap_or(-1)
            );
            return 1;
        }
        Err(error) => {
            eprintln!("failed to run launchctl: {error}");
            return 1;
        }
    }
    match Command::new("launchctl")
        .args(["kickstart", "-k", &launchd_target()])
        .status()
    {
        Ok(status) if status.success() => 0,
        Ok(status) => {
            eprintln!(
                "launchctl kickstart failed (exit {})",
                status.code().unwrap_or(-1)
            );
            1
        }
        Err(error) => {
            eprintln!("failed to run launchctl: {error}");
            1
        }
    }
}

#[cfg(target_os = "macos")]
fn svc_start() -> i32 {
    let plist = definition_path();
    if !plist.exists() {
        eprintln!("background service is not installed; run `nolune gateway install`");
        return 1;
    }
    let domain = format!("gui/{}", uid());
    let status = Command::new("launchctl")
        .args(["bootstrap", &domain, &plist.to_string_lossy()])
        .status();
    match status {
        Ok(s) if s.success() => {
            println!("Nolune service started.");
            0
        }
        // exit code 37 = already loaded
        Ok(s) if s.code() == Some(37) => {
            println!("Nolune service is already running.");
            0
        }
        Ok(s) => {
            eprintln!(
                "launchctl bootstrap failed (exit {}).",
                s.code().unwrap_or(-1)
            );
            1
        }
        Err(e) => {
            eprintln!("failed to run launchctl: {e}");
            1
        }
    }
}

#[cfg(target_os = "macos")]
fn svc_stop() -> i32 {
    let status = Command::new("launchctl")
        .args(["bootout", &launchd_target()])
        .status();
    match status {
        Ok(s) if s.success() => {
            println!("Nolune service stopped.");
            0
        }
        Ok(s) if s.code() == Some(3) => {
            println!("Nolune service is not running.");
            0
        }
        Ok(s) => {
            eprintln!(
                "launchctl bootout failed (exit {}).",
                s.code().unwrap_or(-1)
            );
            1
        }
        Err(e) => {
            eprintln!("failed to run launchctl: {e}");
            1
        }
    }
}

#[cfg(target_os = "macos")]
fn svc_status() -> i32 {
    if !definition_path().exists() {
        println!(
            "background service is not installed (run `nolune gateway` for the foreground, or `nolune gateway install`)"
        );
        return 0;
    }
    let output = Command::new("launchctl")
        .args(["print", &launchd_target()])
        .output();
    match output {
        Ok(out) => {
            let text = String::from_utf8_lossy(&out.stdout);
            if out.status.success() {
                let state = text
                    .lines()
                    .find(|l| l.trim().starts_with("state ="))
                    .map(|l| l.trim().trim_start_matches("state = "))
                    .unwrap_or("unknown");
                let pid = text
                    .lines()
                    .find(|l| l.trim().starts_with("pid ="))
                    .map(|l| l.trim().trim_start_matches("pid = "))
                    .unwrap_or("-");
                println!("Nolune is running (pid {pid}, state: {state})");
            } else {
                println!("Nolune service is installed but not running.");
            }
            0
        }
        Err(e) => {
            eprintln!("failed to run launchctl: {e}");
            1
        }
    }
}

#[cfg(target_os = "macos")]
fn svc_logs() -> i32 {
    let log_path = config::workspace_root().join("nolune.log");
    if !log_path.exists() {
        eprintln!("log file not found at {}", log_path.display());
        return 1;
    }
    let status = Command::new("tail")
        .args(["-f", &log_path.to_string_lossy()])
        .status();
    match status {
        Ok(_) => 0,
        Err(e) => {
            eprintln!("failed to tail logs: {e}");
            1
        }
    }
}

// ── Linux (systemd, user unit only) ─────────────────────────────────────

#[cfg(target_os = "linux")]
fn platform_stop_quietly() {
    let _ = Command::new("systemctl")
        .args(["--user", "disable", "--now", service::UNIT_NAME])
        .output();
}

#[cfg(target_os = "linux")]
fn platform_after_remove() {
    let _ = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output();
}

#[cfg(target_os = "linux")]
fn platform_install_start(_definition: &std::path::Path) -> i32 {
    let reload = run_systemctl(&["daemon-reload"], "reloaded");
    if reload != 0 {
        return reload;
    }
    run_systemctl(
        &["enable", "--now", service::UNIT_NAME],
        "enabled and started",
    )
}

#[cfg(target_os = "linux")]
fn svc_start() -> i32 {
    if !definition_path().exists() {
        eprintln!("background service is not installed; run `nolune gateway install`");
        return 1;
    }
    run_systemctl(&["start", service::UNIT_NAME], "started")
}

#[cfg(target_os = "linux")]
fn svc_stop() -> i32 {
    run_systemctl(&["stop", service::UNIT_NAME], "stopped")
}

#[cfg(target_os = "linux")]
fn svc_status() -> i32 {
    if !definition_path().exists() {
        println!(
            "background service is not installed (run `nolune gateway` for the foreground, or `nolune gateway install`)"
        );
        return 0;
    }
    let output = Command::new("systemctl")
        .args(["--user", "is-active", service::UNIT_NAME])
        .output();
    match output {
        Ok(out) => {
            let state = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if state == "active" {
                println!("Nolune is running.");
            } else {
                println!("Nolune service is installed but not running ({state}).");
            }
            0
        }
        Err(e) => {
            eprintln!("failed to run systemctl: {e}");
            1
        }
    }
}

#[cfg(target_os = "linux")]
fn svc_logs() -> i32 {
    let status = Command::new("journalctl")
        .args(["--user", "-u", service::UNIT_NAME, "-f", "--no-pager"])
        .status();
    match status {
        Ok(_) => 0,
        Err(e) => {
            eprintln!("failed to run journalctl: {e}");
            1
        }
    }
}

#[cfg(target_os = "linux")]
fn run_systemctl(args: &[&str], verb: &str) -> i32 {
    match Command::new("systemctl").arg("--user").args(args).status() {
        Ok(s) if s.success() => {
            println!("Nolune service {verb}.");
            0
        }
        Ok(s) => {
            eprintln!("systemctl --user failed (exit {}).", s.code().unwrap_or(-1));
            1
        }
        Err(e) => {
            eprintln!("failed to run systemctl: {e}");
            1
        }
    }
}

// ── Unsupported platform ────────────────────────────────────────────────

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn platform_stop_quietly() {}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn platform_after_remove() {}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn platform_install_start(_definition: &std::path::Path) -> i32 {
    1
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn svc_start() -> i32 {
    1
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn svc_stop() -> i32 {
    1
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn svc_status() -> i32 {
    1
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn svc_logs() -> i32 {
    1
}
