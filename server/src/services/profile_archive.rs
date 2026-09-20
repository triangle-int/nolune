//! Versioned companion backup archives (#74).
//!
//! An archive is a gzip-compressed tar stream whose entries are all rooted at
//! `companion/`. The identity marker `companion/companion.json` is the
//! manifest: readers accept exactly one format version and one slug, and the
//! writer emits it first so a reader can fail before touching payload data.
//!
//! The reader never resolves ambient paths. It extracts into a caller-supplied
//! cap-std [`Dir`] (a staging directory), creating every file with
//! `create_new` and without following symlinks, and enforces entry-count,
//! per-file, total, and decompressed-byte caps while streaming so a hostile
//! archive fails closed before it can exhaust disk or memory.
//! See `docs/companion-storage.md` for the format.

use std::io::{self, Read, Write};

use cap_std::fs::Dir;

use crate::domain::companion::{CANONICAL_SLUG, CompanionIdentity, STORAGE_FORMAT_VERSION};

/// Directory every archive entry is rooted at.
pub const ARCHIVE_ROOT: &str = CANONICAL_SLUG;

/// Format version a reader accepts and the writer produces. Shares the on-disk
/// storage version because the archive is the companion directory verbatim.
pub const ARCHIVE_FORMAT_VERSION: u32 = STORAGE_FORMAT_VERSION;

/// File name offered for downloads.
pub const ARCHIVE_FILE_NAME: &str = "companion.tar.gz";

/// Media type of the archive stream.
pub const ARCHIVE_CONTENT_TYPE: &str = "application/gzip";

/// Caps a reader enforces while streaming. Production values are generous for
/// a real companion (uploads are capped at 500 MB each) and small enough that
/// a decompression bomb or an entry flood fails long before disk or memory
/// runs out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Raw tar entries, including long-name and pax extension entries.
    pub max_entries: usize,
    /// Declared size of any one regular file.
    pub max_file_bytes: u64,
    /// Sum of declared regular-file sizes.
    pub max_total_bytes: u64,
    /// Bytes read out of the gzip stream, headers and padding included.
    pub max_decompressed_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_entries: 100_000,
            max_file_bytes: 512 * 1024 * 1024,
            max_total_bytes: 8 * 1024 * 1024 * 1024,
            max_decompressed_bytes: 8 * 1024 * 1024 * 1024 + 256 * 1024 * 1024,
        }
    }
}

/// What a reader extracted or a writer emitted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArchiveSummary {
    /// Regular files, the manifest included.
    pub files: usize,
    /// Explicit directory entries.
    pub directories: usize,
    /// Payload bytes of regular files.
    pub bytes: u64,
    /// Source entries the writer left out: links, special files, retired layouts.
    pub skipped: usize,
}

/// Why an archive was refused. Every variant is fail-closed: nothing outside
/// the staging directory has been touched.
#[derive(Debug)]
pub enum ArchiveError {
    /// Not a gzip tar stream, or truncated.
    Malformed(String),
    /// A link, device, fifo, sparse, or unknown entry.
    UnsupportedEntry {
        path: String,
        kind: String,
    },
    /// Absolute, `..`, `.`, backslash, empty segment, NUL, non-UTF-8, or too long.
    InvalidPath(String),
    /// Not under `companion/`.
    WrongRoot(String),
    /// A directory the current layout no longer reads.
    RetiredLayout(String),
    /// Duplicate entry, or a file and a directory claiming the same path.
    Conflict(String),
    TooManyEntries {
        limit: usize,
    },
    FileTooLarge {
        path: String,
        size: u64,
        limit: u64,
    },
    TotalTooLarge {
        limit: u64,
    },
    DecompressedTooLarge {
        limit: u64,
    },
    /// No `companion/companion.json` in the archive or source directory.
    MissingIdentity,
    /// The marker does not parse or names another version or slug.
    InvalidIdentity(String),
    Io(io::Error),
}

