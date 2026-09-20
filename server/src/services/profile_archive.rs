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

use std::{
    collections::BTreeMap,
    io::{self, BufWriter, Read, Write},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsMaybeDirExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use tar::{EntryType, Header, PaxExtensions};

use crate::domain::companion::{
    CANONICAL_SLUG, CompanionIdentity, IDENTITY_FILE, STORAGE_FORMAT_VERSION,
};

/// Directory every archive entry is rooted at.
pub const ARCHIVE_ROOT: &str = CANONICAL_SLUG;

/// Format version a reader accepts and the writer produces. Shares the on-disk
/// storage version because the archive is the companion directory verbatim.
pub const ARCHIVE_FORMAT_VERSION: u32 = STORAGE_FORMAT_VERSION;

/// File name offered for downloads.
pub const ARCHIVE_FILE_NAME: &str = "companion.tar.gz";

/// Media type of the archive stream.
pub const ARCHIVE_CONTENT_TYPE: &str = "application/gzip";

/// Directories the current layout removes at startup and never reads (see
/// the legacy cleanup in `services::media_text`). An archive that still
/// carries them predates the format and is refused rather than trimmed.
const RETIRED_LAYOUT_DIRS: [&str; 4] = ["stats", "agents", "agent_runs", "thoughts"];

/// Upper bound for the identity marker, matching `services::companion`.
const MAX_IDENTITY_BYTES: u64 = 4096;

/// Longest entry path accepted, GNU long-name payloads included.
const MAX_PATH_BYTES: u64 = 4096;

/// Largest pax extended header read; larger ones are refused unread.
const MAX_PAX_BYTES: u64 = 64 * 1024;

/// Buffer in front of the gzip encoder so streaming sinks see sizeable chunks.
const WRITE_BUFFER_BYTES: usize = 64 * 1024;

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
#[allow(dead_code)] // The import route and restore tool adopt the reader in #74's next slice.
pub fn extract_into(archive: impl Read, staging: &Dir) -> Result<ArchiveSummary, ArchiveError> {
    extract_into_with_limits(archive, staging, &Limits::default())
}

pub fn extract_into_with_limits(
    archive: impl Read,
    staging: &Dir,
    limits: &Limits,
) -> Result<ArchiveSummary, ArchiveError> {
    let mut archive = tar::Archive::new(Counted::new(
        GzDecoder::new(archive),
        limits.max_decompressed_bytes,
    ));
    let result = Extractor {
        staging,
        limits,
        seen: BTreeMap::new(),
        summary: ArchiveSummary::default(),
        identity: None,
        pending_path: None,
        entry_count: 0,
    }
    .run(&mut archive);
    if archive.into_inner().tripped {
        return Err(ArchiveError::DecompressedTooLarge {
            limit: limits.max_decompressed_bytes,
        });
    }
    result
}

/// Open the companion directory as the capability [`write_archive`] reads
/// from. This is the one place export turns an ambient path into authority,
/// with the same real-directory check as `MediaStore::open`; the import work
/// in #74 replaces it with a capability issued by the store itself.
pub fn open_companion_dir(companion_dir: &Path) -> io::Result<Dir> {
    let metadata = std::fs::symlink_metadata(companion_dir)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "companion directory must be a real directory",
        ));
    }
    Dir::open_ambient_dir(companion_dir, ambient_authority())
}

/// Write the companion directory `source` as an archive rooted at
/// `companion/`, manifest first. Links, special files, and retired layouts are
/// skipped and counted.
pub fn write_archive(source: &Dir, output: impl Write) -> Result<ArchiveSummary, ArchiveError> {
    let (marker, marker_mtime) = read_marker(source)?;
    let encoder = GzEncoder::new(
        BufWriter::with_capacity(WRITE_BUFFER_BYTES, output),
        Compression::default(),
    );
    let mut writer = Writer {
        builder: tar::Builder::new(encoder),
        summary: ArchiveSummary::default(),
    };
    writer.append_file(
        Path::new(IDENTITY_FILE),
        marker.len() as u64,
        marker_mtime,
        marker.as_slice(),
    )?;
    writer.walk(source, &PathBuf::new())?;
    let encoder = writer.builder.into_inner()?;
    let mut output = encoder
        .finish()?
        .into_inner()
        .map_err(|error| error.into_error())?;
    output.flush()?;
    Ok(writer.summary)
}

