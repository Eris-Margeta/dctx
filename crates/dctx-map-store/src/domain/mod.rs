//! File-owned facts exchanged with the persistence port.
//! Implements map spec sections 7 and 8 without database access.
//! Uses model graph types; consumed by SQLite and indexers.
use dctx_map_model::{Edge, EdgeKind, Node, StableId};

/// Indexed file identity and freshness prefilter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    /// Place-relative path.
    pub path: String,
    /// Content length in bytes.
    pub size: u64,
    /// Caller-defined modification timestamp.
    pub mtime: i64,
    /// Content SHA-256.
    pub sha256: String,
    /// Parser language.
    pub lang: String,
}

/// Unresolved name owned by the file producing the reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unresolved {
    /// Referenced qualified name.
    pub name: String,
    /// Relation being resolved.
    pub kind: EdgeKind,
}

/// Fingerprint-bound imported summary, with provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSummary {
    /// Target graph identity.
    pub id: StableId,
    /// Exact body fingerprint.
    pub fingerprint: String,
    /// Single-line summary.
    pub text: String,
    /// Producing model.
    pub model: String,
    /// Generation timestamp.
    pub at: i64,
}

/// Complete replacement of one file's indexed facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFacts {
    /// File metadata.
    pub file: File,
    /// Nodes defined by this file.
    pub nodes: Vec<Node>,
    /// Relations produced by this file, including cross-file relations.
    pub edges: Vec<Edge>,
    /// Names awaiting resolution.
    pub unresolved: Vec<Unresolved>,
    /// Current imported summaries.
    pub summaries: Vec<StoredSummary>,
}

/// Bounded lexical result, ordered by BM25 then identity.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    /// Matching model identity.
    pub id: StableId,
    /// SQLite BM25 score; lower is better.
    pub score: f64,
}

/// Storage failures, including rebuild-required schema mismatches.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// SQLite operation failed.
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    /// Stored model payload is invalid.
    #[error(transparent)]
    Payload(#[from] serde_json::Error),
    /// Existing schema cannot be read; caller must rebuild the index.
    #[error("schema {0} requires index rebuild")]
    RebuildRequired(i64),
    /// Facts violate file ownership or fingerprint rules.
    #[error("invalid file facts: {0}")]
    InvalidFacts(&'static str),
    /// Persisted identity is invalid.
    #[error(transparent)]
    InvalidId(#[from] dctx_map_model::InvalidId),
}

/// Persistence result.
pub type Result<T> = std::result::Result<T, StoreError>;
