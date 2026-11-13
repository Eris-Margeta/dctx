// SPDX-License-Identifier: Apache-2.0
//! Typed failures for indexing, querying and bounded rendering.
//! Implements the explicit-refusal rules of SPEC-MAP-MODE sections 3 and 7.
//! Wraps public dependency errors for library and CLI callers.
/// Failures never imply an empty successful graph.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Source or cache IO failed.
    #[error("source or cache IO failed: {0}; next: check source/cache access and retry")]
    Io(#[from] std::io::Error),
    /// Scan failed without a complete inventory.
    #[error("source scan failed: {0}; next: correct the source path or scan policy and retry")]
    Scan(#[from] dctx_map_scan::ScanError),
    /// Ignore rules could not be read.
    #[error("ignore rules failed: {0}; next: correct the ignore rules and refresh the index")]
    Rules(#[from] dctx_map_scan::adapters::rules::IgnoreError),
    /// Parsing failed.
    #[error(
        "source parsing failed: {0}; next: correct the source encoding, syntax or size and refresh the index"
    )]
    Parse(#[from] dctx_map_parse::ParseError),
    /// Graph resolution rejected an invalid contract.
    #[error(transparent)]
    Resolve(#[from] dctx_map_resolve::ResolveError),
    /// The SQLite comparison store failed.
    #[error(transparent)]
    Store(#[from] dctx_map_store::StoreError),
    /// Segment publication or a pinned generation failed.
    #[error(
        "segment storage failed: {0}; next: reopen the index; rebuild it from source if recovery fails"
    )]
    Segment(#[from] dctx_adapter_store::StoreError),
    /// Cache admission failed.
    #[error(
        "cache admission failed: {0}; next: release unused cached data or increase the cache cap and retry"
    )]
    Cache(#[from] dctx_adapter_cache::CacheError),
    /// A graph id was invalid.
    #[error("{0}; next: copy a stable id from dctx map map and retry")]
    Id(#[from] dctx_map_model::InvalidId),
    /// Output could not be rendered within its budget.
    #[error("{0}; next: correct answer metadata/spans/counts or increase --budget and retry")]
    Render(#[from] dctx_map_model::adapters::RenderError),
    /// Persisted query metadata is corrupt.
    #[error(
        "invalid query JSON: {0}; next: supply a complete valid payload or rebuild the index from source"
    )]
    Json(#[from] serde_json::Error),
    /// Caller supplied invalid options.
    #[error("{0}; next: {next}", next = request_next(.0))]
    Request(String),
    /// A past revision is not available in this index.
    #[error("revision unavailable: {0}; try: dctx map map")]
    Revision(String),
    /// Source was denied by privacy or containment rules.
    #[error("source access refused: {0}; next: select an authorized path inside the place")]
    Denied(String),
}
/// Result type shared by index and query operations.
pub type Result<T> = std::result::Result<T, Error>;

fn request_next(reason: &str) -> &'static str {
    if reason == "cancelled" {
        "create a new Index before retrying"
    } else if reason.starts_with("limit or depth") {
        "choose a value in the reported range and retry"
    } else if reason.contains("--lines") || reason.contains("line range") {
        "choose a source range inside the node and within the read line limit"
    } else if reason.contains("database") || reason.contains("Index::open") {
        "open and use the selected database with Index::open"
    } else if reason.contains("cache") || reason.contains("journal") {
        "reopen the index; rebuild it from source if recovery fails"
    } else if reason.contains("refreshed")
        || reason.contains("not ready")
        || reason.contains("graph")
    {
        "refresh the index before querying"
    } else {
        "correct the request or source input and retry; use dctx map --help for options"
    }
}
