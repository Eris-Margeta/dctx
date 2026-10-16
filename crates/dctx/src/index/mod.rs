//! In-memory map generation. SQL, tree-sitter nodes and storage paths stay out of this index.
//! Implements the strict half of SPEC-MAP-MODE over an explicit [`crate::ports`] source.
//! Depends on the domain types and dctx-map-model ids. The session owns each [`Generation`].
//! Callers see an [`crate::domain::Answer`], never this module.

mod extract;
mod query;
mod segment;
#[cfg(feature = "syntax-rust")]
mod syn_extract;
mod walk;

pub(crate) use query::{collect, Collected};
pub(crate) use segment::canonical_bytes;
pub(crate) use walk::{walk_source, WalkArgs};

use dctx_core::FileHandlingDecision;
use dctx_hash::ContentDigest;

use crate::domain::FreshnessMode;

/// One visited file. Bodies are not retained; reads hash again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndexedFile {
    /// Root-relative path.
    pub path: String,
    /// Decision at snapshot time. Queries recompute the live decision before emitting bytes.
    pub decision: FileHandlingDecision,
    /// Own content digest. `None` when the file was not read.
    pub digest: Option<ContentDigest>,
    /// Metadata size.
    pub size: u64,
    /// Metadata modification time, in milliseconds.
    pub modified: u64,
}

/// One graph node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndexedNode {
    /// Stable id.
    pub id: String,
    /// Wire kind (`module`, `fn`, `heading`, …).
    pub kind: String,
    /// Unqualified or qualified name. Empty for a module.
    pub name: String,
    /// Root-relative path.
    pub path: String,
    /// Inclusive 1-based lines.
    pub lines: [u32; 2],
    /// Byte span `[start, end)`.
    pub span: [u64; 2],
    /// Summary text.
    pub summary: String,
    /// `doc`, `heading`, `sig` or empty.
    pub summary_source: String,
    /// Signature without the body.
    pub signature: String,
}

/// One contains edge. Calls are not recorded as `exact`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndexedEdge {
    /// Source stable id.
    pub src: String,
    /// Destination stable id.
    pub dst: String,
    /// Edge name.
    pub edge: String,
    /// Confidence label.
    pub confidence: String,
}

/// One pinned generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Generation {
    /// Generation id. The first snapshot is 1.
    pub id: u64,
    /// Own digest of the canonical payload.
    pub digest: String,
    /// Segment bytes charged to the cache.
    pub encoded_len: u64,
    /// Files that were not excluded.
    pub files: Vec<IndexedFile>,
    /// Modules, symbols and headings.
    pub nodes: Vec<IndexedNode>,
    /// Contains edges.
    pub edges: Vec<IndexedEdge>,
    /// Freshness mode requested for this generation.
    pub freshness: FreshnessMode,
    /// Injected clock at snapshot time, in milliseconds.
    pub taken_at: u64,
    /// Policy epoch at snapshot time.
    pub epoch: u64,
    /// Coverage label.
    pub coverage: String,
    /// Completion label.
    pub completion: String,
    /// Parser id.
    pub parser: String,
}