// ---------------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------------

/// Counts bytes leaving the gzip decoder and fails once past the cap, so a
/// bomb is cut off mid-stream no matter which tar entry it hides in.
struct Counted<R> {
    inner: R,
    consumed: u64,
    limit: u64,
    tripped: bool,
}

impl<R> Counted<R> {
    fn new(inner: R, limit: u64) -> Self {
        Self {
            inner,
            consumed: 0,
            limit,
            tripped: false,
        }
    }
}

impl<R: Read> Read for Counted<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.tripped {
            return Err(decompressed_limit_error());
        }
        let read = self.inner.read(buf)?;
        self.consumed = self.consumed.saturating_add(read as u64);
        if self.consumed > self.limit {
            self.tripped = true;
            return Err(decompressed_limit_error());
        }
        Ok(read)
    }
}

fn decompressed_limit_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "archive decompressed size limit exceeded",
    )
}

/// How a staging path has been claimed so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Claim {
    File,
    Directory,
    /// Created because a deeper entry needed it; a later explicit directory
    /// entry for it is fine, a file is not.
    ImplicitDirectory,
}

struct Extractor<'a> {
    staging: &'a Dir,
    limits: &'a Limits,
    /// Every path claimed so far, relative to staging. The root is `""`.
    seen: BTreeMap<PathBuf, Claim>,
    summary: ArchiveSummary,
    identity: Option<CompanionIdentity>,
    /// Path carried by a preceding GNU long-name or pax entry.
    pending_path: Option<Vec<u8>>,
    /// Raw entries seen so far, extension entries included.
    entry_count: usize,
}

