//! What the server can honestly say about the machine it runs on: whether a
//! GUI target could exist here at all, and the identity that target registers
//! under.
//!
//! A headless host (no driver binary, a Linux session without a display, a
//! container) registers nothing and the server stays healthy; it never claims
//! GUI control it does not have. The identity is `server-local:<hostname>`,
//! a namespace no desktop registration can occupy (desktops register under a
//! UUID, or under the bare hostname before #80), so the server and a desktop
//! on one physical machine never share an id.

use std::path::PathBuf;

use cua_protocol::{MAX_ID_BYTES, MachineId};

use super::discovery::DriverLookupError;
use crate::config::CuaConfig;

/// The id prefix of the machine the server runs on.
pub const SERVER_LOCAL_PREFIX: &str = "server-local:";

/// What was seen of the host, gathered once at startup so the decision is a
/// pure function of it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HostProbe {
    /// `std::env::consts::OS`.
    pub os: &'static str,
    /// Whether `DISPLAY` or `WAYLAND_DISPLAY` names a display.
    pub display: bool,
    /// Whether the process runs inside a container (`/.dockerenv`,
    /// `/run/.containerenv`, `container=` or `KUBERNETES_SERVICE_HOST`).
    pub container: bool,
    /// The kernel hostname; empty when it cannot be read.
    pub hostname: String,
}

impl HostProbe {
    /// Probe this process's host and environment.
    pub fn current() -> Self {
        let named = |key: &str| std::env::var_os(key).is_some_and(|value| !value.is_empty());
        Self {
            os: std::env::consts::OS,
            display: named("DISPLAY") || named("WAYLAND_DISPLAY"),
            container: std::path::Path::new("/.dockerenv").exists()
                || std::path::Path::new("/run/.containerenv").exists()
                || named("container")
                || named("KUBERNETES_SERVICE_HOST"),
            hostname: hostname(),
        }
    }
}

/// The kernel hostname, empty when it cannot be read.
pub fn hostname() -> String {
    gethostname::gethostname()
        .to_string_lossy()
        .trim()
        .to_owned()
}

/// Why no server-local target is registered on this host.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Skip {
    /// `[cua].enabled = false`.
    Disabled,
    /// An explicitly named driver that cannot run: reported as an error, never
    /// treated as "no driver".
    Misconfigured(DriverLookupError),
    /// The process runs in a container; there is no desktop session to drive.
    Container,
    /// A Linux session without `DISPLAY` or `WAYLAND_DISPLAY`.
    NoDisplay,
    /// No `cua-driver` binary anywhere it was looked for.
    NoDriver,
}

impl Skip {
    /// One log line explaining the absence of a server-local target.
    pub fn reason(&self) -> String {
        match self {
            Self::Disabled => "[cua].enabled is false".to_owned(),
            Self::Misconfigured(error) => error.to_string(),
            Self::Container => {
                "this process runs in a container, which has no desktop session".to_owned()
            }
            Self::NoDisplay => {
                "neither DISPLAY nor WAYLAND_DISPLAY names a display in this session".to_owned()
            }
            Self::NoDriver => format!(
                "no cua-driver binary: set [cua].driver_path, {} or put {} on PATH",
                super::discovery::DRIVER_ENV,
                super::discovery::DRIVER_BINARY
            ),
        }
    }

    /// Whether the reason deserves an error line rather than an info line: a
    /// named driver that cannot run is a mistake, everything else is a fact
    /// about the host.
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Misconfigured(_))
    }
}

/// Decide whether to start a driver here: the binary to run, or why not.
///
/// A misconfigured driver path is reported before the host is judged, so a
/// typo is never hidden behind a headless verdict.
pub fn startup_plan(
    config: &CuaConfig,
    host: &HostProbe,
    driver: Result<Option<PathBuf>, DriverLookupError>,
) -> Result<PathBuf, Skip> {
    if !config.enabled {
        return Err(Skip::Disabled);
    }
    let driver = driver.map_err(Skip::Misconfigured)?;
    if host.container {
        return Err(Skip::Container);
    }
    if host.os == "linux" && !host.display {
        return Err(Skip::NoDisplay);
    }
    driver.ok_or(Skip::NoDriver)
}

