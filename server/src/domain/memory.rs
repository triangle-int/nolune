use serde::{Deserialize, Serialize};

/// User-set flags on a memory (#84), stored in its frontmatter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryFlags {
    /// Always auto-recalled into the user's chat.
    #[serde(default)]
    pub pinned: bool,
    /// Hidden from the companion's own routines (check-in, reflection).
    #[serde(default)]
    pub exclude_from_proactive: bool,
}

/// A memory file entry in the library catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// Relative path within the memory directory (e.g. "about/basics.md").
    pub path: String,
    /// First non-empty line of the file (used as summary in the catalog).
    pub summary: String,
    /// File size in bytes.
    pub size: usize,
    /// `pinned` / `exclude_from_proactive` (#84); media memories carry none.
    #[serde(flatten)]
    pub flags: MemoryFlags,
}

/// Undirected graph of connections between memory files.
/// Each edge is a sorted pair of paths (a < b) to avoid duplicates.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryGraph {
    pub edges: Vec<[String; 2]>,
}
