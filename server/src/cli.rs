use std::{
    io::{self, IsTerminal},
    path::PathBuf,
    process::Command,
};

use clap::{Parser, Subcommand};

use crate::{
    config::{self, Profile},
    onboard, profiles, service, uninstall,
};

#[derive(Parser)]
#[command(
    name = "nolune",
    about = "Nolune — AI companion",
    version = env!("CARGO_PKG_VERSION"),
)]
pub struct Cli {
    /// Address an isolated server profile: its own data root beside ~/.nolune, config,
    /// port, token, and background service. Omit for the default profile
    #[arg(long, global = true, value_name = "NAME")]
    pub profile: Option<String>,
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
        /// Listen on this port; a named profile otherwise picks a free one above 26559
        #[arg(long, value_name = "PORT")]
        port: Option<u16>,
    },
    /// Run the server in the foreground, or manage the optional background service
    Gateway {
        #[command(subcommand)]
        action: Option<GatewayAction>,
    },
    /// Remove the background service and installed files; keeps data only with --keep-data
    Uninstall {
        /// Keep the data root (config, memory, chats); remove only the service, binary, and log
        #[arg(long)]
        keep_data: bool,
        /// Delete data without asking (required when not running in a terminal)
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

#[derive(Subcommand)]
pub enum GatewayAction {
    /// Run the server in the foreground (the same as a bare `nolune gateway`)
    Run,
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

/// Run a subcommand for `profile`, whose root the entrypoint has already selected as the
/// process workspace, so `config::workspace_root()` and `config::config_path()` agree.
pub fn run(cmd: CliCommand, profile: &Profile) -> i32 {
    match cmd {
        CliCommand::Start => gateway(GatewayAction::Start, profile),
        CliCommand::Stop => gateway(GatewayAction::Stop, profile),
        CliCommand::Restart => gateway(GatewayAction::Restart, profile),
        CliCommand::Status => gateway(GatewayAction::Status, profile),
        CliCommand::Logs => gateway(GatewayAction::Logs, profile),
        // main runs the server for `gateway` and `gateway run` before ever reaching here.
        CliCommand::Gateway { action: None } => unreachable!("gateway is run by main"),
        CliCommand::Gateway {
            action: Some(action),
        } => gateway(action, profile),
        CliCommand::Uninstall { keep_data, yes } => uninstall_cmd(keep_data, yes, profile),
        CliCommand::Version => {
            println!("nolune {}", env!("CARGO_PKG_VERSION"));
            0
        }
        CliCommand::Pair => pair(profile),
        CliCommand::Onboard { json, port } => onboard_cmd(json, port, profile),
    }
}

fn gateway(action: GatewayAction, profile: &Profile) -> i32 {
    if !cfg!(any(target_os = "macos", target_os = "linux")) {
        eprintln!(
            "background service management is not supported on this platform yet; run `nolune gateway` in the foreground"
        );
        return 1;
    }
    match action {
        GatewayAction::Run => unreachable!("gateway run is run by main"),
        GatewayAction::Install => gateway_install(profile),
        GatewayAction::Uninstall => gateway_uninstall(profile),
        GatewayAction::Start => svc_start(profile),
        GatewayAction::Stop => svc_stop(profile),
        GatewayAction::Restart => {
            svc_stop(profile);
            svc_start(profile)
        }
        GatewayAction::Status => svc_status(profile),
        GatewayAction::Logs => svc_logs(profile),
    }
}

// ── Background service (#125) ───────────────────────────────────────────

fn home_dir() -> PathBuf {
    dirs::home_dir().expect("cannot resolve home directory")
}

fn definition_path(profile: &Profile) -> PathBuf {
    service::definition_path(&home_dir(), &profile.name)
}

/// How the profile appears in messages: the default one is just "Nolune".
fn display(profile: &Profile) -> String {
    if profile.is_default() {
        "Nolune".to_owned()
    } else {
        format!("Nolune profile {}", profile.name)
    }
}

/// The `--profile <name>` suffix for commands the user is told to run next.
fn profile_flag(profile: &Profile) -> String {
    if profile.is_default() {
        String::new()
    } else {
        format!(" --profile {}", profile.name)
    }
}

/// The port from config.toml, without loading the whole config (which would create one).
fn configured_port(home: &std::path::Path) -> u16 {
    profiles::configured_port(home).unwrap_or(onboard::DEFAULT_PORT)
}

fn gateway_install(profile: &Profile) -> i32 {
    let home = &profile.root;
    let flag = profile_flag(profile);
    if !home.join("config.toml").exists() {
        eprintln!(
            "no config at {}; run `nolune onboard{flag}` first",
            home.join("config.toml").display()
        );
        return 1;
    }
    let definition = definition_path(profile);
    let port = configured_port(home);
    // Another profile on this host must never share a port, a data root, or a service.
    let collisions = profiles::collisions(&home_dir(), profile, port);
    if !collisions.is_empty() {
        for collision in &collisions {
            eprintln!("{collision}");
        }
        eprintln!("nothing was installed");
        return 1;
    }
    let upgrade = definition.exists();
    if !upgrade && service::port_is_listening(port) {
        eprintln!(
            "something is already listening on port {port}, probably a foreground `nolune gateway`. \
             Stop it, then run `nolune gateway install{flag}` again."
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
        profile: profile.name.clone(),
    };
    if upgrade {
        // Reinstall in place: stop the old definition before overwriting it.
        platform_stop_quietly(profile);
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
    let code = platform_install_start(&definition, profile);
    if code != 0 {
        return code;
    }
    println!(
        "background service {}{} and started (definition: {})",
        if upgrade { "updated" } else { "installed" },
        if profile.is_default() {
            String::new()
        } else {
            format!(" for profile {}", profile.name)
        },
        definition.display()
    );
    println!("  status: nolune gateway status{flag}");
    println!("  logs:   nolune gateway logs{flag}");
    println!("  remove: nolune gateway uninstall{flag}");
    0
}

fn gateway_uninstall(profile: &Profile) -> i32 {
    let definition = definition_path(profile);
    if !definition.exists() {
        println!(
            "no background service is installed{}",
            if profile.is_default() {
                String::new()
            } else {
                format!(" for profile {}", profile.name)
            }
        );
        return 0;
    }
    platform_stop_quietly(profile);
    if let Err(error) = std::fs::remove_file(&definition) {
        eprintln!("cannot remove {}: {error}", definition.display());
        return 1;
    }
    platform_after_remove();
    println!(
        "background service{} removed; data at {} is untouched",
        if profile.is_default() {
            String::new()
        } else {
            format!(" of profile {}", profile.name)
        },
        profile.root.display()
    );
    0
}

// ── Uninstall (#126) ────────────────────────────────────────────────────

fn uninstall_cmd(keep_data: bool, yes: bool, profile: &Profile) -> i32 {
    let home = &profile.root;
    let definition = definition_path(profile);
    if !home.exists() && !definition.exists() {
        println!("nothing to uninstall: {} does not exist", home.display());
        return 0;
    }
    if !keep_data && !yes && !confirm_delete(home) {
        return 1;
    }
    if definition.exists() {
        let code = gateway_uninstall(profile);
        if code != 0 {
            return code;
        }
    }
    let report = match uninstall::remove_files(home, keep_data) {
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
        println!(
            "  to remove it later: nolune uninstall --yes{} (or delete the directory)",
            profile_flag(profile)
        );
    }
    if service::port_is_listening(configured_port(home)) {
        eprintln!(
            "note: something is still listening on port {}; a foreground `nolune gateway{}` may still be running",
            configured_port(home),
            profile_flag(profile)
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

fn onboard_cmd(json: bool, port: Option<u16>, profile: &Profile) -> i32 {
    let dir = &profile.root;
    // A named profile must not take the default port or one a sibling already uses; the
    // default profile keeps 26559 so installers and the desktop app find it.
    let fresh = !dir.join("config.toml").exists();
    let port = match port {
        Some(port) => Some(port),
        None if fresh && !profile.is_default() => {
            let taken: Vec<u16> = profiles::siblings(&home_dir())
                .iter()
                .filter_map(|sibling| profiles::configured_port(&sibling.root))
                .collect();
            match profiles::pick_free_port(&taken, service::port_is_listening) {
                Some(port) => Some(port),
                None => {
                    eprintln!(
                        "no free port found for profile {}; pass one with --port",
                        profile.name
                    );
                    return 1;
                }
            }
        }
        None => None,
    };
    let outcome = match crate::onboard::onboard(dir, &profile.name, port) {
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
        "next: run `nolune gateway{}` to start the server, then open {}",
        profile_flag(profile),
        outcome.url
    );
    0
}

// ── Browser pairing ─────────────────────────────────────────────────────

/// Ask the running server for a pairing code and print it. This is the local
/// owner surface for #112: the code is short-lived and single-use, and the
/// API token itself never leaves this process.
fn pair(profile: &Profile) -> i32 {
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

    let flag = profile_flag(profile);
    match response {
        Err(error) => {
            eprintln!(
                "{} is not reachable at {url} ({error}).
Start it with `nolune gateway{flag}` and try again.",
                display(profile)
            );
            1
        }
        Ok((status, _)) if status == reqwest::StatusCode::UNAUTHORIZED => {
            eprintln!(
                "The running server rejected the token from {}.
If the service was started with NOLUNE_AUTH_TOKEN, run `nolune pair{flag}` with the same value.",
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
                "  It works once and expires in {minutes} minutes. Run `nolune pair{flag}` again for another browser."
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
fn launchd_target(profile: &Profile) -> String {
    format!("gui/{}/{}", uid(), service::label(&profile.name))
}

#[cfg(target_os = "macos")]
fn platform_stop_quietly(profile: &Profile) {
    let _ = Command::new("launchctl")
        .args(["bootout", &launchd_target(profile)])
        .output();
}

#[cfg(target_os = "macos")]
fn platform_after_remove() {}

#[cfg(target_os = "macos")]
fn platform_install_start(definition: &std::path::Path, profile: &Profile) -> i32 {
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
        .args(["kickstart", "-k", &launchd_target(profile)])
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
fn svc_start(profile: &Profile) -> i32 {
    let plist = definition_path(profile);
    if !plist.exists() {
        eprintln!(
            "background service is not installed; run `nolune gateway install{}`",
            profile_flag(profile)
        );
        return 1;
    }
    let domain = format!("gui/{}", uid());
    let status = Command::new("launchctl")
        .args(["bootstrap", &domain, &plist.to_string_lossy()])
        .status();
    match status {
        Ok(s) if s.success() => {
            println!("{} service started.", display(profile));
            0
        }
        // exit code 37 = already loaded
        Ok(s) if s.code() == Some(37) => {
            println!("{} service is already running.", display(profile));
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
fn svc_stop(profile: &Profile) -> i32 {
    let status = Command::new("launchctl")
        .args(["bootout", &launchd_target(profile)])
        .status();
    match status {
        Ok(s) if s.success() => {
            println!("{} service stopped.", display(profile));
            0
        }
        Ok(s) if s.code() == Some(3) => {
            println!("{} service is not running.", display(profile));
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
fn svc_status(profile: &Profile) -> i32 {
    if !definition_path(profile).exists() {
        let flag = profile_flag(profile);
        println!(
            "background service is not installed (run `nolune gateway{flag}` for the foreground, or `nolune gateway install{flag}`)"
        );
        return 0;
    }
    let output = Command::new("launchctl")
        .args(["print", &launchd_target(profile)])
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
                println!(
                    "{} is running (pid {pid}, state: {state})",
                    display(profile)
                );
            } else {
                println!("{} service is installed but not running.", display(profile));
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
fn svc_logs(profile: &Profile) -> i32 {
    let log_path = profile.root.join("nolune.log");
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
fn platform_stop_quietly(profile: &Profile) {
    let _ = Command::new("systemctl")
        .args([
            "--user",
            "disable",
            "--now",
            &service::unit_name(&profile.name),
        ])
        .output();
}

#[cfg(target_os = "linux")]
fn platform_after_remove() {
    let _ = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output();
}

#[cfg(target_os = "linux")]
fn platform_install_start(_definition: &std::path::Path, profile: &Profile) -> i32 {
    let reload = run_systemctl(&["daemon-reload"], "reloaded", profile);
    if reload != 0 {
        return reload;
    }
    run_systemctl(
        &["enable", "--now", &service::unit_name(&profile.name)],
        "enabled and started",
        profile,
    )
}

#[cfg(target_os = "linux")]
fn svc_start(profile: &Profile) -> i32 {
    if !definition_path(profile).exists() {
        eprintln!(
            "background service is not installed; run `nolune gateway install{}`",
            profile_flag(profile)
        );
        return 1;
    }
    run_systemctl(
        &["start", &service::unit_name(&profile.name)],
        "started",
        profile,
    )
}

#[cfg(target_os = "linux")]
fn svc_stop(profile: &Profile) -> i32 {
    run_systemctl(
        &["stop", &service::unit_name(&profile.name)],
        "stopped",
        profile,
    )
}

#[cfg(target_os = "linux")]
fn svc_status(profile: &Profile) -> i32 {
    if !definition_path(profile).exists() {
        let flag = profile_flag(profile);
        println!(
            "background service is not installed (run `nolune gateway{flag}` for the foreground, or `nolune gateway install{flag}`)"
        );
        return 0;
    }
    let output = Command::new("systemctl")
        .args(["--user", "is-active", &service::unit_name(&profile.name)])
        .output();
    match output {
        Ok(out) => {
            let state = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if state == "active" {
                println!("{} is running.", display(profile));
            } else {
                println!(
                    "{} service is installed but not running ({state}).",
                    display(profile)
                );
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
fn svc_logs(profile: &Profile) -> i32 {
    let status = Command::new("journalctl")
        .args([
            "--user",
            "-u",
            &service::unit_name(&profile.name),
            "-f",
            "--no-pager",
        ])
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
fn run_systemctl(args: &[&str], verb: &str, profile: &Profile) -> i32 {
    match Command::new("systemctl").arg("--user").args(args).status() {
        Ok(s) if s.success() => {
            println!("{} service {verb}.", display(profile));
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
fn platform_stop_quietly(_profile: &Profile) {}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn platform_after_remove() {}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn platform_install_start(_definition: &std::path::Path, _profile: &Profile) -> i32 {
    1
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn svc_start(_profile: &Profile) -> i32 {
    1
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn svc_stop(_profile: &Profile) -> i32 {
    1
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn svc_status(_profile: &Profile) -> i32 {
    1
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn svc_logs(_profile: &Profile) -> i32 {
    1
}
