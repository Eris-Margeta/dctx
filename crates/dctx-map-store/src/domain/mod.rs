// SPDX-License-Identifier: Apache-2.0
//! File-owned facts, content-addressed keys and cache accounting.
//! Implements map spec sections 7 and 8, IDENTITY-v1 `FactKey` and CACHE-v1 stage-0 limits.
//! Uses model graph types and dctx-hash. Consumed by the SQLite comparison bridge.
use dctx_map_model::{Edge, EdgeKind, Node, StableId};

mod fact_key;
mod retention;

pub use fact_key::{ExtractionContract, FactKey, GIT_IDENTITY, Retain};
pub(crate) use fact_key::{plain_label, view_label};
pub use retention::{
    ACCOUNTING_UNIT, HARD_CAP_BYTES, MaintainReport, UNREACHABLE_GRACE_MS, VIEW_IDLE_MS,
    accounted_bytes, cache_limits,
};
pub(crate) use retention::{Charge, expired, idle, observed_clock};

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
    #[error(
        "database operation failed: {0}; next: check database access and any FTS expression, then retry"
    )]
    Sqlite(#[from] rusqlite::Error),
    /// Stored model payload is invalid.
    #[error("invalid stored payload: {0}; next: rebuild the index from source")]
    Payload(#[from] serde_json::Error),
    /// Existing schema cannot be read; caller must rebuild the index.
    #[error("schema {0} requires index rebuild; next: rebuild the index from source")]
    RebuildRequired(i64),
    /// Facts violate file ownership or fingerprint rules.
    #[error("invalid file facts: {0}; next: correct the file facts and retry reindex")]
    InvalidFacts(&'static str),
    /// Persisted identity is invalid.
    #[error("{0}; next: rebuild the index with valid stable ids")]
    InvalidId(#[from] dctx_map_model::InvalidId),
    /// A stored bridge object failed validation.
    #[error("corrupt index: {0}; next: preserve this database and rebuild the index from source")]
    Corrupt(&'static str),
    /// The comparison cache cannot admit the requested bytes.
    #[error(
        "cache limit requested {requested} available {available}; next: release unused roots or raise the cache cap, then retry"
    )]
    CacheLimit {
        /// Bytes requested.
        requested: u64,
        /// Bytes still available under the hard cap.
        available: u64,
    },
    /// A fact identity or root label is not acceptable.
    #[error("invalid identity: {0}; next: correct the identity or numeric bound and retry")]
    InvalidKey(&'static str),
    /// The same `FactKey` was stored with a different payload.
    #[error(
        "retained fact payload conflict; next: correct the payload or use a new extraction contract"
    )]
    FactConflict,
}

/// Persistence result.
pub type Result<T> = std::result::Result<T, StoreError>;
