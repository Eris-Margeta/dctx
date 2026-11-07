// SPDX-License-Identifier: Apache-2.0
//! Admission ceilings from CACHE-v1, in bytes and milliseconds.
//! Implements ARCHITECTURE-PROPOSAL §4 defaults: 2 GiB hard, 1.5 GiB soft, 1.2 GiB target.
//! Depends on [`crate::error::CacheError`]. `Lifecycle::new` rejects an unordered quota.
//! [`AutoCleanPolicy`] may tighten the system cap; nothing here raises [`HARD_CAP_BYTES`].

use crate::error::CacheError;

/// Global hard cap: 2 GiB.
pub const HARD_CAP_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Soft watermark: 1.5 GiB. Maintenance starts retiring manifests at this level.
pub const SOFT_WATERMARK_BYTES: u64 = 3 * 1024 * 1024 * 1024 / 2;
/// Eviction target: floor(1.2 GiB).
pub const EVICTION_TARGET_BYTES: u64 = 1_288_490_188;
/// Per-root map view and overlay ceiling: 768 MiB, inside the global cap.
pub const ROOT_VIEW_CEILING_BYTES: u64 = 768 * 1024 * 1024;
/// Per-root pack history ceiling: 768 MiB, inside the global cap.
pub const ROOT_PACK_CEILING_BYTES: u64 = 768 * 1024 * 1024;
/// Per-root managed preview ceiling: 64 MiB, inside the global cap.
pub const ROOT_PREVIEW_CEILING_BYTES: u64 = 64 * 1024 * 1024;
/// Ledger room held back inside the hard cap so a full payload cannot block metadata.
pub const EMERGENCY_METADATA_BYTES: u64 = 64 * 1024;
/// One cache accounting unit. Payload lengths are exact bytes.
pub const ACCOUNTING_UNIT: u64 = 1;
/// Upper bound on committed view manifests per root. A publish retires every unheld older view (P-C10),
/// so only views a lease or pin holds stay beyond the current one.
pub const MAX_VIEW_MANIFESTS: usize = 3;
/// Latest pack snapshot plus one predecessor.
pub const MAX_PACK_SNAPSHOTS: usize = 2;
/// Managed preview generations per root.
pub const MAX_PREVIEW_GENERATIONS: usize = 2;
/// One day, in milliseconds. TTLs are whole multiples of this.
pub const DAY_MS: u64 = 86_400_000;
/// Idle overlay and preview lifetime.
pub const OVERLAY_TTL_MS: u64 = 7 * DAY_MS;
/// Preview lifetime. Same interval as an idle overlay.
pub const PREVIEW_TTL_MS: u64 = OVERLAY_TTL_MS;
/// Unpinned root registration lifetime without use.
pub const ROOT_TTL_MS: u64 = 30 * DAY_MS;
/// How long a missing or offline root keeps its bytes before ordinary expiry.
pub const MISSING_GRACE_MS: u64 = 7 * DAY_MS;
/// Grace after a complete object is observed unreachable, before unlink.
pub const UNLINK_GRACE_MS: u64 = DAY_MS;
/// Age after which an incomplete unowned staged object is reclaimed.
pub const STAGING_TTL_MS: u64 = DAY_MS;
/// Minimum gap between persisted last-use writes for one root.
pub const LAST_USE_COALESCE_MS: u64 = 5 * 60 * 1000;
/// Startup examines at most this many objects.
pub const STARTUP_MAX_OBJECTS: u32 = 128;
/// Startup aims to stop at this elapsed budget, checked between objects.
pub const STARTUP_TARGET_MILLIS: u64 = 25;
/// Dead-byte ratio, in parts per million, above which a segment may be compacted.
pub const DEAD_BYTE_THRESHOLD_PPM: u32 = 250_000;
/// Legacy binary version this dry-run knows how to name. Any other version stays put.
pub const RECOGNIZED_LEGACY_VERSION: u32 = 1;

/// Hard cap, watermarks and per-root ceilings. A test quota may be smaller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quota {
    /// Admission cap H.
    pub hard_cap: u64,
    /// Pressure watermark S. `S <= H`.
    pub soft_watermark: u64,
    /// Stop line T after retirement. `T <= S`.
    pub eviction_target: u64,
    /// Exclusive view and overlay bytes allowed for one root.
    pub root_view_ceiling: u64,
    /// Exclusive pack-history bytes allowed for one root.
    pub root_pack_ceiling: u64,
    /// Exclusive preview bytes allowed for one root.
    pub root_preview_ceiling: u64,
    /// Bytes held back inside `hard_cap` for the ledger.
    pub emergency_metadata: u64,
}

