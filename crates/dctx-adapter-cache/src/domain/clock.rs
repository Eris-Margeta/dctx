// SPDX-License-Identifier: Apache-2.0
//! Injected wall clock, monotonic slice time and boot identity.
//! Implements CACHE-v1: a backward step clamps and sets a diagnostic; it does not expire state.
//! Depends on `dctx_core::ids::Timestamp` and `dctx_ports::Clock`. `Lifecycle::observe` consumes samples.
//! Within one process the monotonic millisecond count never decreases.

use super::quota::DAY_MS;
use dctx_core::ids::Timestamp;
use dctx_ports::Clock;

/// One observation from the host clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockSample {
    /// Wall time in Unix milliseconds. A value behind the ledger is ignored.
    pub wall: Timestamp,
    /// Monotonic milliseconds since an arbitrary origin. Used only as a slice budget.
    pub monotonic_ms: u64,
    /// Boot identity. A change is applied by restart reconciliation, not by `observe`.
    pub boot_id: u64,
}

/// Deterministic clock for tests and for hosts that freeze time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeClock {
    wall_ms: u64,
    monotonic_ms: u64,
    boot_id: u64,
}

impl FakeClock {
    /// Wall and monotonic origin, boot identity 1.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub const fn at(wall_ms: u64) -> Self {
        Self {
            wall_ms,
            monotonic_ms: 0,
            boot_id: 1,
        }
    }

    /// Moves wall and monotonic time forward together.
    pub fn advance(&mut self, ms: u64) {
        self.wall_ms = self.wall_ms.saturating_add(ms);
        self.monotonic_ms = self.monotonic_ms.saturating_add(ms);
    }

    /// Moves wall time forward by whole days.
    pub fn advance_days(&mut self, days: u64) {
        self.advance(days.saturating_mul(DAY_MS));
    }

    /// Moves wall time backward. Monotonic time stays, so a slice cannot rewind.
    pub fn rewind_wall(&mut self, ms: u64) {
        self.wall_ms = self.wall_ms.saturating_sub(ms);
    }

    /// Replaces the boot identity. The ledger notices on reconcile, not here.
    pub fn set_boot(&mut self, boot_id: u64) {
        self.boot_id = boot_id;
    }

    /// The current boot identity.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub const fn boot_id(&self) -> u64 {
        self.boot_id
    }

    /// A sample for [`super::Lifecycle::observe`].
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub const fn sample(&self) -> ClockSample {
        ClockSample {
            wall: Timestamp(self.wall_ms),
            monotonic_ms: self.monotonic_ms,
            boot_id: self.boot_id,
        }
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Timestamp {
        Timestamp(self.wall_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewind_does_not_move_monotonic_time() {
        let mut clock = FakeClock::at(1_000);
        clock.advance(50);
        clock.rewind_wall(25);
        assert_eq!(clock.now(), Timestamp(1_025));
        assert_eq!(clock.sample().monotonic_ms, 50);
        clock.set_boot(9);
        assert_eq!(clock.sample().boot_id, 9);
    }
}
