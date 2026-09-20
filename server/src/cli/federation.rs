//! `nolune federation …` (#108, PR 4): the owner's command-line surface for
//! companion federation, driven through the running server's owner routes
//! (`/api/federation/*`) with the profile's API token, the way `nolune pair`
//! mints a browser code.
//!
//! * `invite` mints a one-time invite and prints it exactly once, as one
//!   line to hand to the other owner out of band.
//! * `accept` takes that line as an argument or on stdin, never from a URL,
//!   and hands it to this server, which redeems it with the issuer.
//! * `peers` lists this companion's id, outstanding invites (without their
//!   secrets), and every peer with its state, origins, and last sighting.
//! * `confirm` pairs a peer that redeemed an invite this server minted.
//! * `revoke` withdraws trust; `rotate` replaces this companion's key.
//!
//! `--profile` selects the server like every other subcommand. Nothing here
//! logs, and the only line that ever carries an invite secret is the one
//! `invite` prints.

use std::io::{self, IsTerminal, Read};

use clap::Subcommand;

use crate::{
    config::{self, Profile},
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
        #[arg(value_name = "COMPANION_ID")]
        companion_id: String,
    },
    /// Withdraw trust from a peer and tell it
    Revoke {
        /// The peer's companion id
        #[arg(value_name = "COMPANION_ID")]
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
    let _ = (
        profile,
        io::stdin().is_terminal(),
        config::config_path(),
        invite_token::INVITE_TOKEN_PREFIX,
        io::stdin().read_to_string(&mut String::new()),
    );
    match action {
        FederationAction::Invite { .. }
        | FederationAction::Accept { .. }
        | FederationAction::Peers { .. }
        | FederationAction::Confirm { .. }
        | FederationAction::Revoke { .. }
        | FederationAction::Rotate { .. } => todo!("#108 PR 4: nolune federation"),
    }
}
