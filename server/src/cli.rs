use std::{
    future::Future,
    io::{self, IsTerminal},
    path::PathBuf,
    process::Command,
};

use clap::{Parser, Subcommand};
use cua_protocol::{
    HealthCheckData, HealthOverall, HealthReportResult, MachineId, Permission,
    cua_driver_pin::{
        self, PINNED_VERSION, PinnedAsset, RELEASE_REPOSITORY, RELEASE_TAG, Target,
        check_driver_version,
    },
    driver_mcp::permissions_from_health,
};

mod federation;
mod settings;

use crate::{
    config::{self, Profile},
    onboard, profiles, service,
    services::cua::{
        daemon::{self, DaemonState},
        discovery::{self, DriverSource},
        driver,
        host::{self, DisplaySession, PlatformSupport},
        install::{self, InstallOutcome, InstallRequest, InstallStep},
        transport::{DriverTransport as _, StdioDriverTransport},
    },
    uninstall,
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
    /// Create a one-time code so a browser or the desktop app can sign in to this server
    Pair,
    /// Prepare ~/.nolune (config, data directories, auth token) without starting anything
    Onboard {
        /// Print the outcome as one JSON line instead of human-readable progress
        #[arg(long)]
        json: bool,
        /// Listen on this port; a named profile otherwise keeps its port, or is moved off
        /// 26559 to a free one above it
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
    /// Install or inspect the pinned Cua Driver that computer use runs on
    Cua {
        #[command(subcommand)]
        action: CuaAction,
    },
    /// Replace the companion with a backup archive, sent to the running server
    Restore {
        /// A companion.tar.gz from Export in Settings or from create_backup
        archive: PathBuf,
        /// Replace without asking (required when not running in a terminal)
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Pair this companion with another one, on this host or elsewhere, through the running server
    Federation {
        #[command(subcommand)]
        action: federation::FederationAction,
    },
    /// Show or change the companion's settings: timezone, name, API keys, GitHub, email
    Config {
        #[command(subcommand)]
        action: settings::ConfigAction,
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

#[derive(Subcommand)]
pub enum CuaAction {
    /// Download the pinned Cua Driver release for this host, verify its checksum, and
    /// install it under the workspace
    Install {
        /// Install again even when the pinned driver is already present
        #[arg(long)]
        force: bool,
    },
    /// Show the pinned version, the installed and discovered driver, and its health
    Status,
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
        CliCommand::Cua { action } => cua(action, profile),
        CliCommand::Restore { archive, yes } => restore_cmd(&archive, yes, profile),
        CliCommand::Federation { action } => federation::run(action, profile),
        CliCommand::Config { action } => settings::run(action, profile),
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

/// Refuse to act on a sibling profile's data root through `NOLUNE_HOME`: the service this
/// command would touch belongs to one profile and the data to another (#107).
fn refuse_foreign_root(profile: &Profile) -> bool {
    match profiles::foreign_root(&home_dir(), profile) {
        Some(collision) => {
            eprintln!("{collision}");
            eprintln!("nothing was removed");
            true
        }
        None => false,
    }
}

fn gateway_uninstall(profile: &Profile) -> i32 {
    if refuse_foreign_root(profile) {
        return 1;
    }
    remove_service(profile)
}

/// Stop and remove the installed service definition; the caller has already checked that
/// `profile` owns its root.
fn remove_service(profile: &Profile) -> i32 {
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
    if refuse_foreign_root(profile) {
        return 1;
    }
    // Every profile runs the one installed binary, which lives under the default root's
    // bin/ and goes with it (with or without --keep-data). Say so before asking anything.
    let dependents = profiles::dependents(&home_dir(), profile);
    if !dependents.is_empty() {
        let (services, profiles, run, they) = if dependents.len() == 1 {
            ("service", "profile", "runs", "it")
        } else {
            ("services", "profiles", "run", "they")
        };
        eprintln!(
            "warning: the background {services} of {profiles} {} {run} the binary under {}, \
             which this removes; {they} will stop working at the next restart",
            dependents.join(", "),
            home.join("bin").display()
        );
        for name in &dependents {
            eprintln!(
                "  profile {name}: `nolune gateway uninstall --profile {name}` removes its service; \
                 after reinstalling nolune, `nolune gateway install --profile {name}` restores it"
            );
        }
    }
    if !keep_data && !yes && !confirm_delete(home) {
        return 1;
    }
    if definition.exists() {
        let code = remove_service(profile);
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
    // default profile keeps 26559 so installers and the desktop app find it. A config that
    // another command created on the way (`pair`, `gateway run`) still sits on the default
    // port, so it is moved just like a missing one.
    let on_default_port =
        profiles::configured_port(dir).is_none_or(|port| port == onboard::DEFAULT_PORT);
    let pick = port.is_none() && on_default_port && !profile.is_default();
    let port = if pick {
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
    } else {
        port
    };
    let outcome = match crate::onboard::onboard(dir, &profile.name, port) {
        Ok(outcome) => outcome,
        Err(error) => {
            eprintln!("onboard failed: {error:#}");
            return 1;
        }
    };
    // Say when an existing config was moved off the default port (or given its first one).
    let moved = (pick && !outcome.created_config).then(|| {
        format!(
            "moved profile {} off the default port {} to {} (the default profile owns {})",
            profile.name,
            onboard::DEFAULT_PORT,
            outcome.port,
            onboard::DEFAULT_PORT
        )
    });
    if json {
        if let Some(note) = &moved {
            eprintln!("{note}");
        }
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
    if let Some(note) = &moved {
        println!("{note}");
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

/// Where this process reaches the running server: the configured port on the
/// loopback address the listen host implies.
fn local_api_url(config: &config::Config, path: &str) -> String {
    let connect_host = match config.host.as_str() {
        "" | "0.0.0.0" => "127.0.0.1".to_string(),
        "::" | "[::]" => "[::1]".to_string(),
        other => other.to_string(),
    };
    format!("http://{connect_host}:{}{path}", config.port)
}

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
            "Authentication is disabled (auth_token is empty in {}), so browsers and the desktop app can open Nolune without pairing.",
            config::config_path().display()
        );
        return 0;
    }

    let url = local_api_url(&config, "/api/session/pairing");
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
            println!("  In a browser: open {open_url} and enter this code.");
            println!("  In the desktop app: enter {open_url} as the server and this code.");
            println!(
                "  It works once and expires in {minutes} minutes. Run `nolune pair{flag}` again for another device."
            );
            println!();
            0
        }
    }
}

// ── Restore (#74) ───────────────────────────────────────────────────────

/// Send a backup archive from an operator-chosen local path to the running
/// server, which validates and restores it (`POST /api/instances/companion/import`)
/// under the same rules as an upload from the browser. The CLI never opens
/// the archive itself beyond streaming its bytes, and the API token never
/// leaves this process.
fn restore_cmd(archive: &std::path::Path, yes: bool, profile: &Profile) -> i32 {
    let size = match std::fs::metadata(archive) {
        Ok(metadata) if metadata.is_file() => metadata.len(),
        Ok(_) => {
            eprintln!("{} is not a file", archive.display());
            return 1;
        }
        Err(error) => {
            eprintln!("cannot read {}: {error}", archive.display());
            return 1;
        }
    };
    if !yes && !confirm_restore(archive, profile) {
        return 1;
    }
    let config = match config::load_config() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("cannot read {}: {error}", config::config_path().display());
            return 1;
        }
    };
    let url = local_api_url(
        &config,
        &format!(
            "/api/instances/{}/import",
            crate::domain::companion::CANONICAL_SLUG
        ),
    );
    let flag = profile_flag(profile);

    // `main` is already inside the tokio runtime, so drive the upload on a
    // thread with its own runtime. No overall timeout: a large archive and
    // the index rebuild behind it take as long as they take.
    let auth_token = config.auth_token.clone();
    let request_url = url.clone();
    let path = archive.to_path_buf();
    let response = on_own_runtime(move || async move {
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(5))
            .build()
            .map_err(|e| e.to_string())?;
        let file = tokio::fs::File::open(&path)
            .await
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let part = reqwest::multipart::Part::stream_with_length(reqwest::Body::from(file), size)
            .file_name("companion.tar.gz")
            .mime_str("application/gzip")
            .map_err(|e| e.to_string())?;
        let form = reqwest::multipart::Form::new().part("file", part);
        let mut request = client.post(&request_url).multipart(form);
        if !auth_token.is_empty() {
            request = request.bearer_auth(&auth_token);
        }
        let response = request.send().await.map_err(|e| e.to_string())?;
        let status = response.status();
        let body: serde_json::Value = response.json().await.unwrap_or_default();
        Ok::<_, String>((status, body))
    })
    .and_then(|result| result);

    match response {
        Err(error) => {
            eprintln!(
                "{} is not reachable at {url} ({error}).
Start it with `nolune gateway{flag}` and try again; nothing was changed.",
                display(profile)
            );
            1
        }
        Ok((status, _)) if status == reqwest::StatusCode::UNAUTHORIZED => {
            eprintln!(
                "The running server rejected the token from {}.
If the service was started with NOLUNE_AUTH_TOKEN, run `nolune restore{flag}` with the same value; nothing was changed.",
                config::config_path().display()
            );
            1
        }
        Ok((status, body)) if !status.is_success() => {
            let message = body["message"].as_str().unwrap_or("");
            let verb = match body["error"].as_str() {
                Some("companion_busy") => "restore refused while the companion is busy",
                Some("archive_refused") | Some("archive_too_large") | Some("invalid_upload") => {
                    "restore refused"
                }
                _ => "restore failed",
            };
            eprintln!("{verb}: HTTP {status} {message}");
            1
        }
        // A 2xx that is not the import's own answer (a proxy page, an empty
        // body) is not a restore: never print one that did not happen.
        Ok((status, body)) if body["ok"] != serde_json::Value::Bool(true) => {
            eprintln!(
                "unexpected reply from the server (HTTP {status}): cannot tell whether {} was restored; check the server log",
                display(profile)
            );
            1
        }
        Ok((_, body)) => {
            let files = body["files"].as_u64().unwrap_or(0);
            let bytes = body["bytes"].as_u64().unwrap_or(0);
            let index = match body["derived_index"].as_str() {
                Some("rebuilt") => format!(
                    "search index rebuilt ({} chunks)",
                    body["indexed_chunks"].as_u64().unwrap_or(0)
                ),
                _ => format!(
                    "search index pending{}; it is rebuilt at the next start",
                    body["pending_reason"]
                        .as_str()
                        .map(|reason| format!(" ({reason})"))
                        .unwrap_or_default()
                ),
            };
            println!(
                "restored {} from {}: {files} files, {bytes} bytes; {index}",
                display(profile),
                archive.display()
            );
            0
        }
    }
}

fn confirm_restore(archive: &std::path::Path, profile: &Profile) -> bool {
    if !io::stdin().is_terminal() {
        eprintln!(
            "refusing to replace {} without confirmation: pass --yes to restore {}; nothing was changed",
            display(profile),
            archive.display()
        );
        return false;
    }
    eprint!(
        "This replaces {}'s memory, personality, drops, and chat history with {}; the current data is not kept. Continue? [y/N] ",
        display(profile),
        archive.display()
    );
    let mut answer = String::new();
    if io::stdin().read_line(&mut answer).is_err() {
        return false;
    }
    if matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        true
    } else {
        eprintln!("aborted; nothing was changed");
        false
    }
}

