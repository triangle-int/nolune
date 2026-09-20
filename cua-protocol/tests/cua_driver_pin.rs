//! The pinned Cua Driver release: one version, one verified asset per target
//! Nolune's release workflow builds for, and a specific error for any driver
//! that reports another version.

use cua_protocol::{
    DriverVersion,
    cua_driver_pin::{
        ChecksumError, DriverIncompatibility, PINNED_VERSION, PinnedAsset, RELEASE_COMMIT,
        RELEASE_REPOSITORY, RELEASE_TAG, Target, asset_for, check_driver_version,
        pinned_driver_version, sha256_hex, verify,
    },
};
use std::{collections::BTreeSet, fs, path::Path};

/// sha256 of the bytes `hello`.
const HELLO_SHA256: &str = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

fn release_workflow() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(".github/workflows/release.yml");
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// Every target triple the release workflow produces a Nolune binary for:
/// explicit `target:` matrix values, `--target <triple>` build arguments, and
/// the host triple of desktop runners that build without `--target`.
fn release_workflow_triples(workflow: &str) -> BTreeSet<String> {
    let mut triples = BTreeSet::new();
    for line in workflow.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("target:") {
            triples.insert(value.trim().to_owned());
        }
        if let Some(rest) = trimmed.split_once("--target ").map(|(_, rest)| rest) {
            // A literal triple; `--target ${{ matrix.target }}` is already
            // covered by the matrix's `target:` lines.
            let triple: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            if !triple.is_empty() {
                triples.insert(triple);
            }
        }
        if let Some(value) = trimmed.strip_prefix("- platform:") {
            let host = match value.trim() {
                "macos-latest" => "aarch64-apple-darwin",
                "windows-latest" => "x86_64-pc-windows-msvc",
                runner if runner.starts_with("ubuntu-") => "x86_64-unknown-linux-gnu",
                runner => {
                    panic!("unknown desktop runner {runner:?}; teach this test its host triple")
                }
            };
            triples.insert(host.to_owned());
        }
    }
    triples
}

#[test]
fn pin_names_one_driver_version_and_the_release_it_ships_from() {
    let pinned = pinned_driver_version().expect("PINNED_VERSION is a valid DriverVersion");
    assert_eq!(pinned.as_str(), PINNED_VERSION);
    assert_eq!(RELEASE_REPOSITORY, "trycua/cua");
    assert_eq!(RELEASE_TAG, format!("cua-driver-rs-v{PINNED_VERSION}"));
    assert_eq!(RELEASE_COMMIT.len(), 40);
    assert!(RELEASE_COMMIT.bytes().all(|b| b.is_ascii_hexdigit()));
}

#[test]
fn asset_table_covers_every_target_the_release_workflow_builds_for() {
    let triples = release_workflow_triples(&release_workflow());
    assert!(
        triples.len() >= 5,
        "release.yml parse found only {triples:?}; the parser is out of step with the workflow"
    );
    for triple in &triples {
        let target = Target::from_triple(triple).unwrap_or_else(|| {
            panic!("release.yml builds for {triple} but cua_driver_pin has no Target for it")
        });
        assert_eq!(target.triple(), triple);
        assert!(
            Target::ALL.contains(&target),
            "{triple} is missing from Target::ALL"
        );
        let asset = asset_for(target);
        assert!(
            asset.name.contains(PINNED_VERSION),
            "{triple} maps to {} which is not a {PINNED_VERSION} asset",
            asset.name
        );
    }
    let all: BTreeSet<&str> = Target::ALL.iter().map(|target| target.triple()).collect();
    assert_eq!(all.len(), Target::ALL.len(), "Target::ALL repeats a triple");
    assert_eq!(
        all,
        triples.iter().map(String::as_str).collect::<BTreeSet<_>>(),
        "Target::ALL and the release workflow's targets must be the same set"
    );
}

