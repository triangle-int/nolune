//! Known machines (#80): every computer that has ever connected to the one
//! companion, kept as a bounded record so a disconnected desktop stays listed
//! as offline instead of disappearing. The record is what the desktop
//! reported at its last registration plus the name the user gave it; nothing
//! here is model text. Live state (online, heartbeat health) is derived from
//! the registry at read time and never written to disk.

use cua_protocol::{MachineHealth, MachineLocation, PermissionState, Platform};
use serde::{Deserialize, Serialize};

pub const MACHINES_FORMAT_VERSION: u32 = 1;
/// The known-machines file under the companion directory.
pub const MACHINES_FILE: &str = "machines.json";

/// Most records kept; the oldest offline record makes room for a new one.
pub const MAX_KNOWN_MACHINES: usize = 64;
/// Longest machine id accepted at registration; matches `cua_protocol::MAX_ID_BYTES`.
pub const MAX_MACHINE_ID_BYTES: usize = 128;
/// Longest hostname or OS label kept on a record, in characters; longer ones
/// are cut (`normalize_label`), so the file the store writes always fits
/// under its read bound.
pub const MAX_LABEL_CHARS: usize = 256;
/// Longest user-given display name.
pub const MAX_DISPLAY_NAME_CHARS: usize = 64;
/// Most capability names kept per machine.
pub const MAX_CAPABILITIES: usize = 64;
/// A connected agent whose last heartbeat is older than this is `degraded`:
/// the socket is open but the desktop has stopped answering pings (sent
/// every 15 seconds by `routes/machine_agents.rs`).
pub const STALE_HEARTBEAT_SECS: i64 = 45;

/// What a desktop agent that predates capability reporting can do: the
/// frozen coordinate action set plus the shell and file toolcalls.
pub const LEGACY_DESKTOP_CAPABILITIES: [&str; 15] = [
    "screenshot",
    "left_click",
    "right_click",
    "middle_click",
    "double_click",
    "mouse_move",
    "type",
    "key",
    "scroll",
    "switch_desktop",
    "bash",
    "file_read",
    "file_write",
    "file_list",
    "upload_file",
];

/// One persisted machine. Unknown fields are refused so a file written by a
/// newer layout is never partially read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineRecord {
    /// Stable id the desktop persists and sends on every registration.
    pub machine_id: String,
    /// The user's name for this computer; `None` shows the hostname.
    pub display_name: Option<String>,
    pub hostname: String,
    pub os: String,
    /// `None` when the OS label is not one the protocol names.
    pub platform: Option<Platform>,
    pub location: MachineLocation,
    pub screen_width: u32,
    pub screen_height: u32,
    /// Desktop permission state at the last registration; `None` when the
    /// desktop did not report it.
    pub permissions: Option<PermissionState>,
    /// Legacy action names the desktop accepts.
    pub capabilities: Vec<String>,
    /// Unix seconds of the first registration.
    pub first_seen: i64,
    /// Unix seconds of the last heartbeat or disconnect.
    pub last_seen: i64,
}

/// The on-disk file: version, owning companion, and the records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachinesFile {
    pub version: u32,
    pub slug: String,
    pub machines: Vec<MachineRecord>,
}

/// A known machine as the API and the `machine_updated` event report it: the
/// record plus what the registry knows right now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownMachine {
    pub machine_id: String,
    /// The user's name when set, otherwise the hostname.
    pub display_name: String,
    /// The user's name, `None` while the hostname is shown.
    pub custom_name: Option<String>,
    pub hostname: String,
    pub os: String,
    pub platform: Option<Platform>,
    pub location: MachineLocation,
    pub screen_width: u32,
    pub screen_height: u32,
    pub permissions: Option<PermissionState>,
    pub capabilities: Vec<String>,
    pub first_seen: i64,
    pub last_seen: i64,
    /// Always the canonical companion; kept for clients that read it.
    pub instance_slug: Option<String>,
    /// Whether the desktop agent's socket is open right now.
    pub online: bool,
    /// Derived from heartbeat age: `healthy`, `degraded` (open socket, stale
    /// heartbeat), or `unavailable` (offline).
    pub health: MachineHealth,
    /// Reserved for the Cua driver (#18); `None` means not reported.
    pub driver_version: Option<String>,
    /// Reserved for the Cua driver's own health (#18); `None` means not reported.
    pub cua_health: Option<MachineHealth>,
}

