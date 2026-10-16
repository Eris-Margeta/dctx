//! A reader handle for one committed generation.
//! Implements SEGMENTS-v1 reader protocol: one generation per handle, no per-record lock.
//! Depends on [`crate::SegmentRole`]. [`crate::SegmentStore::pin`] builds it and [`crate::SegmentStore::release`] drops it.
//! The handle names digests. It does not borrow the store.

use crate::SegmentRole;

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

    /// SegmentObject digest.
    pub fn digest(&self) -> &[u8; 32] {
        &self.digest
    }

    /// Fact or view role stored in the manifest.
    pub fn role(&self) -> SegmentRole {
        self.role
    }
}

/// Short lease on one generation. Release it with [`crate::SegmentStore::release`].
#[derive(Debug, PartialEq, Eq)]
pub struct Pin {
    pub(crate) id: u64,
    pub(crate) root: String,
    pub(crate) generation: u64,
    pub(crate) manifest: [u8; 32],
    pub(crate) segments: Vec<PinnedSegment>,
}

impl Pin {
    /// Lease id. Release uses this once.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Root whose pointer was pinned.
    pub fn root(&self) -> &str {
        &self.root
    }

    /// Generation observed at acquire time. It does not follow later publishes.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// ManifestObject digest for this handle.
    pub fn manifest(&self) -> &[u8; 32] {
        &self.manifest
    }

    /// Segments this handle may read.
    pub fn segments(&self) -> &[PinnedSegment] {
        &self.segments
    }

    pub(crate) fn role_of(&self, digest: &[u8; 32]) -> Option<SegmentRole> {
        self.segments.iter().find(|segment| segment.digest() == digest).map(PinnedSegment::role)
    }
}