impl Extractor<'_> {
    fn run<R: Read>(
        mut self,
        archive: &mut tar::Archive<R>,
    ) -> Result<ArchiveSummary, ArchiveError> {
        // Raw entries: the crate's own long-name and pax handling reads those
        // payloads into memory without a bound, so this reader does it itself.
        let entries = archive.entries().map_err(stream_error)?.raw(true);
        for entry in entries {
            let mut entry = entry.map_err(stream_error)?;
            self.entry_count += 1;
            if self.entry_count > self.limits.max_entries {
                return Err(ArchiveError::TooManyEntries {
                    limit: self.limits.max_entries,
                });
            }
            let kind = entry.header().entry_type();
            match kind {
                EntryType::GNULongName => {
                    let Some(mut name) = read_bounded(&mut entry, MAX_PATH_BYTES)? else {
                        return Err(ArchiveError::InvalidPath(format!(
                            "long name of {} bytes",
                            entry.size()
                        )));
                    };
                    while name.last() == Some(&0) {
                        name.pop();
                    }
                    self.pending_path = Some(name);
                }
                EntryType::XHeader => self.take_pax_path(&mut entry)?,
                EntryType::XGlobalHeader => {
                    // Carries nothing this format uses; the iterator skips its
                    // payload, which still counts against the stream cap.
                    if entry.size() > MAX_PAX_BYTES {
                        return Err(ArchiveError::Malformed(
                            "global pax header is too large".into(),
                        ));
                    }
                }
                EntryType::Regular | EntryType::Directory => {
                    self.extract_entry(&mut entry, kind)?
                }
                other => {
                    return Err(ArchiveError::UnsupportedEntry {
                        path: self.entry_name(&entry),
                        kind: describe_entry_type(other).to_owned(),
                    });
                }
            }
        }
        if self.identity.is_none() {
            return Err(ArchiveError::MissingIdentity);
        }
        self.sync_directories()?;
        Ok(self.summary)
    }

    fn entry_name<R: Read>(&self, entry: &tar::Entry<'_, R>) -> String {
        match &self.pending_path {
            Some(pending) => String::from_utf8_lossy(pending).into_owned(),
            None => String::from_utf8_lossy(&entry.path_bytes()).into_owned(),
        }
    }

    fn take_pax_path<R: Read>(
        &mut self,
        entry: &mut tar::Entry<'_, R>,
    ) -> Result<(), ArchiveError> {
        let Some(bytes) = read_bounded(entry, MAX_PAX_BYTES)? else {
            return Err(ArchiveError::Malformed("pax header is too large".into()));
        };
        for record in PaxExtensions::new(&bytes) {
            let record =
                record.map_err(|error| ArchiveError::Malformed(format!("pax record: {error}")))?;
            match record.key_bytes() {
                b"path" => self.pending_path = Some(record.value_bytes().to_vec()),
                // A size record would make the payload longer than the header
                // says; nothing this format writes needs it.
                b"size" => {
                    return Err(ArchiveError::UnsupportedEntry {
                        path: self.entry_name(entry),
                        kind: "pax size override".to_owned(),
                    });
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn extract_entry<R: Read>(
        &mut self,
        entry: &mut tar::Entry<'_, R>,
        kind: EntryType,
    ) -> Result<(), ArchiveError> {
        let raw = match self.pending_path.take() {
            Some(pending) => pending,
            None => entry.path_bytes().into_owned(),
        };
        let is_dir = kind == EntryType::Directory;
        let (display, relative) = parse_entry_path(&raw, is_dir)?;

        if relative.as_os_str().is_empty() {
            if !is_dir {
                return Err(ArchiveError::WrongRoot(display));
            }
            if self.seen.insert(PathBuf::new(), Claim::Directory).is_some() {
                return Err(ArchiveError::Conflict(display));
            }
            self.summary.directories += 1;
            return Ok(());
        }
        if relative
            .components()
            .next()
            .and_then(|component| component.as_os_str().to_str())
            .is_some_and(|first| RETIRED_LAYOUT_DIRS.contains(&first))
        {
            return Err(ArchiveError::RetiredLayout(display));
        }

        if is_dir {
            return self.extract_directory(relative, &display);
        }

        let size = entry.size();
        if size > self.limits.max_file_bytes {
            return Err(ArchiveError::FileTooLarge {
                path: display,
                size,
                limit: self.limits.max_file_bytes,
            });
        }
        let total = self.summary.bytes.saturating_add(size);
        if total > self.limits.max_total_bytes {
            return Err(ArchiveError::TotalTooLarge {
                limit: self.limits.max_total_bytes,
            });
        }
        if self.seen.contains_key(&relative) {
            return Err(ArchiveError::Conflict(display));
        }

        // Validate the manifest before anything of it reaches staging.
        let marker = if relative == Path::new(IDENTITY_FILE) {
            let bytes = read_bounded(entry, MAX_IDENTITY_BYTES)?.ok_or_else(|| {
                ArchiveError::InvalidIdentity(format!(
                    "marker is {size} bytes, more than {MAX_IDENTITY_BYTES}"
                ))
            })?;
            let identity = parse_identity(&bytes)?;
            Some((bytes, identity))
        } else {
            None
        };

        self.ensure_parents(&relative, &display)?;
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        let mut file = self
            .staging
            .open_with(&relative, &options)
            .map_err(|error| create_error(error, &display))?;
        self.seen.insert(relative, Claim::File);
        match marker {
            Some((bytes, identity)) => {
                file.write_all(&bytes)?;
                self.identity = Some(identity);
            }
            None => {
                let copied = io::copy(entry, &mut file).map_err(stream_error)?;
                if copied != size {
                    return Err(ArchiveError::Malformed(format!(
                        "entry {display:?} is truncated"
                    )));
                }
            }
        }
        file.flush()?;
        file.sync_all()?;
        self.summary.bytes = total;
        self.summary.files += 1;
        Ok(())
    }

    fn extract_directory(&mut self, relative: PathBuf, display: &str) -> Result<(), ArchiveError> {
        match self.seen.get(&relative) {
            Some(Claim::File | Claim::Directory) => {
                return Err(ArchiveError::Conflict(display.to_owned()));
            }
            Some(Claim::ImplicitDirectory) => {}
            None => {
                self.ensure_parents(&relative, display)?;
                self.staging
                    .create_dir(&relative)
                    .map_err(|error| create_error(error, display))?;
            }
        }
        self.seen.insert(relative, Claim::Directory);
        self.summary.directories += 1;
        Ok(())
    }

    /// Create every missing ancestor of `relative`, refusing to pass through a
    /// path already claimed by a file.
    fn ensure_parents(&mut self, relative: &Path, display: &str) -> Result<(), ArchiveError> {
        let Some(parent) = relative.parent() else {
            return Ok(());
        };
        let mut current = PathBuf::new();
        for component in parent.components() {
            current.push(component);
            match self.seen.get(&current) {
                Some(Claim::File) => return Err(ArchiveError::Conflict(display.to_owned())),
                Some(_) => continue,
                None => {
                    self.staging
                        .create_dir(&current)
                        .map_err(|error| create_error(error, display))?;
                    self.seen.insert(current.clone(), Claim::ImplicitDirectory);
                }
            }
        }
        Ok(())
    }

    /// Persist directory entries the same way each file was persisted.
    fn sync_directories(&self) -> io::Result<()> {
        let mut options = OpenOptions::new();
        options
            .read(true)
            .maybe_dir(true)
            .follow(FollowSymlinks::No);
        for (path, claim) in &self.seen {
            if *claim == Claim::File || path.as_os_str().is_empty() {
                continue;
            }
            self.staging.open_with(path, &options)?.sync_all()?;
        }
        self.staging.open(".")?.sync_all()
    }
}

/// Read an entry whose declared size must fit `limit`; `None` when it does not.
fn read_bounded<R: Read>(
    entry: &mut tar::Entry<'_, R>,
    limit: u64,
) -> Result<Option<Vec<u8>>, ArchiveError> {
    let size = entry.size();
    if size > limit {
        return Ok(None);
    }
    let mut bytes = Vec::with_capacity(size as usize);
    entry.read_to_end(&mut bytes).map_err(stream_error)?;
    if bytes.len() as u64 != size {
        return Err(ArchiveError::Malformed("entry is truncated".into()));
    }
    Ok(Some(bytes))
}

fn parse_identity(bytes: &[u8]) -> Result<CompanionIdentity, ArchiveError> {
    let identity: CompanionIdentity = serde_json::from_slice(bytes)
        .map_err(|error| ArchiveError::InvalidIdentity(error.to_string()))?;
    if identity.format_version != ARCHIVE_FORMAT_VERSION {
        return Err(ArchiveError::InvalidIdentity(format!(
            "archive format {} is not supported (expected {ARCHIVE_FORMAT_VERSION})",
            identity.format_version
        )));
    }
    identity
        .validate()
        .map_err(|error| ArchiveError::InvalidIdentity(error.to_string()))?;
    Ok(identity)
}

/// Split an entry path into its lossy display form and the path relative to
/// the archive root, applying the same rules as memory paths: relative,
/// UTF-8, forward slashes only, no empty, `.`, or `..` segments. Only
/// directory entries may end with `/`.
fn parse_entry_path(raw: &[u8], is_dir: bool) -> Result<(String, PathBuf), ArchiveError> {
    let display = String::from_utf8_lossy(raw).into_owned();
    let invalid = || ArchiveError::InvalidPath(display.clone());
    if raw.len() as u64 > MAX_PATH_BYTES {
        return Err(invalid());
    }
    let text = std::str::from_utf8(raw).map_err(|_| invalid())?;
    let has_windows_prefix = raw.len() >= 2 && raw[0].is_ascii_alphabetic() && raw[1] == b':';
    if text.is_empty()
        || text.contains('\0')
        || text.contains('\\')
        || text.starts_with('/')
        || has_windows_prefix
    {
        return Err(invalid());
    }
    let trimmed = if is_dir {
        text.strip_suffix('/').unwrap_or(text)
    } else {
        text
    };
    let mut segments = trimmed.split('/');
    let root = segments.next().unwrap_or_default();
    let rest: Vec<&str> = segments.collect();
    if std::iter::once(root)
        .chain(rest.iter().copied())
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(invalid());
    }
    if root != ARCHIVE_ROOT {
        return Err(ArchiveError::WrongRoot(display));
    }
    Ok((display, rest.iter().collect()))
}

fn describe_entry_type(kind: EntryType) -> &'static str {
    match kind {
        EntryType::Symlink => "symlink",
        EntryType::Link => "hard link",
        EntryType::Char => "character device",
        EntryType::Block => "block device",
        EntryType::Fifo => "fifo",
        EntryType::GNUSparse => "sparse file",
        EntryType::Continuous => "contiguous file",
        EntryType::GNULongLink => "long link name",
        _ => "unsupported entry type",
    }
}

/// Errors while parsing the stream mean the archive is malformed; anything
/// else is a real filesystem failure.
fn stream_error(error: io::Error) -> ArchiveError {
    match error.kind() {
        io::ErrorKind::InvalidData | io::ErrorKind::InvalidInput | io::ErrorKind::UnexpectedEof => {
            ArchiveError::Malformed(error.to_string())
        }
        _ => ArchiveError::Io(error),
    }
}

fn create_error(error: io::Error, display: &str) -> ArchiveError {
    if error.kind() == io::ErrorKind::AlreadyExists {
        ArchiveError::Conflict(display.to_owned())
    } else {
        ArchiveError::Io(error)
    }
}

// ---------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------

struct Writer<W: Write> {
    builder: tar::Builder<W>,
    summary: ArchiveSummary,
}

impl<W: Write> Writer<W> {
    /// Emit `source`'s children in name order, directories before their
    /// contents, so two exports of an unchanged companion are identical.
    fn walk(&mut self, dir: &Dir, relative: &Path) -> Result<(), ArchiveError> {
        let mut children = Vec::new();
        for entry in dir.entries()? {
            let entry = entry?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                log::warn!(
                    "[export] skipping non-UTF-8 name under {}",
                    relative.display()
                );
                self.summary.skipped += 1;
                continue;
            };
            children.push((name, entry.file_type()?));
        }
        children.sort_by(|a, b| a.0.cmp(&b.0));

        for (name, file_type) in children {
            let at_root = relative.as_os_str().is_empty();
            if at_root && name == IDENTITY_FILE {
                continue;
            }
            if at_root && RETIRED_LAYOUT_DIRS.contains(&name.as_str()) {
                self.summary.skipped += 1;
                continue;
            }
            let child = relative.join(&name);
            if file_type.is_symlink() {
                log::warn!("[export] skipping symlink {}", child.display());
                self.summary.skipped += 1;
            } else if file_type.is_dir() {
                let Some(subdir) = open_child_dir(dir, &name)? else {
                    self.summary.skipped += 1;
                    continue;
                };
                let mtime = unix_mtime(&subdir.dir_metadata()?);
                self.append_directory(&child, mtime)?;
                self.walk(&subdir, &child)?;
            } else if file_type.is_file() {
                let Some(file) = open_child_file(dir, &name)? else {
                    self.summary.skipped += 1;
                    continue;
                };
                let metadata = file.metadata()?;
                let size = metadata.len();
                let mut reader = file.take(size);
                self.append_file(&child, size, unix_mtime(&metadata), &mut reader)?;
                if reader.limit() != 0 {
                    return Err(ArchiveError::Io(io::Error::other(format!(
                        "{} shrank while it was being exported",
                        child.display()
                    ))));
                }
            } else {
                log::warn!("[export] skipping special file {}", child.display());
                self.summary.skipped += 1;
            }
        }
        Ok(())
    }

    fn append_file(
        &mut self,
        relative: &Path,
        size: u64,
        mtime: u64,
        data: impl Read,
    ) -> Result<(), ArchiveError> {
        let mut header = Header::new_gnu();
        header.set_entry_type(EntryType::Regular);
        header.set_mode(0o644);
        header.set_size(size);
        header.set_mtime(mtime);
        self.builder
            .append_data(&mut header, Path::new(ARCHIVE_ROOT).join(relative), data)?;
        self.summary.files += 1;
        self.summary.bytes = self.summary.bytes.saturating_add(size);
        Ok(())
    }

    fn append_directory(&mut self, relative: &Path, mtime: u64) -> Result<(), ArchiveError> {
        let mut header = Header::new_gnu();
        header.set_entry_type(EntryType::Directory);
        header.set_mode(0o755);
        header.set_size(0);
        header.set_mtime(mtime);
        let mut path = Path::new(ARCHIVE_ROOT).join(relative).into_os_string();
        path.push("/");
        self.builder.append_data(&mut header, path, io::empty())?;
        self.summary.directories += 1;
        Ok(())
    }
}

