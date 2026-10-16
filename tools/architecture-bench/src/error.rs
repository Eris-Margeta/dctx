//! Typed refusals for the DX-A1 baseline runner.
//! Implements ARCHITECTURE-PROPOSAL §7 Stage 0 with explicit errors.
//! Depends on std and dctx-map-query; the binary and library return these.

use dctx_map_query::Error as MapError;

/// A baseline run stopped before writing a successful report.
#[derive(Debug, thiserror::Error)]
pub enum BenchError {
    /// The caller supplied an unknown or incomplete argument.
    #[error("{0}")]
    Message(String),
    /// Filesystem IO failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// The current map index or a verb refused the request.
    #[error(transparent)]
    Map(#[from] MapError),
    /// A manifest or answer was not valid JSON.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
