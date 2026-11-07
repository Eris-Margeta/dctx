// SPDX-License-Identifier: Apache-2.0
//! In-process exclusion for a disk refresh.
//! Implements SPEC-MAP-MODE section 7 writer serialization without a process.
//! Segment publication takes its own lock; this mutex only covers the refresh body.
use std::sync::{Mutex, MutexGuard};

/// One writer refreshes a disk index. A warm reader does not call this.
pub(super) fn writer_lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
