//! Installing the pinned Cua Driver under a workspace (#20).
//!
//! One installer, shared by everything that can put a driver on a computer:
//! the server's `nolune cua install`, and the desktop app's own Install
//! driver button (#231), so a desktop user never opens a terminal to get
//! one. Both download the release asset the pin names for this host, verify
//! its size and sha256 before a single byte is kept, extract it under
//! `<workspace>/cua-driver/releases/<version>/`, ask the extracted binary
//! for its version and refuse anything but the pin, then record what was
//! installed in `<workspace>/cua-driver/install.json` for discovery and
//! `nolune cua status`. A failure at any step leaves nothing installed.
//!
//! Nolune never updates the driver on its own: a new pin ships with a new
//! Nolune release, and installing again then installs that one.

use std::{
    fs, io,
    io::Cursor,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

use crate::{
    DriverVersion,
    cua_driver_pin::{
        PINNED_VERSION, PinnedAsset, RELEASE_REPOSITORY, RELEASE_TAG, Target, check_driver_version,
    },
};

/// The driver binary's file name, as every release ships it.
pub const DRIVER_BINARY: &str = "cua-driver";

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

/// The deadlines one asset download is held to, so a mirror that stalls or
/// never stops sending fails instead of hanging `nolune cua install` or
/// growing its memory without bound.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DownloadLimits {
    /// Connecting to the release host.
    pub connect: Duration,
    /// Silence between two reads of the body.
    pub read: Duration,
    /// The whole download.
    pub total: Duration,
}

impl Default for DownloadLimits {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(15),
            read: Duration::from_secs(60),
            // The largest pinned asset is 70 MB; this is generous for a slow
            // link and still finite.
            total: Duration::from_secs(30 * 60),
        }
    }
}
/// How deep inside an extracted release the driver binary is looked for.
const MAX_SEARCH_DEPTH: usize = 8;

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
    let path = manifest_path(root);
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(
                anyhow::Error::new(error).context(format!("cannot read {}", path.display()))
            );
        }
    };
    let installed = serde_json::from_str(&raw)
        .with_context(|| format!("{} is not a driver manifest", path.display()))?;
    Ok(Some(installed))
}

/// Record `installed` as the workspace's driver, atomically.
fn write_manifest(root: &Path, installed: &InstalledDriver) -> anyhow::Result<()> {
    let path = manifest_path(root);
    let staged = path.with_extension("json.tmp");
    let json = serde_json::to_vec_pretty(installed)?;
    fs::write(&staged, json).with_context(|| format!("cannot write {}", staged.display()))?;
    fs::rename(&staged, &path).with_context(|| format!("cannot write {}", path.display()))?;
    Ok(())
}

/// The release directory the assets are fetched from: `mirror` (from
/// [`RELEASE_URL_ENV`]) without its trailing slash, or the upstream release.
pub fn release_url(mirror: Option<&str>) -> String {
    match mirror.map(str::trim).filter(|mirror| !mirror.is_empty()) {
        Some(mirror) => mirror.trim_end_matches('/').to_owned(),
        None => format!("https://github.com/{RELEASE_REPOSITORY}/releases/download/{RELEASE_TAG}"),
    }
}

/// `<base>/<asset name>`.
pub fn asset_url(base: &str, asset: &PinnedAsset) -> String {
    format!("{}/{}", base.trim_end_matches('/'), asset.name)
}

