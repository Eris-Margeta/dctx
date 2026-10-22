//! Source and revision boundaries for query freshness.
//! Implements SPEC-MAP-MODE section 7 without choosing a filesystem.
//! Used by index and query adapters to obtain current source facts.
use std::fmt::Write as _;
use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// One authorized file captured by the scanner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceFile {
    /// Exact authorized bytes; absent for omitted or unchanged content.
    #[serde(skip)]
    pub bytes: Option<Vec<u8>>,
    /// Content hash, absent only when content access is forbidden.
    pub hash: String,
    /// Size in bytes.
    pub size: u64,
    /// Modification time in nanoseconds, used by the pre-filter.
    pub mtime: i64,
    /// Whether scan policy and classification allow source reads.
    pub readable: bool,
    /// Whether policy permits classification of this file's content.
    pub allowed: bool,
    /// Metadata captured before reading content.
    pub stamp: FileStamp,
}

/// Metadata pre-filter. Strict mode hashes again when this stamp matches.
/// `changed` stays zero: the filesystem port does not report ctime.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileStamp {
    /// File length.
    pub size: u64,
    /// Modification time in nanoseconds.
    pub modified: i64,
    /// Filesystem change time, when available.
    pub changed: i128,
    /// Filesystem object identity, when available.
    pub identity: u128,
}

/// Captured ignore policy input, retained without rereading unchanged rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleFile {
    /// Missing files have no stamp.
    pub stamp: Option<FileStamp>,
    /// Rule text, absent when the rule file does not exist.
    pub text: Option<String>,
}

/// Complete source inventory and explicit revision metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    /// Place-relative files in deterministic order.
    pub files: BTreeMap<String, SourceFile>,
    /// Full base revision; non-Git places use a content digest.
    pub revision: String,
    /// Paths differing from the base commit.
    pub dirty: BTreeSet<String>,
    /// Paths changed in recent commits, for ranking.
    pub recent: BTreeSet<String>,
    /// Traversed directories, detecting additions and replaced parents.
    pub directories: BTreeMap<String, FileStamp>,
    /// Root ignore/omit rules captured before traversal.
    pub rules: BTreeMap<String, RuleFile>,
}

/// IO boundary for a complete, privacy-filtered source capture.
pub trait Source {
    /// Captures all visible files; an incomplete scan must return an error.
    ///
    /// # Errors
    /// Returns a source, policy or IO error whenever a complete authorized inventory cannot be captured.
    fn snapshot(&self) -> Result<Snapshot>;
    /// Captures changes using previously authorized metadata when supported.
    ///
    /// # Errors
    /// Returns the source capture errors from [`Self::snapshot`] or an implementation-specific incremental capture.
    fn snapshot_since(&self, _previous: Option<&Snapshot>) -> Result<Snapshot> {
        self.snapshot()
    }
    /// Checks a selected record without requiring unchanged content reads.
    ///
    /// # Errors
    /// Propagates source-read or hash-length failures while validating the selected file.
    fn unchanged(&self, path: &str, file: &SourceFile) -> Result<bool> {
        let hashed = dctx_hash::Sha256::digest(&self.read(path)?)
            .map_err(|error| crate::Error::Request(error.to_string()))?;
        let text = hashed
            .as_bytes()
            .iter()
            .fold(String::new(), |mut text, byte| { write!(text, "{byte:02x}").expect("writing to a String cannot fail"); text });
        Ok(text == file.hash)
    }
    /// Rechecks privacy and returns current bytes for a selected source span.
    ///
    /// # Errors
    /// Refuses disallowed or escaping paths and propagates IO failures; no bytes may be returned without rechecking policy.
    fn read(&self, path: &str) -> Result<Vec<u8>>;
}
