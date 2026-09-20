//! `nolune onboard`: prepare the workspace directory so the server can run.
//!
//! This never starts the server and never registers a service (#123).

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use anyhow::Context;
use serde::Serialize;

pub const DEFAULT_PORT: u16 = 26559;

/// What `onboard` did, printed as JSON with `--json` so installers never parse config.toml.
#[derive(Debug, Serialize)]
pub struct OnboardOutcome {
    pub profile: String,
    pub dir: PathBuf,
    pub config_path: PathBuf,
    pub url: String,
    pub port: u16,
    pub token: String,
    pub created_config: bool,
    pub generated_token: bool,
}

/// Prepare `dir` as a Nolune workspace for `profile`. Idempotent: an existing config.toml
/// is kept and only an empty or missing `auth_token` is backfilled; an explicit `port` is
/// written whether the config is new or not.
pub fn onboard(dir: &Path, profile: &str, port: Option<u16>) -> anyhow::Result<OnboardOutcome> {
    let _ = (profile, port);
    for sub in ["", "instances", "skills", "bin"] {
        let path = dir.join(sub);
        fs::create_dir_all(&path).with_context(|| format!("cannot create {}", path.display()))?;
    }

    let config_path = dir.join("config.toml");
    let (created_config, generated_token, token, port) = if config_path.exists() {
        let raw = fs::read_to_string(&config_path)
            .with_context(|| format!("cannot read {}", config_path.display()))?;
        let doc: toml::Value = toml::from_str(&raw)
            .with_context(|| format!("{} is not valid TOML", config_path.display()))?;
        let port = doc
            .get("port")
            .and_then(toml::Value::as_integer)
            .and_then(|p| u16::try_from(p).ok())
            .unwrap_or(DEFAULT_PORT);
        let existing = doc
            .get("auth_token")
            .and_then(toml::Value::as_str)
            .unwrap_or_default();
        if existing.is_empty() {
            let token = generate_token();
            write_private(&config_path, &with_token(&raw, &token))?;
            (false, true, token, port)
        } else {
            (false, false, existing.to_string(), port)
        }
    } else {
        let token = generate_token();
        write_private(&config_path, &fresh_config(&token))?;
        (true, true, token, DEFAULT_PORT)
    };

    Ok(OnboardOutcome {
        profile: profile.to_owned(),
        dir: dir.to_path_buf(),
        config_path,
        url: format!("http://localhost:{port}"),
        port,
        token,
        created_config,
        generated_token,
    })
}

/// 32 characters of `[a-z0-9]` from OS randomness, matching the token the shell installer generated.
pub fn generate_token() -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    // Largest multiple of the alphabet size below 256, so rejection keeps the draw uniform.
    const LIMIT: u8 = (256 / ALPHABET.len() * ALPHABET.len()) as u8;
    let mut token = String::with_capacity(32);
    let mut buf = [0u8; 64];
    while token.len() < 32 {
        getrandom::fill(&mut buf).expect("OS randomness is unavailable");
        for byte in buf {
            if byte < LIMIT && token.len() < 32 {
                token.push(ALPHABET[usize::from(byte) % ALPHABET.len()] as char);
            }
        }
    }
    token
}

fn fresh_config(token: &str) -> String {
    format!(
        r#"host = "0.0.0.0"
port = {DEFAULT_PORT}
auth_token = "{token}"

[llm]
# Model presets (#156): name the models you want, then pick which preset
# handles conversations and which does background work.
chat_preset = "sonnet"
background_preset = "haiku"

[[llm.presets]]
id = "sonnet"
name = "Claude Sonnet"
provider = "anthropic"
model = "claude-sonnet-4-6"

[[llm.presets]]
id = "haiku"
name = "Claude Haiku"
provider = "anthropic"
model = "claude-haiku-4-5-20251001"

[llm.tokens]
ANTHROPIC = ""       # Required — get a key at https://console.anthropic.com
OPEN_AI = ""         # Optional — text embeddings for memory search
ELEVENLABS = ""      # Optional — text-to-speech
"#
    )
}

/// Replace the top-level `auth_token` line, or insert one first, leaving every other byte alone.
fn with_token(raw: &str, token: &str) -> String {
    let line = format!("auth_token = \"{token}\"");
    let mut out: Vec<String> = Vec::new();
    let mut replaced = false;
    let mut in_table = false;
    for current in raw.lines() {
        let trimmed = current.trim_start();
        if trimmed.starts_with('[') {
            in_table = true;
        }
        let is_token_key = trimmed
            .strip_prefix("auth_token")
            .is_some_and(|rest| rest.trim_start().starts_with('='));
        if !in_table && !replaced && is_token_key {
            out.push(line.clone());
            replaced = true;
        } else {
            out.push(current.to_string());
        }
    }
    if !replaced {
        out.insert(0, line);
    }
    let mut result = out.join("\n");
    result.push('\n');
    result
}