/// Fetch `url`, which the pin says is `expected_size` bytes: an announced
/// size that differs is refused before the body is read, the body is
/// abandoned as soon as it exceeds the pin, and every phase runs under
/// `limits`.
async fn download(
    url: &str,
    expected_size: u64,
    limits: DownloadLimits,
) -> anyhow::Result<Vec<u8>> {
    use futures_util::StreamExt as _;

    let client = reqwest::Client::builder()
        .connect_timeout(limits.connect)
        .read_timeout(limits.read)
        .timeout(limits.total)
        .build()?;
    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("cannot download {url}"))?;
    let status = response.status();
    if !status.is_success() {
        anyhow::bail!("{url} answered HTTP {status}");
    }
    if let Some(announced) = response.content_length()
        && announced != expected_size
    {
        anyhow::bail!(
            "{url} announces {announced} bytes, the pin expects {expected_size}; nothing was downloaded"
        );
    }
    // The pin bounds the allocation; a body that keeps coming past it is
    // abandoned, not buffered.
    let mut bytes = Vec::with_capacity(usize::try_from(expected_size).unwrap_or(0));
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.with_context(|| format!("the download of {url} was interrupted"))?;
        if bytes.len() as u64 + chunk.len() as u64 > expected_size {
            anyhow::bail!(
                "{url} sent more than the pinned {expected_size} bytes; the download was abandoned"
            );
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// Unpack a downloaded `.tar.gz` or `.zip` asset into `dest`. Entries that
/// would escape `dest` are refused by the archive readers.
pub fn extract(bytes: &[u8], asset_name: &str, dest: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dest)?;
    if asset_name.ends_with(".tar.gz") {
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes));
        for entry in archive
            .entries()
            .with_context(|| format!("cannot read {asset_name}"))?
        {
            let mut entry = entry.with_context(|| format!("cannot read {asset_name}"))?;
            let path = entry.path()?.into_owned();
            // `unpack_in` answers false instead of writing an entry that
            // would land outside `dest`; such an archive is not the driver.
            if !entry
                .unpack_in(dest)
                .with_context(|| format!("cannot unpack {} from {asset_name}", path.display()))?
            {
                anyhow::bail!(
                    "{asset_name} contains {} which would escape the install directory",
                    path.display()
                );
            }
        }
    } else if asset_name.ends_with(".zip") {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
            .with_context(|| format!("cannot read {asset_name}"))?;
        archive
            .extract(dest)
            .with_context(|| format!("cannot unpack {asset_name}"))?;
    } else {
        anyhow::bail!("{asset_name} is neither a .tar.gz nor a .zip archive");
    }
    Ok(())
}

/// The driver binary inside an extracted release: on macOS the one inside
/// `CuaDriver.app` (so Accessibility and Screen Recording attribute to the
/// driver's bundle), otherwise the shallowest `cua-driver` binary.
pub fn find_driver_binary(dir: &Path) -> Option<PathBuf> {
    fn walk(dir: &Path, depth: usize, name: &str, found: &mut Vec<(usize, PathBuf)>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() && depth < MAX_SEARCH_DEPTH {
                walk(&path, depth + 1, name, found);
            } else if kind.is_file() && path.file_name().is_some_and(|file| file == name) {
                found.push((depth, path));
            }
        }
    }
    let name = format!("{DRIVER_BINARY}{}", std::env::consts::EXE_SUFFIX);
    let mut found = Vec::new();
    walk(dir, 0, &name, &mut found);
    let in_bundle = |path: &Path| {
        cfg!(target_os = "macos")
            && path
                .to_string_lossy()
                .contains("CuaDriver.app/Contents/MacOS/")
    };
    found.sort_by_key(|(depth, path)| (!in_bundle(path), *depth, path.clone()));
    found.into_iter().next().map(|(_, path)| path)
}

/// The version from `cua-driver --version` output (`cua-driver 0.28.2`).
pub fn parse_version_output(stdout: &str) -> anyhow::Result<DriverVersion> {
    let line = stdout
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    let unexpected = || anyhow::anyhow!("unexpected `--version` output {line:?}");
    // `cua-driver 0.28.2`, tolerating a `v` prefix and trailing build info.
    let token = line.split_whitespace().nth(1).ok_or_else(unexpected)?;
    let token = token.strip_prefix('v').unwrap_or(token);
    if !token.starts_with(|c: char| c.is_ascii_digit()) {
        return Err(unexpected());
    }
    DriverVersion::try_from(token).map_err(|error| anyhow::anyhow!("{}: {error}", unexpected()))
}