/// The id the server-local target registers under: `server-local:` plus the
/// hostname reduced to the protocol's identifier grammar and bounded to
/// `MAX_ID_BYTES`; an empty hostname reads `server-local:host`.
pub fn server_local_machine_id(hostname: &str) -> MachineId {
    let budget = MAX_ID_BYTES - SERVER_LOCAL_PREFIX.len();
    let reduced: String = hostname
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let reduced = reduced.trim_matches(|c| matches!(c, '-' | '.'));
    let reduced = if reduced.is_empty() { "host" } else { reduced };
    let id = format!(
        "{SERVER_LOCAL_PREFIX}{}",
        &reduced[..reduced.len().min(budget)]
    );
    MachineId::try_from(id).expect("the reduced hostname is an identifier within bounds")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gui(os: &'static str) -> HostProbe {
        HostProbe {
            os,
            display: true,
            container: false,
            hostname: "studio".into(),
        }
    }

    fn driver() -> Result<Option<PathBuf>, DriverLookupError> {
        Ok(Some(PathBuf::from("/opt/cua/cua-driver")))
    }

    #[test]
    fn a_gui_host_with_a_driver_starts_it() {
        let config = CuaConfig::default();
        assert_eq!(
            startup_plan(&config, &gui("macos"), driver()),
            Ok(PathBuf::from("/opt/cua/cua-driver"))
        );
        assert_eq!(
            startup_plan(&config, &gui("linux"), driver()),
            Ok(PathBuf::from("/opt/cua/cua-driver"))
        );
        // Windows and macOS sessions do not announce a display; the driver's
        // own health report decides there.
        let no_display_env = HostProbe {
            display: false,
            ..gui("macos")
        };
        assert!(startup_plan(&config, &no_display_env, driver()).is_ok());
    }

    #[test]
    fn a_linux_session_without_a_display_is_headless() {
        let config = CuaConfig::default();
        let headless = HostProbe {
            display: false,
            ..gui("linux")
        };
        assert_eq!(
            startup_plan(&config, &headless, driver()),
            Err(Skip::NoDisplay)
        );
        assert!(Skip::NoDisplay.reason().contains("DISPLAY"));
    }

    #[test]
    fn a_container_is_headless_even_with_a_display_variable() {
        let config = CuaConfig::default();
        let container = HostProbe {
            container: true,
            ..gui("linux")
        };
        assert_eq!(
            startup_plan(&config, &container, driver()),
            Err(Skip::Container)
        );
    }

    #[test]
    fn no_driver_anywhere_registers_nothing() {
        let config = CuaConfig::default();
        assert_eq!(
            startup_plan(&config, &gui("macos"), Ok(None)),
            Err(Skip::NoDriver)
        );
        assert!(Skip::NoDriver.reason().contains("cua-driver"));
    }

    #[test]
    fn a_disabled_section_never_looks_for_a_driver() {
        let config = CuaConfig {
            enabled: false,
            ..CuaConfig::default()
        };
        assert_eq!(
            startup_plan(&config, &gui("macos"), driver()),
            Err(Skip::Disabled)
        );
    }

    #[test]
    fn a_misconfigured_driver_path_is_an_error_not_a_headless_host() {
        let config = CuaConfig::default();
        let error = DriverLookupError::NotExecutable {
            source: "[cua].driver_path",
            path: PathBuf::from("/typo/cua-driver"),
        };
        // Reported even where nothing could run, so the typo is never hidden
        // behind a headless verdict.
        let headless = HostProbe {
            display: false,
            ..gui("linux")
        };
        assert_eq!(
            startup_plan(&config, &headless, Err(error.clone())),
            Err(Skip::Misconfigured(error.clone()))
        );
        assert!(
            Skip::Misconfigured(error)
                .reason()
                .contains("/typo/cua-driver")
        );
    }

    #[test]
    fn the_server_local_id_is_namespaced_and_never_a_desktop_id() {
        let id = server_local_machine_id("studio");
        assert_eq!(id.as_str(), "server-local:studio");
        // The legacy desktop id was the bare hostname; the new one is a UUID.
        assert_ne!(id.as_str(), "studio");
        assert_ne!(
            id.as_str(),
            "4f3c1c2e-9b5e-4d2b-8f0a-1c2d3e4f5a6b",
            "a UUID never carries the prefix"
        );
        assert!(id.as_str().starts_with(SERVER_LOCAL_PREFIX));

        // Hostnames outside the identifier grammar are reduced, never refused.
        assert_eq!(
            server_local_machine_id("Tim's MacBook Pro.local").as_str(),
            "server-local:Tim-s-MacBook-Pro.local"
        );
        assert_eq!(
            server_local_machine_id("  ").as_str(),
            "server-local:host",
            "a blank hostname still yields a usable id"
        );
        assert_eq!(server_local_machine_id("").as_str(), "server-local:host");
        assert_eq!(
            server_local_machine_id("héllo:wörld").as_str(),
            "server-local:h-llo-w-rld",
            "non-ASCII and colons are replaced so the id stays one token"
        );
        let long = server_local_machine_id(&"h".repeat(500));
        assert_eq!(long.as_str().len(), MAX_ID_BYTES);
        assert!(long.as_str().starts_with("server-local:hhh"));
    }

    #[test]
    fn the_probe_reads_this_process_without_panicking() {
        let probe = HostProbe::current();
        assert_eq!(probe.os, std::env::consts::OS);
        // This test process is not a container and has some hostname.
        assert!(!probe.hostname.contains('\n'));
    }
}