/// The identity marker's bytes and mtime, validated before anything is written.
fn read_marker(source: &Dir) -> Result<(Vec<u8>, u64), ArchiveError> {
    let file = match open_child_file(source, IDENTITY_FILE) {
        Ok(Some(file)) => file,
        Ok(None) => return Err(ArchiveError::MissingIdentity),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(ArchiveError::MissingIdentity);
        }
        Err(error) => return Err(ArchiveError::Io(error)),
    };
    let metadata = file.metadata()?;
    if metadata.len() > MAX_IDENTITY_BYTES {
        return Err(ArchiveError::InvalidIdentity(format!(
            "marker is {} bytes, more than {MAX_IDENTITY_BYTES}",
            metadata.len()
        )));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_IDENTITY_BYTES).read_to_end(&mut bytes)?;
    parse_identity(&bytes)?;
    Ok((bytes, unix_mtime(&metadata)))
}

/// Open a direct child without following a symlink in its place. `None` when
/// it is not a regular file (or vanished).
fn open_child_file(dir: &Dir, name: &str) -> io::Result<Option<cap_std::fs::File>> {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    match dir.open_with(name, &options) {
        Ok(file) if file.metadata()?.is_file() => Ok(Some(file)),
        Ok(_) => Ok(None),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => match dir.symlink_metadata(name) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => Ok(None),
            _ => Err(error),
        },
    }
}

