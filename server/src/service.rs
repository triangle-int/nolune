//! Opt-in background service for the gateway (#125).
//!
//! `nolune gateway install` writes a user-level launchd agent (macOS) or systemd user
//! unit (Linux) that runs `nolune gateway`, and starts it. Nothing here runs unless the
//! user asks; a plain install leaves the gateway in the foreground.
//!
//! Every profile (#107) gets its own definition, label, and unit name; the default
//! profile keeps the pre-profile names and arguments so an upgrade finds its own service.

use std::{
    fs, io,
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    time::Duration,
};

use crate::config::DEFAULT_PROFILE;

pub const LABEL: &str = "dev.nolune.nolune";
pub const UNIT_NAME: &str = "nolune";

/// What the service definition needs to know: which binary to run, where its home is, and
/// which profile it serves (#107).
pub struct ServiceSpec {
    pub binary: PathBuf,
    pub home: PathBuf,
    pub profile: String,
}

/// The launchd label for a profile's service.
pub fn label(profile: &str) -> String {
    if profile == DEFAULT_PROFILE {
        LABEL.to_owned()
    } else {
        format!("{LABEL}.{profile}")
    }
}

/// The systemd user unit name for a profile's service.
pub fn unit_name(profile: &str) -> String {
    if profile == DEFAULT_PROFILE {
        UNIT_NAME.to_owned()
    } else {
        format!("{UNIT_NAME}-{profile}")
    }
}

/// The data root a written definition runs, read back from either format, so install can
/// tell whose service already owns a root.
pub fn definition_home(contents: &str) -> Option<PathBuf> {
    let mut lines = contents.lines().map(str::trim);
    while let Some(line) = lines.next() {
        if let Some(value) = line.strip_prefix("Environment=NOLUNE_HOME=") {
            return Some(PathBuf::from(value));
        }
        if line == "<key>NOLUNE_HOME</key>" {
            return lines
                .next()
                .and_then(|next| next.strip_prefix("<string>"))
                .and_then(|rest| rest.strip_suffix("</string>"))
                .map(PathBuf::from);
        }
    }
    None
}

/// Where this platform keeps the service definition, relative to the user's home directory.
pub fn definition_path(home_dir: &Path, profile: &str) -> PathBuf {
    if cfg!(target_os = "macos") {
        home_dir
            .join("Library/LaunchAgents")
            .join(format!("{}.plist", label(profile)))
    } else {
        home_dir
            .join(".config/systemd/user")
            .join(format!("{}.service", unit_name(profile)))
    }
}

/// The `ProgramArguments` entries after the binary: the default profile runs the bare
/// `gateway` it always did; a named profile names itself so `ps` and the logs show it.
fn plist_arguments(profile: &str) -> String {
    if profile == DEFAULT_PROFILE {
        "        <string>gateway</string>\n".to_owned()
    } else {
        format!(
            "        <string>gateway</string>\n        <string>run</string>\n        <string>--profile</string>\n        <string>{profile}</string>\n"
        )
    }
}

/// The launchd agent, byte-compatible with what the shell installer wrote, except it runs
/// `nolune gateway` explicitly.
pub fn render_launchd_plist(spec: &ServiceSpec) -> String {
    let binary = spec.binary.display();
    let home = spec.home.display();
    let label = label(&spec.profile);
    let arguments = plist_arguments(&spec.profile);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{binary}</string>
{arguments}    </array>
    <key>WorkingDirectory</key>
    <string>{home}</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>NOLUNE_HOME</key>
        <string>{home}</string>
        <key>RUST_LOG</key>
        <string>info</string>
    </dict>
    <key>KeepAlive</key>
    <true/>
    <key>RunAtLoad</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{home}/nolune.log</string>
    <key>StandardErrorPath</key>
    <string>{home}/nolune.log</string>
</dict>
</plist>
"#
    )
}

