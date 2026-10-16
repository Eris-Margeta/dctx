//! Typed domain errors shared by the core, the ports and the engines.
//! Implements the minimum stable error codes of contracts/API-PORTS-v1.md ("Typed error set").
//! Depends on nothing. Messages never carry denied paths or file contents.
//! Used by `crate::ids`, `crate::budget`, `crate::facts`, dctx-ports and dctx-pack.

#![deny(missing_docs)]

use std::fmt;

/// Longest adapter-supplied IO detail kept in an error, in bytes.
pub const MAX_IO_DETAIL_BYTES: usize = 160;

/// A bounded quantity that a request or a source can exceed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resource {
    /// Tree entries visited by one snapshot.
    Entries,
    /// Directory nesting depth.
    Depth,
    /// Bytes read from one file.
    FileBytes,
    /// Bytes read from the whole source by one snapshot.
    SourceBytes,
    /// Bytes of a path or key.
    PathBytes,
}

/// Stable machine-readable code of a [`DctxError`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// See [`DctxError::InvalidRequest`].
    InvalidRequest,
    /// See [`DctxError::PolicyDenied`].
    PolicyDenied,
    /// See [`DctxError::PolicyUnavailable`].
    PolicyUnavailable,
    /// See [`DctxError::UnsupportedCapability`].
    UnsupportedCapability,
    /// See [`DctxError::UnsupportedVersion`].
    UnsupportedVersion,
    /// See [`DctxError::BudgetTooSmall`].
    BudgetTooSmall,
    /// See [`DctxError::ResourceLimit`].
    ResourceLimit,
    /// See [`DctxError::CacheLimit`].
    CacheLimit,
    /// See [`DctxError::Busy`].
    Busy,
    /// See [`DctxError::Cancelled`].
    Cancelled,
    /// See [`DctxError::CorruptIndex`].
    CorruptIndex,
    /// See [`DctxError::SourceChanged`].
    SourceChanged,
    /// See [`DctxError::SourceUnavailable`].
    SourceUnavailable,
    /// See [`DctxError::Io`].
    Io,
}

/// A typed refusal or failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DctxError {
    /// The request or a value in it is malformed.
    InvalidRequest {
        /// Static description of the violated rule.
        reason: &'static str,
    },
    /// The selection policy does not authorize the operation.
    PolicyDenied,
    /// The policy needed for an authorization could not be loaded.
    PolicyUnavailable,
    /// The operation needs a capability this build or session lacks.
    UnsupportedCapability {
        /// Name of the missing capability.
        capability: &'static str,
    },
    /// A stored object has a version this build cannot read.
    UnsupportedVersion {
        /// Version found.
        found: u32,
    },
    /// Even the smallest answer envelope does not fit the budget.
    BudgetTooSmall {
        /// Smallest encoded size that would fit.
        minimum_bytes: u64,
    },
    /// A bounded quantity was exceeded.
    ResourceLimit {
        /// Which quantity.
        resource: Resource,
        /// The limit in force.
        limit: u64,
    },
    /// The cache cannot admit the requested bytes.
    CacheLimit {
        /// Bytes requested.
        requested: u64,
        /// Bytes still available.
        available: u64,
    },
    /// Another writer holds the resource; retry later.
    Busy,
    /// The caller cancelled the operation.
    Cancelled,
    /// A stored object failed validation.
    CorruptIndex {
        /// Static description of the failed check.
        reason: &'static str,
    },
    /// The source changed while it was being read.
    SourceChanged,
    /// The source root is missing or cannot be opened.
    SourceUnavailable,
    /// An adapter IO failure, with a bounded detail.
    Io {
        /// Truncated adapter message.
        detail: String,
    },
}

