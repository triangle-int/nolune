//! Where the `codex` binary is and whether it is the pinned release: the
//! [`CODEX_ENV`] override, then a `PATH` lookup for an executable
//! [`CODEX_BINARY`], then `codex --version`, which must print exactly
//! [`CODEX_VERSION`]. Nothing found is [`AppServerError::NotInstalled`];
//! another release is [`AppServerError::Incompatible`], never a silent
//! best effort against a protocol the fixtures were not recorded for.

use std::{
    ffi::OsStr,
    fmt,
    path::{Path, PathBuf},
    time::Duration,
};

use super::{AppServerError, CODEX_BINARY, CODEX_ENV, CODEX_VERSION};

/// How long `codex --version` may take to answer.
const VERSION_TIMEOUT: Duration = Duration::from_secs(10);

/// Where a lookup may find the binary, in precedence order.
#[derive(Clone, Copy, Debug, Default)]
pub struct BinaryLookup<'a> {
    /// The value of [`CODEX_ENV`].
    pub env_override: Option<&'a OsStr>,
    /// The value of `PATH`.
    pub path: Option<&'a OsStr>,
}

/// Which source a lookup found the binary through.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BinarySource {
    Environment,
    SearchPath,
}

impl fmt::Display for BinarySource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Environment => CODEX_ENV,
            Self::SearchPath => "PATH",
        })
    }
}

/// A binary a lookup found and verified: it runs and it is the pin.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocatedBinary {
    pub path: PathBuf,
    pub source: BinarySource,
    /// The version it reported, which equals [`CODEX_VERSION`].
    pub version: String,
}

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

/// The binary's file name on this platform.
fn binary_name() -> String {
    format!("{CODEX_BINARY}{}", std::env::consts::EXE_SUFFIX)
}

/// Find the binary without running it: the environment override when set
/// (a value that is not an executable file is [`AppServerError::Unusable`],
/// so a typo never silently means "not installed"), else the first
/// executable `codex` on `PATH`.
pub fn locate(lookup: BinaryLookup<'_>) -> Result<(PathBuf, BinarySource), AppServerError> {
    if let Some(named) = lookup.env_override {
        let path = PathBuf::from(named);
        return if is_executable(&path) {
            Ok((path, BinarySource::Environment))
        } else {
            Err(AppServerError::Unusable {
                path,
                reason: format!("{CODEX_ENV} does not name an executable file"),
            })
        };
    }
    let name = binary_name();
    lookup
        .path
        .into_iter()
        .flat_map(std::env::split_paths)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join(&name))
        .find(|candidate| is_executable(candidate))
        .map(|path| (path, BinarySource::SearchPath))
        .ok_or(AppServerError::NotInstalled)
}

/// The version from `codex --version` output (`codex-cli 0.155.0`): the
/// last whitespace-separated token of the first non-empty line, without a
/// `v` prefix, when it starts with a digit.
pub fn parse_version_output(stdout: &str) -> Option<String> {
    let line = stdout
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    let token = line.split_whitespace().last()?;
    let token = token.strip_prefix('v').unwrap_or(token);
    token
        .starts_with(|c: char| c.is_ascii_digit())
        .then(|| token.to_owned())
}

/// Ask `binary --version` which version it is.
pub async fn reported_version(binary: &Path) -> Result<String, AppServerError> {
    let unusable = |reason: String| AppServerError::Unusable {
        path: binary.to_path_buf(),
        reason,
    };
    let output = tokio::time::timeout(
        VERSION_TIMEOUT,
        tokio::process::Command::new(binary)
            .arg("--version")
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| {
        unusable(format!(
            "--version did not answer within {VERSION_TIMEOUT:?}"
        ))
    })?
    .map_err(|error| unusable(format!("cannot run --version: {error}")))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(unusable(format!(
            "--version failed ({}): {}",
            output.status,
            stderr.trim()
        )));
    }
    parse_version_output(&stdout)
        .ok_or_else(|| unusable(format!("unexpected --version output {:?}", stdout.trim())))
}

/// [`locate`], then `--version`, then the pin check.
pub async fn discover_with(lookup: BinaryLookup<'_>) -> Result<LocatedBinary, AppServerError> {
    let (path, source) = locate(lookup)?;
    let version = reported_version(&path).await?;
    if version != CODEX_VERSION {
        return Err(AppServerError::Incompatible {
            path,
            found: version,
        });
    }
    Ok(LocatedBinary {
        path,
        source,
        version,
    })
}

