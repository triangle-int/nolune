//! What the server can honestly say about the machine it runs on: whether
//! Nolune drives computers here at all (#20), whether a display session
//! exists to drive, whether a GUI target could exist here (#16), and the
//! identity that target registers under.
//!
//! The platform and session answers are plain facts about the host, computed
//! without starting a driver, so `nolune cua status` can explain an
//! unsupported or headless server before touching anything, and the runtime
//! reads the same facts once at startup. A headless host (no driver binary,
//! a Linux session without a display, a macOS login without a window server,
//! a container) registers nothing and the server stays healthy; it never
//! claims GUI control it does not have. The identity is
//! `server-local:<hostname>`, a namespace no desktop registration can occupy
//! (desktops register under a UUID, or under the bare hostname before #80),
//! so the server and a desktop on one physical machine never share an id.

use std::{env, ffi::OsStr, path::PathBuf};

use cua_protocol::{MAX_ID_BYTES, MachineId, cua_driver_pin::Target};

use super::discovery::DriverLookupError;
use crate::config::CuaConfig;

/// Whether Nolune supports computer use on this host.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlatformSupport {
    /// Nolune drives computers here with the pinned driver.
    Supported,
    /// It does not, and the reason is fit for a status line.
    Unsupported(String),
}

/// Whether a graphical session exists for a driver to act in.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DisplaySession {
    /// A display was found; the value says which.
    Present(String),
    /// None was found: a headless host, with the reason.
    Headless(String),
    /// This platform is not checked here; the driver's own health report
    /// decides.
    NotChecked,
}

impl DisplaySession {
    pub fn is_headless(&self) -> bool {
        matches!(self, Self::Headless(_))
    }
}

/// Support for the host this binary runs on.
pub fn platform_support() -> PlatformSupport {
    platform_support_for(Target::current(), env::consts::OS, host_triple())
}

/// Support for a `target` (`None` when Nolune ships no driver for the host)
/// on operating system `os`, naming `triple` in the reason.
pub fn platform_support_for(target: Option<Target>, os: &str, triple: &str) -> PlatformSupport {
    match (target, os) {
        (None, _) => PlatformSupport::Unsupported(format!(
            "Nolune ships no Cua Driver for {triple}; computer use is unavailable here"
        )),
        (Some(_), "macos") => PlatformSupport::Supported,
        (Some(_), _) => PlatformSupport::Unsupported(format!(
            "Nolune drives computers on macOS only for now; on {triple} the pinned driver \
             still installs, so a later release can turn it on without changing the pin"
        )),
    }
}

/// What a host says about its session, gathered without starting anything.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SessionFacts<'a> {
    /// Linux: `DISPLAY`.
    pub display: Option<&'a OsStr>,
    /// Linux: `WAYLAND_DISPLAY`.
    pub wayland_display: Option<&'a OsStr>,
    /// Windows: `SESSIONNAME` (`Console`, `RDP-Tcp#3`), which only an
    /// interactive session has.
    pub session_name: Option<&'a OsStr>,
    /// macOS: what `launchctl managername` answers (`Aqua` in a graphical
    /// login, `Background` over SSH, `System` for daemons); `None` when it
    /// could not be asked.
    pub launchd_manager: Option<&'a str>,
}

/// The display session of this process.
pub fn display_session() -> DisplaySession {
    let display = env::var_os("DISPLAY");
    let wayland_display = env::var_os("WAYLAND_DISPLAY");
    let session_name = env::var_os("SESSIONNAME");
    let launchd_manager = launchd_manager_name();
    display_session_for(
        env::consts::OS,
        &SessionFacts {
            display: display.as_deref(),
            wayland_display: wayland_display.as_deref(),
            session_name: session_name.as_deref(),
            launchd_manager: launchd_manager.as_deref(),
        },
    )
}