impl std::fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed(message) => write!(f, "archive is malformed: {message}"),
            Self::UnsupportedEntry { path, kind } => {
                write!(
                    f,
                    "archive entry {path:?} is a {kind}, which is not allowed"
                )
            }
            Self::InvalidPath(path) => write!(f, "archive entry path {path:?} is invalid"),
            Self::WrongRoot(path) => {
                write!(f, "archive entry {path:?} is not under {ARCHIVE_ROOT}/")
            }
            Self::RetiredLayout(path) => {
                write!(f, "archive entry {path:?} uses a retired layout")
            }
            Self::Conflict(path) => {
                write!(f, "archive entry {path:?} conflicts with an earlier entry")
            }
            Self::TooManyEntries { limit } => write!(f, "archive has more than {limit} entries"),
            Self::FileTooLarge { path, size, limit } => write!(
                f,
                "archive entry {path:?} is {size} bytes, more than the {limit} byte limit"
            ),
            Self::TotalTooLarge { limit } => {
                write!(f, "archive payload exceeds the {limit} byte limit")
            }
            Self::DecompressedTooLarge { limit } => {
                write!(f, "archive decompresses to more than {limit} bytes")
            }
            Self::MissingIdentity => write!(f, "archive has no {ARCHIVE_ROOT}/companion.json"),
            Self::InvalidIdentity(message) => write!(f, "archive manifest is invalid: {message}"),
            Self::Io(error) => write!(f, "archive i/o failed: {error}"),
        }
    }
}

impl std::error::Error for ArchiveError {}

impl From<io::Error> for ArchiveError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Stream `archive` into `staging` with the default [`Limits`]. On error the
/// caller discards `staging`; nothing else has been written.
pub fn extract_into(archive: impl Read, staging: &Dir) -> Result<ArchiveSummary, ArchiveError> {
    extract_into_with_limits(archive, staging, &Limits::default())
}

pub fn extract_into_with_limits(
    _archive: impl Read,
    _staging: &Dir,
    _limits: &Limits,
) -> Result<ArchiveSummary, ArchiveError> {
    todo!("#74 archive reader")
}

/// Write the companion directory `source` as an archive rooted at
/// `companion/`, manifest first. Links, special files, and retired layouts are
/// skipped and counted.
pub fn write_archive(_source: &Dir, _output: impl Write) -> Result<ArchiveSummary, ArchiveError> {
    todo!("#74 archive writer")
}

#[cfg(test)]
mod tests {
    use super::*;
    use cap_std::ambient_authority;
    use flate2::{Compression, write::GzEncoder};
    use std::{fs, path::Path};
    use tar::{Builder, EntryType, Header};

    const MIB: u64 = 1024 * 1024;

    /// `root/staging/` is the capability handed to the reader; `root/outside.txt`
    /// is a sentinel that must survive every hostile archive untouched.
    struct Sandbox {
        root: tempfile::TempDir,
        staging: Dir,
    }