/// Write `contents` so only the owner can read it: the file holds the auth token.
fn write_private(path: &Path, contents: &str) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(contents.as_bytes())?;
        // `mode` only applies on creation; an existing file keeps its old bits otherwise.
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        fs::write(path, contents)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    fn parsed(path: &Path) -> crate::config::Config {
        toml::from_str(&read(path)).unwrap()
    }

    #[test]
    fn fresh_dir_gets_layout_config_and_private_token() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("home");

        let out = onboard(&dir, "default", None).unwrap();

        assert!(out.created_config);
        assert!(out.generated_token);
        assert_eq!(out.dir, dir);
        assert_eq!(out.config_path, dir.join("config.toml"));
        assert_eq!(out.url, "http://localhost:26559");
        assert_eq!(out.port, 26559);
        assert_eq!(out.profile, "default");
        for sub in ["instances", "skills", "bin"] {
            assert!(dir.join(sub).is_dir(), "{sub}/ should exist");
        }

        let config = parsed(&out.config_path);
        assert_eq!(config.auth_token, out.token);
        assert_eq!(config.port, 26559);
        assert_eq!(config.host, "0.0.0.0");

        // The shell installer still writes retired keys; onboard must not.
        let doc: toml::Value = toml::from_str(&read(&out.config_path)).unwrap();
        let tokens = doc["llm"]["tokens"].as_table().unwrap();
        for key in crate::config::RETIRED_TOKEN_KEYS {
            assert!(!tokens.contains_key(key), "retired key {key} written");
        }
        assert!(tokens.contains_key("ANTHROPIC"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&out.config_path).unwrap().permissions().mode() & 0o777;
            assert_eq!(
                mode, 0o600,
                "config.toml holds the auth token and must be private"
            );
        }
    }

    #[test]
    fn token_is_32_lowercase_alphanumerics_and_unique() {
        let a = generate_token();
        let b = generate_token();
        assert_eq!(a.len(), 32);
        assert!(
            a.bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
            "{a}"
        );
        assert_ne!(a, b);
    }

    #[test]
    fn second_run_changes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let first = onboard(tmp.path(), "default", None).unwrap();
        let raw_before = read(&first.config_path);

        let second = onboard(tmp.path(), "default", None).unwrap();

        assert!(!second.created_config);
        assert!(!second.generated_token);
        assert_eq!(second.token, first.token);
        assert_eq!(read(&second.config_path), raw_before);
    }

    #[test]
    fn backfills_empty_token_and_keeps_everything_else() {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config.toml");
        fs::write(
            &config,
            "host = \"127.0.0.1\"\nport = 4242\nauth_token = \"\"\npublic_url = \"https://nolune.example\"\n\n[llm]\nchat_preset = \"opus\"\n",
        )
        .unwrap();

        let out = onboard(tmp.path(), "default", None).unwrap();

        assert!(!out.created_config);
        assert!(out.generated_token);
        assert_eq!(out.url, "http://localhost:4242");
        assert_eq!(out.port, 4242);
        let parsed = parsed(&config);
        assert_eq!(parsed.auth_token, out.token);
        assert_eq!(parsed.host, "127.0.0.1");
        assert_eq!(parsed.port, 4242);
        assert_eq!(parsed.public_url, "https://nolune.example");
        assert!(read(&config).contains("chat_preset = \"opus\""));
    }

    #[test]
    fn backfills_missing_token_line() {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config.toml");
        fs::write(&config, "port = 26559\n\n[llm]\nchat_preset = \"sonnet\"\n").unwrap();

        let out = onboard(tmp.path(), "default", None).unwrap();

        assert!(out.generated_token);
        let parsed = parsed(&config);
        assert_eq!(parsed.auth_token, out.token);
        assert_eq!(parsed.port, 26559);
        assert!(read(&config).contains("chat_preset = \"sonnet\""));
    }

    #[test]
    fn keeps_existing_token() {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config.toml");
        fs::write(
            &config,
            "auth_token = \"keepme0000000000000000000000000\"\n",
        )
        .unwrap();

        let out = onboard(tmp.path(), "default", None).unwrap();

        assert!(!out.generated_token);
        assert_eq!(out.token, "keepme0000000000000000000000000");
        assert_eq!(parsed(&config).auth_token, out.token);
    }

    #[test]
    fn explicit_port_and_profile_go_into_a_fresh_config() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("molinka");

        let out = onboard(&dir, "molinka", Some(26560)).unwrap();

        assert_eq!(out.profile, "molinka");
        assert_eq!(out.port, 26560);
        assert_eq!(out.url, "http://localhost:26560");
        let config = parsed(&out.config_path);
        assert_eq!(config.port, 26560);
        assert_eq!(config.auth_token, out.token);
        assert!(
            !read(&out.config_path).contains("molinka"),
            "the profile name is deployment metadata, not configuration"
        );
    }

    #[test]
    fn explicit_port_rewrites_an_existing_config_and_keeps_the_rest() {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config.toml");
        fs::write(
            &config,
            "host = \"127.0.0.1\"\nport = 4242\nauth_token = \"keepme0000000000000000000000000\"\n\n[llm]\nchat_preset = \"opus\"\n",
        )
        .unwrap();

        let out = onboard(tmp.path(), "default", Some(5000)).unwrap();

        assert!(!out.created_config);
        assert!(!out.generated_token);
        assert_eq!(out.port, 5000);
        assert_eq!(out.url, "http://localhost:5000");
        let parsed = parsed(&config);
        assert_eq!(parsed.port, 5000);
        assert_eq!(parsed.host, "127.0.0.1");
        assert_eq!(parsed.auth_token, "keepme0000000000000000000000000");
        assert!(read(&config).contains("chat_preset = \"opus\""));
        assert_eq!(read(&config).matches("port =").count(), 1);

        // Without an explicit port a rerun keeps what is there.
        let again = onboard(tmp.path(), "default", None).unwrap();
        assert_eq!(again.port, 5000);
    }

    #[test]
    fn missing_port_line_is_inserted_when_a_port_is_requested() {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config.toml");
        fs::write(&config, "[llm]\nchat_preset = \"sonnet\"\n").unwrap();

        let out = onboard(tmp.path(), "default", Some(26561)).unwrap();

        assert_eq!(out.port, 26561);
        assert_eq!(parsed(&config).port, 26561);
        assert_eq!(parsed(&config).auth_token, out.token);
        assert!(read(&config).contains("chat_preset = \"sonnet\""));
    }
}
