// SPDX-License-Identifier: Apache-2.0
//! Persistence boundary for incremental map indexing.
//! Implements spec sections 7 and 8 as synchronous operations.
//! Uses domain facts only; implemented by SQLite adapters.
use crate::domain::{FileFacts, Result, SearchHit};

/// Atomic file replacement and bounded lexical lookup.
pub trait MapStore {
    /// Replaces exactly the facts owned by this file, atomically.
    /// Duplicate node/edge/unresolved/summary rows are refused without losing the
    /// previous facts. Input order does not affect the stored read/search results.
    ///
    /// # Errors
    /// Refuses inconsistent file ownership, identities or summaries; propagates persistence and serialization failures.
    fn reindex(&mut self, facts: &FileFacts) -> Result<()>;
    /// Removes files absent from the complete live-path list.
    ///
    /// # Errors
    /// Returns persistence failures while listing or removing file-owned facts.
    fn prune(&mut self, live_paths: &[&str]) -> Result<usize>;
    /// Searches an FTS5 expression, returning at most 40 matches.
    /// `limit == 0` returns no matches; larger limits are clamped to 40.
    /// Expressions use SQLite FTS syntax, so quote literal terms and correct
    /// malformed expressions before retrying an expression error.
    ///
    /// # Errors
    /// Returns invalid-expression, database or stored-identity decoding errors.
    fn search(&self, expression: &str, limit: usize) -> Result<Vec<SearchHit>>;
    /// Reads all current facts owned by one file.
    ///
    /// # Errors
    /// Returns persistence or stored-fact decoding errors; a missing path returns `None`.
    fn file(&self, path: &str) -> Result<Option<FileFacts>>;
}
