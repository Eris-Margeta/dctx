//! Shared ledger helpers for the cache lifecycle integration tests.
//! Each test binary pulls this file in with `#[path]`. It is not a test crate of its own.
//! Depends on the public `Lifecycle` API. Tests call `gc` and `drain`.

use dctx_adapter_cache::{GcRequest, Lifecycle, MaintainReport, SliceBudget};

/// One explicit slice. Grace bypass is the caller's choice.
pub fn gc(life: &mut Lifecycle, bypass_grace: bool) -> MaintainReport {
    life.gc(GcRequest {
        root: None,
        budget: SliceBudget::explicit(128),
        bypass_grace,
        max_bytes: None,
        cancel: None,
    })
    .expect("gc")
}

/// Repeats slices until a pass examines nothing.
pub fn drain(life: &mut Lifecycle, bypass_grace: bool) {
    for _ in 0..16 {
        if gc(life, bypass_grace).examined == 0 {
            break;
        }
    }
}
