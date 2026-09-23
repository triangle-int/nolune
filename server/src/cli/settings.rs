//! `nolune config …`: read and change the companion's settings from a
//! terminal. The companion uses it too, through run_command and the built-in
//! `configure-nolune` skill; its shell carries the server's own `NOLUNE_HOME`
//! and binary directory, so a bare `nolune config` addresses the server it
//! runs under.
//!
//! * `show` prints the settings; secret values are never printed, only
//!   whether they are set.
//! * `set` and `unset` change one setting. Plain values (timezone, name) are
//!   arguments; secret values (API keys, the GitHub token) are read from
//!   stdin only, because a command line is logged and shown in the chat's
//!   tool activity.
//! * `email add` and `email remove` manage SMTP/IMAP accounts; the password
//!   is read from stdin for the same reason.
//!
//! Everything is written to the same files the Settings page writes, and the
//! server reads them again on the next message.

use std::{
    fs,
    io::{self, IsTerminal, Read},
    path::PathBuf,
};

use clap::{Subcommand, ValueEnum};

use crate::{
    config::{self, Config, EmailAccounts, EmailConfig, InstanceConfig, Profile},
    domain::companion::CANONICAL_SLUG,
};

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Show the companion's settings (secret values are never printed)
    Show,
    /// Change one setting; secret keys read their value from stdin, never from arguments
    Set {
        key: ConfigKey,
        /// The new value (timezone and name only)
        value: Option<String>,
    },
    /// Clear one setting (an unset timezone means UTC)
    Unset { key: ConfigKey },
    /// Add or remove an SMTP/IMAP email account
    Email {
        #[command(subcommand)]
        action: EmailAction,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ConfigKey {
    /// The user's IANA timezone, e.g. Asia/Bishkek
    Timezone,
    /// The companion's display name
    Name,
    /// OpenAI API key (secret)
    OpenaiKey,
    /// Anthropic API key (secret)
    AnthropicKey,
    /// Brave Search API key (secret)
    BraveSearchKey,
    /// GitHub personal access token (secret)
    GithubToken,
}

impl ConfigKey {
    fn is_secret(self) -> bool {
        !matches!(self, Self::Timezone | Self::Name)
    }

    fn label(self) -> &'static str {
        match self {
            Self::Timezone => "timezone",
            Self::Name => "companion name",
            Self::OpenaiKey => "openai key",
            Self::AnthropicKey => "anthropic key",
            Self::BraveSearchKey => "brave search key",
            Self::GithubToken => "github token",
        }
    }
}

#[derive(Subcommand)]
pub enum EmailAction {
    /// Add an account; the password is read from stdin (a second line, when present, is the IMAP password)
    Add {
        #[arg(long)]
        smtp_host: String,
        #[arg(long, default_value_t = 587)]
        smtp_port: u16,
        #[arg(long)]
        smtp_user: String,
        /// The address mail is sent from
        #[arg(long)]
        from: String,
        /// IMAP host, to read mail as well as send it
        #[arg(long, default_value = "")]
        imap_host: String,
        #[arg(long, default_value_t = 993)]
        imap_port: u16,
        /// IMAP user; defaults to the SMTP user
        #[arg(long)]
        imap_user: Option<String>,
    },
    /// Remove an account by its address (the from address or a user name)
    Remove { address: String },
}

pub fn run(action: ConfigAction, profile: &Profile) -> i32 {
    let target = match Target::open(profile) {
        Ok(target) => target,
        Err(message) => {
            eprintln!("{message}");
            return 1;
        }
    };
    let outcome = match action {
        ConfigAction::Show => target.show().map(|text| print!("{text}")),
        ConfigAction::Set { key, value } => {
            read_value(key, value).and_then(|value| target.set(key, &value))
        }
        ConfigAction::Unset { key } => target.set(key, ""),
        ConfigAction::Email { action } => match action {
            EmailAction::Add {
                smtp_host,
                smtp_port,
                smtp_user,
                from,
                imap_host,
                imap_port,
                imap_user,
            } => read_passwords().and_then(|(smtp_password, imap_password)| {
                target.add_email(EmailConfig {
                    imap_user: imap_user.unwrap_or_else(|| smtp_user.clone()),
                    smtp_host,
                    smtp_port,
                    smtp_user,
                    smtp_password,
                    smtp_from: from,
                    imap_host,
                    imap_port,
                    imap_password,
                })
            }),
            EmailAction::Remove { address } => target.remove_email(address.trim()),
        },
    };
    match outcome {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("{message}");
            1
        }
    }
}

