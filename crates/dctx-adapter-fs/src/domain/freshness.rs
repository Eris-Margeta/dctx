//! Freshness results for authorized files.
//! Implements DX-B3 metadata pre-filter and strict digest validation (decision 078).
//! Depends on [`super::stamp`] and [`crate::ContentDigest`]. Adapters fill these values.

use super::selection::FileHandlingDecision;
use super::stamp::FileStamp;
use crate::ContentDigest;

/// How much evidence `validate` and `inventory` may collect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreshnessMode {
    /// Compare size, mtime, and inode. Does not open content.
    Metadata,
    /// Hash authorized bytes with `dctx-hash` and report a race as stale.
    Strict,
}

/// Why a result is not a current identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaleReason {
    /// Metadata or kind changed while the file was being read.
    Race,
    /// The path disappeared before validation finished.
    Missing,
}

impl std::fmt::Display for StaleReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Race => formatter.write_str("race"),
            Self::Missing => formatter.write_str("missing"),
        }
    }
}

/// Outcome of [`crate::FsSource::validate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Validation {
    /// The pre-filter matches. Bytes were not opened, so this is not a digest proof.
    MetadataMatch {
        /// Stamp observed without a content read.
        stamp: FileStamp,
    },
    /// The pre-filter differs. Bytes were not opened.
    MetadataChanged {
        /// New stamp observed without a content read.
        stamp: FileStamp,
    },
    /// Strict mode hashed the same bytes as the recorded digest.
    DigestMatch {
        /// Stamp taken around the hash.
        stamp: FileStamp,
        /// Digest of the bytes just read.
        digest: ContentDigest,
    },
    /// Strict mode hashed different bytes. The stamp may still match.
    DigestMismatch {
        /// Stamp taken around the hash.
        stamp: FileStamp,
        /// Digest of the new bytes.
        digest: ContentDigest,
    },
    /// The read could not be tied to one file identity.
    Stale {
        /// Race or disappearance.
        reason: StaleReason,
    },
}

/// What an inventory may store for one visible file. Excluded paths are omitted entirely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileBody {
    /// Metadata only. Omitted files and metadata mode use this.
    Metadata {
        /// Size, mtime, and inode.
        stamp: FileStamp,
    },
    /// Strict digest of an included file.
    Digested {
        /// Stamp taken around the hash.
        stamp: FileStamp,
        /// `dctx-hash` identity of the authorized bytes.
        digest: ContentDigest,
    },
    /// The file changed during the read. No digest is claimed.
    Stale {
        /// Why the digest was discarded.
        reason: StaleReason,
    },
}

/// One visible file in a source inventory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    /// `/`-separated path relative to the source root.
    pub relative: String,
    /// Selection decision that authorized this record.
    pub decision: FileHandlingDecision,
    /// Metadata, digest, or a stale race.
    pub body: FileBody,
}

/// Stamp plus digest recorded for a later validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordedIdentity {
    /// Metadata observed with the digest.
    pub stamp: FileStamp,
    /// Content identity from `dctx-hash`.
    pub digest: ContentDigest,
}

/// Authorized bytes plus the identity computed from those same bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedFile {
    /// `/`-separated path relative to the source root.
    pub relative: String,
    /// Stamp taken around the read.
    pub stamp: FileStamp,
    /// Digest of `bytes`.
    pub digest: ContentDigest,
    /// File bytes. Present only after an Include decision.
    pub bytes: Vec<u8>,
}

impl AuthorizedFile {
    /// The identity a later [`Validation`] compares against.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn identity(&self) -> RecordedIdentity {
        RecordedIdentity {
            stamp: self.stamp,
            digest: self.digest,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_reason_names_the_race() {
        assert_eq!(StaleReason::Race.to_string(), "race");
        assert_eq!(StaleReason::Missing.to_string(), "missing");
        assert_ne!(FreshnessMode::Metadata, FreshnessMode::Strict);
    }
}
