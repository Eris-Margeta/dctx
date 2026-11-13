// SPDX-License-Identifier: Apache-2.0
//! Per-thread allocation events and peak live blocks during an isolated stage.
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::time::{Duration, Instant};

pub struct CountingAllocator;

#[derive(Clone, Copy, Default, Debug)]
pub struct Counts {
    pub allocations: usize,
    pub peak: usize,
    live: usize,
}

thread_local! {
    static COUNTS: Cell<Option<Counts>> = const { Cell::new(None) };
}

fn allocated() {
    let _ = COUNTS.try_with(|state| {
        if let Some(mut counts) = state.get() {
            counts.allocations += 1;
            counts.live += 1;
            counts.peak = counts.peak.max(counts.live);
            state.set(Some(counts));
        }
    });
}

fn freed() {
    let _ = COUNTS.try_with(|state| {
        if let Some(mut counts) = state.get() {
            counts.live -= 1;
            state.set(Some(counts));
        }
    });
}

// SAFETY: Every operation forwards the original pointer/layout to System. The
// counters use allocation-free TLS and never alter the allocation or its layout.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: The caller supplies the layout required by GlobalAlloc.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            allocated();
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: Same contract as System::alloc_zeroed.
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            allocated();
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        freed();
        // SAFETY: The caller supplies a live System allocation and its layout.
        unsafe { System.dealloc(ptr, layout) };
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        // SAFETY: The original allocation and requested size satisfy GlobalAlloc.
        let new = unsafe { System.realloc(ptr, layout, size) };
        if !new.is_null() {
            // Count realloc as an event, with no change to live block count.
            let _ = COUNTS.try_with(|state| {
                if let Some(mut counts) = state.get() {
                    counts.allocations += 1;
                    state.set(Some(counts));
                }
            });
        }
        new
    }
}

/// The closure must not free allocations made before measurement. Inputs are
/// borrowed; returned allocations remain live until after counters are disabled.
pub fn measure<T>(f: impl FnOnce() -> T) -> (T, Duration, Counts) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            COUNTS.with(|state| state.set(None));
        }
    }
    COUNTS.with(|state| {
        assert!(state.get().is_none());
        state.set(Some(Counts::default()));
    });
    let reset = Reset;
    let start = Instant::now();
    let output = f();
    let elapsed = start.elapsed();
    let counts = COUNTS.with(|state| state.get().unwrap());
    drop(reset);
    (output, elapsed, counts)
}

#[test]
fn counts_reallocation_without_inflating_peak_live_blocks() {
    let (retained, _, counts) = measure(|| {
        let mut retained = Vec::<u64>::with_capacity(1);
        retained.push(7);
        std::hint::black_box(&retained);
        retained.reserve_exact(7);
        let transient = Box::new([0u8; 16]);
        std::hint::black_box(&transient);
        drop(transient);
        retained
    });
    assert_eq!(counts.allocations, 3);
    assert_eq!(counts.peak, 2);
    assert_eq!(counts.live, 1);
    assert_eq!(retained, [7]);
}
