// SPDX-License-Identifier: Apache-2.0
//! Pure cache admission, expiry and pressure rules for the comparison bridge.
//! Implements CACHE-v1 stage-0 ratios: 2 GiB cap, 1.5 GiB soft watermark, 1.2 GiB target.
//! Depends on no IO. The SQLite ledger applies these decisions to stored charges.
//! A referenced fact is never expired. A backward clock does not retire state.

/// Stage-0 global hard cap. 2 GiB.
pub const HARD_CAP_BYTES: u64 = 2_147_483_648;

/// Bytes added to each stored payload so ledger rows are not free.
pub const ACCOUNTING_UNIT: u64 = 256;

/// Grace before an unreferenced fact may be deleted. 24 hours.
pub const UNREACHABLE_GRACE_MS: u64 = 24 * 86_400 * 1_000;

/// Idle time before an unpinned pack view may be deleted. 30 days.
pub const VIEW_IDLE_MS: u64 = 30 * 86_400 * 1_000;

/// One retained object as the cleaner sees it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Charge {
    pub retired_at: Option<u64>,
    pub referenced: bool,
}

/// Result of one automatic cleaning pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaintainReport {
    /// Accounted bytes removed by this pass.
    pub reclaimed_bytes: u64,
    /// Bytes still above the eviction target because references kept them.
    pub blocked_bytes: u64,
    /// The injected clock moved backward. Expiry used the previous clock.
    pub clock_rewound: bool,
}

/// Payload length plus [`ACCOUNTING_UNIT`]. `None` when the sum overflows.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn accounted_bytes(payload_len: u64) -> Option<u64> {
    payload_len.checked_add(ACCOUNTING_UNIT)
}

/// Soft watermark and eviction target for `hard_cap`, using the stage-0 ratios.
///
/// Soft is `floor(3/4 · cap)` (1.5/2). Target is `floor(3/5 · cap)` (1.2/2).
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn cache_limits(hard_cap: u64) -> (u64, u64) {
    (hard_cap.saturating_mul(3) / 4, hard_cap.saturating_mul(3) / 5)
}

pub(crate) fn observed_clock(previous: u64, now: u64) -> (u64, bool) {
    if now < previous {
        (previous, true)
    } else {
        (now, false)
    }
}

pub(crate) fn expired(fact: &Charge, now: u64, grace: u64) -> bool {
    if fact.referenced {
        return false;
    }
    let Some(retired) = fact.retired_at else {
        return false;
    };
    now >= retired && now.saturating_sub(retired) >= grace
}

pub(crate) fn idle(last_used: u64, now: u64, ttl: u64) -> bool {
    now >= last_used && now.saturating_sub(last_used) >= ttl
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn stage0_limits_match_the_cache_contract() {
        let (soft, target) = cache_limits(HARD_CAP_BYTES);
        assert_eq!(soft, 1_610_612_736);
        assert_eq!(target, 1_288_490_188);
        assert!(target < soft && soft < HARD_CAP_BYTES);
    }

    #[test]
    fn backward_clock_does_not_expire_and_the_grace_boundary_does() {
        let fact = Charge { retired_at: Some(100), referenced: false };
        assert!(!expired(&fact, 50, 10));
        assert!(!expired(&fact, 109, 10));
        assert!(expired(&fact, 110, 10));
        let pinned = Charge { referenced: true, ..fact };
        assert!(!expired(&pinned, 10_000, 0));
        assert!(!expired(
            &Charge { retired_at: None, ..fact },
            10_000,
            0
        ));
        assert_eq!(observed_clock(80, 10), (80, true));
        assert_eq!(observed_clock(80, 90), (90, false));
        assert!(!idle(100, 99, 30));
        assert!(idle(100, 130, 30));
    }

    proptest! {
        #[test]
        fn limits_stay_ordered(cap in 0u64..5_000_000u64) {
            let (soft, target) = cache_limits(cap);
            prop_assert!(target <= soft);
            prop_assert!(soft <= cap);
        }

        #[test]
        fn referenced_facts_never_expire(
            now in any::<u64>(),
            grace in any::<u64>(),
            retired in any::<u64>(),
        ) {
            let fact = Charge { retired_at: Some(retired), referenced: true };
            prop_assert!(!expired(&fact, now, grace));
        }
    }
}
