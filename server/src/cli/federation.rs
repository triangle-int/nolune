//! `nolune federation …` (#108, PR 4): the owner's command-line surface for
//! companion federation, driven through the running server's owner routes
//! (`/api/federation/*`) with the profile's API token, the way `nolune pair`
//! mints a browser code.
//!
//! * `invite` mints a one-time invite and prints it exactly once, as one
//!   line to hand to the other owner out of band.
//! * `accept` takes that line as an argument or on stdin, never from a URL,
//!   checks its shape locally, and hands it to this server, which redeems
//!   it with the issuer over the wire.
//! * `peers` lists this companion's id, outstanding invites (without their
//!   secrets), and every peer with its state, origins, and last sighting.
//! * `confirm` pairs a peer that redeemed an invite this server minted.
//! * `revoke` withdraws trust; `rotate` replaces this companion's key.
//!
//! `--profile` selects the server like every other subcommand. Nothing here
//! logs, nothing reads or builds a URL that carries an invite, and the only
//! line that ever carries an invite secret is the one `invite` prints.
//!
//! The commands that make the server talk to peers (`accept`, `confirm`,
//! `revoke`, `rotate`) wait for its report however long that takes: the
//! server bounds every peer attempt with its own transport timeout, one
//! origin after another, and the CLI does not guess at how many peers and
//! origins that is. A server that was reached but never answered is told
//! apart from one that is not running, because the work may stand there.

