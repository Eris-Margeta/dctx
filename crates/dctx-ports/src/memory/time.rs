// SPDX-License-Identifier: Apache-2.0
//! Frozen clock and cancellation doubles for deterministic replay.
//! Implements `Clock` with a fixed instant and `Cancellation` as a shared flag or a
//! cancel-after-N-checkpoints trigger, so tests can cancel at every boundary.
//! Depends on [`crate::clock`] and [`crate::cancellation`]. Used by dctx-pack tests and ephemeral hosts.

use crate::cancellation::Cancellation;
use crate::clock::Clock;
use dctx_core::ids::Timestamp;
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};

/// A clock frozen at one instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedClock(pub Timestamp);

impl Clock for FixedClock {
    fn now(&self) -> Timestamp {
        self.0
    }
}

/// A cancellation flag; `cancel` may be called from any thread.
#[derive(Debug, Default)]
pub struct CancelFlag(AtomicBool);

impl CancelFlag {
    /// A flag that is not raised.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the memory adapter API's existing caller lint behavior."
    )]
    pub fn new() -> Self {
        Self::default()
    }

    /// Raises the flag.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

impl Cancellation for CancelFlag {
    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Reports cancellation after `n` successful checks, counting every check.
/// Both [`Cancellation::is_cancelled`] and [`Cancellation::checkpoint`] consume a check;
/// [`CancelAfter::checks`] only observes the count. This double is not thread-safe.
#[derive(Debug)]
pub struct CancelAfter {
    remaining: Cell<u64>,
    checks: Cell<u64>,
}

impl CancelAfter {
    /// Lets `n` checks pass, then cancels permanently. Zero cancels at the first check.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the memory adapter API's existing caller lint behavior."
    )]
    pub fn new(n: u64) -> Self {
        Self {
            remaining: Cell::new(n),
            checks: Cell::new(0),
        }
    }

    /// Number of checks made so far.
    pub fn checks(&self) -> u64 {
        self.checks.get()
    }
}

impl Cancellation for CancelAfter {
    fn is_cancelled(&self) -> bool {
        self.checks.set(self.checks.get() + 1);
        match self.remaining.get() {
            0 => true,
            n => {
                self.remaining.set(n - 1);
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dctx_core::error::DctxError;

    #[test]
    fn fixed_clock_never_moves() {
        let clock = FixedClock(Timestamp(42));
        assert_eq!(clock.now(), clock.now());
        assert_eq!(clock.now().millis(), 42);
    }

    #[test]
    fn flag_cancels_after_raise_with_positive_control() {
        let flag = CancelFlag::new();
        assert!(flag.checkpoint().is_ok());
        flag.cancel();
        assert_eq!(flag.checkpoint(), Err(DctxError::Cancelled));
    }

    #[test]
    fn cancel_after_counts_checks() {
        let cancel = CancelAfter::new(2);
        assert!(cancel.checkpoint().is_ok());
        assert!(cancel.checkpoint().is_ok());
        assert_eq!(cancel.checkpoint(), Err(DctxError::Cancelled));
        assert_eq!(cancel.checks(), 3);
    }
}