    fn sandbox() -> Sandbox {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("staging")).unwrap();
        fs::write(root.path().join("outside.txt"), b"untouched").unwrap();
        let staging =
            Dir::open_ambient_dir(root.path().join("staging"), ambient_authority()).unwrap();
        Sandbox { root, staging }
    }

    impl Sandbox {
        fn extract(&self, archive: &[u8]) -> Result<ArchiveSummary, ArchiveError> {
            extract_into(archive, &self.staging)
        }

        fn extract_with(
            &self,
            archive: &[u8],
            limits: &Limits,
        ) -> Result<ArchiveSummary, ArchiveError> {
            extract_into_with_limits(archive, &self.staging, limits)
        }

        /// Nothing escaped the staging capability.
        fn assert_confined(&self) {
            let mut names: Vec<_> = fs::read_dir(self.root.path())
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            assert_eq!(names, vec!["outside.txt", "staging"]);
            assert_eq!(
                fs::read(self.root.path().join("outside.txt")).unwrap(),
                b"untouched"
            );
        }

        /// Sorted relative paths under staging, directories with a trailing `/`.
        fn staged(&self) -> Vec<String> {
            list_tree(&self.root.path().join("staging"))
        }

        fn assert_staging_empty(&self) {
            self.assert_confined();
            assert_eq!(self.staged(), Vec::<String>::new());
        }
    }

    fn list_tree(root: &Path) -> Vec<String> {
        fn visit(root: &Path, dir: &Path, out: &mut Vec<String>) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                let relative = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                if path.symlink_metadata().unwrap().is_dir() {
                    out.push(format!("{relative}/"));
                    visit(root, &path, out);
                } else {
                    out.push(relative);
                }
            }
        }
        let mut out = Vec::new();
        visit(root, root, &mut out);
        out.sort();
        out
    }

    fn marker_json() -> Vec<u8> {
        serde_json::to_vec_pretty(&CompanionIdentity::canonical()).unwrap()
    }

    fn gzip(tar: Vec<u8>) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
        encoder.write_all(&tar).unwrap();
        encoder.finish().unwrap()
    }

    fn file_header(size: u64) -> Header {
        let mut header = Header::new_gnu();
        header.set_entry_type(EntryType::Regular);
        header.set_mode(0o644);
        header.set_size(size);
        header
    }

    fn add_file(builder: &mut Builder<Vec<u8>>, path: &str, data: &[u8]) {
        let mut header = file_header(data.len() as u64);
        builder.append_data(&mut header, path, data).unwrap();
    }

    fn add_dir(builder: &mut Builder<Vec<u8>>, path: &str) {
        let mut header = Header::new_gnu();
        header.set_entry_type(EntryType::Directory);
        header.set_mode(0o755);
        header.set_size(0);
        builder.append_data(&mut header, path, io::empty()).unwrap();
    }

    /// Bypass `tar::Builder`'s own path checks so fixtures can carry the raw
    /// names a hostile archive would.
    fn add_raw(builder: &mut Builder<Vec<u8>>, name: &[u8], kind: EntryType, data: &[u8]) {
        let mut header = Header::new_gnu();
        header.as_gnu_mut().unwrap().name[..name.len()].copy_from_slice(name);
        header.set_entry_type(kind);
        header.set_mode(0o644);
        header.set_size(data.len() as u64);
        header.set_cksum();
        builder.append(&header, data).unwrap();
    }

    fn add_marker(builder: &mut Builder<Vec<u8>>) {
        add_file(builder, "companion/companion.json", &marker_json());
    }

    fn finish(builder: Builder<Vec<u8>>) -> Vec<u8> {
        gzip(builder.into_inner().unwrap())
    }

    fn archive(build: impl FnOnce(&mut Builder<Vec<u8>>)) -> Vec<u8> {
        let mut builder = Builder::new(Vec::new());
        build(&mut builder);
        finish(builder)
    }

    fn valid_archive() -> Vec<u8> {
        archive(|b| {
            add_dir(b, "companion/");
            add_marker(b);
            add_file(b, "companion/soul.md", b"# soul\n");
            add_dir(b, "companion/memory/");
            add_dir(b, "companion/memory/notes/");
            add_file(b, "companion/memory/notes/tea.md", b"oolong");
            add_dir(b, "companion/memory/empty/");
        })
    }

    fn entry_paths(archive: &[u8]) -> Vec<String> {
        let mut reader = tar::Archive::new(flate2::read::GzDecoder::new(archive));
        reader
            .entries()
            .unwrap()
            .map(|entry| {
                entry
                    .unwrap()
                    .path()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    fn source_dir(root: &Path) -> Dir {
        Dir::open_ambient_dir(root, ambient_authority()).unwrap()
    }

    // -- reader: valid input ------------------------------------------------

    #[test]
    fn extracts_a_valid_archive_with_the_root_stripped() {
        let sandbox = sandbox();
        let summary = sandbox.extract(&valid_archive()).unwrap();

        assert_eq!(
            sandbox.staged(),
            vec![
                "companion.json",
                "memory/",
                "memory/empty/",
                "memory/notes/",
                "memory/notes/tea.md",
                "soul.md",
            ]
        );
        assert_eq!(
            fs::read(sandbox.root.path().join("staging/memory/notes/tea.md")).unwrap(),
            b"oolong"
        );
        assert_eq!(summary.files, 3);
        assert_eq!(summary.directories, 4);
        assert_eq!(summary.bytes, marker_json().len() as u64 + 7 + 6);
        sandbox.assert_confined();
    }

    #[test]
    fn creates_missing_parent_directories_and_accepts_the_manifest_later() {
        let sandbox = sandbox();
        let archive = archive(|b| {
            add_file(b, "companion/memory/deep/nested/note.md", b"n");
            add_marker(b);
        });
        sandbox.extract(&archive).unwrap();
        assert_eq!(
            sandbox.staged(),
            vec![
                "companion.json",
                "memory/",
                "memory/deep/",
                "memory/deep/nested/",
                "memory/deep/nested/note.md",
            ]
        );
    }

    #[test]
    fn accepts_gnu_long_names_and_pax_paths_within_bounds() {
        let long_name = format!("companion/memory/{}.md", "n".repeat(180));
        let sandbox = sandbox();
        let archive = archive(|b| {
            add_marker(b);
            // tar::Builder emits a GNU long-name entry for paths over 100 bytes.
            add_file(b, &long_name, b"long");
            // A pax extended header carrying the path.
            let pax = b"30 path=companion/memory/pax.md\n";
            let mut header = Header::new_ustar();
            header.set_entry_type(EntryType::XHeader);
            header.set_size(pax.len() as u64);
            header.set_cksum();
            b.append(&header, pax.as_slice()).unwrap();
            add_raw(b, b"companion/memory/short.md", EntryType::Regular, b"pax");
        });
        sandbox.extract(&archive).unwrap();
        assert_eq!(
            sandbox.staged(),
            vec![
                "companion.json".to_owned(),
                "memory/".to_owned(),
                format!("memory/{}.md", "n".repeat(180)),
                "memory/pax.md".to_owned(),
            ]
        );
    }

    // -- reader: hostile paths ----------------------------------------------

    #[test]
    fn rejects_absolute_paths() {
        let sandbox = sandbox();
        let escaped = sandbox.root.path().join("escaped.txt");
        let target = escaped.to_string_lossy().into_owned();
        let archive = archive(|b| {
            add_raw(b, target.as_bytes(), EntryType::Regular, b"x");
            add_marker(b);
        });
        let error = sandbox.extract(&archive).unwrap_err();
        assert!(matches!(error, ArchiveError::InvalidPath(_)), "{error}");
        assert!(!escaped.exists());
        sandbox.assert_staging_empty();
    }

    #[test]
    fn rejects_parent_directory_segments() {
        for name in [
            "companion/../escaped.txt",
            "../staging/escaped.txt",
            "companion/memory/../../escaped.txt",
        ] {
            let sandbox = sandbox();
            let archive = archive(|b| {
                add_raw(b, name.as_bytes(), EntryType::Regular, b"x");
                add_marker(b);
            });
            let error = sandbox.extract(&archive).unwrap_err();
            assert!(
                matches!(error, ArchiveError::InvalidPath(_)),
                "{name}: {error}"
            );
            assert!(!sandbox.root.path().join("escaped.txt").exists());
            sandbox.assert_staging_empty();
        }
    }

    #[test]
    fn rejects_dot_segments_backslashes_empty_segments_and_control_bytes() {
        for name in [
            b"./companion/soul.md".as_slice(),
            b"companion/./soul.md",
            b"companion\\soul.md",
            b"companion//soul.md",
            b"companion/soul.md/",
            b"companion/so\0ul.md",
            b"companion/\xff.md",
            b"C:companion/soul.md",
        ] {
            let sandbox = sandbox();
            let archive = archive(|b| {
                add_raw(b, name, EntryType::Regular, b"x");
                add_marker(b);
            });
            let error = sandbox.extract(&archive).unwrap_err();
            assert!(
                matches!(error, ArchiveError::InvalidPath(_)),
                "{}: {error}",
                String::from_utf8_lossy(name)
            );
            sandbox.assert_staging_empty();
        }
    }

    #[test]
    fn rejects_over_long_names_before_reading_them() {
        let sandbox = sandbox();
        let archive = archive(|b| {
            add_marker(b);
            let long_name = format!("companion/memory/{}.md", "n".repeat(5000));
            add_file(b, &long_name, b"x");
        });
        let error = sandbox.extract(&archive).unwrap_err();
        assert!(matches!(error, ArchiveError::InvalidPath(_)), "{error}");
        sandbox.assert_confined();
    }

    // -- reader: hostile entry types ----------------------------------------

    #[test]
    fn rejects_symlinks() {
        let sandbox = sandbox();
        let archive = archive(|b| {
            let mut header = Header::new_gnu();
            header.set_entry_type(EntryType::Symlink);
            header.set_size(0);
            b.append_link(&mut header, "companion/soul.md", "/etc/passwd")
                .unwrap();
            add_marker(b);
        });
        let error = sandbox.extract(&archive).unwrap_err();
        assert!(
            matches!(&error, ArchiveError::UnsupportedEntry { kind, .. } if kind == "symlink"),
            "{error}"
        );
        sandbox.assert_staging_empty();
    }

    #[test]
    fn rejects_hard_links() {
        let sandbox = sandbox();
        let archive = archive(|b| {
            add_marker(b);
            let mut header = Header::new_gnu();
            header.set_entry_type(EntryType::Link);
            header.set_size(0);
            b.append_link(&mut header, "companion/soul.md", "companion/companion.json")
                .unwrap();
        });
        let error = sandbox.extract(&archive).unwrap_err();
        assert!(
            matches!(&error, ArchiveError::UnsupportedEntry { kind, .. } if kind == "hard link"),
            "{error}"
        );
        assert_eq!(sandbox.staged(), vec!["companion.json"]);
        sandbox.assert_confined();
    }

    #[test]
    fn rejects_devices_fifos_and_sparse_entries() {
        for (kind, name) in [
            (EntryType::Char, "character device"),
            (EntryType::Block, "block device"),
            (EntryType::Fifo, "fifo"),
            (EntryType::GNUSparse, "sparse file"),
            (EntryType::Continuous, "contiguous file"),
        ] {
            let sandbox = sandbox();
            let archive = archive(|b| {
                let mut header = Header::new_gnu();
                header.set_entry_type(kind);
                header.set_size(0);
                header.set_device_major(1).unwrap();
                header.set_device_minor(3).unwrap();
                b.append_data(&mut header, "companion/dev", io::empty())
                    .unwrap();
                add_marker(b);
            });
            let error = sandbox.extract(&archive).unwrap_err();
            assert!(
                matches!(&error, ArchiveError::UnsupportedEntry { kind, .. } if kind == name),
                "{name}: {error}"
            );
            sandbox.assert_staging_empty();
        }
    }

    // -- reader: duplicates and conflicts -----------------------------------

    #[test]
    fn rejects_duplicate_and_conflicting_entries() {
        let cases: Vec<(&str, Box<dyn Fn(&mut Builder<Vec<u8>>)>)> = vec![
            (
                "same file twice",
                Box::new(|b| {
                    add_file(b, "companion/soul.md", b"one");
                    add_file(b, "companion/soul.md", b"two");
                }),
            ),
            (
                "file then directory",
                Box::new(|b| {
                    add_file(b, "companion/memory", b"file");
                    add_dir(b, "companion/memory/");
                }),
            ),
            (
                "directory then file",
                Box::new(|b| {
                    add_dir(b, "companion/memory/");
                    add_file(b, "companion/memory", b"file");
                }),
            ),
            (
                "file used as a parent",
                Box::new(|b| {
                    add_file(b, "companion/memory", b"file");
                    add_file(b, "companion/memory/note.md", b"child");
                }),
            ),
            (
                "same directory twice",
                Box::new(|b| {
                    add_dir(b, "companion/memory/");
                    add_dir(b, "companion/memory/");
                }),
            ),
            (
                "manifest twice",
                Box::new(|b| {
                    add_marker(b);
                    add_marker(b);
                }),
            ),
        ];
        for (name, build) in cases {
            let sandbox = sandbox();
            let archive = archive(|b| {
                build(b);
                add_marker(b);
            });
            let error = sandbox.extract(&archive).unwrap_err();
            assert!(
                matches!(error, ArchiveError::Conflict(_)),
                "{name}: {error}"
            );
            sandbox.assert_confined();
        }
    }

    #[test]
    fn rejects_a_regular_file_named_like_the_root() {
        let sandbox = sandbox();
        let archive = archive(|b| add_raw(b, b"companion", EntryType::Regular, b"x"));
        let error = sandbox.extract(&archive).unwrap_err();
        assert!(matches!(error, ArchiveError::WrongRoot(_)), "{error}");
        sandbox.assert_staging_empty();
    }

    // -- reader: limits -----------------------------------------------------

    #[test]
    fn rejects_an_oversized_file_from_its_header_before_reading_data() {
        let sandbox = sandbox();
        let declared = Limits::default().max_file_bytes + 1;
        let archive = archive(|b| {
            add_marker(b);
            // The header claims more than the cap; no payload follows.
            let mut header = file_header(declared);
            header.set_path("companion/uploads/huge.bin").unwrap();
            header.set_cksum();
            b.append(&header, io::empty()).unwrap();
        });
        let error = sandbox.extract(&archive).unwrap_err();
        assert!(
            matches!(error, ArchiveError::FileTooLarge { size, .. } if size == declared),
            "{error}"
        );
        assert_eq!(sandbox.staged(), vec!["companion.json"]);
        sandbox.assert_confined();
    }

    #[test]
    fn rejects_an_oversized_file_with_a_real_payload() {
        let sandbox = sandbox();
        let limits = Limits {
            max_file_bytes: 1024,
            ..Limits::default()
        };
        let archive = archive(|b| {
            add_marker(b);
            add_file(b, "companion/uploads/big.bin", &vec![7_u8; 2048]);
        });
        let error = sandbox.extract_with(&archive, &limits).unwrap_err();
        assert!(
            matches!(error, ArchiveError::FileTooLarge { .. }),
            "{error}"
        );
        assert_eq!(sandbox.staged(), vec!["companion.json"]);
    }

    #[test]
    fn rejects_total_payload_over_the_limit() {
        let sandbox = sandbox();
        let limits = Limits {
            max_file_bytes: MIB,
            max_total_bytes: MIB + MIB / 2,
            ..Limits::default()
        };
        let archive = archive(|b| {
            add_marker(b);
            add_file(b, "companion/uploads/a.bin", &vec![1_u8; MIB as usize]);
            add_file(b, "companion/uploads/b.bin", &vec![2_u8; MIB as usize]);
        });
        let error = sandbox.extract_with(&archive, &limits).unwrap_err();
        assert!(
            matches!(error, ArchiveError::TotalTooLarge { .. }),
            "{error}"
        );
        assert!(!sandbox.root.path().join("staging/uploads/b.bin").exists());
        sandbox.assert_confined();
    }

    #[test]
    fn rejects_too_many_entries() {
        let sandbox = sandbox();
        let limits = Limits {
            max_entries: 8,
            ..Limits::default()
        };
        let archive = archive(|b| {
            add_marker(b);
            for index in 0..8 {
                add_file(b, &format!("companion/memory/{index}.md"), b"x");
            }
        });
        let error = sandbox.extract_with(&archive, &limits).unwrap_err();
        assert!(
            matches!(error, ArchiveError::TooManyEntries { limit: 8 }),
            "{error}"
        );
        sandbox.assert_confined();
        assert_eq!(Limits::default().max_entries, 100_000);
    }

    #[test]
    fn rejects_a_decompression_bomb_while_streaming() {
        let sandbox = sandbox();
        // Declared sizes fit the per-file and total caps; only the stream cap
        // can catch this, and it must do so before the payload is on disk.
        let limits = Limits {
            max_file_bytes: 64 * MIB,
            max_total_bytes: 64 * MIB,
            max_decompressed_bytes: MIB,
            ..Limits::default()
        };
        let archive = archive(|b| {
            add_marker(b);
            add_file(
                b,
                "companion/uploads/zeros.bin",
                &vec![0_u8; 8 * MIB as usize],
            );
        });
        assert!(
            archive.len() < 64 * 1024,
            "fixture must be a real bomb, got {} bytes",
            archive.len()
        );
        let error = sandbox.extract_with(&archive, &limits).unwrap_err();
        assert!(
            matches!(error, ArchiveError::DecompressedTooLarge { .. }),
            "{error}"
        );
        let staged = sandbox.root.path().join("staging/uploads/zeros.bin");
        if staged.exists() {
            assert!(fs::metadata(&staged).unwrap().len() <= 2 * MIB);
        }
        sandbox.assert_confined();
    }

    // -- reader: manifest ---------------------------------------------------

    #[test]
    fn rejects_wrong_root() {
        let sandbox = sandbox();
        let archive = archive(|b| {
            add_dir(b, "alice/");
            add_file(b, "alice/companion.json", &marker_json());
            add_file(b, "alice/soul.md", b"x");
        });
        let error = sandbox.extract(&archive).unwrap_err();
        assert!(matches!(error, ArchiveError::WrongRoot(_)), "{error}");
        sandbox.assert_staging_empty();
    }

    #[test]
    fn rejects_wrong_slug_and_wrong_format_version_and_unknown_fields() {
        for (name, marker) in [
            (
                "slug",
                serde_json::json!({"format_version": 1, "slug": "alice"}),
            ),
            (
                "version",
                serde_json::json!({"format_version": 2, "slug": "companion"}),
            ),
            (
                "unknown field",
                serde_json::json!({"format_version": 1, "slug": "companion", "instances": ["alice"]}),
            ),
            ("not json", serde_json::json!("companion")),
        ] {
            let sandbox = sandbox();
            let marker = serde_json::to_vec(&marker).unwrap();
            let archive = archive(|b| {
                add_file(b, "companion/companion.json", &marker);
                add_file(b, "companion/soul.md", b"x");
            });
            let error = sandbox.extract(&archive).unwrap_err();
            assert!(
                matches!(error, ArchiveError::InvalidIdentity(_)),
                "{name}: {error}"
            );
            assert!(!sandbox.root.path().join("staging/soul.md").exists());
            sandbox.assert_confined();
        }
    }

    #[test]
    fn rejects_a_missing_manifest() {
        let sandbox = sandbox();
        let archive = archive(|b| {
            add_file(b, "companion/soul.md", b"x");
        });
        let error = sandbox.extract(&archive).unwrap_err();
        assert!(matches!(error, ArchiveError::MissingIdentity), "{error}");
        sandbox.assert_confined();
    }

    #[test]
    fn rejects_an_oversized_manifest() {
        let sandbox = sandbox();
        let archive = archive(|b| {
            add_file(b, "companion/companion.json", &vec![b' '; 8192]);
        });
        let error = sandbox.extract(&archive).unwrap_err();
        assert!(matches!(error, ArchiveError::InvalidIdentity(_)), "{error}");
        sandbox.assert_staging_empty();
    }

    #[test]
    fn rejects_retired_layouts() {
        for retired in ["stats", "agents", "agent_runs", "thoughts"] {
            let sandbox = sandbox();
            let archive = archive(|b| {
                add_marker(b);
                add_file(b, &format!("companion/{retired}/x.json"), b"{}");
            });
            let error = sandbox.extract(&archive).unwrap_err();
            assert!(
                matches!(error, ArchiveError::RetiredLayout(_)),
                "{retired}: {error}"
            );
            assert_eq!(sandbox.staged(), vec!["companion.json"]);
            sandbox.assert_confined();

            let dir_sandbox = self::sandbox();
            let dir_archive = self::archive(|b| {
                add_dir(b, &format!("companion/{retired}/"));
                add_marker(b);
            });
            let error = dir_sandbox.extract(&dir_archive).unwrap_err();
            assert!(
                matches!(error, ArchiveError::RetiredLayout(_)),
                "{retired}/: {error}"
            );
            dir_sandbox.assert_staging_empty();
        }
    }

    #[test]
    fn rejects_non_gzip_and_truncated_input() {
        let sandbox = sandbox();
        let error = sandbox.extract(b"not an archive").unwrap_err();
        assert!(
            matches!(error, ArchiveError::Malformed(_) | ArchiveError::Io(_)),
            "{error}"
        );
        sandbox.assert_staging_empty();

        let truncated_sandbox = self::sandbox();
        let whole = valid_archive();
        let truncated = &whole[..whole.len() / 2];
        let error = truncated_sandbox.extract(truncated).unwrap_err();
        assert!(
            matches!(error, ArchiveError::Malformed(_) | ArchiveError::Io(_)),
            "{error}"
        );
        truncated_sandbox.assert_confined();
    }

    #[test]
    fn rejects_pax_size_overrides() {
        let sandbox = sandbox();
        let archive = archive(|b| {
            add_marker(b);
            let pax = b"14 size=99999\n";
            let mut header = Header::new_ustar();
            header.set_entry_type(EntryType::XHeader);
            header.set_size(pax.len() as u64);
            header.set_cksum();
            b.append(&header, pax.as_slice()).unwrap();
            add_file(b, "companion/soul.md", b"x");
        });
        let error = sandbox.extract(&archive).unwrap_err();
        assert!(
            matches!(error, ArchiveError::UnsupportedEntry { .. }),
            "{error}"
        );
        sandbox.assert_confined();
    }

    // -- writer -------------------------------------------------------------

    fn populate_source(root: &Path) {
        fs::write(root.join("companion.json"), marker_json()).unwrap();
        fs::write(root.join("soul.md"), b"# soul\n").unwrap();
        fs::create_dir_all(root.join("memory/notes")).unwrap();
        fs::create_dir_all(root.join("memory/empty")).unwrap();
        fs::write(root.join("memory/notes/tea.md"), b"oolong").unwrap();
        fs::write(
            root.join(format!("memory/notes/{}.md", "long".repeat(40))),
            b"long name",
        )
        .unwrap();
        fs::create_dir_all(root.join("uploads")).unwrap();
        fs::write(
            root.join("uploads/blob.bin"),
            (0..=255_u8).collect::<Vec<_>>(),
        )
        .unwrap();
    }

    #[test]
    fn round_trip_is_byte_identical_and_manifest_comes_first() {
        let source = tempfile::tempdir().unwrap();
        populate_source(source.path());

        let mut bytes = Vec::new();
        let written = write_archive(&source_dir(source.path()), &mut bytes).unwrap();
        assert_eq!(written.files, 5);
        assert_eq!(written.skipped, 0);

        let paths = entry_paths(&bytes);
        assert_eq!(paths[0], "companion/companion.json");
        assert!(
            paths.iter().all(|path| path.starts_with("companion/")),
            "{paths:?}"
        );
        assert!(paths.contains(&"companion/memory/empty/".to_owned()));
        let sorted = {
            let mut sorted = paths[1..].to_vec();
            sorted.sort();
            sorted
        };
        assert_eq!(
            paths[1..],
            sorted[..],
            "entries are emitted in sorted order"
        );

        let sandbox = sandbox();
        let read = sandbox.extract(&bytes).unwrap();
        assert_eq!(read.files, written.files);
        assert_eq!(read.bytes, written.bytes);
        let staged = sandbox.root.path().join("staging");
        assert_eq!(list_tree(&staged), list_tree(source.path()));
        for relative in list_tree(source.path()) {
            if relative.ends_with('/') {
                continue;
            }
            assert_eq!(
                fs::read(staged.join(&relative)).unwrap(),
                fs::read(source.path().join(&relative)).unwrap(),
                "{relative}"
            );
        }
        sandbox.assert_confined();
    }

    #[test]
    fn writer_is_deterministic() {
        let source = tempfile::tempdir().unwrap();
        populate_source(source.path());
        let dir = source_dir(source.path());
        let mut first = Vec::new();
        let mut second = Vec::new();
        write_archive(&dir, &mut first).unwrap();
        write_archive(&dir, &mut second).unwrap();
        assert_eq!(first, second);
    }

    #[cfg(unix)]
    #[test]
    fn writer_skips_links_special_files_and_retired_layouts() {
        use std::os::unix::fs::symlink;

        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret"), b"secret").unwrap();
        let source = tempfile::tempdir().unwrap();
        populate_source(source.path());
        symlink(outside.path().join("secret"), source.path().join("leak.md")).unwrap();
        symlink(outside.path(), source.path().join("memory/leak")).unwrap();
        fs::hard_link(source.path().join("soul.md"), source.path().join("hard.md")).unwrap();
        fs::create_dir_all(source.path().join("stats")).unwrap();
        fs::write(source.path().join("stats/2024-01-01.json"), b"{}").unwrap();
        fs::create_dir_all(source.path().join("thoughts")).unwrap();

        let mut bytes = Vec::new();
        let summary = write_archive(&source_dir(source.path()), &mut bytes).unwrap();
        let paths = entry_paths(&bytes);
        assert!(!paths.iter().any(|p| p.contains("leak")), "{paths:?}");
        assert!(!paths.iter().any(|p| p.contains("secret")), "{paths:?}");
        assert!(!paths.iter().any(|p| p.contains("stats")), "{paths:?}");
        assert!(!paths.iter().any(|p| p.contains("thoughts")), "{paths:?}");
        // A hard link is a regular file on disk and is exported as one.
        assert!(paths.contains(&"companion/hard.md".to_owned()));
        assert_eq!(summary.skipped, 4);

        let sandbox = sandbox();
        sandbox.extract(&bytes).unwrap();
        sandbox.assert_confined();
    }

    #[test]
    fn writer_refuses_a_source_without_a_valid_manifest() {
        let source = tempfile::tempdir().unwrap();
        fs::write(source.path().join("soul.md"), b"x").unwrap();
        let error = write_archive(&source_dir(source.path()), Vec::new()).unwrap_err();
        assert!(matches!(error, ArchiveError::MissingIdentity), "{error}");

        fs::write(
            source.path().join("companion.json"),
            br#"{"format_version": 2, "slug": "companion"}"#,
        )
        .unwrap();
        let error = write_archive(&source_dir(source.path()), Vec::new()).unwrap_err();
        assert!(matches!(error, ArchiveError::InvalidIdentity(_)), "{error}");
    }

    #[test]
    fn writer_output_lists_with_the_system_tar() {
        let source = tempfile::tempdir().unwrap();
        populate_source(source.path());
        let mut bytes = Vec::new();
        write_archive(&source_dir(source.path()), &mut bytes).unwrap();
        let path = source.path().join("out.tar.gz");
        fs::write(&path, &bytes).unwrap();
        let listing = std::process::Command::new("tar")
            .arg("-tzf")
            .arg(&path)
            .output()
            .unwrap();
        assert!(listing.status.success());
        let listing = String::from_utf8_lossy(&listing.stdout);
        assert!(
            listing
                .lines()
                .next()
                .unwrap()
                .ends_with("companion/companion.json")
        );
        assert!(listing.contains(&format!("companion/memory/notes/{}.md", "long".repeat(40))));
    }

    #[test]
    fn constants_match_the_storage_format() {
        assert_eq!(ARCHIVE_ROOT, "companion");
        assert_eq!(ARCHIVE_FORMAT_VERSION, 1);
        assert_eq!(ARCHIVE_FILE_NAME, "companion.tar.gz");
        let limits = Limits::default();
        assert!(limits.max_file_bytes >= crate::services::uploads::MAX_FILE_SIZE);
        assert!(limits.max_total_bytes > limits.max_file_bytes);
        assert!(limits.max_decompressed_bytes > limits.max_total_bytes);
    }
}
