//! Installing the pinned Cua Driver under a workspace (#20).
//!
//! `nolune cua install` downloads the release asset the pin names for this
//! host, verifies its size and sha256 before a single byte is kept, extracts
//! it under `<workspace>/cua-driver/releases/<version>/`, asks the extracted
//! binary for its version and refuses anything but the pin, then records
//! what it installed in `<workspace>/cua-driver/install.json` for discovery
//! and `nolune cua status`. A failure at any step leaves nothing installed.
//!
//! Nolune never updates the driver on its own: a new pin ships with a new
//! Nolune release, and `nolune cua install` then installs that one.

use std::{
    io::Cursor,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::Context as _;
use cua_protocol::{
    DriverVersion,
    cua_driver_pin::{
        PINNED_VERSION, PinnedAsset, RELEASE_REPOSITORY, RELEASE_TAG, Target, check_driver_version,
    },
};
use serde::{Deserialize, Serialize};

/// The directory under the workspace root that holds the driver.
pub const INSTALL_DIR: &str = "cua-driver";
/// The manifest inside [`INSTALL_DIR`] describing the installed driver.
pub const MANIFEST_FILE: &str = "install.json";
/// The directory inside [`INSTALL_DIR`] holding one extracted release per
/// version.
pub const RELEASES_DIR: &str = "releases";
/// Names a mirror of the pinned release assets (`<mirror>/<asset name>`) in
/// place of the upstream GitHub release. The checksum is enforced either way.
pub const RELEASE_URL_ENV: &str = "NOLUNE_CUA_RELEASE_URL";

/// How long the extracted driver may take to answer `--version`.
const VERSION_TIMEOUT: Duration = Duration::from_secs(15);

/// What `install.json` records about the driver a workspace carries.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct InstalledDriver {
    /// The version the extracted driver reported, which equals the pin.
    pub version: String,
    /// The target triple the asset was pinned for.
    pub target: String,
    /// The release asset that was downloaded.
    pub asset: String,
    /// The sha256 the asset verified against.
    pub sha256: String,
    pub size: u64,
    /// The driver binary, absolute.
    pub driver: PathBuf,
    /// RFC 3339 timestamp of the install.
    pub installed_at: String,
}

/// One install: where, what, and from where.
pub struct InstallRequest<'a> {
    /// The workspace root (a profile's data root).
    pub root: &'a Path,
    pub target: Target,
    /// The asset to fetch and verify; the pinned one outside tests.
    pub asset: &'a PinnedAsset,
    /// The release directory URL the asset is fetched from.
    pub release_url: &'a str,
    /// Install again even when the pinned driver is already present.
    pub force: bool,
}

