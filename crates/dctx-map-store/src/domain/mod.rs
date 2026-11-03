// SPDX-License-Identifier: Apache-2.0
//! File-owned facts, content-addressed keys and cache accounting.
//! Implements map spec sections 7 and 8, IDENTITY-v1 `FactKey` and CACHE-v1 stage-0 limits.
//! Uses model graph types and dctx-hash. Consumed by the SQLite comparison bridge.
use dctx_map_model::{Edge, EdgeKind, Node, StableId};

mod fact_key;
mod retention;

pub use fact_key::{ExtractionContract, FactKey, Retain, GIT_IDENTITY};
pub use retention::{
    accounted_bytes, cache_limits, MaintainReport, ACCOUNTING_UNIT, HARD_CAP_BYTES,
    UNREACHABLE_GRACE_MS, VIEW_IDLE_MS,
};
pub(crate) use fact_key::{plain_label, view_label};
pub(crate) use retention::{expired, idle, observed_clock, Charge};

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
    /// A stored bridge object failed validation.
    #[error("corrupt index: {0}")]
    Corrupt(&'static str),
    /// The comparison cache cannot admit the requested bytes.
    #[error("cache limit requested {requested} available {available}")]
    CacheLimit {
        /// Bytes requested.
        requested: u64,
        /// Bytes still available under the hard cap.
        available: u64,
    },
    /// A fact identity or root label is not acceptable.
    #[error("invalid identity: {0}")]
    InvalidKey(&'static str),
    /// The same `FactKey` was stored with a different payload.
    #[error("retained fact payload conflict")]
    FactConflict,
}

/// Persistence result.
pub type Result<T> = std::result::Result<T, StoreError>;
