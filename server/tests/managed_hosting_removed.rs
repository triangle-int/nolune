//! Guard for #37: Nolune is one self-hosted product. Nothing in the
//! repository provisions, bills, or describes a managed service any more:
//! no Stripe, Neon or Fly artifacts, no tenant or cloud-mode switches, no
//! per-user subdomains, no plan or subscription copy, no dashboard links,
//! and no secret names from the retired control plane. The example
//! configuration is one a contributor can copy verbatim: it parses against
//! the current schema, names no retired key, and boots a gateway without an
//! obsolete-setting warning.
//!
//! The scan covers the whole tree. The other guards name the strings they
//! forbid, and `config.rs` names the retired keys it drops on load, so those
//! files are excused exactly; a new mention anywhere else fails CI.

#[path = "../test-support/source_scan.rs"]
mod source_scan;

use source_scan::without_cfg_test_items;
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{IpAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const BIN: &str = env!("CARGO_BIN_EXE_nolune");

/// Build output, dependency trees, editor state, nested worktrees and the
/// scratch directories agents keep beside the checkout.
const SKIPPED_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "build",
    "gen",
    ".git",
    ".claude",
    ".codex",
    ".svelte-kit",
    ".vercel",
];

/// Lockfiles carry third-party package names, not product decisions.
const SKIPPED_FILES: &[&str] = &["Cargo.lock", "pnpm-lock.yaml"];

const SCANNED_EXTENSIONS: &[&str] = &[
    "rs", "ts", "js", "mjs", "cjs", "svelte", "md", "toml", "json", "yml", "yaml", "sh", "py",
    "html", "css", "txt", "plist", "xml", "ps1",
];

/// Guards that spell out the strings they forbid, and therefore contain them.
const GUARD_TESTS: &[&str] = &[
    "server/tests/managed_hosting_removed.rs",
    "server/tests/self_hosted_surface.rs",
    "server/tests/identity_language.rs",
    "server/tests/nolune_rename.rs",
    "server/tests/landing_static_site.rs",
];

/// Intentional remnants: (path, exact substring, occurrences). Each substring
/// is removed from its file before the case-insensitive scan, and its count
/// must match so a remnant cannot quietly grow or move.
const ALLOWED: &[(&str, &str, usize)] = &[
    // The loader names the obsolete keys it drops so an old config.toml
    // produces one warning instead of silently changing behaviour.
    (
        "server/src/config.rs",
        "for key in [\"landing_url\", \"plan\", \"landing_auth_token\"] {",
        1,
    ),
    (
        "server/src/config.rs",
        "for obsolete in [\"landing_url\", \"plan\", \"landing_auth_token\"] {",
        1,
    ),
    (
        "server/src/config.rs",
        "for key in [\"LANDING_URL\", \"FLY_APP_NAME\", \"FLY_MACHINE_ID\"] {",
        1,
    ),
    // The router tests assert the managed status field and usage route are gone.
    (
        "server/test-support/router_security.rs",
        "assert!(status.get(\"is_managed\").is_none());",
        1,
    ),
    (
        "server/test-support/router_security.rs",
        ".uri(\"/api/usage\")",
        1,
    ),
];

/// Managed-hosting artifacts, matched case-insensitively.
const FORBIDDEN: &[&str] = &[
    // Billing and plans.
    "stripe",
    "billing",
    "subscription",
    "paid plan",
    "companion plan",
    "upgrade your plan",
    "upgrade their plan",
    "pricing page",
    // The managed database.
    "neondatabase",
    "neon.tech",
    "drizzle",
    "DATABASE_URL",
    // Fly provisioning.
    "fly.io",
    "fly.toml",
    "flyctl",
    "fly machines",
    "FLY_API_TOKEN",
    "FLY_APP_NAME",
    "FLY_MACHINE_ID",
    "_api.internal",
    // Tenants, cloud mode and the control plane.
    "tenant",
    "cloud_mode",
    "cloudmode",
    "is_cloud",
    "iscloud",
    "is_managed",
    "ismanaged",
    "landing_url",
    "landing_auth_token",
    "hosted-instance",
    "subdomain",
    "managed ai companion platform",
    // Hosted product surfaces.
    "/api/usage",
    "on the dashboard",
    "nolune.dev/dashboard",
    "mintlify",
    "ghcr.io",
    // Secrets of the retired control plane.
    "GOOGLE_CLIENT_SECRET",
    "GOOGLE_CLIENT_ID",
];

/// Files and directories of the managed product that must stay deleted.
const ABSENT_PATHS: &[&str] = &[
    "Dockerfile",
    "docker-compose.yml",
    "docker-compose.yaml",
    "fly.toml",
    "server/fly.toml",
    "landing/fly.toml",
    ".env.example",
    "server/.env.example",
    "client/.env.example",
    "desktop/.env.example",
    "landing/.env.example",
    "landing/src/hooks.server.ts",
    "landing/src/lib/server",
    "landing/src/routes/api",
    "landing/src/routes/dashboard",
    "landing/src/routes/pricing",
    "landing/src/routes/login",
    "landing/src/routes/signup",
    "server/src/routes/tenants.rs",
    "server/src/routes/billing.rs",
    "server/src/routes/stripe.rs",
];

