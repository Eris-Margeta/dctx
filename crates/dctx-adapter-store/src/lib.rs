//! Filesystem publication of immutable map segments and manifest generations.
//! Implements ARCHITECTURE-PROPOSAL §7 DX-C3, SEGMENTS-v1 publication, and `CacheLifecycle` P2/P3/P8.
//! Depends on `dctx-map-segment` for codec validation and `dctx-hash` for manifest digests.
//! Callers publish through [`SegmentStore`]; engines never see filesystem paths as handles.
//! No git, no `.git` paths, and no PID-age lock clearing (decision 078).

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod adapters;
pub mod domain;
pub mod ports;

pub use adapters::{MemoryFs, SegmentStore, StdFs};
pub use domain::{
    Boundary, DurabilityMode, GcBudget, GcReport, Manifest, MarkMode, Pin, PinnedSegment,
    PublishRequest, Published, Recovery, Revisions, SegmentBlob, SegmentRef, StoreError,
    StoreOptions,
};
pub use dctx_map_segment::SegmentRole;