/// A plain value comes from the argument; a secret one from stdin only.
fn read_value(key: ConfigKey, value: Option<String>) -> Result<String, String> {
    if key.is_secret() {
        if value.is_some() {
            return Err(format!(
                "{} is a secret: pass it on stdin, not as an argument",
                key.label()
            ));
        }
        let value = read_stdin(&format!("{}: ", key.label()))?;
        let value = value.lines().next().unwrap_or("").trim().to_owned();
        if value.is_empty() {
            return Err(format!(
                "no {} on stdin; use `nolune config unset` to clear it",
                key.label()
            ));
        }
        return Ok(value);
    }
    let value = value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            format!(
                "give the new {}, or use `nolune config unset` to clear it",
                key.label()
            )
        })?;
    Ok(value)
}

/// The SMTP password, and the IMAP one when a second line gives it.
fn read_passwords() -> Result<(String, String), String> {
    let input = read_stdin("password: ")?;
    let mut lines = input.lines().map(str::trim);
    let smtp = lines.next().unwrap_or("").to_owned();
    if smtp.is_empty() {
        return Err("no password on stdin".into());
    }
    let imap = lines
        .next()
        .filter(|line| !line.is_empty())
        .unwrap_or(&smtp)
        .to_owned();
    Ok((smtp, imap))
}

fn read_stdin(prompt: &str) -> Result<String, String> {
    let mut stdin = io::stdin();
    if stdin.is_terminal() {
        eprint!("{prompt}");
        let mut line = String::new();
        stdin
            .read_line(&mut line)
            .map_err(|error| format!("cannot read stdin: {error}"))?;
        return Ok(line);
    }
    let mut input = String::new();
    stdin
        .read_to_string(&mut input)
        .map_err(|error| format!("cannot read stdin: {error}"))?;
    Ok(input)
}

/// The profile's data root and its one companion.
struct Target {
    root: PathBuf,
    config_path: PathBuf,
    instance_dir: PathBuf,
}

impl Target {
    fn open(profile: &Profile) -> Result<Self, String> {
        let config_path = profile.root.join("config.toml");
        if !config_path.exists() {
            let flag = if profile.is_default() {
                String::new()
            } else {
                format!(" --profile {}", profile.name)
            };
            return Err(format!(
                "no config at {}; run `nolune onboard{flag}` first",
                config_path.display()
            ));
        }
        Ok(Self {
            instance_dir: profile.root.join("instances").join(CANONICAL_SLUG),
            root: profile.root.clone(),
            config_path,
        })
    }

    fn load_config(&self) -> Result<(Config, String), String> {
        let raw = fs::read_to_string(&self.config_path)
            .map_err(|error| format!("cannot read {}: {error}", self.config_path.display()))?;
        let config = toml::from_str(&raw)
            .map_err(|error| format!("cannot parse {}: {error}", self.config_path.display()))?;
        Ok((config, raw))
    }

    fn save_config(&self, config: &Config, original: &str) -> Result<(), String> {
        let raw = config::serialize_config_preserving_keys(config, original)
            .map_err(|error| format!("cannot serialize config: {error}"))?;
        fs::write(&self.config_path, raw)
            .map_err(|error| format!("cannot write {}: {error}", self.config_path.display()))
    }

    fn project_state_path(&self) -> PathBuf {
        self.instance_dir.join("project_state.json")
    }

    fn load_project_state(&self) -> serde_json::Value {
        fs::read_to_string(self.project_state_path())
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_else(|| serde_json::json!({}))
    }

    fn save_project_state(&self, state: &serde_json::Value) -> Result<(), String> {
        fs::create_dir_all(&self.instance_dir)
            .map_err(|error| format!("cannot create {}: {error}", self.instance_dir.display()))?;
        let body = serde_json::to_string_pretty(state)
            .map_err(|error| format!("cannot serialize project state: {error}"))?;
        let path = self.project_state_path();
        fs::write(&path, body).map_err(|error| format!("cannot write {}: {error}", path.display()))
    }