impl DctxError {
    /// Builds an [`DctxError::Io`] whose detail is cut to [`MAX_IO_DETAIL_BYTES`] at a char boundary.
    pub fn io(detail: impl fmt::Display) -> Self {
        let mut text = detail.to_string();
        if text.len() > MAX_IO_DETAIL_BYTES {
            let mut end = MAX_IO_DETAIL_BYTES;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
        }
        Self::Io { detail: text }
    }

    /// The stable code of this error.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidRequest { .. } => ErrorCode::InvalidRequest,
            Self::PolicyDenied => ErrorCode::PolicyDenied,
            Self::PolicyUnavailable => ErrorCode::PolicyUnavailable,
            Self::UnsupportedCapability { .. } => ErrorCode::UnsupportedCapability,
            Self::UnsupportedVersion { .. } => ErrorCode::UnsupportedVersion,
            Self::BudgetTooSmall { .. } => ErrorCode::BudgetTooSmall,
            Self::ResourceLimit { .. } => ErrorCode::ResourceLimit,
            Self::CacheLimit { .. } => ErrorCode::CacheLimit,
            Self::Busy => ErrorCode::Busy,
            Self::Cancelled => ErrorCode::Cancelled,
            Self::CorruptIndex { .. } => ErrorCode::CorruptIndex,
            Self::SourceChanged => ErrorCode::SourceChanged,
            Self::SourceUnavailable => ErrorCode::SourceUnavailable,
            Self::Io { .. } => ErrorCode::Io,
        }
    }

    /// True if the same request may succeed when retried unchanged.
    pub fn is_retryable(&self) -> bool {
        matches!(self.code(), ErrorCode::Busy | ErrorCode::SourceChanged | ErrorCode::Io)
    }
}

impl fmt::Display for DctxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest { reason } => write!(f, "invalid request: {reason}"),
            Self::PolicyDenied => f.write_str("denied by selection policy"),
            Self::PolicyUnavailable => f.write_str("selection policy unavailable"),
            Self::UnsupportedCapability { capability } => {
                write!(f, "unsupported capability: {capability}")
            }
            Self::UnsupportedVersion { found } => write!(f, "unsupported version {found}"),
            Self::BudgetTooSmall { minimum_bytes } => {
                write!(f, "budget too small: needs at least {minimum_bytes} bytes")
            }
            Self::ResourceLimit { resource, limit } => {
                write!(f, "resource limit exceeded: {resource:?} > {limit}")
            }
            Self::CacheLimit { requested, available } => {
                write!(f, "cache limit: requested {requested} bytes, {available} available")
            }
            Self::Busy => f.write_str("busy"),
            Self::Cancelled => f.write_str("cancelled"),
            Self::CorruptIndex { reason } => write!(f, "corrupt index: {reason}"),
            Self::SourceChanged => f.write_str("source changed during read"),
            Self::SourceUnavailable => f.write_str("source unavailable"),
            Self::Io { detail } => write!(f, "io: {detail}"),
        }
    }
}

impl std::error::Error for DctxError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_detail_is_bounded_at_a_char_boundary() {
        let long = "é".repeat(200);
        let DctxError::Io { detail } = DctxError::io(&long) else {
            panic!("expected Io");
        };
        assert!(detail.len() <= MAX_IO_DETAIL_BYTES);
        assert!(detail.chars().all(|c| c == 'é'));
        let DctxError::Io { detail: short } = DctxError::io("disk full") else {
            panic!("expected Io");
        };
        assert_eq!(short, "disk full");
    }

    #[test]
    fn codes_and_retryability() {
        assert_eq!(DctxError::Cancelled.code(), ErrorCode::Cancelled);
        assert!(!DctxError::Cancelled.is_retryable());
        assert!(DctxError::Busy.is_retryable());
        assert!(DctxError::io("x").is_retryable());
        let limit = DctxError::ResourceLimit { resource: Resource::Entries, limit: 3 };
        assert_eq!(limit.code(), ErrorCode::ResourceLimit);
        assert_eq!(limit.to_string(), "resource limit exceeded: Entries > 3");
    }
}