#[test]
fn every_pinned_asset_is_a_verifiable_release_download() {
    for target in Target::ALL {
        let asset = asset_for(*target);
        assert!(
            asset
                .name
                .starts_with(&format!("cua-driver-rs-{PINNED_VERSION}-")),
            "{target}: {} is not named like a pinned driver asset",
            asset.name
        );
        assert!(
            asset.name.ends_with(".tar.gz") || asset.name.ends_with(".zip"),
            "{target}: {} is not an archive",
            asset.name
        );
        assert_eq!(
            asset.sha256.len(),
            64,
            "{target}: sha256 must be 32 bytes of hex"
        );
        assert!(
            asset
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "{target}: sha256 must be lowercase hex"
        );
        assert!(
            asset.size > 1024 * 1024,
            "{target}: a driver archive is megabytes, not {} bytes",
            asset.size
        );
        assert_eq!(
            asset.download_url(),
            format!(
                "https://github.com/{RELEASE_REPOSITORY}/releases/download/{RELEASE_TAG}/{}",
                asset.name
            )
        );
    }
    let macos = asset_for(Target::Aarch64AppleDarwin);
    assert_eq!(
        macos,
        asset_for(Target::X86_64AppleDarwin),
        "both macOS targets install the universal bundle so the CuaDriver.app identity is the same everywhere"
    );
    assert!(macos.name.contains("darwin-universal"));
}

#[test]
fn verify_accepts_matching_bytes_and_rejects_a_tampered_blob() {
    assert_eq!(sha256_hex(b"hello"), HELLO_SHA256);
    assert_eq!(verify(b"hello", HELLO_SHA256), Ok(()));
    assert_eq!(
        verify(b"hello", &HELLO_SHA256.to_uppercase()),
        Ok(()),
        "case of the expected digest does not matter"
    );

    let mut tampered = b"hello".to_vec();
    tampered[0] ^= 0x01;
    match verify(&tampered, HELLO_SHA256) {
        Err(ChecksumError::Mismatch { expected, actual }) => {
            assert_eq!(expected, HELLO_SHA256);
            assert_eq!(actual, sha256_hex(&tampered));
            assert_ne!(actual, expected);
        }
        other => panic!("tampered bytes must fail with Mismatch, got {other:?}"),
    }
    assert!(
        matches!(
            verify(b"", HELLO_SHA256),
            Err(ChecksumError::Mismatch { .. })
        ),
        "an empty download is never the pinned asset"
    );

    let too_long = format!("{HELLO_SHA256}0");
    for malformed in ["", "abc", "zz", &HELLO_SHA256[..63], too_long.as_str()] {
        assert!(
            matches!(
                verify(b"hello", malformed),
                Err(ChecksumError::InvalidExpected(_))
            ),
            "{malformed:?} is not a sha256 digest and must never verify"
        );
    }
}

#[test]
fn a_pinned_asset_checks_its_size_before_its_digest() {
    let asset = PinnedAsset {
        name: "cua-driver-rs-0.0.0-test.tar.gz",
        sha256: HELLO_SHA256,
        size: 5,
    };
    assert_eq!(asset.verify(b"hello"), Ok(()));
    assert_eq!(
        asset.verify(b"hello!"),
        Err(ChecksumError::SizeMismatch {
            expected: 5,
            actual: 6
        })
    );
    assert!(matches!(
        asset.verify(b"hellp"),
        Err(ChecksumError::Mismatch { .. })
    ));
    let message = asset.verify(b"hellp").unwrap_err().to_string();
    assert!(
        message.contains(HELLO_SHA256),
        "the error names the expected digest so a self-hoster can compare: {message}"
    );
}

#[test]
fn a_driver_reporting_any_other_version_is_incompatible() {
    let pinned = DriverVersion::try_from(PINNED_VERSION).unwrap();
    assert_eq!(check_driver_version(&pinned), Ok(()));

    for other in [
        "0.28.1",
        "0.28.3",
        "0.27.0",
        "0.29.0",
        "1.0.0",
        "0.28.2-nightly.20260915",
    ] {
        let reported = DriverVersion::try_from(other).unwrap();
        let error = check_driver_version(&reported)
            .expect_err("a driver version other than the pin is incompatible");
        assert_eq!(
            error,
            DriverIncompatibility::VersionMismatch {
                reported: reported.clone(),
                pinned: PINNED_VERSION,
            }
        );
        let message = error.to_string();
        assert!(message.contains(other), "{message}");
        assert!(message.contains(PINNED_VERSION), "{message}");
        assert!(
            message.to_lowercase().contains("does not update"),
            "the message states the no-self-update policy: {message}"
        );
        let _: &dyn std::error::Error = &error;
    }
}

#[test]
fn the_current_host_resolves_to_its_target() {
    let expected = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
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
    };
    assert_eq!(Target::current(), expected);
    assert_eq!(Target::from_triple("riscv64gc-unknown-linux-gnu"), None);
    assert_eq!(Target::from_triple(""), None);
}