// ── Cua Driver (#20) ────────────────────────────────────────────────────

fn cua(action: CuaAction, profile: &Profile) -> i32 {
    match action {
        CuaAction::Install { force } => cua_install(force, profile),
        CuaAction::Status => cua_status(profile),
    }
}

/// Run `work` on a thread with its own runtime: `main` already sits inside tokio, and
/// these steps (a download, a driver handshake, an archive upload) have to block.
fn on_own_runtime<T, F>(work: impl FnOnce() -> F + Send + 'static) -> Result<T, String>
where
    F: Future<Output = T>,
    T: Send + 'static,
{
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        Ok(runtime.block_on(work()))
    })
    .join()
    .unwrap_or_else(|_| Err("the request thread panicked".to_owned()))
}

/// The asset `nolune cua install` fetches for this host: the pinned one.
fn host_asset(target: Target) -> &'static PinnedAsset {
    #[cfg(debug_assertions)]
    if let Some(overridden) = test_pin_override(target) {
        return overridden;
    }
    cua_driver_pin::asset_for(target)
}

/// Test seam of debug builds only: `NOLUNE_CUA_TEST_PIN=<sha256>:<size>` replaces the
/// pinned digest and size of this host's asset so the CLI tests can serve a small
/// stand-in archive from a local server. Release binaries carry no such knob.
#[cfg(debug_assertions)]
fn test_pin_override(target: Target) -> Option<&'static PinnedAsset> {
    let value = std::env::var("NOLUNE_CUA_TEST_PIN").ok()?;
    let (sha256, size) = value.split_once(':')?;
    let size = size.parse().ok()?;
    let pinned = cua_driver_pin::asset_for(target);
    eprintln!(
        "warning: NOLUNE_CUA_TEST_PIN replaces the pinned checksum of {}; this seam exists in debug builds only",
        pinned.name
    );
    Some(Box::leak(Box::new(PinnedAsset {
        name: pinned.name,
        sha256: Box::leak(sha256.to_owned().into_boxed_str()),
        size,
    })))
}