/// The display session for operating system `os` given `facts` about it.
pub fn display_session_for(os: &str, facts: &SessionFacts<'_>) -> DisplaySession {
    let set = |name: &str, value: Option<&OsStr>| {
        value
            .filter(|value| !value.is_empty())
            .map(|value| format!("{name}={}", value.to_string_lossy()))
    };
    match os {
        "linux" => match set("WAYLAND_DISPLAY", facts.wayland_display)
            .or_else(|| set("DISPLAY", facts.display))
        {
            Some(found) => DisplaySession::Present(found),
            None => DisplaySession::Headless(
                "no display session (DISPLAY and WAYLAND_DISPLAY are unset): a headless host, \
                 so the driver is never started"
                    .to_owned(),
            ),
        },
        // A graphical login runs its processes under launchd's Aqua manager;
        // SSH sessions and daemons run under Background or System, where
        // no window server is reachable and `open` cannot launch the app.
        "macos" => match facts.launchd_manager.map(str::trim) {
            Some("Aqua") => {
                DisplaySession::Present("launchd session Aqua (a graphical login)".to_owned())
            }
            Some(manager) => DisplaySession::Headless(format!(
                "no graphical session (launchctl managername reports {manager}, an SSH or \
                 background session, not Aqua): a headless host, so the driver is never started"
            )),
            None => DisplaySession::NotChecked,
        },
        // Only an interactive session (the console or an RDP one) carries
        // SESSIONNAME; services and headless hosts have none.
        "windows" => match set("SESSIONNAME", facts.session_name) {
            Some(found) => DisplaySession::Present(found),
            None => DisplaySession::Headless(
                "no interactive session (SESSIONNAME is unset): a service or headless host, so \
                 the driver is never started"
                    .to_owned(),
            ),
        },
        _ => DisplaySession::NotChecked,
    }
}

/// `launchctl managername` on macOS; `None` elsewhere or when it fails.
fn launchd_manager_name() -> Option<String> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let output = std::process::Command::new("launchctl")
        .arg("managername")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!name.is_empty()).then_some(name)
}

/// The target triple of this build, for messages about hosts the pin does
/// not cover.
fn host_triple() -> &'static str {
    Target::current().map_or("this host", Target::triple)
}

/// The id prefix of the machine the server runs on.
pub const SERVER_LOCAL_PREFIX: &str = "server-local:";

/// What was seen of the host, gathered once at startup so the decision is a
/// pure function of it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostProbe {
    /// `std::env::consts::OS`.
    pub os: &'static str,
    /// Whether Nolune drives computers on this platform at all (#20).
    pub support: PlatformSupport,
    /// The graphical session a driver could act in, from
    /// [`display_session`].
    pub session: DisplaySession,
    /// Whether the process runs inside a container (`/.dockerenv`,
    /// `/run/.containerenv`, `container=` or `KUBERNETES_SERVICE_HOST`).
    pub container: bool,
    /// The kernel hostname; empty when it cannot be read.
    pub hostname: String,
}