/// Open a direct child directory without following a symlink in its place.
fn open_child_dir(dir: &Dir, name: &str) -> io::Result<Option<Dir>> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .maybe_dir(true)
        .follow(FollowSymlinks::No);
    match dir.open_with(name, &options) {
        Ok(file) if file.metadata()?.is_dir() => Ok(Some(Dir::from_std_file(file.into_std()))),
        Ok(_) => Ok(None),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => match dir.symlink_metadata(name) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => Ok(None),
            _ => Err(error),
        },
    }
}

fn unix_mtime(metadata: &cap_std::fs::Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.into_std().duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_secs())
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
    /// names a hostile archive would. Names that do not fit the header, or
    /// that carry a NUL, travel in a GNU long-name entry like real tars do.
    fn add_raw(builder: &mut Builder<Vec<u8>>, name: &[u8], kind: EntryType, data: &[u8]) {
        let mut header = Header::new_gnu();
        if name.len() > 100 || name.contains(&0) {
            let mut long = Header::new_gnu();
            long.as_gnu_mut().unwrap().name[..13].copy_from_slice(b"././@LongLink");
            long.set_entry_type(EntryType::GNULongName);
            long.set_size(name.len() as u64 + 1);
            long.set_cksum();
            let mut payload = name.to_vec();
            payload.push(0);
            builder.append(&long, payload.as_slice()).unwrap();
            header.as_gnu_mut().unwrap().name[..1].copy_from_slice(b"x");
        } else {
            header.as_gnu_mut().unwrap().name[..name.len()].copy_from_slice(name);
        }
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
            let pax = b"32 path=companion/memory/pax.md\n";
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

    type Build = Box<dyn Fn(&mut Builder<Vec<u8>>)>;

    #[test]
    fn rejects_duplicate_and_conflicting_entries() {
        let cases: Vec<(&str, Build)> = vec![
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

    #[cfg(unix)]
    #[test]
    fn open_companion_dir_refuses_symlinks_and_files() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let real = root.path().join("companion");
        fs::create_dir(&real).unwrap();
        symlink(&real, root.path().join("link")).unwrap();
        fs::write(root.path().join("file"), b"x").unwrap();

        assert!(open_companion_dir(&real).is_ok());
        assert!(open_companion_dir(&root.path().join("link")).is_err());
        assert!(open_companion_dir(&root.path().join("file")).is_err());
        assert!(open_companion_dir(&root.path().join("missing")).is_err());
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
