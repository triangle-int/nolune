//! Where the `cua-driver` binary is: an explicit `[cua].driver_path`, then the
//! `NOLUNE_CUA_DRIVER` environment variable, then a `PATH` lookup. A host
//! without a driver is not an error; the runtime simply registers no
//! server-local target.

use std::{
    ffi::OsStr,
    fmt,
    path::{Path, PathBuf},
};

/// The environment variable that names the driver binary outright.
pub const DRIVER_ENV: &str = "NOLUNE_CUA_DRIVER";

/// The binary name looked up on `PATH`.
pub const DRIVER_BINARY: &str = "cua-driver";

/// Where a lookup may find the driver, in precedence order.
#[derive(Clone, Copy, Debug, Default)]
pub struct DriverLookup<'a> {
    /// `[cua].driver_path` from the config file.
    pub configured: Option<&'a Path>,
    /// The value of [`DRIVER_ENV`].
    pub env_override: Option<&'a OsStr>,
    /// The value of `PATH`.
    pub path: Option<&'a OsStr>,
}

/// An explicitly named driver that cannot be run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DriverLookupError {
    /// The configured or environment-named path is not an executable file.
    NotExecutable { source: &'static str, path: PathBuf },
}

impl fmt::Display for DriverLookupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotExecutable { source, path } => write!(
                f,
                "{source} names {} which is not an executable file",
                path.display()
            ),
        }
    }
}

impl std::error::Error for DriverLookupError {}

/// Whether `path` is a file this process could execute.
fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// The driver binary's file name on this platform.
fn binary_name() -> String {
    format!("{DRIVER_BINARY}{}", std::env::consts::EXE_SUFFIX)
}

/// Resolve the driver binary from the given sources. `Ok(None)` means no
/// driver is installed anywhere it was looked for; a path that was named
/// explicitly but cannot run is an error so a typo is never a silent
/// "no GUI target".
pub fn locate_driver(lookup: DriverLookup<'_>) -> Result<Option<PathBuf>, DriverLookupError> {
    let explicit = [
        (
            "[cua].driver_path",
            lookup.configured.map(Path::to_path_buf),
        ),
        (DRIVER_ENV, lookup.env_override.map(PathBuf::from)),
    ];
    for (source, named) in explicit {
        if let Some(path) = named {
            return if is_executable(&path) {
                Ok(Some(path))
            } else {
                Err(DriverLookupError::NotExecutable { source, path })
            };
        }
    }
    let name = binary_name();
    let found = lookup
        .path
        .into_iter()
        .flat_map(std::env::split_paths)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join(&name))
        .find(|candidate| is_executable(candidate));
    Ok(found)
}

/// Resolve the driver binary from the config value and this process's
/// environment.
pub fn discover(configured: Option<&Path>) -> Result<Option<PathBuf>, DriverLookupError> {
    let env_override = std::env::var_os(DRIVER_ENV);
    let path = std::env::var_os("PATH");
    locate_driver(DriverLookup {
        configured,
        env_override: env_override.as_deref(),
        path: path.as_deref(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn executable(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    #[test]
    fn nothing_configured_and_nothing_on_path_is_no_driver() {
        let empty = tempfile::tempdir().unwrap();
        let lookup = DriverLookup {
            configured: None,
            env_override: None,
            path: Some(empty.path().as_os_str()),
        };
        assert_eq!(locate_driver(lookup), Ok(None));
        assert_eq!(locate_driver(DriverLookup::default()), Ok(None));
    }

    #[test]
    fn the_configured_path_wins() {
        let dir = tempfile::tempdir().unwrap();
        let configured = executable(dir.path(), "my-driver");
        let other = executable(dir.path(), "env-driver");
        let lookup = DriverLookup {
            configured: Some(&configured),
            env_override: Some(other.as_os_str()),
            path: Some(dir.path().as_os_str()),
        };
        assert_eq!(locate_driver(lookup), Ok(Some(configured)));
    }

    #[test]
    fn a_configured_path_that_cannot_run_is_an_error_not_a_silent_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing-driver");
        let lookup = DriverLookup {
            configured: Some(&missing),
            env_override: None,
            path: Some(dir.path().as_os_str()),
        };
        assert_eq!(
            locate_driver(lookup),
            Err(DriverLookupError::NotExecutable {
                source: "[cua].driver_path",
                path: missing.clone(),
            })
        );

        let plain_file = dir.path().join("notes.txt");
        fs::write(&plain_file, "hello").unwrap();
        let lookup = DriverLookup {
            configured: None,
            env_override: Some(plain_file.as_os_str()),
            path: None,
        };
        let error = locate_driver(lookup).unwrap_err();
        assert!(error.to_string().contains(DRIVER_ENV), "{error}");
    }

    #[test]
    fn the_environment_override_beats_the_path_lookup() {
        let dir = tempfile::tempdir().unwrap();
        let on_path = tempfile::tempdir().unwrap();
        let named = executable(dir.path(), "named-driver");
        executable(on_path.path(), DRIVER_BINARY);
        let lookup = DriverLookup {
            configured: None,
            env_override: Some(named.as_os_str()),
            path: Some(on_path.path().as_os_str()),
        };
        assert_eq!(locate_driver(lookup), Ok(Some(named)));
    }

    #[test]
    fn the_path_lookup_finds_the_first_executable_cua_driver() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        // The first directory only has a non-executable file of the right name.
        fs::write(first.path().join(DRIVER_BINARY), "not a program").unwrap();
        let found = executable(second.path(), DRIVER_BINARY);
        let joined = std::env::join_paths([first.path(), second.path()]).unwrap();
        let lookup = DriverLookup {
            configured: None,
            env_override: None,
            path: Some(joined.as_os_str()),
        };
        #[cfg(unix)]
        assert_eq!(locate_driver(lookup), Ok(Some(found)));
        #[cfg(not(unix))]
        assert!(locate_driver(lookup).unwrap().is_some());
    }

    #[test]
    fn discover_reads_this_process_environment() {
        // Only the config value is injected; the result depends on the host,
        // but it must never panic and must honour a configured driver.
        let dir = tempfile::tempdir().unwrap();
        let configured = executable(dir.path(), "driver");
        assert_eq!(discover(Some(&configured)), Ok(Some(configured)));
    }
}
