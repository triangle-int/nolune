//! Opt-in background service for the gateway (#125).
//!
//! `nolune gateway install` writes a user-level launchd agent (macOS) or systemd user
//! unit (Linux) that runs `nolune gateway`, and starts it. Nothing here runs unless the
//! user asks; a plain install leaves the gateway in the foreground.

use std::{
    fs, io,
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    time::Duration,
};

pub const LABEL: &str = "dev.nolune.nolune";
pub const UNIT_NAME: &str = "nolune";

/// What the service definition needs to know: which binary to run and where its home is.
pub struct ServiceSpec {
    pub binary: PathBuf,
    pub home: PathBuf,
}

/// Where this platform keeps the service definition, relative to the user's home directory.
pub fn definition_path(home_dir: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home_dir
            .join("Library/LaunchAgents")
            .join(format!("{LABEL}.plist"))
    } else {
        home_dir
            .join(".config/systemd/user")
            .join(format!("{UNIT_NAME}.service"))
    }
}

/// The launchd agent, byte-compatible with what the shell installer wrote, except it runs
/// `nolune gateway` explicitly.
pub fn render_launchd_plist(spec: &ServiceSpec) -> String {
    let binary = spec.binary.display();
    let home = spec.home.display();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{binary}</string>
        <string>gateway</string>
    </array>
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
    format!(
        "[Unit]
Description=Nolune AI Companion
After=network.target

[Service]
Type=simple
WorkingDirectory={home}
Environment=NOLUNE_HOME={home}
Environment=RUST_LOG=info
ExecStart={binary} gateway
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
            definition_path(Path::new("/Users/me")),
            PathBuf::from("/Users/me/Library/LaunchAgents/dev.nolune.nolune.plist")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn definition_lives_in_user_systemd_dir() {
        assert_eq!(
            definition_path(Path::new("/home/me")),
            PathBuf::from("/home/me/.config/systemd/user/nolune.service")
        );
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
