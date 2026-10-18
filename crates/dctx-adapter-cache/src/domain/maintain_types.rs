//! Maintenance slice bounds, requests and reports.
//! Implements CACHE-v1 startup bounds and explicit garbage collection inputs.
//! Depends on blockers, root ids and cancellation; the maintenance facade uses these types.

use super::quota::{STARTUP_MAX_OBJECTS, STARTUP_TARGET_MILLIS};
use super::{Blocker, RootId};
use dctx_ports::Cancellation;

/// Work bound for one slice. `millis_per_object` is the host's measured cost of one object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SliceBudget {
    /// Stop before examining more than this many objects.
    pub max_objects: u32,
    /// Stop before starting another object once elapsed time reaches this.
    pub max_millis: u64,
    /// Elapsed time charged after each object. Zero means the time bound does not trip.
    pub millis_per_object: u64,
}

impl SliceBudget {
    /// The startup slice: 128 objects and a 25 ms target.
    pub const fn startup() -> Self {
        Self {
            max_objects: STARTUP_MAX_OBJECTS,
            max_millis: STARTUP_TARGET_MILLIS,
            millis_per_object: 0,
        }
    }

    /// An explicit slice that stops on object count. Time is not sampled.
    pub const fn explicit(max_objects: u32) -> Self {
        Self { max_objects, max_millis: u64::MAX, millis_per_object: 0 }
    }
}

/// What one slice did. `ran` is false when maintenance was not due.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaintainReport {
    /// Objects examined.
    pub examined: u32,
    /// Bytes unlinked.
    pub reclaimed_bytes: u64,
    /// Bytes still waiting for unlink.
    pub pending_bytes: u64,
    /// The maintenance lock was busy.
    pub deferred: bool,
    /// The slice ran.
    pub ran: bool,
    /// The cursor pass finished.
    pub completed_pass: bool,
    /// Why retirement or unlink stopped.
    pub blockers: Vec<Blocker>,
}

/// Explicit GC request. Dry-run uses [`super::Lifecycle::plan_gc`], which does not take `&mut self`.
#[derive(Clone, Copy)]
pub struct GcRequest<'a> {
    /// Limit retirement to one root. Sweep still respects reachability from other roots.
    pub root: Option<RootId>,
    /// Object, time and per-object cost bounds.
    pub budget: SliceBudget,
    /// Explicit GC may skip the 24 hour unlink grace after it has proved the object is unreferenced.
    pub bypass_grace: bool,
    /// Stop once this many bytes have been unlinked.
    pub max_bytes: Option<u64>,
    /// Checked between objects.
    pub cancel: Option<&'a dyn Cancellation>,
}