    fn show(&self) -> Result<String, String> {
        let (config, _) = self.load_config()?;
        let state = self.load_project_state();
        let text = |value: Option<&serde_json::Value>| {
            value
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        };
        let mut lines = vec![
            format!("data root: {}", self.root.display()),
            format!(
                "companion name: {}",
                text(
                    state
                        .get("identity")
                        .and_then(|identity| identity.get("name"))
                )
                .unwrap_or_else(|| "(not set)".into())
            ),
            format!(
                "timezone: {}",
                text(state.get("timezone")).unwrap_or_else(|| "UTC (not set)".into())
            ),
        ];

        if let Some(reason) = config.llm.setup_required() {
            lines.push(format!("llm: setup required: {reason}"));
        } else {
            for (slot, preset) in [
                ("chat", config.llm.chat_preset()),
                ("background", config.llm.background_preset()),
            ] {
                lines.push(match preset {
                    Some(preset) => format!(
                        "{slot} model: {} ({} / {})",
                        preset.name,
                        preset.provider.label(),
                        preset.model
                    ),
                    None => format!("{slot} model: not set"),
                });
            }
        }
        let providers = config.llm.configured_providers();
        lines.push(if providers.is_empty() {
            "api keys: none configured".into()
        } else {
            format!("api keys: {}", providers.join(", "))
        });

        let instance = InstanceConfig::load(&self.root, CANONICAL_SLUG);
        lines.push(format!(
            "github: {}",
            if instance.effective_github_token(&config).is_some() {
                "token configured"
            } else {
                "not connected"
            }
        ));

        lines.push(if config.mcp_servers.is_empty() {
            "extensions (mcp): none".into()
        } else {
            let names: Vec<&str> = config
                .mcp_servers
                .iter()
                .map(|server| server.name.as_str())
                .collect();
            format!("extensions (mcp): {}", names.join(", "))
        });

        let accounts = EmailAccounts::load(&self.root, CANONICAL_SLUG);
        lines.push(if accounts.is_empty() {
            "email accounts (smtp/imap): none".into()
        } else {
            let addresses: Vec<&str> = accounts.iter().map(email_address).collect();
            format!("email accounts (smtp/imap): {}", addresses.join(", "))
        });

        let mut out = lines.join("\n");
        out.push('\n');
        Ok(out)
    }

    /// Set `key` to `value`; an empty value clears it.
    fn set(&self, key: ConfigKey, value: &str) -> Result<(), String> {
        match key {
            ConfigKey::Timezone => {
                if !value.is_empty() && value.parse::<chrono_tz::Tz>().is_err() {
                    return Err(format!(
                        "invalid timezone {value:?}: use an IANA name like Asia/Bishkek"
                    ));
                }
                let mut state = self.load_project_state();
                state["timezone"] = serde_json::Value::String(value.to_owned());
                self.save_project_state(&state)?;
            }
            ConfigKey::Name => {
                let mut state = self.load_project_state();
                if !state
                    .get("identity")
                    .is_some_and(|identity| identity.is_object())
                {
                    state["identity"] = serde_json::json!({});
                }
                state["identity"]["name"] = serde_json::Value::String(value.to_owned());
                self.save_project_state(&state)?;
            }
            ConfigKey::OpenaiKey | ConfigKey::AnthropicKey | ConfigKey::BraveSearchKey => {
                let (mut config, original) = self.load_config()?;
                let tokens = &mut config.llm.tokens;
                *match key {
                    ConfigKey::OpenaiKey => &mut tokens.open_ai,
                    ConfigKey::AnthropicKey => &mut tokens.anthropic,
                    _ => &mut tokens.brave_search,
                } = value.to_owned();
                self.save_config(&config, &original)?;
            }
            ConfigKey::GithubToken => {
                let mut instance = InstanceConfig::load(&self.root, CANONICAL_SLUG);
                instance.github.token = value.to_owned();
                instance
                    .save(&self.root, CANONICAL_SLUG)
                    .map_err(|error| format!("cannot save instance config: {error}"))?;
            }
        }
        match (value.is_empty(), key.is_secret()) {
            (true, _) => println!("{} cleared", key.label()),
            (false, true) => println!("{} updated", key.label()),
            (false, false) => println!("{} → {value}", key.label()),
        }
        Ok(())
    }

    fn add_email(&self, account: EmailConfig) -> Result<(), String> {
        let mut accounts = EmailAccounts::load(&self.root, CANONICAL_SLUG);
        let address = email_address(&account).to_owned();
        if accounts
            .iter()
            .any(|existing| email_address(existing) == address)
        {
            return Err(format!("email account {address} already exists"));
        }
        accounts.push(account);
        EmailAccounts::save(&accounts, &self.root, CANONICAL_SLUG)
            .map_err(|error| format!("cannot save email accounts: {error}"))?;
        println!("added email account {address}");
        Ok(())
    }

    fn remove_email(&self, address: &str) -> Result<(), String> {
        let mut accounts = EmailAccounts::load(&self.root, CANONICAL_SLUG);
        let before = accounts.len();
        accounts.retain(|account| {
            account.smtp_from != address
                && account.smtp_user != address
                && account.imap_user != address
        });
        if accounts.len() == before {
            return Err(format!("no email account {address}"));
        }
        EmailAccounts::save(&accounts, &self.root, CANONICAL_SLUG)
            .map_err(|error| format!("cannot save email accounts: {error}"))?;
        println!("removed email account {address}");
        Ok(())
    }
}

fn email_address(account: &EmailConfig) -> &str {
    if account.smtp_from.is_empty() {
        &account.smtp_user
    } else {
        &account.smtp_from
    }
}
