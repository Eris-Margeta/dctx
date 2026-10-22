//! Publication cuts where a crash must leave only an old or new valid generation.
//! Implements SEGMENTS-v1 steps and the `p2_publish_crash_matrix` flush/rename/pointer cuts.
//! Depends on no IO. [`crate::SegmentStore::publish`] calls [`Boundary::ALL`] in order.
//! [`Boundary::publishes_new_generation`] is true only after the pointer directory sync.

/// One durable step in a generation publish.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boundary {
    /// Writer lock is held.
    AfterLock,
    /// Byte reservation was admitted.
    AfterReserve,
    /// Segment temp file was created.
    AfterSegmentCreate,
    /// Segment bytes were written.
    AfterSegmentWrite,
    /// Segment userspace buffer was flushed.
    AfterSegmentFlush,
    /// Segment file was synced.
    AfterSegmentSync,
    /// Segment bytes were decoded and digested.
    AfterSegmentValidate,
    /// Segment temp name was renamed to its immutable digest name.
    AfterSegmentRename,
    /// Segment directory entry was synced.
    AfterSegmentDirSync,
    /// Manifest temp file was created.
    AfterManifestCreate,
    /// Manifest bytes were written.
    AfterManifestWrite,
    /// Manifest userspace buffer was flushed.
    AfterManifestFlush,
    /// Manifest file was synced.
    AfterManifestSync,
    /// Manifest bytes were decoded and digested.
    AfterManifestValidate,
    /// Manifest was renamed to its immutable digest name.
    AfterManifestRename,
    /// Manifest directory entry was synced.
    AfterManifestDirSync,
    /// Previous pointer was retained.
    AfterPreviousRetain,
    /// Pointer temp file was created.
    AfterPointerCreate,
    /// Pointer bytes were written.
    AfterPointerWrite,
    /// Pointer userspace buffer was flushed.
    AfterPointerFlush,
    /// Pointer file was synced.
    AfterPointerSync,
    /// Pointer was renamed over current. Visibility without directory durability.
    AfterPointerRename,
    /// Pointer directory entry was synced. This is the durable visibility point.
    AfterPointerDirSync,
    /// Accounting finished and the lock is about to drop.
    BeforeUnlock,
}

impl Boundary {
    /// Every cut, in publication order.
    pub const ALL: [Self; 24] = [
        Self::AfterLock,
        Self::AfterReserve,
        Self::AfterSegmentCreate,
        Self::AfterSegmentWrite,
        Self::AfterSegmentFlush,
        Self::AfterSegmentSync,
        Self::AfterSegmentValidate,
        Self::AfterSegmentRename,
        Self::AfterSegmentDirSync,
        Self::AfterManifestCreate,
        Self::AfterManifestWrite,
        Self::AfterManifestFlush,
        Self::AfterManifestSync,
        Self::AfterManifestValidate,
        Self::AfterManifestRename,
        Self::AfterManifestDirSync,
        Self::AfterPreviousRetain,
        Self::AfterPointerCreate,
        Self::AfterPointerWrite,
        Self::AfterPointerFlush,
        Self::AfterPointerSync,
        Self::AfterPointerRename,
        Self::AfterPointerDirSync,
        Self::BeforeUnlock,
    ];

    /// Stable name stored on [`super::StoreError::Crashed`].
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AfterLock => "after-lock",
            Self::AfterReserve => "after-reserve",
            Self::AfterSegmentCreate => "after-segment-create",
            Self::AfterSegmentWrite => "after-segment-write",
            Self::AfterSegmentFlush => "after-segment-flush",
            Self::AfterSegmentSync => "after-segment-sync",
            Self::AfterSegmentValidate => "after-segment-validate",
            Self::AfterSegmentRename => "after-segment-rename",
            Self::AfterSegmentDirSync => "after-segment-dir-sync",
            Self::AfterManifestCreate => "after-manifest-create",
            Self::AfterManifestWrite => "after-manifest-write",
            Self::AfterManifestFlush => "after-manifest-flush",
            Self::AfterManifestSync => "after-manifest-sync",
            Self::AfterManifestValidate => "after-manifest-validate",
            Self::AfterManifestRename => "after-manifest-rename",
            Self::AfterManifestDirSync => "after-manifest-dir-sync",
            Self::AfterPreviousRetain => "after-previous-retain",
            Self::AfterPointerCreate => "after-pointer-create",
            Self::AfterPointerWrite => "after-pointer-write",
            Self::AfterPointerFlush => "after-pointer-flush",
            Self::AfterPointerSync => "after-pointer-sync",
            Self::AfterPointerRename => "after-pointer-rename",
            Self::AfterPointerDirSync => "after-pointer-dir-sync",
            Self::BeforeUnlock => "before-unlock",
        }
    }

    /// True when the new pointer name is already durable, so recovery must see it.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn publishes_new_generation(self) -> bool {
        matches!(self, Self::AfterPointerDirSync | Self::BeforeUnlock)
    }
}