fn cua_install(force: bool, profile: &Profile) -> i32 {
    let flag = profile_flag(profile);
    let Some(target) = Target::current() else {
        eprintln!(
            "cannot install Cua Driver: Nolune ships no driver for this host; nothing was installed"
        );
        return 1;
    };
    if let PlatformSupport::Unsupported(reason) = host::platform_support() {
        eprintln!("note: {reason}");
    }
    let asset = host_asset(target);
    let release_url = install::release_url(std::env::var(install::RELEASE_URL_ENV).ok().as_deref());
    let root = profile.root.clone();
    let outcome = on_own_runtime(move || async move {
        let request = InstallRequest {
            root: &root,
            target,
            asset,
            release_url: &release_url,
            force,
        };
        install::install(&request, &mut |step| match step {
            InstallStep::Downloading { url, size } => {
                println!("downloading Cua Driver {PINNED_VERSION} for {target}");
                println!("  {url} ({:.1} MB)", size as f64 / 1_000_000.0);
            }
            InstallStep::Verified { sha256 } => println!("verified sha256 {sha256}"),
            InstallStep::VersionChecked { version } => {
                println!("the driver reports {version}, which is the pin");
            }
        })
        .await
    });
    match outcome {
        Err(error) => {
            eprintln!("cannot install Cua Driver: {error}; nothing was installed");
            1
        }
        Ok(Err(error)) => {
            eprintln!("cannot install Cua Driver: {error:#}; nothing was installed");
            1
        }
        Ok(Ok(InstallOutcome::AlreadyInstalled(installed))) => {
            println!(
                "Cua Driver {} is already installed at {}; `nolune cua install{flag} --force` reinstalls it",
                installed.version,
                installed.driver.display()
            );
            0
        }
        Ok(Ok(InstallOutcome::Installed(installed))) => {
            println!(
                "installed Cua Driver {} at {}",
                installed.version,
                installed.driver.display()
            );
            println!("  status: nolune cua status{flag}");
            println!(
                "  Nolune never updates the driver on its own; a new Nolune release moves the pin."
            );
            0
        }
    }
}