use std::{
    io::{self, IsTerminal, Read},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use clap::Subcommand;
use serde_json::Value;

use crate::{
    config::{self, Config, Profile},
    services::federation::invite_token,
};

#[derive(Subcommand)]
pub enum FederationAction {
    /// Mint a one-time invite for another owner and print it once
    Invite {
        /// Print the outcome as one JSON line instead of human-readable text
        #[arg(long)]
        json: bool,
    },
    /// Redeem an invite another owner handed you (as an argument, or on stdin when omitted)
    Accept {
        /// The invite line (`nolune-invite-v1.…`); `-` or nothing reads it from stdin
        #[arg(value_name = "INVITE")]
        invite: Option<String>,
        /// Print the outcome as one JSON line
        #[arg(long)]
        json: bool,
    },
    /// List this companion's identity, outstanding invites, and peers
    Peers {
        /// Print the server's listing as one JSON line
        #[arg(long)]
        json: bool,
    },
    /// Pair a peer that redeemed an invite this server minted
    Confirm {
        /// The peer's companion id, as `nolune federation peers` lists it
        // A companion id is base64url, so one in 64 starts with `-`.
        #[arg(value_name = "COMPANION_ID", allow_hyphen_values = true)]
        companion_id: String,
    },
    /// Withdraw trust from a peer and tell it
    Revoke {
        /// The peer's companion id
        #[arg(value_name = "COMPANION_ID", allow_hyphen_values = true)]
        companion_id: String,
    },
    /// Replace this companion's signing key and tell every paired peer
    Rotate {
        /// Rotate without asking (required when not running in a terminal)
        #[arg(long, short = 'y')]
        yes: bool,
        /// Print the report as one JSON line
        #[arg(long)]
        json: bool,
    },
}

pub fn run(action: FederationAction, profile: &Profile) -> i32 {
    match action {
        FederationAction::Invite { json } => invite(json, profile),
        FederationAction::Accept { invite, json } => accept(invite, json, profile),
        FederationAction::Peers { json } => peers(json, profile),
        FederationAction::Confirm { companion_id } => confirm(&companion_id, profile),
        FederationAction::Revoke { companion_id } => revoke(&companion_id, profile),
        FederationAction::Rotate { yes, json } => rotate(yes, json, profile),
    }
}

// ── Talking to the running server ───────────────────────────────────────

/// The running server of the selected profile, as the owner.
struct Owner {
    base: String,
    token: String,
}

impl Owner {
    /// Reads the profile's config; a missing one is created like `pair` does.
    fn load() -> Result<Self, i32> {
        let config = match config::load_config() {
            Ok(config) => config,
            Err(error) => {
                eprintln!("cannot read {}: {error}", config::config_path().display());
                return Err(1);
            }
        };
        Ok(Self {
            base: connect_base(&config),
            token: config.auth_token,
        })
    }
}

/// `http://<host>:<port>` of the local server, from the bind address.
fn connect_base(config: &Config) -> String {
    let host = match config.host.as_str() {
        "" | "0.0.0.0" => "127.0.0.1".to_string(),
        "::" | "[::]" => "[::1]".to_string(),
        other => other.to_string(),
    };
    format!("http://{host}:{}", config.port)
}

/// Connecting to the server: loopback answers or refuses at once, and a
/// bound address elsewhere that swallows the handshake is given this long.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// A server answering from its own state that takes longer than this is
/// wedged, and saying so beats hanging.
const LOCAL_ANSWER_TIMEOUT: Duration = Duration::from_secs(30);

/// How long the CLI waits for the server's answer.
#[derive(Clone, Copy)]
enum Wait {
    /// The server answers from its own state.
    Local,
    /// The server talks to peers before it can answer, each attempt bounded
    /// by its own transport timeout and one origin after another. Only the
    /// server knows how many that is, so the CLI puts no cap of its own
    /// on the answer: a cap shorter than the server's worst case would
    /// call a rotation that happened "not reachable".
    Peers,
}

enum CallError {
    /// The server did not take the request: nothing happened there.
    Unreachable(String),
    /// The server took the request and no answer came back: the work
    /// may stand there.
    Unanswered(String),
    /// The server refused the profile's token.
    Unauthorized,
    /// The server answered with an error body.
    Refused { status: u16, body: Value },
}

/// One owner request: the answer is JSON, or nothing (204).
fn call(
    owner: &Owner,
    method: &str,
    path: &str,
    body: Option<Value>,
    wait: Wait,
) -> Result<Value, CallError> {
    let url = format!("{}{path}", owner.base);
    let token = owner.token.clone();
    let method = reqwest::Method::from_bytes(method.as_bytes()).expect("static method");
    // `main` is already inside the tokio runtime; the request runs on its own.
    let outcome = super::on_own_runtime(move || async move {
        let mut builder = reqwest::Client::builder().connect_timeout(CONNECT_TIMEOUT);
        if let Wait::Local = wait {
            builder = builder.timeout(LOCAL_ANSWER_TIMEOUT);
        }
        let client = builder
            .build()
            .map_err(|error| CallError::Unreachable(describe(&error)))?;
        let mut request = client.request(method, &url);
        if !token.is_empty() {
            request = request.bearer_auth(&token);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.map_err(|error| {
            if error.is_connect() {
                CallError::Unreachable(describe(&error))
            } else {
                CallError::Unanswered(describe(&error))
            }
        })?;
        let status = response.status();
        // The status arrived, so the server acted; a body cut short is
        // still an answer that never came whole.
        let text = response
            .text()
            .await
            .map_err(|error| CallError::Unanswered(describe(&error)))?;
        let value = serde_json::from_str(&text).unwrap_or(Value::Null);
        Ok::<_, CallError>((status, value))
    });
    match outcome {
        Err(error) => Err(CallError::Unreachable(error)),
        Ok(Err(error)) => Err(error),
        Ok(Ok((status, _))) if status == reqwest::StatusCode::UNAUTHORIZED => {
            Err(CallError::Unauthorized)
        }
        Ok(Ok((status, body))) if !status.is_success() => Err(CallError::Refused {
            status: status.as_u16(),
            body,
        }),
        Ok(Ok((_, body))) => Ok(body),
    }
}

/// A transport error with its causes, so "error sending request" says
/// whether the connection was refused, timed out, or closed.
fn describe(error: &reqwest::Error) -> String {
    let mut text = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

/// Reports a failed call in the profile's terms and returns the exit code.
fn report(error: CallError, verb: &str, profile: &Profile) -> i32 {
    let flag = super::profile_flag(profile);
    match error {
        CallError::Unreachable(error) => {
            eprintln!(
                "{} is not reachable ({error}).\nStart it with `nolune gateway{flag}` and try again.",
                super::display(profile)
            );
        }
        CallError::Unanswered(error) => {
            eprintln!(
                "{} took the request but no answer came back ({error}).\nIt may have finished {verb} anyway: `nolune federation peers{flag}` shows what stands there, so check that before running this again.",
                super::display(profile)
            );
        }
        CallError::Unauthorized => {
            eprintln!(
                "The running server rejected the token from {}.\nIf the service was started with NOLUNE_AUTH_TOKEN, run this command with the same value.",
                config::config_path().display()
            );
        }
        CallError::Refused { status, body } => {
            let code = body["error"].as_str().unwrap_or("error");
            let message = body["message"].as_str().unwrap_or("");
            let detail = match (body["peer_status"].as_u64(), body["peer_error"].as_str()) {
                (Some(peer_status), Some(peer_error)) => {
                    format!(" (the peer answered {peer_status} {peer_error})")
                }
                _ => String::new(),
            };
            eprintln!("{verb} failed: HTTP {status} {code}: {message}{detail}");
        }
    }
    1
}

// ── Subcommands ─────────────────────────────────────────────────────────

fn invite(json: bool, profile: &Profile) -> i32 {
    let owner = match Owner::load() {
        Ok(owner) => owner,
        Err(code) => return code,
    };
    let issued = match call(&owner, "POST", "/api/federation/invites", None, Wait::Local) {
        Ok(issued) => issued,
        Err(error) => return report(error, "minting an invite", profile),
    };
    let Some(line) = issued["invite"].as_str() else {
        eprintln!("the server did not return an invite line");
        return 1;
    };
    let minutes = issued["expires_in_secs"].as_u64().unwrap_or(600) / 60;
    let flag = super::profile_flag(profile);
    if json {
        // One machine-readable line: the invite line and its handles, no
        // separate secret field.
        let out = serde_json::json!({
            "id": issued["id"],
            "invite": line,
            "created_at": issued["created_at"],
            "expires_at": issued["expires_at"],
            "expires_in_secs": issued["expires_in_secs"],
            "origin": issued["origin"],
            "issuer_companion_id": issued["issuer"]["companion_id"],
        });
        println!("{out}");
        return 0;
    }
    println!();
    println!(
        "  Companion invite from {} (works once, expires in {minutes} minutes):",
        super::display(profile)
    );
    println!();
    print_invite_once(line);
    println!();
    println!("  Hand this line to the other owner out of band; never put it in a URL.");
    println!(
        "  On their server they run `nolune federation accept` and paste it, or paste it under"
    );
    println!("  Settings › Connections › Companions. Their companion then shows as pending in");
    println!(
        "  `nolune federation peers{flag}`; `nolune federation confirm <companion id>{flag}` pairs it."
    );
    println!();
    0
}

/// The one place an invite line reaches stdout.
fn print_invite_once(line: &str) {
    println!("  {line}");
}

/// The invite line from the argument, or from stdin when the argument is
/// absent or `-`. Trimmed; empty is an error.
fn read_invite_line(argument: Option<String>) -> Result<String, i32> {
    let text = match argument {
        Some(text) if text != "-" => text,
        _ => {
            let mut text = String::new();
            if let Err(error) = io::stdin().read_to_string(&mut text) {
                eprintln!("cannot read the invite from stdin: {error}");
                return Err(1);
            }
            text
        }
    };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        eprintln!(
            "no invite given: pass the nolune-invite line as an argument, or pipe it on stdin"
        );
        return Err(1);
    }
    Ok(trimmed.to_owned())
}

fn accept(argument: Option<String>, json: bool, profile: &Profile) -> i32 {
    let line = match read_invite_line(argument) {
        Ok(line) => line,
        Err(code) => return code,
    };
    // Checked here with the same codec the server uses, before anything is
    // read from disk or sent anywhere: a URL is never an invite.
    if invite_token::looks_like_url(&line) {
        eprintln!(
            "an invite is never a URL and is never fetched from one; paste the nolune-invite line the other owner gave you"
        );
        return 1;
    }
    if let Err(error) = invite_token::decode_invite(&line) {
        eprintln!("{error}");
        return 1;
    }
    let owner = match Owner::load() {
        Ok(owner) => owner,
        Err(code) => return code,
    };
    // The server redeems the invite with the issuer over the wire.
    let answer = match call(
        &owner,
        "POST",
        "/api/federation/accept",
        Some(serde_json::json!({ "invite": line })),
        Wait::Peers,
    ) {
        Ok(answer) => answer,
        Err(error) => return report(error, "accepting the invite", profile),
    };
    if json {
        println!("{answer}");
        return 0;
    }
    let peer = &answer["peer"];
    let id = peer["companion_id"].as_str().unwrap_or("?");
    let origin = peer["approved_origins"][0].as_str().unwrap_or("?");
    let flag = super::profile_flag(profile);
    println!("Accepted the invite from companion {id} at {origin}.");
    println!(
        "It is pending until its owner confirms it on their server (`nolune federation confirm <your companion id>` there, or under Settings › Connections › Companions)."
    );
    println!("`nolune federation peers{flag}` shows it paired once they have.");
    0
}

fn peers(json: bool, profile: &Profile) -> i32 {
    let owner = match Owner::load() {
        Ok(owner) => owner,
        Err(code) => return code,
    };
    let overview = match call(&owner, "GET", "/api/federation/peers", None, Wait::Local) {
        Ok(overview) => overview,
        Err(error) => return report(error, "listing peers", profile),
    };
    if json {
        println!("{overview}");
        return 0;
    }
    let flag = super::profile_flag(profile);
    let now = now_secs();
    println!(
        "Companion {}{}",
        overview["companion_id"].as_str().unwrap_or("?"),
        if profile.is_default() {
            String::new()
        } else {
            format!(" (profile {})", profile.name)
        }
    );
    println!(
        "  key: {}",
        overview["identity"]["public_key"].as_str().unwrap_or("?")
    );
    let rotations = overview["rotations"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if let Some(last) = rotations.last() {
        println!(
            "  identity rotated {} time{}; previous id: {}",
            rotations.len(),
            if rotations.len() == 1 { "" } else { "s" },
            last["previous"]["companion_id"].as_str().unwrap_or("?")
        );
    }
    let invites = overview["invites"].as_array().cloned().unwrap_or_default();
    if invites.is_empty() {
        println!("  invites: none outstanding");
    } else {
        let expiries: Vec<String> = invites
            .iter()
            .map(|invite| {
                let left = invite["expires_at"]
                    .as_u64()
                    .unwrap_or(0)
                    .saturating_sub(now);
                format!("{} min", left.div_ceil(60))
            })
            .collect();
        println!(
            "  invites: {} outstanding (expire in {}); each secret was shown once when minted",
            invites.len(),
            expiries.join(", ")
        );
    }
    let peers = overview["peers"].as_array().cloned().unwrap_or_default();
    if peers.is_empty() {
        println!(
            "  peers: no peers yet; `nolune federation invite{flag}` mints an invite, `nolune federation accept{flag}` redeems one"
        );
        return 0;
    }
    println!("  peers:");
    for peer in &peers {
        let id = peer["companion_id"].as_str().unwrap_or("?");
        let state = peer["state"].as_str().unwrap_or("?");
        let role = match peer["role"].as_str() {
            Some("issuer") => "you invited it",
            Some("accepter") => "it invited you",
            _ => "?",
        };
        let origins: Vec<&str> = peer["approved_origins"]
            .as_array()
            .map(|list| list.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let reached = if origins.is_empty() {
            "no approved origin".to_owned()
        } else {
            format!("reached at {}", origins.join(", "))
        };
        let seen = match peer["last_seen_at"].as_u64() {
            Some(at) => format!("last seen {}", ago(now, at)),
            None => "never seen".to_owned(),
        };
        let history = peer["rotation_history"]
            .as_array()
            .map_or(0, |list| list.len());
        let rotated = match history {
            0 => String::new(),
            1 => " · rotated its key once".to_owned(),
            n => format!(" · rotated its key {n} times"),
        };
        println!("    {id}");
        println!("      {state} · {role} · {reached} · {seen}{rotated}");
        match (state, peer["role"].as_str()) {
            ("pending", Some("issuer")) => println!(
                "      waiting for you: `nolune federation confirm {id}{flag}` pairs it; it reported {}",
                peer["pending_origin"].as_str().unwrap_or("no origin")
            ),
            ("pending", _) => println!("      waiting for its owner to confirm on their server"),
            ("revoked", _) => {
                println!("      trust withdrawn; a new invite pairs it again")
            }
            _ => {}
        }
    }
    0
}

fn confirm(companion_id: &str, profile: &Profile) -> i32 {
    let owner = match Owner::load() {
        Ok(owner) => owner,
        Err(code) => return code,
    };
    let answer = match call(
        &owner,
        "POST",
        &format!(
            "/api/federation/peers/{}/confirm",
            encode_path(companion_id)
        ),
        None,
        Wait::Peers,
    ) {
        Ok(answer) => answer,
        Err(error) => return report(error, "confirming the peer", profile),
    };
    let peer = &answer["peer"];
    let origins: Vec<&str> = peer["approved_origins"]
        .as_array()
        .map(|list| list.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    println!(
        "Companion {} is {}; approved origin{}: {}",
        peer["companion_id"].as_str().unwrap_or(companion_id),
        peer["state"].as_str().unwrap_or("?"),
        if origins.len() == 1 { "" } else { "s" },
        if origins.is_empty() {
            "none".to_owned()
        } else {
            origins.join(", ")
        }
    );
    if answer["notified"].as_bool() == Some(true) {
        println!("Its owner's server was notified and acknowledged.");
    } else {
        println!(
            "Its server could not be reached; the pairing stands here, and confirming again resends the notice."
        );
    }
    0
}

fn revoke(companion_id: &str, profile: &Profile) -> i32 {
    let owner = match Owner::load() {
        Ok(owner) => owner,
        Err(code) => return code,
    };
    let answer = match call(
        &owner,
        "POST",
        &format!("/api/federation/peers/{}/revoke", encode_path(companion_id)),
        None,
        Wait::Peers,
    ) {
        Ok(answer) => answer,
        Err(error) => return report(error, "revoking the peer", profile),
    };
    let id = answer["peer"]["companion_id"]
        .as_str()
        .unwrap_or(companion_id);
    if answer["notified"].as_bool() == Some(true) {
        println!(
            "Companion {id} revoked; its server was notified and no longer trusts this companion either."
        );
    } else {
        println!(
            "Companion {id} revoked here; its server was not notified (it was already revoked, or could not be reached)."
        );
    }
    0
}

fn rotate(yes: bool, json: bool, profile: &Profile) -> i32 {
    if !yes && !confirm_rotation(profile) {
        return 1;
    }
    let owner = match Owner::load() {
        Ok(owner) => owner,
        Err(code) => return code,
    };
    // Every paired peer is told before the report exists; this waits for it.
    let report_body = match call(&owner, "POST", "/api/federation/rotate", None, Wait::Peers) {
        Ok(report_body) => report_body,
        Err(error) => return report(error, "rotating the identity", profile),
    };
    if json {
        println!("{report_body}");
        return 0;
    }
    let names = |key: &str| -> Vec<String> {
        report_body[key]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    };
    let notified = names("notified");
    let unreachable = names("unreachable");
    println!(
        "Identity rotated: companion {} is now companion {}.",
        report_body["rotation"]["previous"]["companion_id"]
            .as_str()
            .unwrap_or("?"),
        report_body["identity"]["companion_id"]
            .as_str()
            .unwrap_or("?")
    );
    println!(
        "  proof kept in federation/rotations.json; outstanding invites and pending pairings were withdrawn"
    );
    if notified.is_empty() && unreachable.is_empty() {
        println!("  no paired peers to tell");
    }
    if !notified.is_empty() {
        println!("  notified: {}", notified.join(", "));
    }
    if !unreachable.is_empty() {
        println!(
            "  unreachable: {} (they keep trusting the old key until they hear the proof)",
            unreachable.join(", ")
        );
    }
    0
}

/// Ask before rotating. Non-interactive callers must pass `--yes` explicitly.
fn confirm_rotation(profile: &Profile) -> bool {
    if !io::stdin().is_terminal() {
        eprintln!(
            "refusing to rotate the identity of {} without confirmation: pass --yes",
            super::display(profile)
        );
        return false;
    }
    eprint!(
        "This gives {} a new signing key and id, withdraws its invites and pending pairings, and tells every paired peer. Continue? [y/N] ",
        super::display(profile)
    );
    let mut answer = String::new();
    if io::stdin().read_line(&mut answer).is_err() {
        return false;
    }
    if matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        true
    } else {
        eprintln!("aborted; nothing was rotated");
        false
    }
}

// ── Small helpers ───────────────────────────────────────────────────────

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

/// "just now", "5 min ago", "3 h ago", "2 days ago".
fn ago(now: u64, then: u64) -> String {
    let delta = now.saturating_sub(then);
    if delta < 90 {
        "just now".to_owned()
    } else if delta < 3600 {
        format!("{} min ago", delta.div_ceil(60))
    } else if delta < 86_400 * 2 {
        format!("{} h ago", delta / 3600)
    } else {
        format!("{} days ago", delta / 86_400)
    }
}

/// Companion ids are base64url, so only a stray character needs escaping.
fn encode_path(segment: &str) -> String {
    segment
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
                c.to_string()
            } else {
                format!("%{:02X}", c as u32)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Companion ids are base64url, whose alphabet has `-`: an id that
    /// starts with one is still the id, not an unknown flag.
    #[test]
    fn a_companion_id_that_starts_with_a_hyphen_is_an_id() {
        use clap::Parser as _;
        let id = "-6Qx0bW9vbi1jb21wYW5pb24taWQtZm9yLXRlc3Rz";
        for command in ["confirm", "revoke"] {
            let cli = crate::cli::Cli::try_parse_from(["nolune", "federation", command, id])
                .unwrap_or_else(|error| panic!("{command}: {error}"));
            let Some(crate::cli::CliCommand::Federation { action }) = cli.command else {
                panic!("{command}: not a federation command");
            };
            match action {
                FederationAction::Confirm { companion_id }
                | FederationAction::Revoke { companion_id } => assert_eq!(companion_id, id),
                _ => panic!("{command}: parsed as another action"),
            }
        }
    }

    #[test]
    fn connect_base_follows_the_bind_address() {
        let mut config = Config {
            port: 26701,
            ..Config::default()
        };
        for (host, expected) in [
            ("", "http://127.0.0.1:26701"),
            ("0.0.0.0", "http://127.0.0.1:26701"),
            ("::", "http://[::1]:26701"),
            ("127.0.0.1", "http://127.0.0.1:26701"),
            ("192.168.1.5", "http://192.168.1.5:26701"),
        ] {
            config.host = host.into();
            assert_eq!(connect_base(&config), expected, "{host:?}");
        }
    }

    #[test]
    fn sightings_are_words() {
        assert_eq!(ago(1000, 1000), "just now");
        assert_eq!(ago(1000, 950), "just now");
        assert_eq!(ago(1000, 1500), "just now", "a clock ahead of ours");
        assert_eq!(ago(10_000, 9_400), "10 min ago");
        assert_eq!(ago(100_000, 92_800), "2 h ago");
        assert_eq!(ago(1_000_000, 700_000), "3 days ago");
    }

    #[test]
    fn path_segments_keep_base64url_and_escape_the_rest() {
        assert_eq!(encode_path("TFccHElq-_1"), "TFccHElq-_1");
        assert_eq!(encode_path("a/b c"), "a%2Fb%20c");
    }
}
