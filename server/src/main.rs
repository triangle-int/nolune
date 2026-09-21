mod app;
mod cli;
mod config;
mod domain;
mod onboard;
mod profiles;
mod routes;
mod service;
mod services;
mod uninstall;

use std::{net::SocketAddr, time::Duration};

use clap::Parser;
use log::info;

#[tokio::main]
async fn main() {
    let args = cli::Cli::parse();

    // Which isolated deployment this process addresses (#107). Everything below reads the
    // workspace through `config`, so selecting it once is enough.
    let profile = config::resolve_profile(args.profile.as_deref()).unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(2);
    });
    config::select_workspace(profile.root.clone());

    // Everything except the server itself is a synchronous subcommand.
    match args.command {
        None
        | Some(cli::CliCommand::Gateway {
            action: None | Some(cli::GatewayAction::Run),
        }) => {}
        Some(cmd) => {
            let code = cli::run(cmd, &profile);
            std::process::exit(code);
        }
    }

    // `nolune`, `nolune gateway`, and `nolune gateway run` run the server in the foreground.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .filter_module("tracing::span", log::LevelFilter::Warn)
        .format(app::logging::format_record)
        .init();
    if !profile.is_default() {
        info!(
            "profile {} — workspace {}",
            profile.name,
            profile.root.display()
        );
    }

    let config = config::load_config().unwrap_or_else(|err| {
        panic!(
            "failed to load config from {}: {err}",
            config::config_path().display()
        )
    });

    services::tools::register_control_secret(&config.auth_token);
    let host = config.host.clone();
    let port = config.port;
    let static_dir = if config.static_dir.is_empty() {
        None
    } else {
        let path = std::path::PathBuf::from(&config.static_dir);
        if path.is_dir() {
            Some(path)
        } else {
            log::warn!("static_dir {} does not exist, skipping", config.static_dir);
            None
        }
    };

    // load_config already fills in the local default so every reader agrees;
    // only the operator-facing hint lives here.
    if config::uses_local_public_url(&config) {
        log::warn!(
            "public_url not set — using {}. Shared file links and attachments \
             only work from this machine. If you reach Nolune through another \
             address, set public_url in config.toml or NOLUNE_PUBLIC_URL.",
            config.public_url
        );
    }

    let state = app::state::AppState::new(config).await;

    // Paired browsers survive restarts; the file holds only hashes.
    state
        .browser_sessions
        .attach_storage(state.workspace_dir.join("browser_sessions.json"));

    // Remove unpublished passive-capture state before any agents start, using
    // the persistent workspace capability opened by the media store.
    let media_store = state.vector_store.media_store();
    if let Err(error) = media_store.cleanup_legacy_child_agents() {
        log::warn!("legacy child-agent cleanup was incomplete: {error}");
    }
    if let Err(error) = media_store.cleanup_legacy_screen_capture() {
        log::warn!("legacy passive screen-capture cleanup was incomplete: {error}");
    }
    if let Err(error) = media_store.cleanup_legacy_thoughts() {
        log::warn!("legacy thoughts cleanup was incomplete: {error}");
    }
    if let Err(error) = media_store.cleanup_legacy_stats() {
        log::warn!("legacy stats aggregate cleanup was incomplete: {error}");
    }

    // A companion import (#74) the previous process did not finish: a parked
    // tree goes back into place when the companion is missing, one beside a
    // published import is discarded, staging and upload leftovers are swept.
    // Before the obsolete-directory report, the migration, and every writer.
    match services::profile_import::recover_on_startup(
        &state.vector_store,
        domain::companion::CANONICAL_SLUG,
    )
    .await
    {
        Ok(recovery) if recovery.is_noop() => {}
        Ok(recovery) => log::warn!("[import] reconciled imports/ after a restart: {recovery:?}"),
        Err(error) => log::warn!("[import] could not reconcile imports/: {error}"),
    }

    // One companion per server (#103). Sibling directories from the unpublished
    // multi-instance layout are reported and otherwise ignored.
    let obsolete = services::companion::obsolete_instance_dirs(&state.workspace_dir);
    if !obsolete.is_empty() {
        log::warn!(
            "ignoring {} obsolete multi-instance director{}: {:?}. This server owns one companion \
             ({}); see docs/companion-storage.md",
            obsolete.len(),
            if obsolete.len() == 1 { "y" } else { "ies" },
            obsolete,
            domain::companion::CANONICAL_SLUG,
        );
    }

    // Migrate legacy memory (facts.md + episodes.md → library) for the companion
    services::memory::migrate_companion(&media_store);

    let addr: SocketAddr = format!("{host}:{port}").parse().unwrap_or_else(|_| {
        log::warn!("invalid host:port {host}:{port}, falling back to 0.0.0.0:{port}");
        SocketAddr::from(([0, 0, 0, 0], port))
    });

    // Notify active chats that the server restarted and spawn agent loops
    let restart_chats = services::chat::notify_restart(&state.workspace_dir, &state.events);
    for (slug, chat_id) in restart_chats {
        let cancel = tokio_util::sync::CancellationToken::new();
        let key = format!("{slug}/{chat_id}");
        {
            let mut tasks = state.agent_tasks.lock().await;
            tasks.insert(key, cancel.clone());
        }
        let bg_state = state.clone();
        tokio::spawn(async move {
            routes::chat::run_agent_loop(bg_state, slug, chat_id, cancel, false, None).await;
        });
    }

    // Start background scheduler for scheduled messages
    services::scheduler::start(state.clone());

    // Report a connected computer whose heartbeat goes stale (#80).
    state.machine_registry.start_health_watch();

    // One proactive loop (#92): finish what a previous process left running,
    // then trim old receipts.
    {
        let now = chrono::Utc::now().timestamp();
        let recovered = state.proactive.recover_on_restart(now);
        if recovered > 0 {
            log::warn!("[proactive] marked {recovered} interrupted run(s) failed and retryable");
        }
        // A handoff continuation (#82) that died with the process is closed
        // on its record too, so the card offers the task again.
        let closed = services::handoff::recover_on_restart(&state, now).await;
        if closed > 0 {
            log::warn!("[handoff] closed {closed} interrupted continuation(s) on their records");
        }
        let removed = state.proactive.enforce_retention(now);
        if removed > 0 {
            info!("[proactive] removed {removed} old activity record(s)");
        }
    }

    // Start heartbeat — companion's autonomous inner life
    {
        services::heartbeat::start(
            &state.workspace_dir,
            state.background_llm.clone(),
            state.events.clone(),
            state.vector_store.clone(),
            state.resources.clone(),
            state.proactive.clone(),
        );

        // Backfill missing or invalid local indexes from memory files (background, non-blocking)
        let vs = state.vector_store.clone();
        let ws = state.workspace_dir.clone();
        tokio::spawn(async move {
            let slug = domain::companion::CANONICAL_SLUG;
            match services::companion::read_identity(&ws) {
                Ok(Some(_)) => {}
                Ok(None) => {
                    info!("[backfill] companion not created yet; nothing to index");
                    return;
                }
                Err(error) => {
                    log::warn!("[backfill] companion storage is unusable: {error}");
                    return;
                }
            }
            match vs.needs_backfill(slug).await {
                Ok(false) => {
                    info!("[backfill] completed");
                    return;
                }
                Ok(true) => {}
                Err(e) => {
                    log::warn!(
                        "[backfill] {slug}: cannot prepare index: {e} — will retry on next restart"
                    );
                    return;
                }
            }
            info!("[backfill] starting for companion {slug}");
            match vs.backfill_text_memories(&ws, slug).await {
                Ok(count) => info!("[backfill] {slug}: indexed {count} chunks"),
                Err(e) => log::warn!("[backfill] {slug}: failed: {e} — will retry on next restart"),
            }
        });
    }

    let cua = state.cua.clone();
    let codex_auth = state.codex_auth.clone();
    let app = app::router::build_router(state, static_dir);

    info!("Starting server on http://{addr}");

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            eprintln!(
                "port {port} is already in use on {host}. Another Nolune gateway or service is \
                 probably running; stop it, or change `port` in {}.",
                config::config_path().display()
            );
            std::process::exit(1);
        }
        Err(error) => {
            eprintln!("failed to listen on {addr}: {error}");
            std::process::exit(1);
        }
    };

    // Installers and the desktop app wait for this exact stdout line (#124).
    println!("nolune: ready http://localhost:{port}");

    // The machine this server runs on as a computer-use target (#16): registered
    // when a Cua driver and a display are there, skipped honestly otherwise. It
    // starts behind the ready line, in the background, so a driver that stalls
    // on its handshake or health report never delays serving; the registry is
    // shared, so the target is listed as soon as it is registered.
    tokio::spawn({
        let cua = cua.clone();
        async move { cua.start().await }
    });

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            // End every open driver session and stop the driver child before
            // connections drain; the grace timer still bounds the whole exit.
            cua.shutdown().await;
            codex_auth.shutdown().await;
        })
        .await
        .expect("server exited unexpectedly");
    info!("gateway stopped");
}

/// How long open connections may linger after a shutdown signal before the process exits anyway.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

/// Resolve on Ctrl-C or SIGTERM. Long-lived streams would otherwise keep graceful shutdown
/// waiting forever, so a bounded timer force-exits once the grace period passes.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.ok();
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                log::warn!("cannot listen for SIGTERM: {error}");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
    info!("shutdown signal received; stopping gateway");
    tokio::spawn(async {
        tokio::time::sleep(SHUTDOWN_GRACE).await;
        log::warn!("connections still open after {SHUTDOWN_GRACE:?}; exiting");
        std::process::exit(0);
    });
}
