//! Typed failures for indexing, querying and bounded rendering.
//! Implements the explicit-refusal rules of SPEC-MAP-MODE sections 3 and 7.
//! Wraps public dependency errors for library and CLI callers.
/// Failures never imply an empty successful graph.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Source or cache IO failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Scan failed without a complete inventory.
    #[error(transparent)]
    Scan(#[from] dctx_map_scan::ScanError),
    /// Ignore rules could not be read.
    #[error(transparent)]
    Rules(#[from] dctx_map_scan::adapters::rules::IgnoreError),
    /// Parsing failed.
    #[error(transparent)]
    Parse(#[from] dctx_map_parse::ParseError),
    /// Graph resolution rejected an invalid contract.
    #[error(transparent)]
    Resolve(#[from] dctx_map_resolve::ResolveError),
    /// Persistence failed.
    #[error(transparent)]
    Store(#[from] dctx_map_store::StoreError),
    /// A graph id was invalid.
    #[error(transparent)]
    Id(#[from] dctx_map_model::InvalidId),
    /// Output could not be rendered within its budget.
    #[error(transparent)]
    Render(#[from] dctx_map_model::adapters::RenderError),
    /// Persisted query metadata is corrupt.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Caller supplied invalid options.
    #[error("{0}")]
    Request(String),
    /// A past revision is not available in this index.
    #[error("revision unavailable: {0}; try: dctx map map")]
    Revision(String),
    /// Source was denied by privacy or containment rules.
    #[error("source access refused: {0}")]
    Denied(String),
}
/// Result type shared by index and query operations.
pub type Result<T> = std::result::Result<T, Error>;