impl HostProbe {
    /// Probe this process's host and environment.
    pub fn current() -> Self {
        let named = |key: &str| env::var_os(key).is_some_and(|value| !value.is_empty());
        Self {
            os: env::consts::OS,
            support: platform_support(),
            session: display_session(),
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
    /// Nolune does not drive computers on this platform yet (#20); the
    /// reason is [`PlatformSupport::Unsupported`]'s.
    Unsupported(String),
    /// The process runs in a container; there is no desktop session to drive.
    Container,
    /// No graphical session to act in; the reason is
    /// [`DisplaySession::Headless`]'s.
    Headless(String),
    /// No `cua-driver` binary anywhere it was looked for.
    NoDriver,
}

impl Skip {
    /// One log line explaining the absence of a server-local target.
    pub fn reason(&self) -> String {
        match self {
            Self::Disabled => "[cua].enabled is false".to_owned(),
            Self::Misconfigured(error) => error.to_string(),
            Self::Unsupported(reason) | Self::Headless(reason) => reason.clone(),
            Self::Container => {
                "this process runs in a container, which has no desktop session".to_owned()
            }
            Self::NoDriver => format!(
                "no cua-driver binary: run `nolune cua install`, set [cua].driver_path, {} or put {} on PATH",
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
/// typo is never hidden behind an unsupported or headless verdict.
pub fn startup_plan(
    config: &CuaConfig,
    host: &HostProbe,
    driver: Result<Option<PathBuf>, DriverLookupError>,
) -> Result<PathBuf, Skip> {
    if !config.enabled {
        return Err(Skip::Disabled);
    }
    let driver = driver.map_err(Skip::Misconfigured)?;
    if let PlatformSupport::Unsupported(reason) = &host.support {
        return Err(Skip::Unsupported(reason.clone()));
    }
    if host.container {
        return Err(Skip::Container);
    }
    if let DisplaySession::Headless(reason) = &host.session {
        return Err(Skip::Headless(reason.clone()));
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

    #[test]
    fn macos_targets_are_supported() {
        for target in [Target::Aarch64AppleDarwin, Target::X86_64AppleDarwin] {
            assert_eq!(
                platform_support_for(Some(target), "macos", target.triple()),
                PlatformSupport::Supported
            );
        }
    }

    #[test]
    fn linux_and_windows_are_unsupported_with_a_reason_that_names_the_policy() {
        for target in [
            Target::X86_64UnknownLinuxGnu,
            Target::Aarch64UnknownLinuxGnu,
            Target::X86_64PcWindowsMsvc,
        ] {
            let os = if target == Target::X86_64PcWindowsMsvc {
                "windows"
            } else {
                "linux"
            };
            let support = platform_support_for(Some(target), os, target.triple());
            let PlatformSupport::Unsupported(reason) = support else {
                panic!("{target} is not supported yet, got {support:?}");
            };
            assert!(reason.contains(target.triple()), "{reason}");
            assert!(
                reason.to_lowercase().contains("macos"),
                "says where computer use works: {reason}"
            );
            assert!(
                reason.contains("pinned"),
                "explains why the driver still installs: {reason}"
            );
        }
    }

    #[test]
    fn a_host_without_a_pinned_driver_is_unsupported() {
        let support = platform_support_for(None, "freebsd", "x86_64-unknown-freebsd");
        let PlatformSupport::Unsupported(reason) = support else {
            panic!("no pinned asset means no support, got {support:?}");
        };
        assert!(reason.contains("x86_64-unknown-freebsd"), "{reason}");
        assert!(reason.contains("no Cua Driver"), "{reason}");
    }

    fn linux(display: Option<&str>, wayland: Option<&str>) -> DisplaySession {
        display_session_for(
            "linux",
            &SessionFacts {
                display: display.map(OsStr::new),
                wayland_display: wayland.map(OsStr::new),
                ..SessionFacts::default()
            },
        )
    }

    #[test]
    fn linux_without_a_display_is_headless() {
        let session = linux(None, None);
        let DisplaySession::Headless(reason) = session else {
            panic!("no DISPLAY and no WAYLAND_DISPLAY is headless, got {session:?}");
        };
        assert!(reason.contains("DISPLAY"), "{reason}");
        assert!(reason.contains("WAYLAND_DISPLAY"), "{reason}");
        assert!(linux(Some(""), Some("")).is_headless());
    }

    #[test]
    fn linux_with_a_display_names_it() {
        assert_eq!(
            linux(Some(":0"), None),
            DisplaySession::Present("DISPLAY=:0".to_owned())
        );
        assert_eq!(
            linux(None, Some("wayland-1")),
            DisplaySession::Present("WAYLAND_DISPLAY=wayland-1".to_owned())
        );
    }

    fn macos(manager: Option<&str>) -> DisplaySession {
        display_session_for(
            "macos",
            &SessionFacts {
                launchd_manager: manager,
                ..SessionFacts::default()
            },
        )
    }

    #[test]
    fn macos_is_graphical_only_in_an_aqua_session() {
        let DisplaySession::Present(found) = macos(Some("Aqua")) else {
            panic!("an Aqua session is a graphical login");
        };
        assert!(found.contains("Aqua"), "{found}");

        for manager in ["Background", "System", "Unknown"] {
            let session = macos(Some(manager));
            let DisplaySession::Headless(reason) = session else {
                panic!("launchd manager {manager} is no graphical session, got {session:?}");
            };
            assert!(reason.contains(manager), "{reason}");
            assert!(reason.contains("SSH"), "explains the usual cause: {reason}");
            assert!(
                reason.contains("never started"),
                "says the driver is not started: {reason}"
            );
        }
    }

    #[test]
    fn macos_leaves_the_check_to_the_driver_when_launchctl_cannot_be_asked() {
        assert_eq!(macos(None), DisplaySession::NotChecked);
        // The DISPLAY variables mean nothing on macOS.
        assert_eq!(
            display_session_for(
                "macos",
                &SessionFacts {
                    display: Some(OsStr::new(":0")),
                    ..SessionFacts::default()
                }
            ),
            DisplaySession::NotChecked
        );
    }

    fn windows(session_name: Option<&str>) -> DisplaySession {
        display_session_for(
            "windows",
            &SessionFacts {
                session_name: session_name.map(OsStr::new),
                ..SessionFacts::default()
            },
        )
    }

    #[test]
    fn windows_is_headless_without_an_interactive_session() {
        assert_eq!(
            windows(Some("Console")),
            DisplaySession::Present("SESSIONNAME=Console".to_owned())
        );
        assert_eq!(
            windows(Some("RDP-Tcp#3")),
            DisplaySession::Present("SESSIONNAME=RDP-Tcp#3".to_owned())
        );
        for missing in [None, Some("")] {
            let session = windows(missing);
            let DisplaySession::Headless(reason) = session else {
                panic!("no SESSIONNAME is a service or headless session, got {session:?}");
            };
            assert!(reason.contains("SESSIONNAME"), "{reason}");
            assert!(reason.contains("never started"), "{reason}");
        }
    }

    #[test]
    fn other_platforms_are_not_checked() {
        assert_eq!(
            display_session_for("freebsd", &SessionFacts::default()),
            DisplaySession::NotChecked
        );
    }

    #[test]
    fn the_host_answers_do_not_panic() {
        let _ = platform_support();
        let _ = display_session();
    }

    fn gui(os: &'static str) -> HostProbe {
        HostProbe {
            os,
            support: PlatformSupport::Supported,
            session: DisplaySession::Present("a graphical login".into()),
            container: false,
            hostname: "studio".into(),
        }
    }

    fn headless(os: &'static str) -> HostProbe {
        HostProbe {
            session: DisplaySession::Headless(format!("no display session on {os}")),
            ..gui(os)
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
        // A platform whose session is not checked here leaves the verdict to
        // the driver's own health report.
        let not_checked = HostProbe {
            session: DisplaySession::NotChecked,
            ..gui("macos")
        };
        assert!(startup_plan(&config, &not_checked, driver()).is_ok());
    }

    #[test]
    fn a_session_without_a_display_is_headless() {
        let config = CuaConfig::default();
        let plan = startup_plan(&config, &headless("linux"), driver());
        let Err(Skip::Headless(reason)) = plan else {
            panic!("a headless host starts nothing, got {plan:?}");
        };
        assert!(reason.contains("linux"), "{reason}");
        assert_eq!(Skip::Headless(reason.clone()).reason(), reason);
        assert!(!Skip::Headless(reason).is_error());

        // The detection itself is `display_session_for`'s: a Linux session
        // without DISPLAY or WAYLAND_DISPLAY, a macOS login outside Aqua, a
        // Windows service.
        let probe = HostProbe {
            session: display_session_for("linux", &SessionFacts::default()),
            ..gui("linux")
        };
        let Err(Skip::Headless(reason)) = startup_plan(&config, &probe, driver()) else {
            panic!("no DISPLAY is headless on Linux");
        };
        assert!(reason.contains("DISPLAY"), "{reason}");
    }

    #[test]
    fn an_unsupported_platform_registers_nothing_even_with_a_display() {
        let config = CuaConfig::default();
        let linux = HostProbe {
            support: platform_support_for(
                Some(Target::X86_64UnknownLinuxGnu),
                "linux",
                Target::X86_64UnknownLinuxGnu.triple(),
            ),
            ..gui("linux")
        };
        let plan = startup_plan(&config, &linux, driver());
        let Err(Skip::Unsupported(reason)) = plan else {
            panic!("Linux is not driven yet (#20), got {plan:?}");
        };
        assert!(reason.contains("macOS"), "{reason}");
        assert!(!Skip::Unsupported(reason).is_error());
    }

    #[test]
    fn a_container_is_headless_even_with_a_display_variable() {
        let config = CuaConfig::default();
        let container = HostProbe {
            container: true,
            ..gui("macos")
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
        let reason = Skip::NoDriver.reason();
        assert!(reason.contains("cua-driver"), "{reason}");
        assert!(reason.contains("nolune cua install"), "{reason}");
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
        // behind a headless or unsupported verdict.
        let unsupported_and_headless = HostProbe {
            support: PlatformSupport::Unsupported("not here".into()),
            ..headless("linux")
        };
        assert_eq!(
            startup_plan(&config, &unsupported_and_headless, Err(error.clone())),
            Err(Skip::Misconfigured(error.clone()))
        );
        assert!(Skip::Misconfigured(error.clone()).is_error());
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
        assert_eq!(probe.os, env::consts::OS);
        assert_eq!(probe.support, platform_support());
        // This test process is not a container and has some hostname.
        assert!(!probe.hostname.contains('\n'));
    }
}