/// Progress a caller can narrate while [`install`] runs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InstallStep {
    Downloading { url: String, size: u64 },
    Verified { sha256: String },
    Extracted { dir: PathBuf },
    VersionChecked { version: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InstallOutcome {
    /// The pinned driver was already in place; nothing was downloaded.
    AlreadyInstalled(InstalledDriver),
    Installed(InstalledDriver),
}

/// `<root>/cua-driver`.
pub fn install_dir(root: &Path) -> PathBuf {
    root.join(INSTALL_DIR)
}

/// `<root>/cua-driver/install.json`.
pub fn manifest_path(root: &Path) -> PathBuf {
    install_dir(root).join(MANIFEST_FILE)
}

/// The manifest of the driver installed under `root`, `None` when there is
/// none, an error when there is one that cannot be read.
pub fn read_manifest(root: &Path) -> anyhow::Result<Option<InstalledDriver>> {
    let _ = root;
    todo!("read the manifest")
}

/// The release directory the assets are fetched from: `mirror` (from
/// [`RELEASE_URL_ENV`]) without its trailing slash, or the upstream release.
pub fn release_url(mirror: Option<&str>) -> String {
    let _ = mirror;
    todo!("release url")
}

/// `<base>/<asset name>`.
pub fn asset_url(base: &str, asset: &PinnedAsset) -> String {
    let _ = (base, asset);
    todo!("asset url")
}

/// Unpack a downloaded `.tar.gz` or `.zip` asset into `dest`. Entries that
/// would escape `dest` are refused by the archive readers.
pub fn extract(bytes: &[u8], asset_name: &str, dest: &Path) -> anyhow::Result<()> {
    let _ = (bytes, asset_name, dest, Cursor::new(()));
    todo!("extract")
}

/// The driver binary inside an extracted release: on macOS the one inside
/// `CuaDriver.app` (so Accessibility and Screen Recording attribute to the
/// driver's bundle), otherwise the shallowest `cua-driver` binary.
pub fn find_driver_binary(dir: &Path) -> Option<PathBuf> {
    let _ = dir;
    todo!("find driver binary")
}

/// The version from `cua-driver --version` output (`cua-driver 0.28.2`).
pub fn parse_version_output(stdout: &str) -> anyhow::Result<DriverVersion> {
    let _ = stdout;
    todo!("parse version")
}

/// Ask `driver --version` which version it is.
pub async fn reported_version(driver: &Path) -> anyhow::Result<DriverVersion> {
    let _ = (driver, VERSION_TIMEOUT);
    todo!("reported version")
}

/// Download, verify, extract, check and record the driver.
pub async fn install(
    request: InstallRequest<'_>,
    progress: &mut dyn FnMut(InstallStep),
) -> anyhow::Result<InstallOutcome> {
    let _ = (request, progress);
    let _ = (
        PINNED_VERSION,
        RELEASE_REPOSITORY,
        RELEASE_TAG,
        check_driver_version,
        RELEASES_DIR,
    );
    let _: Option<anyhow::Error> = None::<anyhow::Error>.map(|e| e.context("install"));
    todo!("install")
}

#[cfg(test)]
mod tests {
    use super::*;
    use cua_protocol::cua_driver_pin::asset_for;
    use std::{fs, io::Write as _};

    #[test]
    fn the_default_release_url_is_the_upstream_release_and_mirrors_lose_their_slash() {
        let asset = asset_for(Target::X86_64UnknownLinuxGnu);
        assert_eq!(asset_url(&release_url(None), asset), asset.download_url());
        assert_eq!(
            release_url(Some("http://127.0.0.1:1/release/")),
            "http://127.0.0.1:1/release"
        );
        assert_eq!(
            asset_url("http://127.0.0.1:1/release", asset),
            format!("http://127.0.0.1:1/release/{}", asset.name)
        );
        assert_eq!(
            release_url(Some("")),
            release_url(None),
            "an empty mirror is no mirror"
        );
    }

    fn tarball(entries: &[(&str, &[u8], u32)]) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        for (path, contents, mode) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(*mode);
            header.set_cksum();
            builder.append_data(&mut header, path, *contents).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    fn zipfile(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (path, contents) in entries {
            zip.start_file(*path, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(contents).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn extract_unpacks_tarballs_and_zips_and_refuses_other_names() {
        let dir = tempfile::tempdir().unwrap();
        let tar_dest = dir.path().join("tar");
        extract(
            &tarball(&[("stage/cua-driver", b"#!/bin/sh\n", 0o755)]),
            "cua-driver-rs-x-linux-x86_64-binary.tar.gz",
            &tar_dest,
        )
        .unwrap();
        let unpacked = tar_dest.join("stage/cua-driver");
        assert_eq!(fs::read(&unpacked).unwrap(), b"#!/bin/sh\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_ne!(
                fs::metadata(&unpacked).unwrap().permissions().mode() & 0o111,
                0,
                "the executable bit survives extraction"
            );
        }

        let zip_dest = dir.path().join("zip");
        extract(
            &zipfile(&[("cua-driver.exe", b"MZ")]),
            "cua-driver-rs-x-windows-x86_64.zip",
            &zip_dest,
        )
        .unwrap();
        assert_eq!(fs::read(zip_dest.join("cua-driver.exe")).unwrap(), b"MZ");

        let error = extract(b"whatever", "cua-driver.dmg", &dir.path().join("dmg")).unwrap_err();
        assert!(error.to_string().contains("cua-driver.dmg"), "{error}");
    }

    #[test]
    fn extract_refuses_entries_that_escape_the_destination() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("dest");
        let escaping = tarball(&[("../escaped", b"nope", 0o644)]);
        assert!(extract(&escaping, "x.tar.gz", &dest).is_err());
        assert!(!dir.path().join("escaped").exists());
    }

    #[test]
    fn find_driver_binary_prefers_the_app_bundle_on_macos_and_the_shallowest_binary_elsewhere() {
        let dir = tempfile::tempdir().unwrap();
        let stage = dir.path().join("cua-driver-rs-0-darwin-universal");
        let bundle = stage.join("CuaDriver.app/Contents/MacOS");
        fs::create_dir_all(&bundle).unwrap();
        fs::write(stage.join("cua-driver"), "bare").unwrap();
        fs::write(bundle.join("cua-driver"), "bundled").unwrap();
        fs::write(bundle.join("cua-cursor-theme"), "theme").unwrap();

        let found = find_driver_binary(dir.path()).expect("a driver is there");
        if cfg!(target_os = "macos") {
            assert_eq!(found, bundle.join("cua-driver"));
        } else {
            assert_eq!(found, stage.join("cua-driver"));
        }

        let flat = tempfile::tempdir().unwrap();
        fs::write(flat.path().join("cua-driver"), "bin").unwrap();
        fs::write(flat.path().join("libcua_driver_sdk.so"), "lib").unwrap();
        assert_eq!(
            find_driver_binary(flat.path()),
            Some(flat.path().join("cua-driver"))
        );

        let empty = tempfile::tempdir().unwrap();
        fs::create_dir_all(empty.path().join("cua-driver")).unwrap();
        assert_eq!(
            find_driver_binary(empty.path()),
            None,
            "a directory of that name is not the binary"
        );
    }

    #[test]
    fn parse_version_output_reads_the_drivers_own_format() {
        assert_eq!(
            parse_version_output("cua-driver 0.28.2\n")
                .unwrap()
                .as_str(),
            "0.28.2"
        );
        assert_eq!(
            parse_version_output("cua-driver v0.30.0-rc.1 (abc)\n")
                .unwrap()
                .as_str(),
            "0.30.0-rc.1"
        );
        for bad in ["", "\n", "cua-driver", "usage: cua-driver [SUBCOMMAND]"] {
            assert!(parse_version_output(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn read_manifest_is_none_until_an_install_writes_it() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_manifest(dir.path()).unwrap(), None);
        assert!(
            !install_dir(dir.path()).exists(),
            "reading never creates the directory"
        );

        fs::create_dir_all(install_dir(dir.path())).unwrap();
        fs::write(manifest_path(dir.path()), "{not json").unwrap();
        assert!(read_manifest(dir.path()).is_err());

        let installed = InstalledDriver {
            version: PINNED_VERSION.to_owned(),
            target: Target::X86_64UnknownLinuxGnu.triple().to_owned(),
            asset: "a.tar.gz".to_owned(),
            sha256: "0".repeat(64),
            size: 1,
            driver: dir.path().join("cua-driver/releases/x/cua-driver"),
            installed_at: "2026-09-20T00:00:00Z".to_owned(),
        };
        fs::write(
            manifest_path(dir.path()),
            serde_json::to_vec(&installed).unwrap(),
        )
        .unwrap();
        assert_eq!(read_manifest(dir.path()).unwrap(), Some(installed));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn reported_version_runs_the_binary_and_fails_on_nonsense() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let driver = dir.path().join("cua-driver");
        fs::write(&driver, "#!/bin/sh\necho \"cua-driver 0.28.2\"\n").unwrap();
        fs::set_permissions(&driver, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(reported_version(&driver).await.unwrap().as_str(), "0.28.2");

        let mute = dir.path().join("mute");
        fs::write(&mute, "#!/bin/sh\nexit 3\n").unwrap();
        fs::set_permissions(&mute, fs::Permissions::from_mode(0o755)).unwrap();
        let error = reported_version(&mute).await.unwrap_err();
        assert!(error.to_string().contains("mute"), "{error}");

        let missing = dir.path().join("missing");
        assert!(reported_version(&missing).await.is_err());
    }
}
