//! Memory corrections (#84): the user's own statements about a memory.
//!
//! A correction rewrites the canonical memory file with what the user said
//! and is recorded in a small versioned ledger next to the memory store
//! (`instances/companion/memory_corrections.json`). The ledger exists so a
//! second, different correction of the same memory is detected and put to
//! the user instead of silently merged: the earlier statement stays in force
//! until the user says which one is authoritative.

use serde::{Deserialize, Serialize};

/// Ledger file format version; a file with any other version is left alone.
pub const LEDGER_VERSION: u32 = 1;

/// What became of one correction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorrectionStatus {
    /// In force: the memory reads as this statement.
    Applied,
    /// A later correction, a resolution, or the companion's own rewrite of
    /// the memory replaced this statement.
    Superseded,
    /// Waiting for the user to choose between this statement and the one
    /// in force (`conflicts_with`).
    NeedsResolution,
    /// The user kept the earlier statement; this one was never applied.
    Withdrawn,
}

/// One correction as recorded in the ledger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorrectionEntry {
    /// `corr_<unix seconds>_<8 hex>`; one path component.
    pub id: String,
    /// Memory path as the library shows it.
    pub path: String,
    /// The statement the user asserted, in full: a resolution re-applies it.
    pub statement: String,
    /// Bounded excerpt of what the memory said before this correction.
    #[serde(default)]
    pub previous: String,
    pub status: CorrectionStatus,
    /// RFC 3339 UTC time the correction was made.
    pub corrected_at: String,
    /// RFC 3339 UTC time a `needs_resolution` entry was resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<String>,
    /// The applied entry this statement conflicts with (`needs_resolution`
    /// and `withdrawn` entries).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conflicts_with: Option<String>,
}

/// The whole ledger, one file per companion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorrectionLedger {
    pub version: u32,
    #[serde(default)]
    pub entries: Vec<CorrectionEntry>,
}

impl Default for CorrectionLedger {
    fn default() -> Self {
        Self {
            version: LEDGER_VERSION,
            entries: Vec::new(),
        }
    }
}

/// One side of a conflict as shown to the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorrectionStatement {
    pub id: String,
    pub statement: String,
    pub corrected_at: String,
}

impl From<&CorrectionEntry> for CorrectionStatement {
    fn from(entry: &CorrectionEntry) -> Self {
        Self {
            id: entry.id.clone(),
            statement: entry.statement.clone(),
            corrected_at: entry.corrected_at.clone(),
        }
    }
}

/// Two user statements about one memory; the user picks one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorrectionConflict {
    /// Id of the `needs_resolution` entry; resolve with it.
    pub conflict_id: String,
    pub path: String,
    /// The statement in force.
    pub current: CorrectionStatement,
    /// The statement that was just proposed.
    pub proposed: CorrectionStatement,
}

/// Which statement the user keeps when resolving a conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Keep {
    Current,
    Proposed,
}