/// How long the app daemon gets to come up after `open` on macOS.
const DAEMON_START_WAIT: std::time::Duration = std::time::Duration::from_secs(15);

/// What probing the driver found: lines about its daemon, then the report.
struct Probe {
    notes: Vec<String>,
    report: anyhow::Result<HealthReportResult>,
}

/// Start the driver, take its health report, stop it.
///
/// On macOS `<driver> mcp` proxies to the login session's `CuaDriver.app`
/// daemon and would start one by name when none runs, which picks whatever
/// `CuaDriver.app` LaunchServices knows (none on a fresh Mac, the upstream
/// installer's copy otherwise). When the driver runs from a genuine bundle
/// and no daemon is up, that bundle is started by path first, so the daemon
/// that answers is the one the driver above belongs to.
async fn probe_driver(driver: PathBuf) -> Probe {
    let mut notes = Vec::new();
    if cfg!(target_os = "macos")
        && let Some(bundle) = daemon::app_bundle(&driver)
    {
        match daemon::state(&driver).await {
            // Another CuaDriver.app's daemon of another release refuses the
            // driver above before it can report, so it is named here, where
            // the `answered by` line below never gets to.
            DaemonState::Running => match daemon::foreign_daemon(&driver).await {
                None => notes.push("daemon: running".to_owned()),
                Some(foreign) => notes.push(format!(
                    "daemon: running from {}, another CuaDriver.app, not the driver above; \
                     `{} stop` stops it, and the next `nolune cua status` starts the driver \
                     above instead",
                    foreign.display(),
                    foreign.display()
                )),
            },
            DaemonState::NotRunning => {
                let launch = daemon::launch_command(&bundle);
                if let Err(error) = daemon::start(&driver, launch, DAEMON_START_WAIT).await {
                    return Probe {
                        notes,
                        report: Err(error.context("the driver's daemon did not start")),
                    };
                }
                notes.push(format!(
                    "daemon: started {} by path (it keeps running; `{} stop` stops it)",
                    bundle.display(),
                    driver.display()
                ));
            }
            DaemonState::Unknown(why) => notes.push(format!("daemon: {why}")),
        }
    }
    let report = async {
        let transport = StdioDriverTransport::spawn(&driver).await?;
        let machine_id = MachineId::try_from("server-local").expect("static id");
        let report = driver::health_report(&transport, &machine_id).await;
        transport.close();
        report
    }
    .await;
    Probe { notes, report }
}

