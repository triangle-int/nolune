//! File removal for `nolune uninstall` (#126). Service teardown lives in `service`.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// What was removed and what was deliberately left behind.
#[derive(Debug, Default)]
pub struct UninstallReport {
    pub removed: Vec<PathBuf>,
    pub kept: Option<PathBuf>,
}

/// Remove the installed binary directory and log. With `keep_data` the workspace itself
/// (config, memory, chats) stays; otherwise the whole home directory goes.
pub fn remove_files(home: &Path, keep_data: bool) -> io::Result<UninstallReport> {
    let mut report = UninstallReport::default();
    if !home.exists() {
        return Ok(report);
    }
    if keep_data {
        let bin = home.join("bin");
        if bin.exists() {
            fs::remove_dir_all(&bin)?;
            report.removed.push(bin);
        }
        let log = home.join("nolune.log");
        if log.exists() {
            fs::remove_file(&log)?;
            report.removed.push(log);
        }
        report.kept = Some(home.to_path_buf());
    } else {
        fs::remove_dir_all(home)?;
        report.removed.push(home.to_path_buf());
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn seeded_home() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        fs::create_dir_all(home.join("bin")).unwrap();
        fs::write(home.join("bin/nolune"), "binary").unwrap();
        fs::write(home.join("bin/update"), "#!/bin/bash").unwrap();
        fs::write(home.join("nolune.log"), "log").unwrap();
        fs::write(home.join("config.toml"), "auth_token = \"x\"\n").unwrap();
        fs::create_dir_all(home.join("instances/companion")).unwrap();
        fs::write(home.join("instances/companion/soul.md"), "hi").unwrap();
        tmp
    }

    #[test]
    fn keep_data_removes_only_bin_and_log() {
        let tmp = seeded_home();
        let home = tmp.path();

        let report = remove_files(home, true).unwrap();

        assert!(!home.join("bin").exists());
        assert!(!home.join("nolune.log").exists());
        assert!(home.join("config.toml").exists());
        assert!(home.join("instances/companion/soul.md").exists());
        assert_eq!(report.kept.as_deref(), Some(home));
        assert!(report.removed.contains(&home.join("bin")));
        assert!(report.removed.contains(&home.join("nolune.log")));
    }

    #[test]
    fn full_uninstall_removes_the_whole_home() {
        let tmp = seeded_home();
        let home = tmp.path().to_path_buf();

        let report = remove_files(&home, false).unwrap();

        assert!(!home.exists());
        assert_eq!(report.removed, vec![home.clone()]);
        assert!(report.kept.is_none());
    }

    #[test]
    fn missing_pieces_are_not_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("never-installed");

        let report = remove_files(&home, true).unwrap();
        assert!(report.removed.is_empty());

        let report = remove_files(&home, false).unwrap();
        assert!(report.removed.is_empty());
    }
}
