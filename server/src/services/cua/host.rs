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

impl PlatformSupport {
    pub fn is_supported(&self) -> bool {
        matches!(self, Self::Supported)
    }
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
    let _ = (target, os, triple);
    todo!("platform support")
}

/// The display session of this process.
pub fn display_session() -> DisplaySession {
    display_session_for(
        env::consts::OS,
        env::var_os("DISPLAY").as_deref(),
        env::var_os("WAYLAND_DISPLAY").as_deref(),
    )
}

/// The display session for operating system `os` given its `DISPLAY` and
/// `WAYLAND_DISPLAY` variables.
pub fn display_session_for(
    os: &str,
    display: Option<&OsStr>,
    wayland_display: Option<&OsStr>,
) -> DisplaySession {
    let _ = (os, display, wayland_display);
    todo!("display session")
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

    #[test]
    fn linux_without_a_display_is_headless() {
        let session = display_session_for("linux", None, None);
        let DisplaySession::Headless(reason) = session else {
            panic!("no DISPLAY and no WAYLAND_DISPLAY is headless, got {session:?}");
        };
        assert!(reason.contains("DISPLAY"), "{reason}");
        assert!(reason.contains("WAYLAND_DISPLAY"), "{reason}");

        let empty = OsStr::new("");
        assert!(display_session_for("linux", Some(empty), Some(empty)).is_headless());
    }

    #[test]
    fn linux_with_a_display_names_it() {
        assert_eq!(
            display_session_for("linux", Some(OsStr::new(":0")), None),
            DisplaySession::Present("DISPLAY=:0".to_owned())
        );
        assert_eq!(
            display_session_for("linux", None, Some(OsStr::new("wayland-1"))),
            DisplaySession::Present("WAYLAND_DISPLAY=wayland-1".to_owned())
        );
    }

    #[test]
    fn macos_and_windows_leave_the_check_to_the_driver() {
        assert_eq!(
            display_session_for("macos", None, None),
            DisplaySession::NotChecked
        );
        assert_eq!(
            display_session_for("windows", None, None),
            DisplaySession::NotChecked
        );
    }

    #[test]
    fn the_host_answers_do_not_panic() {
        let _ = platform_support();
        let _ = display_session();
    }
}