/// The executable that produced `report`, when it says: the `bundle_identity`
/// check of a macOS daemon carries it.
fn answering_executable(report: &HealthReportResult) -> Option<PathBuf> {
    report.checks.iter().find_map(|check| match &check.data {
        Some(HealthCheckData::BundleIdentity(identity)) => {
            Some(PathBuf::from(identity.executable_path.as_str()))
        }
        _ => None,
    })
}

/// Whether two paths name the same file, following symlinks and `/private`.
fn same_executable(a: &std::path::Path, b: &std::path::Path) -> bool {
    same_file::is_same_file(a, b).unwrap_or(a == b)
}

fn permission_word(permission: Permission) -> &'static str {
    match permission {
        Permission::Granted => "granted",
        Permission::Denied => "denied",
        Permission::PromptRequired => "not granted yet (prompt required)",
        Permission::Unavailable => "unavailable",
    }
}

fn cua_status(profile: &Profile) -> i32 {
    let flag = profile_flag(profile);
    println!(
        "Cua Driver status{}",
        if profile.is_default() {
            String::new()
        } else {
            format!(" for profile {}", profile.name)
        }
    );
    println!(
        "  pinned: {PINNED_VERSION} ({RELEASE_TAG} from {RELEASE_REPOSITORY}); Nolune never updates the driver on its own"
    );
    let target = Target::current();
    let triple = target.map_or("this host", Target::triple);
    match host::platform_support() {
        PlatformSupport::Supported => println!("  platform: supported ({triple})"),
        PlatformSupport::Unsupported(reason) => {
            println!("  platform: unsupported ({triple}): {reason}");
        }
    }
    let display = host::display_session();
    match &display {
        DisplaySession::Present(found) => println!("  display: {found}"),
        DisplaySession::Headless(reason) => println!("  display: {reason}"),
        DisplaySession::NotChecked => {}
    }

    // What `nolune cua install` recorded, checked against the pin.
    let manifest = match install::read_manifest(&profile.root) {
        Ok(manifest) => manifest,
        Err(error) => {
            println!("  installed: unreadable ({error:#}); run `nolune cua install{flag} --force`");
            None
        }
    };
    match &manifest {
        None => println!("  installed: none (run `nolune cua install{flag}`)"),
        Some(installed) if !installed.driver.is_file() => println!(
            "  installed: {} at {}, but the binary is missing; run `nolune cua install{flag} --force`",
            installed.version,
            installed.driver.display()
        ),
        Some(installed) => {
            let pinned_sha256 = target.map(|target| host_asset(target).sha256);
            let verdict = if installed.version != PINNED_VERSION {
                format!("not the pinned version; run `nolune cua install{flag}`")
            } else if pinned_sha256.is_some_and(|sha256| sha256 != installed.sha256) {
                format!(
                    "its checksum is not the pinned one; run `nolune cua install{flag} --force`"
                )
            } else {
                "verified against the pin".to_owned()
            };
            println!(
                "  installed: {} at {} ({verdict})",
                installed.version,
                installed.driver.display()
            );
        }
    }

    // The driver the server would actually run, from the same lookup the runtime uses.
    let installed_driver = manifest
        .as_ref()
        .map(|installed| installed.driver.as_path());
    let located = match discovery::discover(None, installed_driver) {
        Ok(located) => located,
        Err(error) => {
            println!("  driver: {error}");
            return 1;
        }
    };
    let Some(located) = located else {
        println!(
            "  driver: none found (run `nolune cua install{flag}`, or put cua-driver on PATH)"
        );
        return 0;
    };
    println!("  driver: {} ({})", located.path.display(), located.source);
    if display.is_headless() {
        println!("  health: not probed (headless host)");
        return 0;
    }

    let driver_path = located.path.clone();
    let probe = match on_own_runtime(move || probe_driver(driver_path)) {
        Ok(probe) => probe,
        Err(error) => {
            println!("  health: the driver could not report ({error})");
            return 1;
        }
    };
    for note in &probe.notes {
        println!("  {note}");
    }
    let report = match probe.report {
        Ok(report) => report,
        Err(error) => {
            println!("  health: the driver could not report ({error:#})");
            return 1;
        }
    };
    if let Some(answered_by) = answering_executable(&report) {
        if same_executable(&answered_by, &located.path) {
            println!(
                "  answered by: {} ({})",
                answered_by.display(),
                if located.source == DriverSource::Installed {
                    "the Nolune install"
                } else {
                    "the driver above"
                }
            );
        } else {
            println!(
                "  answered by: {}, not the driver above: another CuaDriver.app owns this login \
                 session's daemon, so the health below is its own; `{} stop` stops it, and the \
                 next `nolune cua status{flag}` starts the driver above instead",
                answered_by.display(),
                answered_by.display()
            );
        }
    }
    let version_ok = match check_driver_version(&report.driver_version) {
        Ok(()) => {
            println!(
                "  version: {} matches the pin",
                report.driver_version.as_str()
            );
            true
        }
        Err(incompatible) => {
            println!(
                "  version: {incompatible} (`nolune cua install{flag}` installs the pinned release)"
            );
            false
        }
    };
    let overall = match report.overall {
        HealthOverall::Ok => "ok",
        HealthOverall::Degraded => "degraded",
        HealthOverall::Failed => "failed",
    };
    let permissions = permissions_from_health(&report);
    println!(
        "  health: {overall}; accessibility: {}, screen recording: {}",
        permission_word(permissions.accessibility),
        permission_word(permissions.screen_capture)
    );
    for check in report
        .checks
        .iter()
        .filter(|check| check.status == cua_protocol::HealthCheckStatus::Fail)
    {
        match &check.hint {
            Some(hint) => println!(
                "    {}: {} ({})",
                check.name.as_str(),
                check.message.as_str(),
                hint.as_str()
            ),
            None => println!("    {}: {}", check.name.as_str(), check.message.as_str()),
        }
    }
    if version_ok && report.overall != HealthOverall::Failed {
        0
    } else {
        1
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
