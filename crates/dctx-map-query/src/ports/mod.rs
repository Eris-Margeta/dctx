//! Source and revision boundaries for query freshness.
//! Implements SPEC-MAP-MODE section 7 without choosing a filesystem.
//! Used by index and query adapters to obtain current source facts.
use crate::Result;
use std::collections::{BTreeMap, BTreeSet};

/// One authorized file captured by the scanner.
#[derive(Debug, Clone)]
pub struct SourceFile {
    /// Exact authorized bytes; absent for omitted content.
    pub bytes: Option<Vec<u8>>,
    /// Content hash, absent only when content access is forbidden.
    pub hash: String,
    /// Size in bytes.
    pub size: u64,
    /// Modification time in nanoseconds, for diagnostics only.
    pub mtime: i64,
}

/// Complete source inventory and explicit revision metadata.
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Place-relative files in deterministic order.
    pub files: BTreeMap<String, SourceFile>,
    /// Full base revision; non-Git places use a content digest.
    pub revision: String,
    /// Paths differing from the base commit.
    pub dirty: BTreeSet<String>,
    /// Paths changed in recent commits, for ranking.
    pub recent: BTreeSet<String>,
}

/// IO boundary for a complete, privacy-filtered source capture.
pub trait Source {
    /// Captures all visible files; an incomplete scan must return an error.
    fn snapshot(&self) -> Result<Snapshot>;
    /// Rechecks privacy and returns current bytes for a selected source span.
    fn read(&self, path: &str) -> Result<Vec<u8>>;
}
