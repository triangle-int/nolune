//! The one Cua Driver release Nolune ships, verifies and accepts.
//!
//! Both runtimes read the pin from here: the server when it installs or
//! inspects a driver, the desktop app when it bundles one. Nolune never asks
//! the driver to update itself; a driver reporting any other version is
//! incompatible until Nolune moves the pin, and moving it means changing
//! [`PINNED_VERSION`], [`RELEASE_TAG`], [`RELEASE_COMMIT`] and every digest
//! below together, from the release's own `checksums.txt` and the GitHub
//! asset digests (the two must agree).
//!
//! Asset selection mirrors the upstream installers for the same release: on
//! macOS the `darwin-universal` directory tarball, because it carries the
//! `CuaDriver.app` bundle that owns the `com.trycua.driver` Accessibility and
//! Screen Recording grants (one universal binary serves both Mac targets);
//! on Linux the bare-binary tarball; on Windows the archive the PowerShell
//! installer fetches.

use crate::{DriverVersion, ValidationError};
use sha2::{Digest, Sha256};
use std::fmt::{self, Write as _};

/// The tested Cua Driver version, as the driver reports it in `driver_version`.
pub const PINNED_VERSION: &str = "0.28.2";
/// GitHub repository the pinned release is published from.
pub const RELEASE_REPOSITORY: &str = "trycua/cua";
/// Release tag carrying the pinned assets.
pub const RELEASE_TAG: &str = "cua-driver-rs-v0.28.2";
/// Commit the pinned release was cut from.
pub const RELEASE_COMMIT: &str = "fc188250b4ca8549b8e61f937fdb1fb560770e86";

/// A host Nolune's release workflow builds for and therefore needs a driver on.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Target {
    Aarch64AppleDarwin,
    X86_64AppleDarwin,
    X86_64UnknownLinuxGnu,
    Aarch64UnknownLinuxGnu,
    X86_64PcWindowsMsvc,
}

impl Target {
    pub const ALL: &'static [Target] = &[
        Target::Aarch64AppleDarwin,
        Target::X86_64AppleDarwin,
        Target::X86_64UnknownLinuxGnu,
        Target::Aarch64UnknownLinuxGnu,
        Target::X86_64PcWindowsMsvc,
    ];

    /// The Rust target triple, as `release.yml` and `rustup` spell it.
    pub fn triple(self) -> &'static str {
        match self {
            Target::Aarch64AppleDarwin => "aarch64-apple-darwin",
            Target::X86_64AppleDarwin => "x86_64-apple-darwin",
            Target::X86_64UnknownLinuxGnu => "x86_64-unknown-linux-gnu",
            Target::Aarch64UnknownLinuxGnu => "aarch64-unknown-linux-gnu",
            Target::X86_64PcWindowsMsvc => "x86_64-pc-windows-msvc",
        }
    }

    pub fn from_triple(triple: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|target| target.triple() == triple)
    }

    /// The target this binary was compiled for, or `None` on a host Nolune
    /// ships no driver for.
    pub fn current() -> Option<Self> {
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            Some(Target::Aarch64AppleDarwin)
        } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
            Some(Target::X86_64AppleDarwin)
        } else if cfg!(all(
            target_os = "linux",
            target_arch = "x86_64",
            target_env = "gnu"
        )) {
            Some(Target::X86_64UnknownLinuxGnu)
        } else if cfg!(all(
            target_os = "linux",
            target_arch = "aarch64",
            target_env = "gnu"
        )) {
            Some(Target::Aarch64UnknownLinuxGnu)
        } else if cfg!(all(
            target_os = "windows",
            target_arch = "x86_64",
            target_env = "msvc"
        )) {
            Some(Target::X86_64PcWindowsMsvc)
        } else {
            None
        }
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.triple())
    }
}

/// One release asset: its file name, the sha256 it must hash to and its size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PinnedAsset {
    pub name: &'static str,
    /// Lowercase hex sha256 of the asset bytes.
    pub sha256: &'static str,
    pub size: u64,
}

impl PinnedAsset {
    /// The release download URL for this asset.
    pub fn download_url(&self) -> String {
        format!(
            "https://github.com/{RELEASE_REPOSITORY}/releases/download/{RELEASE_TAG}/{}",
            self.name
        )
    }

    /// Accepts `bytes` only when they are exactly this asset: the size is
    /// checked before the digest so a truncated download fails fast.
    pub fn verify(&self, bytes: &[u8]) -> Result<(), ChecksumError> {
        let actual = bytes.len() as u64;
        if actual != self.size {
            return Err(ChecksumError::SizeMismatch {
                expected: self.size,
                actual,
            });
        }
        verify(bytes, self.sha256)
    }
}