/// Ask `driver --version` which version it is.
pub async fn reported_version(driver: &Path) -> anyhow::Result<DriverVersion> {
    let output = tokio::time::timeout(
        VERSION_TIMEOUT,
        tokio::process::Command::new(driver)
            .arg("--version")
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| {
        anyhow::anyhow!(
            "{} --version did not answer within {VERSION_TIMEOUT:?}",
            driver.display()
        )
    })?
    .with_context(|| format!("cannot run {} --version", driver.display()))?;
    if !output.status.success() {
        anyhow::bail!(
            "{} --version failed ({}): {}",
            driver.display(),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    parse_version_output(&String::from_utf8_lossy(&output.stdout))
        .with_context(|| format!("{} --version", driver.display()))
}

/// Download, verify, extract, check and record the driver.
pub async fn install(
    request: &InstallRequest<'_>,
    // `Send` so the whole install can be awaited from a Tauri command, which
    // is how the desktop app's Install driver button runs it.
    progress: &mut (dyn FnMut(InstallStep) + Send),
) -> anyhow::Result<InstallOutcome> {
    let InstallRequest {
        root,
        target,
        asset,
        release_url,
        force,
    } = *request;
    // The manifest records absolute paths; a relative NOLUNE_HOME must not
    // make them depend on where the next command runs from.
    let root = std::path::absolute(root)?;
    if !force
        && let Some(existing) = read_manifest(&root)?
        && existing.version == PINNED_VERSION
        && existing.sha256 == asset.sha256
        && existing.driver.is_file()
    {
        return Ok(InstallOutcome::AlreadyInstalled(existing));
    }

    let url = asset_url(release_url, asset);
    progress(InstallStep::Downloading {
        url: url.clone(),
        size: asset.size,
    });
    let bytes = download(&url, asset.size, DownloadLimits::default()).await?;
    // Nothing is written before the bytes are exactly the pinned asset.
    asset.verify(&bytes).map_err(|error| {
        anyhow::anyhow!(
            "{} from {url} failed verification: {error} (the pin expects sha256 {} over {} bytes)",
            asset.name,
            asset.sha256,
            asset.size
        )
    })?;
    progress(InstallStep::Verified {
        sha256: asset.sha256.to_owned(),
    });

    let dir = install_dir(&root);
    fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
    // Everything below happens in a staging directory that goes away with
    // this scope, so a failure leaves nothing installed.
    let staging = tempfile::Builder::new()
        .prefix(".staging-")
        .tempdir_in(&dir)
        .with_context(|| format!("cannot stage under {}", dir.display()))?;
    let files = staging.path().join("files");
    extract(&bytes, asset.name, &files)?;
    let driver = find_driver_binary(&files)
        .ok_or_else(|| anyhow::anyhow!("{} contains no {DRIVER_BINARY} binary", asset.name))?;
    let relative = driver
        .strip_prefix(&files)
        .expect("found under the staging directory")
        .to_path_buf();
    let reported = reported_version(&driver).await?;
    check_driver_version(&reported).map_err(|error| {
        anyhow::anyhow!(
            "the downloaded driver reports {}: {error}",
            reported.as_str()
        )
    })?;
    progress(InstallStep::VersionChecked {
        version: reported.as_str().to_owned(),
    });

    let releases = dir.join(RELEASES_DIR);
    fs::create_dir_all(&releases)
        .with_context(|| format!("cannot create {}", releases.display()))?;
    let release_dir = releases.join(reported.as_str());
    if release_dir.exists() {
        fs::remove_dir_all(&release_dir)
            .with_context(|| format!("cannot replace {}", release_dir.display()))?;
    }
    fs::rename(&files, &release_dir)
        .with_context(|| format!("cannot move the driver into {}", release_dir.display()))?;
    let installed = InstalledDriver {
        version: reported.as_str().to_owned(),
        target: target.triple().to_owned(),
        asset: asset.name.to_owned(),
        sha256: asset.sha256.to_owned(),
        size: asset.size,
        driver: release_dir.join(relative),
        installed_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    };
    write_manifest(&root, &installed)?;
    Ok(InstallOutcome::Installed(installed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cua_driver_pin::asset_for;
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
        // The tar writer refuses `..` itself, so the name goes in raw.
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        let mut header = tar::Header::new_gnu();
        let name = b"../escaped";
        header.as_gnu_mut().unwrap().name[..name.len()].copy_from_slice(name);
        header.set_size(4);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, &b"nope"[..]).unwrap();
        let escaping = builder.into_inner().unwrap().finish().unwrap();

        let error = extract(&escaping, "x.tar.gz", &dest).unwrap_err();
        assert!(error.to_string().contains("escape"), "{error}");
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

    /// A one-shot HTTP/1.1 server answering the next connection with `head`
    /// (status line and headers, no trailing blank line) and `body`, written
    /// in `chunks` with `pause` between them, then holding the connection
    /// open for `linger` before closing it.
    fn one_shot_server(
        head: &'static str,
        body: Vec<u8>,
        chunks: usize,
        pause: Duration,
        linger: Duration,
    ) -> String {
        use std::{io::Write as _, net::TcpListener};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 4096];
            let _ = std::io::Read::read(&mut stream, &mut request);
            let _ = stream.write_all(format!("{head}\r\n\r\n").as_bytes());
            let _ = stream.flush();
            let step = body.len().div_ceil(chunks.max(1)).max(1);
            for chunk in body.chunks(step) {
                let _ = stream.write_all(chunk);
                let _ = stream.flush();
                std::thread::sleep(pause);
            }
            std::thread::sleep(linger);
        });
        format!("http://127.0.0.1:{port}/asset.tar.gz")
    }

    fn quick_limits() -> DownloadLimits {
        DownloadLimits {
            connect: Duration::from_secs(5),
            read: Duration::from_millis(400),
            total: Duration::from_secs(5),
        }
    }

    #[tokio::test]
    async fn download_refuses_an_announced_size_that_is_not_the_pin_before_reading_the_body() {
        let url = one_shot_server(
            "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: 999",
            vec![b'x'; 999],
            1,
            Duration::ZERO,
            Duration::from_millis(100),
        );
        let error = download(&url, 64, quick_limits()).await.unwrap_err();
        let message = format!("{error:#}");
        assert!(
            message.contains("999"),
            "names the announced size: {message}"
        );
        assert!(message.contains("64"), "names the pinned size: {message}");
        assert!(
            message.contains("nothing was downloaded"),
            "says the body was never read: {message}"
        );
    }

    #[tokio::test]
    async fn download_stops_once_more_than_the_pinned_size_arrives() {
        // No Content-Length, so only the running total can catch the excess:
        // 4096 bytes of body against a 1024-byte pin, in several writes.
        let url = one_shot_server(
            "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nConnection: close",
            vec![b'y'; 4096],
            8,
            Duration::from_millis(10),
            Duration::from_secs(3),
        );
        let started = std::time::Instant::now();
        let error = download(&url, 1024, quick_limits()).await.unwrap_err();
        let message = format!("{error:#}");
        assert!(
            message.contains("more than") && message.contains("1024"),
            "{message}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "the download is abandoned as soon as the pin is exceeded, took {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn download_fails_when_the_mirror_stalls_instead_of_hanging() {
        // Half the announced body, then silence far longer than the read
        // deadline.
        let url = one_shot_server(
            "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: 2048",
            vec![b'z'; 1024],
            1,
            Duration::ZERO,
            Duration::from_secs(30),
        );
        let started = std::time::Instant::now();
        let error = download(&url, 2048, quick_limits()).await.unwrap_err();
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "a stalled body fails within the deadlines, took {:?}",
            started.elapsed()
        );
        let message = format!("{error:#}");
        assert!(message.contains(&url), "names the URL: {message}");
    }

    #[test]
    fn the_default_download_limits_are_finite() {
        let limits = DownloadLimits::default();
        assert!(limits.read >= Duration::from_secs(10));
        assert!(limits.total >= Duration::from_secs(60));
        assert!(limits.total <= Duration::from_secs(3600));
    }
}
