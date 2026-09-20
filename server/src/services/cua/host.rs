//! What this host can do with a Cua Driver (#20): whether Nolune drives
//! computers on it at all, and whether a display session exists to drive.
//!
//! Both answers are plain facts about the host, computed without starting a
//! driver, so `nolune cua status` can explain an unsupported or headless
//! server before touching anything.

use std::{env, ffi::OsStr};

use cua_protocol::cua_driver_pin::Target;

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
    let _ = (os, facts);
    todo!("session facts per platform")
}

/// `launchctl managername` on macOS; `None` elsewhere or when it fails.
fn launchd_manager_name() -> Option<String> {
    todo!("launchctl managername")
}

/// The target triple of this build, for messages about hosts the pin does
/// not cover.
fn host_triple() -> &'static str {
    Target::current().map_or("this host", Target::triple)
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
}