/// Health from heartbeat age alone: offline machines are unavailable, a
/// connected one is healthy until its heartbeat goes stale.
pub fn heartbeat_health(online: bool, heartbeat_age_secs: i64) -> MachineHealth {
    if !online {
        MachineHealth::Unavailable
    } else if heartbeat_age_secs > STALE_HEARTBEAT_SECS {
        MachineHealth::Degraded
    } else {
        MachineHealth::Healthy
    }
}

/// The protocol platform for an `std::env::consts::OS` label, when it names one.
pub fn platform_from_os(os: &str) -> Option<Platform> {
    match os {
        "macos" => Some(Platform::Macos),
        "windows" => Some(Platform::Windows),
        "linux" => Some(Platform::Linux),
        _ => None,
    }
}

fn is_capability_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Capability names as the record keeps them: short lower-case identifiers,
/// deduplicated, at most `MAX_CAPABILITIES`. An empty report means a desktop
/// that predates capability reporting, which accepts the legacy set.
pub fn normalize_capabilities(reported: Vec<String>) -> Vec<String> {
    if reported.is_empty() {
        return LEGACY_DESKTOP_CAPABILITIES
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
    }
    let mut kept: Vec<String> = Vec::new();
    for name in reported {
        if is_capability_name(&name) && !kept.contains(&name) {
            kept.push(name);
        }
        if kept.len() == MAX_CAPABILITIES {
            break;
        }
    }
    kept
}

/// A hostname or OS label as the record keeps it: the first
/// `MAX_LABEL_CHARS` characters of what the desktop reported.
pub fn normalize_label(label: &str) -> String {
    label.chars().take(MAX_LABEL_CHARS).collect()
}

/// A machine id the record can carry: one non-empty line of at most
/// `MAX_MACHINE_ID_BYTES` in the protocol's identifier grammar (letters,
/// digits, `-`, `_`, `.`, `:`), so a UUID and a hostname both fit and a path
/// or a control character never does.
pub fn validate_machine_id(machine_id: &str) -> Result<(), String> {
    if machine_id.is_empty() || machine_id.len() > MAX_MACHINE_ID_BYTES {
        return Err(format!(
            "machine id must be 1 to {MAX_MACHINE_ID_BYTES} bytes"
        ));
    }
    if !machine_id
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
    {
        return Err("machine id may only contain letters, digits, '-', '_', '.', and ':'".into());
    }
    Ok(())
}