/// [`discover_with`] from this process's environment.
pub async fn discover() -> Result<LocatedBinary, AppServerError> {
    let env_override = std::env::var_os(CODEX_ENV);
    let path = std::env::var_os("PATH");
    discover_with(BinaryLookup {
        env_override: env_override.as_deref(),
        path: path.as_deref(),
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// An executable script named `name` under `dir` that prints `version`
    /// for `--version`, or fails when `version` is `None`.
    fn fake_binary(dir: &Path, name: &str, version: Option<&str>) -> PathBuf {
        let path = dir.join(name);
        let body = match version {
            Some(version) => format!("#!/bin/sh\necho 'codex-cli {version}'\n"),
            None => "#!/bin/sh\necho 'no such command' >&2\nexit 2\n".to_owned(),
        };
        fs::write(&path, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    #[test]
    fn parse_version_output_reads_the_cli_format() {
        assert_eq!(
            parse_version_output("codex-cli 0.155.0\n").as_deref(),
            Some("0.155.0")
        );
        assert_eq!(
            parse_version_output("\ncodex-cli v0.155.0\n").as_deref(),
            Some("0.155.0")
        );
        assert_eq!(parse_version_output("codex-cli").as_deref(), None);
        assert_eq!(parse_version_output("").as_deref(), None);
        assert_eq!(parse_version_output("command not found").as_deref(), None);
    }

    #[test]
    fn nothing_on_path_and_no_override_is_not_installed() {
        let empty = tempfile::tempdir().unwrap();
        let lookup = BinaryLookup {
            env_override: None,
            path: Some(empty.path().as_os_str()),
        };
        assert_eq!(locate(lookup), Err(AppServerError::NotInstalled));
        assert_eq!(
            locate(BinaryLookup::default()),
            Err(AppServerError::NotInstalled)
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_path_lookup_skips_a_file_that_cannot_run() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(CODEX_BINARY), "not executable").unwrap();
        let lookup = BinaryLookup {
            env_override: None,
            path: Some(dir.path().as_os_str()),
        };
        assert_eq!(locate(lookup), Err(AppServerError::NotInstalled));
    }

    #[cfg(unix)]
    #[test]
    fn the_environment_override_beats_the_path_and_must_run() {
        let dir = tempfile::tempdir().unwrap();
        let on_path = tempfile::tempdir().unwrap();
        let named = fake_binary(dir.path(), "my-codex", Some(CODEX_VERSION));
        fake_binary(on_path.path(), CODEX_BINARY, Some(CODEX_VERSION));
        let lookup = BinaryLookup {
            env_override: Some(named.as_os_str()),
            path: Some(on_path.path().as_os_str()),
        };
        assert_eq!(
            locate(lookup),
            Ok((named.clone(), BinarySource::Environment))
        );

        let missing = dir.path().join("missing-codex");
        let lookup = BinaryLookup {
            env_override: Some(missing.as_os_str()),
            path: Some(on_path.path().as_os_str()),
        };
        let error = locate(lookup).unwrap_err();
        assert!(
            matches!(&error, AppServerError::Unusable { path, reason } if path == &missing && reason.contains(CODEX_ENV)),
            "{error:?}"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_pinned_version_on_path_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let binary = fake_binary(dir.path(), CODEX_BINARY, Some(CODEX_VERSION));
        let found = discover_with(BinaryLookup {
            env_override: None,
            path: Some(dir.path().as_os_str()),
        })
        .await
        .expect("the pin is accepted");
        assert_eq!(
            found,
            LocatedBinary {
                path: binary,
                source: BinarySource::SearchPath,
                version: CODEX_VERSION.to_owned(),
            }
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn another_version_is_incompatible_not_a_best_effort() {
        let dir = tempfile::tempdir().unwrap();
        let binary = fake_binary(dir.path(), CODEX_BINARY, Some("0.154.0"));
        let error = discover_with(BinaryLookup {
            env_override: None,
            path: Some(dir.path().as_os_str()),
        })
        .await
        .unwrap_err();
        assert_eq!(
            error,
            AppServerError::Incompatible {
                path: binary.clone(),
                found: "0.154.0".into(),
            }
        );
        let message = error.to_string();
        assert!(message.contains("0.154.0"), "{message}");
        assert!(message.contains(CODEX_VERSION), "{message}");
        assert!(message.contains(&binary.display().to_string()), "{message}");

        // A newer release is refused the same way: the protocol is
        // experimental, so "at least" is not good enough.
        let dir = tempfile::tempdir().unwrap();
        fake_binary(dir.path(), CODEX_BINARY, Some("9.0.0"));
        let error = discover_with(BinaryLookup {
            env_override: None,
            path: Some(dir.path().as_os_str()),
        })
        .await
        .unwrap_err();
        assert!(matches!(
            error,
            AppServerError::Incompatible { found, .. } if found == "9.0.0"
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_binary_that_cannot_report_its_version_is_unusable() {
        let dir = tempfile::tempdir().unwrap();
        let binary = fake_binary(dir.path(), CODEX_BINARY, None);
        let error = discover_with(BinaryLookup {
            env_override: None,
            path: Some(dir.path().as_os_str()),
        })
        .await
        .unwrap_err();
        assert!(
            matches!(&error, AppServerError::Unusable { path, reason } if path == &binary && reason.contains("no such command")),
            "{error:?}"
        );

        // Output in another format is unusable too, not "version unknown".
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CODEX_BINARY);
        fs::write(&path, "#!/bin/sh\necho 'something else entirely'\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let error = reported_version(&path).await.unwrap_err();
        assert!(
            matches!(&error, AppServerError::Unusable { reason, .. } if reason.contains("something else entirely")),
            "{error:?}"
        );
    }

    /// Needs the real binary on `PATH`; run with `--ignored`.
    #[tokio::test]
    #[ignore]
    async fn live_discovery_finds_the_pinned_release() {
        let found = discover()
            .await
            .expect("codex on PATH at the pinned version");
        assert_eq!(found.version, CODEX_VERSION);
    }
}