static DARWIN_UNIVERSAL: PinnedAsset = PinnedAsset {
    name: "cua-driver-rs-0.28.2-darwin-universal.tar.gz",
    sha256: "e273181b26709c88b1d809474deb3c592b4efae3530b11d76318f1887fc3fbb1",
    size: 70_078_129,
};
static LINUX_X86_64: PinnedAsset = PinnedAsset {
    name: "cua-driver-rs-0.28.2-linux-x86_64-binary.tar.gz",
    sha256: "a1d99fd04bb4927ef5ffdbe60eb91ed8b51a2bab60e10fc604a75bd59ce69c3e",
    size: 30_603_028,
};
static LINUX_ARM64: PinnedAsset = PinnedAsset {
    name: "cua-driver-rs-0.28.2-linux-arm64-binary.tar.gz",
    sha256: "55e8a32839a4ac369a773df4dac87b345bd4567779221ade4a5e39223a45a2e8",
    size: 30_639_613,
};
static WINDOWS_X86_64: PinnedAsset = PinnedAsset {
    name: "cua-driver-rs-0.28.2-windows-x86_64.zip",
    sha256: "3c1fcf10ff9513b94e4af78ad6a216ab62aa95b2c9a3b70dfbdba9f04e021533",
    size: 29_086_255,
};

/// The release asset a target installs. Total over [`Target`], so adding a
/// target to the release workflow means choosing and pinning its asset here.
pub fn asset_for(target: Target) -> &'static PinnedAsset {
    match target {
        Target::Aarch64AppleDarwin | Target::X86_64AppleDarwin => &DARWIN_UNIVERSAL,
        Target::X86_64UnknownLinuxGnu => &LINUX_X86_64,
        Target::Aarch64UnknownLinuxGnu => &LINUX_ARM64,
        Target::X86_64PcWindowsMsvc => &WINDOWS_X86_64,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChecksumError {
    /// The expected digest is not 64 hex characters, so nothing can match it.
    InvalidExpected(String),
    SizeMismatch {
        expected: u64,
        actual: u64,
    },
    Mismatch {
        expected: String,
        actual: String,
    },
}

impl fmt::Display for ChecksumError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChecksumError::InvalidExpected(expected) => {
                write!(f, "expected sha256 {expected:?} is not a hex digest")
            }
            ChecksumError::SizeMismatch { expected, actual } => {
                write!(f, "expected {expected} bytes, got {actual}")
            }
            ChecksumError::Mismatch { expected, actual } => {
                write!(f, "sha256 mismatch: expected {expected}, got {actual}")
            }
        }
    }
}
impl std::error::Error for ChecksumError {}

/// Lowercase hex sha256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

/// Accepts `bytes` only when their sha256 is `expected_sha256` (hex, either case).
pub fn verify(bytes: &[u8], expected_sha256: &str) -> Result<(), ChecksumError> {
    if expected_sha256.len() != 64 || !expected_sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ChecksumError::InvalidExpected(expected_sha256.to_owned()));
    }
    let expected = expected_sha256.to_ascii_lowercase();
    let actual = sha256_hex(bytes);
    if actual == expected {
        Ok(())
    } else {
        Err(ChecksumError::Mismatch { expected, actual })
    }
}

/// Why a running driver cannot be used with this build of Nolune.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DriverIncompatibility {
    /// The driver reports a version other than the pin. Nolune does not update
    /// the driver by itself, so the remedy is to install the pinned release.
    VersionMismatch {
        reported: DriverVersion,
        pinned: &'static str,
    },
}

impl fmt::Display for DriverIncompatibility {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DriverIncompatibility::VersionMismatch { reported, pinned } => write!(
                f,
                "Cua Driver {} is not the pinned {pinned}; Nolune does not update the driver on \
                 its own, so install Cua Driver {pinned} and restart it",
                reported.as_str()
            ),
        }
    }
}
impl std::error::Error for DriverIncompatibility {}

/// [`PINNED_VERSION`] as the typed version the protocol carries.
pub fn pinned_driver_version() -> Result<DriverVersion, ValidationError> {
    DriverVersion::try_from(PINNED_VERSION)
}

/// Accepts exactly the pinned version; anything else is a [`DriverIncompatibility`].
pub fn check_driver_version(reported: &DriverVersion) -> Result<(), DriverIncompatibility> {
    if reported.as_str() == PINNED_VERSION {
        Ok(())
    } else {
        Err(DriverIncompatibility::VersionMismatch {
            reported: reported.clone(),
            pinned: PINNED_VERSION,
        })
    }
}