/// A display name as the record keeps it: trimmed, single line, at most
/// `MAX_DISPLAY_NAME_CHARS`. Blank clears the name.
pub fn normalize_display_name(display_name: Option<&str>) -> Result<Option<String>, String> {
    let Some(name) = display_name.map(str::trim).filter(|name| !name.is_empty()) else {
        return Ok(None);
    };
    if name.chars().any(char::is_control) {
        return Err("display name must be a single line".into());
    }
    if name.chars().count() > MAX_DISPLAY_NAME_CHARS {
        return Err(format!(
            "display name must be at most {MAX_DISPLAY_NAME_CHARS} characters"
        ));
    }
    Ok(Some(name.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_follows_heartbeat_age() {
        assert_eq!(heartbeat_health(false, 0), MachineHealth::Unavailable);
        assert_eq!(heartbeat_health(false, 10_000), MachineHealth::Unavailable);
        assert_eq!(heartbeat_health(true, 0), MachineHealth::Healthy);
        assert_eq!(
            heartbeat_health(true, STALE_HEARTBEAT_SECS),
            MachineHealth::Healthy
        );
        assert_eq!(
            heartbeat_health(true, STALE_HEARTBEAT_SECS + 1),
            MachineHealth::Degraded
        );
        // A clock that went backwards is not a stale heartbeat.
        assert_eq!(heartbeat_health(true, -30), MachineHealth::Healthy);
    }

    #[test]
    fn platform_is_derived_from_the_os_label() {
        assert_eq!(platform_from_os("macos"), Some(Platform::Macos));
        assert_eq!(platform_from_os("windows"), Some(Platform::Windows));
        assert_eq!(platform_from_os("linux"), Some(Platform::Linux));
        assert_eq!(platform_from_os("freebsd"), None);
        assert_eq!(platform_from_os(""), None);
    }

    #[test]
    fn capabilities_are_normalized_and_default_to_the_legacy_set() {
        let legacy: Vec<String> = LEGACY_DESKTOP_CAPABILITIES
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        assert_eq!(normalize_capabilities(Vec::new()), legacy);

        let reported = vec![
            "screenshot".to_owned(),
            "screenshot".to_owned(),
            "Bash".to_owned(),
            "has space".to_owned(),
            "".to_owned(),
            "x".repeat(65),
            "file_read".to_owned(),
        ];
        assert_eq!(
            normalize_capabilities(reported),
            vec!["screenshot".to_owned(), "file_read".to_owned()],
            "duplicates, empty, oversized and non-identifier names are dropped"
        );

        let many: Vec<String> = (0..MAX_CAPABILITIES + 10)
            .map(|i| format!("cap_{i}"))
            .collect();
        assert_eq!(normalize_capabilities(many).len(), MAX_CAPABILITIES);

        // Nothing valid reported: that is not a legacy desktop, it reports nothing.
        assert!(normalize_capabilities(vec!["has space".to_owned()]).is_empty());
    }

    #[test]
    fn labels_are_cut_at_the_bound() {
        assert_eq!(normalize_label("studio.local"), "studio.local");
        assert_eq!(normalize_label(""), "");
        assert_eq!(
            normalize_label(&"é".repeat(MAX_LABEL_CHARS + 10)),
            "é".repeat(MAX_LABEL_CHARS),
            "the bound counts characters, not bytes"
        );
    }

    #[test]
    fn machine_ids_are_bounded_identifiers() {
        assert!(validate_machine_id("4f3c1c2e-9b5e-4d2b-8f0a-1c2d3e4f5a6b").is_ok());
        assert!(validate_machine_id("Tims-MacBook-Pro.local").is_ok());
        assert!(validate_machine_id("server-local:studio").is_ok());
        assert!(validate_machine_id("").is_err());
        assert!(validate_machine_id(&"a".repeat(MAX_MACHINE_ID_BYTES + 1)).is_err());
        assert!(validate_machine_id("has space").is_err());
        assert!(validate_machine_id("new\nline").is_err());
        assert!(validate_machine_id("../escape").is_err());
    }

    #[test]
    fn display_names_are_trimmed_bounded_and_blank_clears() {
        assert_eq!(
            normalize_display_name(Some("  Studio Mac  ")).unwrap(),
            Some("Studio Mac".to_owned())
        );
        assert_eq!(normalize_display_name(Some("   ")).unwrap(), None);
        assert_eq!(normalize_display_name(Some("")).unwrap(), None);
        assert_eq!(normalize_display_name(None).unwrap(), None);
        assert!(normalize_display_name(Some(&"n".repeat(MAX_DISPLAY_NAME_CHARS + 1))).is_err());
        assert!(normalize_display_name(Some("two\nlines")).is_err());
        assert_eq!(
            normalize_display_name(Some(&"é".repeat(MAX_DISPLAY_NAME_CHARS))).unwrap(),
            Some("é".repeat(MAX_DISPLAY_NAME_CHARS)),
            "the bound counts characters, not bytes"
        );
    }
}