/// The systemd user unit, likewise.
pub fn render_systemd_unit(spec: &ServiceSpec) -> String {
    let binary = spec.binary.display();
    let home = spec.home.display();
    let (description, arguments) = if spec.profile == DEFAULT_PROFILE {
        (String::new(), String::new())
    } else {
        (
            format!(" (profile {})", spec.profile),
            format!(" run --profile {}", spec.profile),
        )
    };
    format!(
        "[Unit]
Description=Nolune AI Companion{description}
After=network.target

[Service]
Type=simple
WorkingDirectory={home}
Environment=NOLUNE_HOME={home}
Environment=RUST_LOG=info
ExecStart={binary} gateway{arguments}
Restart=always
RestartSec=3

[Install]
WantedBy=default.target
"
    )
}

/// Write a definition, creating its directory if needed.
pub fn write_definition(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents)
}

/// True when something accepts connections on 127.0.0.1:`port`.
pub fn port_is_listening(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, net::TcpListener};

    fn spec() -> ServiceSpec {
        ServiceSpec {
            binary: PathBuf::from("/Users/me/.nolune/bin/nolune"),
            home: PathBuf::from("/Users/me/.nolune"),
            profile: "default".into(),
        }
    }

    fn molinka() -> ServiceSpec {
        ServiceSpec {
            binary: PathBuf::from("/Users/me/.nolune/bin/nolune"),
            home: PathBuf::from("/Users/me/.nolune-profiles/molinka"),
            profile: "molinka".into(),
        }
    }

    #[test]
    fn launchd_plist_runs_gateway_with_home_and_logs() {
        let plist = render_launchd_plist(&spec());
        assert!(plist.contains("<string>dev.nolune.nolune</string>"));
        assert!(
            plist.contains(
                "<array>\n        <string>/Users/me/.nolune/bin/nolune</string>\n        <string>gateway</string>\n    </array>"
            ),
            "{plist}"
        );
        assert!(
            plist.contains("<key>NOLUNE_HOME</key>\n        <string>/Users/me/.nolune</string>")
        );
        assert!(plist.contains(
            "<key>StandardOutPath</key>\n    <string>/Users/me/.nolune/nolune.log</string>"
        ));
        assert!(plist.contains("<key>KeepAlive</key>\n    <true/>"));
        assert!(plist.starts_with("<?xml"));
    }

    #[test]
    fn systemd_unit_runs_gateway_as_user_service() {
        let unit = render_systemd_unit(&spec());
        assert!(
            unit.contains("ExecStart=/Users/me/.nolune/bin/nolune gateway\n"),
            "{unit}"
        );
        assert!(unit.contains("Environment=NOLUNE_HOME=/Users/me/.nolune\n"));
        assert!(unit.contains("WorkingDirectory=/Users/me/.nolune\n"));
        assert!(unit.contains("Restart=always\n"));
        assert!(unit.contains("WantedBy=default.target\n"));
        assert!(!unit.contains("User="), "user units must not set User=");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn definition_lives_in_launch_agents() {
        assert_eq!(
            definition_path(Path::new("/Users/me"), "default"),
            PathBuf::from("/Users/me/Library/LaunchAgents/dev.nolune.nolune.plist")
        );
        assert_eq!(
            definition_path(Path::new("/Users/me"), "molinka"),
            PathBuf::from("/Users/me/Library/LaunchAgents/dev.nolune.nolune.molinka.plist")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn definition_lives_in_user_systemd_dir() {
        assert_eq!(
            definition_path(Path::new("/home/me"), "default"),
            PathBuf::from("/home/me/.config/systemd/user/nolune.service")
        );
        assert_eq!(
            definition_path(Path::new("/home/me"), "molinka"),
            PathBuf::from("/home/me/.config/systemd/user/nolune-molinka.service")
        );
    }

    #[test]
    fn service_names_are_functions_of_the_profile() {
        // The default profile keeps the pre-#107 names so an upgrade finds its own service.
        assert_eq!(label("default"), LABEL);
        assert_eq!(unit_name("default"), UNIT_NAME);
        assert_eq!(label("molinka"), "dev.nolune.nolune.molinka");
        assert_eq!(unit_name("molinka"), "nolune-molinka");
    }

    #[test]
    fn default_profile_definitions_carry_no_profile_arguments() {
        let plist = render_launchd_plist(&spec());
        assert!(
            plist.contains("<string>gateway</string>\n    </array>"),
            "`gateway` must stay the last argument:\n{plist}"
        );
        assert!(!plist.contains("--profile"), "{plist}");
        assert!(!plist.contains("<string>run</string>"), "{plist}");
        assert!(!plist.contains("default"), "{plist}");

        let unit = render_systemd_unit(&spec());
        assert!(unit.contains("Description=Nolune AI Companion\n"), "{unit}");
        assert!(
            unit.contains("ExecStart=/Users/me/.nolune/bin/nolune gateway\n"),
            "{unit}"
        );
        assert!(!unit.contains("profile"), "{unit}");
    }

    #[test]
    fn named_profile_plist_has_its_own_label_home_log_and_run_arguments() {
        let plist = render_launchd_plist(&molinka());
        assert!(
            plist.contains("<key>Label</key>\n    <string>dev.nolune.nolune.molinka</string>"),
            "{plist}"
        );
        assert!(
            plist.contains(
                "<array>\n        <string>/Users/me/.nolune/bin/nolune</string>\n        <string>gateway</string>\n        <string>run</string>\n        <string>--profile</string>\n        <string>molinka</string>\n    </array>"
            ),
            "{plist}"
        );
        assert!(
            plist.contains(
                "<key>NOLUNE_HOME</key>\n        <string>/Users/me/.nolune-profiles/molinka</string>"
            ),
            "{plist}"
        );
        assert!(
            plist.contains(
                "<key>WorkingDirectory</key>\n    <string>/Users/me/.nolune-profiles/molinka</string>"
            ),
            "{plist}"
        );
        assert!(
            plist.contains(
                "<key>StandardOutPath</key>\n    <string>/Users/me/.nolune-profiles/molinka/nolune.log</string>"
            ),
            "{plist}"
        );
        assert!(
            plist.contains(
                "<key>StandardErrorPath</key>\n    <string>/Users/me/.nolune-profiles/molinka/nolune.log</string>"
            ),
            "{plist}"
        );
        assert!(
            !plist.contains("<string>dev.nolune.nolune</string>"),
            "{plist}"
        );
    }

    #[test]
    fn named_profile_unit_has_its_own_name_home_and_run_arguments() {
        let unit = render_systemd_unit(&molinka());
        assert!(
            unit.contains("ExecStart=/Users/me/.nolune/bin/nolune gateway run --profile molinka\n"),
            "{unit}"
        );
        assert!(
            unit.contains("Environment=NOLUNE_HOME=/Users/me/.nolune-profiles/molinka\n"),
            "{unit}"
        );
        assert!(
            unit.contains("WorkingDirectory=/Users/me/.nolune-profiles/molinka\n"),
            "{unit}"
        );
        assert!(
            unit.contains("Description=Nolune AI Companion (profile molinka)\n"),
            "{unit}"
        );
        assert!(!unit.contains("User="), "user units must not set User=");
    }

    #[test]
    fn definition_home_reads_the_root_back_from_either_format() {
        assert_eq!(
            definition_home(&render_launchd_plist(&molinka())),
            Some(PathBuf::from("/Users/me/.nolune-profiles/molinka"))
        );
        assert_eq!(
            definition_home(&render_systemd_unit(&molinka())),
            Some(PathBuf::from("/Users/me/.nolune-profiles/molinka"))
        );
        assert_eq!(
            definition_home(&render_systemd_unit(&spec())),
            Some(PathBuf::from("/Users/me/.nolune"))
        );
        assert_eq!(definition_home("[Unit]\nDescription=x\n"), None);
        assert_eq!(definition_home("<plist></plist>"), None);
    }

    #[test]
    fn write_definition_creates_parent_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a/b/nolune.service");
        write_definition(&path, "[Unit]\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "[Unit]\n");
        // Overwrite is fine: an upgrade rewrites the definition in place.
        write_definition(&path, "[Unit]\nDescription=x\n").unwrap();
        assert!(fs::read_to_string(&path).unwrap().contains("Description=x"));
    }

    #[test]
    fn port_probe_distinguishes_bound_from_free() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(port_is_listening(port));
        drop(listener);
        assert!(!port_is_listening(port));
    }
}
