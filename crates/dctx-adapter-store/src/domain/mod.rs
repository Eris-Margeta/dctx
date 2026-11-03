// SPDX-License-Identifier: Apache-2.0
//! Pure publication rules: names, manifests, pointers, and the cache lifecycle model.
//! Implements SEGMENTS-v1 manifest/pointer bytes and CacheLifecycle.tla (DX-A2) for P2/P3/P8.
//! Depends on `dctx-hash` and `dctx-map-segment` checksums. The store adapter executes these rules.
//! This module does not touch the filesystem. A staged object is never a committed generation.

mod boundary;
mod bytes;
mod envelope;
mod error;
#[cfg(test)]
mod lifecycle;
mod manifest;
mod names;
mod options;
mod pin;
mod pointer;

pub use boundary::Boundary;
pub use error::StoreError;
pub use manifest::{Manifest, SegmentRef};
pub use options::{
    DurabilityMode, GcBudget, GcReport, MarkMode, PublishRequest, Published, Recovery, Revisions,
    SegmentBlob, StoreOptions, EMERGENCY_METADATA, MAX_ACQUIRE,
};
pub use pin::{Pin, PinnedSegment};

pub(crate) use bytes::sha256;
pub(crate) use manifest::{decode_manifest, encode_manifest, manifest_digest};
pub(crate) use names::{
    check_rel, check_root, current_path, cursor_path, manifest_path, object_digest, previous_path,
    segment_path, tmp_path,
};
pub(crate) use pointer::{decode_pointer, encode_pointer, Pointer, POINTER_LEN};
