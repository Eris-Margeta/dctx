//! Pack and cache reports. Diagnostic paths stay inside the engine; callers see counts.
//! Implements contracts/API-PORTS-v1.md cache administration and the pack report.
//! Depends on [`super::error::DctxError`] and dctx-hash. The session fills these after a run.
//! A purge that is not confirmed is [`DctxError::InvalidRequest`] and changes nothing.

use dctx_hash::Sha256;

use super::error::{DctxError, Resource};

/// Segment magic `DCTXSEG` plus a trailing NUL.
const MAGIC: &[u8; 8] = b"DCTXSEG\0";
/// Smallest well-formed segment: magic, version, empty payload, 32-byte sum.
const MIN_SEGMENT: usize = 8 + 4 + 32;

/// Public pack result. Paths and file bytes are not copied out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackReport {
    /// Display form of the pack version (`V1`, `V1.1`, …).
    pub version: String,
    /// Previous version, when one was stored.
    pub previous: Option<String>,
    /// True when this run published a new snapshot.
    pub published: bool,
    /// True when the version was a major reset.
    pub major_reset: bool,
    /// Own snapshot digest.
    pub digest: String,
    /// Files in the new tree.
    pub files: u64,
    /// Added, removed and modified files.
    pub changes: u64,
    /// How many non-fatal diagnostics the scan recorded.
    pub diagnostics: u64,
}

/// Cache accounting visible to the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CacheReport {
    /// Pack bytes committed plus segment bytes still held.
    pub committed_bytes: u64,
    /// Pack bytes reserved and not yet settled.
    pub reserved_bytes: u64,
    /// Generations currently stored.
    pub generations: u64,
    /// Generations with a live handle.
    pub pinned_generations: u64,
    /// Bytes released by this call. Zero for status and for a dry run.
    pub reclaimed_bytes: u64,
    /// Unpinned-looking generations this call refused to destroy because a handle is live.
    pub blocked_generations: u64,
}

/// Reachability collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GcRequest {
    /// False reports what would be reclaimed and deletes nothing.
    pub apply: bool,
}

/// Explicit destructive cache request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PurgeRequest {
    /// Must be true. A false value is [`DctxError::InvalidRequest`].
    pub confirm: bool,
    /// False reports the effect and deletes nothing.
    pub apply: bool,
    /// Which generations to consider.
    pub scope: PurgeScope,
}

/// Target of a purge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PurgeScope {
    /// Every generation.
    All,
    /// One generation id.
    Generation(u64),
}

/// What [`validate_segment`] accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentInfo {
    /// Envelope version. This build accepts 1.
    pub version: u32,
    /// Payload length in bytes, excluding magic, version and the trailing sum.
    pub payload_len: u64,
}

/// Checks the segment magic, version and trailing SHA-256.
pub fn validate_segment(bytes: &[u8]) -> Result<SegmentInfo, DctxError> {
    if bytes.len() < MIN_SEGMENT {
        return Err(DctxError::CorruptIndex { reason: "segment too short" });
    }
    if bytes.get(..8) != Some(MAGIC.as_slice()) {
        return Err(DctxError::CorruptIndex { reason: "segment magic" });
    }
    let version_bytes: [u8; 4] = bytes
        .get(8..12)
        .and_then(|slice| slice.try_into().ok())
        .ok_or(DctxError::CorruptIndex { reason: "segment version" })?;
    let version = u32::from_le_bytes(version_bytes);
    if version != 1 {
        return Err(DctxError::UnsupportedVersion { found: version });
    }
    let split = bytes.len() - 32;
    let (prefix, sum) = bytes.split_at(split);
    let digest = Sha256::digest(prefix).map_err(|_| DctxError::ResourceLimit {
        resource: Resource::Memory,
        limit: u64::MAX,
    })?;
    if sum != digest.as_bytes().as_slice() {
        return Err(DctxError::CorruptIndex { reason: "segment digest" });
    }
    let payload_len = u64::try_from(split - 12).unwrap_or(u64::MAX);
    Ok(SegmentInfo { version, payload_len })
}

/// Builds a version-1 segment around `payload`.
pub(crate) fn seal_segment(payload: &[u8]) -> Result<Vec<u8>, DctxError> {
    let mut bytes = Vec::with_capacity(MIN_SEGMENT + payload.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(payload);
    let digest = Sha256::digest(&bytes).map_err(|_| DctxError::ResourceLimit {
        resource: Resource::Memory,
        limit: u64::MAX,
    })?;
    bytes.extend_from_slice(digest.as_bytes());
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_and_corrupt_segments_are_refused() {
        assert!(validate_segment(b"short").is_err());
        let mut sealed = seal_segment(b"payload").expect("seal");
        assert!(validate_segment(&sealed).is_ok());
        let last = sealed.len() - 1;
        sealed[last] ^= 0xff;
        assert_eq!(
            validate_segment(&sealed).expect_err("corrupt"),
            DctxError::CorruptIndex { reason: "segment digest" }
        );
        let mut bad = seal_segment(b"payload").expect("seal");
        bad[8] = 9;
        assert!(matches!(validate_segment(&bad), Err(DctxError::UnsupportedVersion { found: 9 })));
    }
}
