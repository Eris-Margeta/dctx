// SPDX-License-Identifier: Apache-2.0
//! A reader handle for one committed generation.
//! Implements SEGMENTS-v1 reader protocol: one generation per handle, no per-record lock.
//! Depends on [`crate::SegmentRole`]. [`crate::SegmentStore::pin`] builds it and [`crate::SegmentStore::release`] drops it.
//! The handle names digests. It does not borrow the store.

use crate::SegmentRole;
use std::sync::Arc;

/// One segment inside a pin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinnedSegment {
    digest: [u8; 32],
    role: SegmentRole,
}

impl PinnedSegment {
    pub(crate) fn new(digest: [u8; 32], role: SegmentRole) -> Self {
        Self { digest, role }
    }

    /// `SegmentObject` digest.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn digest(&self) -> &[u8; 32] {
        &self.digest
    }

    /// Fact or view role stored in the manifest.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn role(&self) -> SegmentRole {
        self.role
    }
}

/// Short lease on one generation, valid only in the store instance that acquired it.
/// Release it with [`crate::SegmentStore::release`]; another store cannot use this handle.
#[derive(Debug, PartialEq, Eq)]
pub struct Pin {
    pub(crate) id: u64,
    pub(crate) token: Arc<()>,
    pub(crate) root: String,
    pub(crate) generation: u64,
    pub(crate) manifest: [u8; 32],
    pub(crate) segments: Vec<PinnedSegment>,
}

impl Pin {
    /// Lease id. Release uses this once.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Root whose pointer was pinned.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn root(&self) -> &str {
        &self.root
    }

    /// Generation observed at acquire time. It does not follow later publishes.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// `ManifestObject` digest for this handle.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn manifest(&self) -> &[u8; 32] {
        &self.manifest
    }

    /// Segments this handle may read.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn segments(&self) -> &[PinnedSegment] {
        &self.segments
    }

    pub(crate) fn role_of(&self, digest: &[u8; 32]) -> Option<SegmentRole> {
        self.segments
            .iter()
            .find(|segment| segment.digest() == digest)
            .map(PinnedSegment::role)
    }
}
