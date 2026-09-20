//! The one Cua Driver release Nolune ships, verifies and accepts.
//!
//! Both runtimes read the pin from here: the server when it installs or
//! inspects a driver, the desktop app when it bundles one. Nolune never asks
//! the driver to update itself; a driver reporting any other version is
//! incompatible until Nolune moves the pin.

use crate::{DriverVersion, ValidationError};
use std::fmt;

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
    pub const ALL: &'static [Target] = &[];

    pub fn triple(self) -> &'static str {
        todo!("cua_driver_pin::Target::triple")
    }

    pub fn from_triple(_triple: &str) -> Option<Self> {
        todo!("cua_driver_pin::Target::from_triple")
    }

    pub fn current() -> Option<Self> {
        todo!("cua_driver_pin::Target::current")
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
    pub fn download_url(&self) -> String {
        todo!("cua_driver_pin::PinnedAsset::download_url")
    }

    pub fn verify(&self, _bytes: &[u8]) -> Result<(), ChecksumError> {
        todo!("cua_driver_pin::PinnedAsset::verify")
    }
}

pub fn asset_for(_target: Target) -> &'static PinnedAsset {
    todo!("cua_driver_pin::asset_for")
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChecksumError {
    InvalidExpected(String),
    SizeMismatch { expected: u64, actual: u64 },
    Mismatch { expected: String, actual: String },
}

impl fmt::Display for ChecksumError {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!("cua_driver_pin::ChecksumError::fmt")
    }
}
impl std::error::Error for ChecksumError {}

pub fn sha256_hex(_bytes: &[u8]) -> String {
    todo!("cua_driver_pin::sha256_hex")
}

pub fn verify(_bytes: &[u8], _expected_sha256: &str) -> Result<(), ChecksumError> {
    todo!("cua_driver_pin::verify")
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DriverIncompatibility {
    VersionMismatch {
        reported: DriverVersion,
        pinned: &'static str,
    },
}

impl fmt::Display for DriverIncompatibility {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!("cua_driver_pin::DriverIncompatibility::fmt")
    }
}
impl std::error::Error for DriverIncompatibility {}

pub fn pinned_driver_version() -> Result<DriverVersion, ValidationError> {
    todo!("cua_driver_pin::pinned_driver_version")
}

pub fn check_driver_version(_reported: &DriverVersion) -> Result<(), DriverIncompatibility> {
    todo!("cua_driver_pin::check_driver_version")
}
