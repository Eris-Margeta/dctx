// SPDX-License-Identifier: Apache-2.0
//! Typed failures for the filesystem Source adapter.
//! Implements DX-B3: escapes are refused and IO stays attached to a path.
//! Depends on [`crate::domain::StaleReason`] and `dctx-hash`. Callers match on [`SourceError`].

use crate::domain::StaleReason;
use dctx_hash::HashError;
use std::{io, path::PathBuf};

/// Failure from rule loading, confinement, or an authorized read.
#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    /// The filesystem rejected an operation at `path`.
    #[error("source IO at {path:?}: {source}")]
    Io {
        /// Path passed to the filesystem.
        path: PathBuf,
        /// Underlying IO error.
        #[source]
        source: io::Error,
    },
    /// A symlink target is outside the source root. Content was not read.
    #[error("symlink escapes the source root at {path:?} -> {target:?}")]
    Escape {
        /// Link path inside the tree.
        path: PathBuf,
        /// Normalized target that left the root.
        target: PathBuf,
    },
    /// A rule file exists but could not be read. Defaults are not substituted.
    #[error("rule file unreadable at {path:?}: {source}")]
    Rule {
        /// Rule path that failed.
        path: PathBuf,
        /// Underlying IO or encoding error.
        #[source]
        source: io::Error,
    },
    /// Selection did not authorize content for this relative path.
    #[error("content is not authorized for {relative:?}")]
    Denied {
        /// Relative path that was refused.
        relative: String,
    },
    /// The path is not a usable source-relative file path.
    #[error("invalid source path: {path:?}")]
    InvalidPath {
        /// Rejected path.
        path: PathBuf,
    },
    /// An authorized read raced. The bytes are not returned as current.
    #[error("source race at {path:?}: {reason}")]
    Stale {
        /// Path whose identity changed.
        path: PathBuf,
        /// Race or disappearance.
        reason: StaleReason,
    },
    /// `dctx-hash` refused the byte length.
    #[error("content digest refused: {0}")]
    Digest(#[from] HashError),
}

impl SourceError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