fn files(root: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(root).unwrap_or_else(|_| panic!("{} is missing", root.display())) {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        // A symlink (CLAUDE.md -> AGENTS.md) would scan its target twice.
        if path.symlink_metadata().unwrap().file_type().is_symlink() {
            continue;
        }
        if path.is_dir() {
            if SKIPPED_DIRS.contains(&name.as_str()) || name.starts_with(".scratch") {
                continue;
            }
            files(&path, out);
        } else if !SKIPPED_FILES.contains(&name.as_str())
            && path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| SCANNED_EXTENSIONS.contains(&extension))
        {
            out.push(path);
        }
    }
}

fn scanned_sources(repo: &Path) -> Vec<(String, String)> {
    let mut paths = Vec::new();
    files(repo, &mut paths);
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let relative = path
                .strip_prefix(repo)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let text = String::from_utf8_lossy(&fs::read(&path).unwrap()).into_owned();
            // Unit tests may exercise the loader with old keys; production
            // source may not carry them.
            let text = if relative.starts_with("server/src/") && relative.ends_with(".rs") {
                without_cfg_test_items(&text)
            } else {
                text
            };
            (relative, text)
        })
        .filter(|(relative, _)| !GUARD_TESTS.contains(&relative.as_str()))
        .collect()
}

#[test]
fn no_managed_hosting_artifact_survives_outside_the_allowlist() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut violations = Vec::new();
    let mut sources = scanned_sources(repo);

    for guard in GUARD_TESTS {
        if !repo.join(guard).is_file() {
            violations.push(format!(
                "{guard} is excused from the scan but does not exist"
            ));
        }
    }
    for (path, _, _) in ALLOWED {
        if !sources.iter().any(|(scanned, _)| scanned == path) {
            violations.push(format!("{path} is allowed but was not scanned"));
        }
    }

    let forbidden: Vec<String> = FORBIDDEN.iter().map(|s| s.to_lowercase()).collect();
    for (path, source) in &mut sources {
        for (allowed_path, allowed, expected) in ALLOWED {
            if path != allowed_path {
                continue;
            }
            let actual = source.matches(allowed).count();
            if actual != *expected {
                violations.push(format!(
                    "{path}: expected {expected} exact allowed occurrence(s) of {allowed:?}, found {actual}"
                ));
            }
            *source = source.replace(allowed, "");
        }
        for line in source.lines() {
            let lowered = line.to_lowercase();
            for needle in &forbidden {
                if lowered.contains(needle) {
                    violations.push(format!("{path} contains {needle:?}: {:?}", line.trim()));
                }
            }
        }
    }

    for absent in ABSENT_PATHS {
        if repo.join(absent).exists() {
            violations.push(format!(
                "{absent} belongs to the managed product and was restored"
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "managed-hosting remnants:\n{}",
        violations.join("\n")
    );
}

/// The quoted strings of the `const NAME: ... = [` / `&[` array that follows
/// `marker` in `source`, so the example-config checks read the loader's own
/// retired-key lists instead of copying them.
fn string_array(source: &str, marker: &str) -> Vec<String> {
    let start = source
        .find(marker)
        .unwrap_or_else(|| panic!("config.rs no longer defines {marker}"));
    let body = &source[start..];
    let assign = body.find('=').unwrap();
    let open = body[assign..].find('[').unwrap() + assign;
    let close = body[open..].find(']').unwrap() + open;
    body[open + 1..close]
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(|item| item.trim_matches('"').to_owned())
        .collect()
}

/// The `pub <field>:` names of `pub struct Config`.
fn config_fields(source: &str) -> Vec<String> {
    let start = source.find("pub struct Config {").unwrap();
    let body = &source[start..];
    let end = body.find("\n}").unwrap();
    body[..end]
        .lines()
        .filter_map(|line| line.trim().strip_prefix("pub "))
        .filter_map(|rest| rest.split(':').next())
        .map(|name| name.trim().to_owned())
        .collect()
}

#[test]
fn example_config_matches_the_current_schema() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let raw = fs::read_to_string(repo.join("server/config.example.toml")).unwrap();
    let config_rs = fs::read_to_string(repo.join("server/src/config.rs")).unwrap();
    let document: toml::Value = toml::from_str(&raw).expect("config.example.toml must parse");
    let mut violations = Vec::new();

    let fields = config_fields(&config_rs);
    for key in document.as_table().unwrap().keys() {
        if !fields.iter().any(|field| field == key) {
            violations.push(format!("top-level key {key:?} is not a Config field"));
        }
    }
    for obsolete in ["landing_url", "plan", "landing_auth_token"] {
        if document.get(obsolete).is_some() {
            violations.push(format!("managed control-plane key {obsolete:?} is set"));
        }
    }

    let llm = document.get("llm").and_then(toml::Value::as_table);
    for key in string_array(&config_rs, "pub const RETIRED_LLM_KEYS") {
        if llm.is_some_and(|llm| llm.contains_key(&key)) {
            violations.push(format!("retired [llm] key {key:?} is set"));
        }
    }
    let tokens = llm
        .and_then(|llm| llm.get("tokens"))
        .and_then(toml::Value::as_table);
    for key in string_array(&config_rs, "pub const RETIRED_TOKEN_KEYS") {
        if tokens.is_some_and(|tokens| tokens.contains_key(&key)) {
            violations.push(format!("retired [llm.tokens] key {key:?} is set"));
        }
    }

    // Model choice is a list of presets and two slots (#156); the example
    // shows a working shape rather than the retired tiers.
    let presets: Vec<&toml::Value> = llm
        .and_then(|llm| llm.get("presets"))
        .and_then(toml::Value::as_array)
        .map(|presets| presets.iter().collect())
        .unwrap_or_default();
    if presets.is_empty() {
        violations.push("[[llm.presets]] is missing".to_owned());
    }
    let mut ids = Vec::new();
    for preset in &presets {
        for field in ["id", "name", "provider", "model"] {
            if preset.get(field).and_then(toml::Value::as_str).is_none() {
                violations.push(format!("preset {preset} lacks {field:?}"));
            }
        }
        let provider = preset.get("provider").and_then(toml::Value::as_str);
        if !matches!(provider, Some("anthropic" | "openai")) {
            violations.push(format!(
                "preset {preset} names unknown provider {provider:?}"
            ));
        }
        ids.extend(preset.get("id").and_then(toml::Value::as_str));
    }
    for slot in ["chat_preset", "background_preset"] {
        match llm
            .and_then(|llm| llm.get(slot))
            .and_then(toml::Value::as_str)
        {
            Some(id) if ids.contains(&id) => {}
            other => violations.push(format!("{slot} = {other:?} does not name a preset")),
        }
    }

    for stale in ["per-instance", "video analysis", "provider is"] {
        if raw.to_lowercase().contains(stale) {
            violations.push(format!("config.example.toml still says {stale:?}"));
        }
    }

    assert!(
        violations.is_empty(),
        "config.example.toml is stale:\n{}",
        violations.join("\n")
    );
}

/// One raw HTTP/1.1 GET; returns the whole response text.
fn http_get(port: u16, path: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

/// `cp config.example.toml config.toml` is what CONTRIBUTING tells a
/// contributor to do. The result must start cleanly: no obsolete-setting
/// warning on load, and `/healthz` answering on the configured port. The
/// example runs verbatim, so it must bind loopback: with its empty token
/// any other bind would be an open server, here and on a contributor's LAN.
#[cfg(unix)]
#[test]
fn example_config_boots_a_gateway_without_obsolete_warnings() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let example = fs::read_to_string(repo.join("server/config.example.toml")).unwrap();
    let document: toml::Value = toml::from_str(&example).unwrap();
    let host = document
        .get("host")
        .and_then(toml::Value::as_str)
        .unwrap_or("0.0.0.0");
    assert!(
        host.parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback()),
        "config.example.toml must bind loopback, not host = {host:?}"
    );

    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join("config.toml"), example).unwrap();
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();

    let mut child = Command::new(BIN)
        .env("NOLUNE_HOME", &home)
        .env("PORT", port.to_string())
        .env_remove("NOLUNE_AUTH_TOKEN")
        .env("RUST_LOG", "warn")
        .arg("gateway")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let (tx, rx) = mpsc::channel::<String>();
    let stdout = child.stdout.take().unwrap();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let stderr = child.stderr.take().unwrap();
    let stderr_lines = thread::spawn(move || {
        let mut buf = String::new();
        BufReader::new(stderr).read_to_string(&mut buf).ok();
        buf
    });

    let expected = format!("nolune: ready http://localhost:{port}");
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(remaining) {
            Ok(line) if line == expected => break,
            Ok(_) => continue,
            Err(_) => {
                child.kill().ok();
                panic!(
                    "no ready line within timeout; stderr:\n{}",
                    stderr_lines.join().unwrap()
                );
            }
        }
    }

    let response = http_get(port, "/healthz");
    let killed = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if start.elapsed() > Duration::from_secs(15) {
            child.kill().ok();
            panic!("gateway did not exit within 15s of SIGTERM");
        }
        thread::sleep(Duration::from_millis(100));
    };
    let stderr = stderr_lines.join().unwrap();

    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(status.success(), "gateway exited with {status}:\n{stderr}");
    for warning in ["ignoring obsolete", "retired", "replaced model modes"] {
        assert!(
            !stderr.contains(warning),
            "the example config triggered an obsolete-setting warning ({warning:?}):\n{stderr}"
        );
    }
}