impl Quota {
    /// CACHE-v1 product defaults.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub const fn product_default() -> Self {
        Self {
            hard_cap: HARD_CAP_BYTES,
            soft_watermark: SOFT_WATERMARK_BYTES,
            eviction_target: EVICTION_TARGET_BYTES,
            root_view_ceiling: ROOT_VIEW_CEILING_BYTES,
            root_pack_ceiling: ROOT_PACK_CEILING_BYTES,
            root_preview_ceiling: ROOT_PREVIEW_CEILING_BYTES,
            emergency_metadata: EMERGENCY_METADATA_BYTES,
        }
    }

    /// A small quota with the same 3/4 and 3/5 ratios and no metadata reserve.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub const fn scaled(hard: u64) -> Self {
        let mut soft = hard / 4 * 3;
        if soft == 0 {
            soft = hard;
        }
        let mut target = hard / 5 * 3;
        if target > soft {
            target = soft;
        }
        Self {
            hard_cap: hard,
            soft_watermark: soft,
            eviction_target: target,
            root_view_ceiling: hard,
            root_pack_ceiling: hard,
            root_preview_ceiling: hard,
            emergency_metadata: 0,
        }
    }

    /// Replaces the three root ceilings. Global watermarks stay.
    #[allow(
        clippy::must_use_candidate,
        clippy::return_self_not_must_use,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub const fn with_ceilings(mut self, view: u64, pack: u64, preview: u64) -> Self {
        self.root_view_ceiling = view;
        self.root_pack_ceiling = pack;
        self.root_preview_ceiling = preview;
        self
    }

    pub(super) fn check(self) -> Result<(), CacheError> {
        if self.soft_watermark > self.hard_cap || self.eviction_target > self.soft_watermark {
            return Err(CacheError::InvalidRequest {
                reason: "quota watermarks are not ordered",
            });
        }
        if self.emergency_metadata > self.hard_cap {
            return Err(CacheError::InvalidRequest {
                reason: "emergency metadata exceeds the hard cap",
            });
        }
        Ok(())
    }
}

/// When [`super::Lifecycle::auto_clean`] may run, and which ceilings it may apply.
/// A system cap at or above [`HARD_CAP_BYTES`] is refused. A cap above the ledger's current hard cap is ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoCleanPolicy {
    /// `false` makes auto-clean a no-op.
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
    /// System-wide hard cap. Defaults to [`HARD_CAP_BYTES`] and can only tighten.
    pub system_cap_bytes: u64,
}

impl AutoCleanPolicy {
    /// Product defaults: enabled, daily, the CACHE-v1 TTLs, and the 2 GiB cap.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub const fn product_default() -> Self {
        Self {
            enabled: true,
            min_interval_ms: DAY_MS,
            overlay_ttl_ms: OVERLAY_TTL_MS,
            preview_ttl_ms: PREVIEW_TTL_MS,
            root_ttl_ms: ROOT_TTL_MS,
            staging_ttl_ms: STAGING_TTL_MS,
            system_cap_bytes: HARD_CAP_BYTES,
        }
    }

    /// Refuses a cap above the product hard cap. A lower cap is left for the ledger to apply.
    /// # Errors
    /// Refuses a system cap above the product hard cap.
    pub fn check(self) -> Result<(), CacheError> {
        if self.system_cap_bytes > HARD_CAP_BYTES {
            return Err(CacheError::InvalidRequest {
                reason: "system cap cannot exceed the product hard cap",
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_figures_match_the_contract() {
        assert_eq!(HARD_CAP_BYTES, 2_147_483_648);
        assert_eq!(SOFT_WATERMARK_BYTES, 1_610_612_736);
        assert_eq!(
            EVICTION_TARGET_BYTES,
            u64::try_from(12 * (1024u128 * 1024 * 1024) / 10).expect("fixture cap fits u64")
        );
        assert_eq!(ROOT_VIEW_CEILING_BYTES, 805_306_368);
        assert_eq!(ROOT_PACK_CEILING_BYTES, 805_306_368);
        const {
            assert!(
                ROOT_VIEW_CEILING_BYTES < HARD_CAP_BYTES
                    && ROOT_PACK_CEILING_BYTES < HARD_CAP_BYTES
            );
        };
        assert_eq!(ROOT_PREVIEW_CEILING_BYTES, 67_108_864);
        let quota = Quota::product_default();
        assert!(quota.check().is_ok());
        assert!(quota.eviction_target <= quota.soft_watermark);
        assert!(quota.soft_watermark <= quota.hard_cap);
    }

    #[test]
    fn unordered_watermarks_are_refused() {
        let mut quota = Quota::scaled(100);
        quota.soft_watermark = 101;
        assert!(quota.check().is_err());
        quota.soft_watermark = 80;
        quota.eviction_target = 81;
        assert!(quota.check().is_err());
        quota.eviction_target = 60;
        assert!(quota.check().is_ok());
    }

    #[test]
    fn auto_clean_policy_defaults_match_the_product_and_a_raise_is_refused() {
        let policy = AutoCleanPolicy::product_default();
        assert!(policy.enabled);
        assert_eq!(policy.min_interval_ms, DAY_MS);
        assert_eq!(policy.overlay_ttl_ms, OVERLAY_TTL_MS);
        assert_eq!(policy.preview_ttl_ms, PREVIEW_TTL_MS);
        assert_eq!(policy.root_ttl_ms, ROOT_TTL_MS);
        assert_eq!(policy.staging_ttl_ms, STAGING_TTL_MS);
        assert_eq!(policy.system_cap_bytes, HARD_CAP_BYTES);
        assert!(policy.check().is_ok());
        let mut raised = policy;
        raised.system_cap_bytes = HARD_CAP_BYTES - 1;
        assert!(raised.check().is_ok());
        raised.system_cap_bytes = HARD_CAP_BYTES + 1;
        assert!(matches!(
            raised.check(),
            Err(CacheError::InvalidRequest {
                reason: "system cap cannot exceed the product hard cap"
            })
        ));
    }
}
