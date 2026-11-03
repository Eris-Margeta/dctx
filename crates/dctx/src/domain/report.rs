// SPDX-License-Identifier: Apache-2.0
//! Pack and cache reports. Diagnostic paths stay inside the engine; callers see counts.
//! Implements contracts/API-PORTS-v1.md cache administration, CACHE-v1 auto-clean and the pack report.
//! Depends on [`super::error::DctxError`], dctx-hash and the cache adapter's product figures.
//! The session fills these after a run. A purge that is not confirmed changes nothing.

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
    /// Legacy layout actually emitted, or `None` for the default text/JSON summary.
    /// A requested diff becomes a snapshot on a first run or major reset.
    pub legacy_layout: Option<dctx_template::LegacyLayout>,
}

/// Auto-clean settings for one managed cache root.
///
/// The product default is enabled, once a day, with the CACHE-v1 TTLs and the 2 GiB cap.
/// Library isolation does not apply this value: a session with no cache root never cleans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoCleanConfig {
    /// `false` makes the open-time slice a no-op.
    pub enabled: bool,
    /// Minimum wall gap between slices. The product default is one day.
    pub min_interval_ms: u64,
    /// Idle overlay lifetime used by the slice.
    pub overlay_ttl_ms: u64,
    /// Preview lifetime used by the slice.
    pub preview_ttl_ms: u64,
    /// Unpinned root lifetime used by the slice.
    pub root_ttl_ms: u64,
    /// Abandoned staging lifetime used by the slice.
    pub staging_ttl_ms: u64,
    /// System-wide hard cap. It can only tighten the product cap.
    pub system_cap_bytes: u64,
}

impl Default for AutoCleanConfig {
    /// Enabled, daily, and bounded by the product TTLs and hard cap.
    fn default() -> Self {
        let policy = dctx_adapter_cache::AutoCleanPolicy::product_default();
        Self {
            enabled: policy.enabled,
            min_interval_ms: policy.min_interval_ms,
            overlay_ttl_ms: policy.overlay_ttl_ms,
            preview_ttl_ms: policy.preview_ttl_ms,
            root_ttl_ms: policy.root_ttl_ms,
            staging_ttl_ms: policy.staging_ttl_ms,
            system_cap_bytes: policy.system_cap_bytes,
        }
    }
}

impl AutoCleanConfig {
    /// The adapter policy this configuration names. The cap is checked when the ledger opens.
    pub(crate) fn to_policy(self) -> dctx_adapter_cache::AutoCleanPolicy {
        dctx_adapter_cache::AutoCleanPolicy {
            enabled: self.enabled,
            min_interval_ms: self.min_interval_ms,
            overlay_ttl_ms: self.overlay_ttl_ms,
            preview_ttl_ms: self.preview_ttl_ms,
            root_ttl_ms: self.root_ttl_ms,
            staging_ttl_ms: self.staging_ttl_ms,
            system_cap_bytes: self.system_cap_bytes,
        }
    }
}

/// Managed ledger figures. Absent on a session that has no cache root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheLedger {
    /// Bytes the lifecycle has admitted.
    pub admitted_bytes: u64,
    /// Hard cap in force after open-time auto-clean.
    pub hard_cap: u64,
    /// Registered roots, including expired ones not yet compacted.
    pub roots: u64,
    /// Roots whose registration TTL has fired.
    pub expired_roots: u64,
    /// Wall time of the last auto-clean slice recorded in the ledger.
    pub last_auto_clean_ms: Option<u64>,
    /// Bytes reclaimed by auto-clean when this session opened.
    pub reclaimed_bytes: u64,
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
    /// Bytes released by this call. Status reports the open-time auto-clean total, which is zero in isolation.
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
/// # Errors
/// Refuses short/invalid segment framing, unsupported versions, hash length overflow, or digest mismatches.
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
    fn auto_clean_defaults_are_enabled_daily_and_bounded() {
        let policy = AutoCleanConfig::default();
        assert!(policy.enabled);
        assert_eq!(policy.min_interval_ms, 86_400_000);
        assert_eq!(policy.system_cap_bytes, 2_147_483_648);
        assert!(policy.root_ttl_ms > policy.min_interval_ms);
        let mut disabled = policy;
        disabled.enabled = false;
        assert!(!disabled.to_policy().enabled);
        assert!(policy.to_policy().check().is_ok());
    }

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
