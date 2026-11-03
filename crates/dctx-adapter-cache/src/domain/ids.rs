// SPDX-License-Identifier: Apache-2.0
//! Caller-facing identifiers for roots, objects, leases and sessions.
//! Implements CACHE-v1 ownership: a root id is assigned when a caller opens it.
//! Depends on nothing else. The ledger uses these as map keys.
//! None of them is a path, a process id, or a version-control object id.

/// A root the caller registered. The ledger never invents one from a directory walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RootId(u64);

impl RootId {
    /// Wraps a caller-chosen id.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// The raw id.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// One managed object: fact, segment, manifest payload, staging byte range or legacy wal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId(u64);

impl ObjectId {
    pub(super) const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// The raw id.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// One view, overlay, pack or preview manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ManifestId(u64);

impl ManifestId {
    pub(super) const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// The raw id.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// One reader handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LeaseId(u64);

impl LeaseId {
    pub(super) const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// The raw id.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// Session identity carried with an OS lock. Age of a process id is not this value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionId(u64);

impl SessionId {
    /// Wraps the host session identity.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// The raw id.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// One published or in-flight generation of a root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GenerationId(u64);

impl GenerationId {
    pub(super) const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// The raw id.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// Why a byte reservation exists. Root ceilings use the class, not the name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// A committed view manifest.
    View,
    /// A dirty overlay.
    Overlay,
    /// Pack history.
    Pack,
    /// A managed temporary preview.
    Preview,
    /// A shared or exclusive fact payload.
    Fact,
    /// An immutable segment.
    Segment,
    /// An incomplete publication write.
    Staging,
    /// Anonymous bytes admitted through the `Cache` port.
    Payload,
}

/// Physical lifecycle of one object. Absent objects are removed from the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Physical {
    /// Reserved bytes that have been written and are not yet published.
    Staged,
    /// Reachable or simply committed.
    Published,
    /// The owning manifest was retired. Bytes stay until reachability says otherwise.
    Retired,
    /// Unlink was requested. A failed unlink stays here and still counts toward the cap.
    PendingUnlink,
}

/// Which root ceiling a byte charges, or none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ChargeClass {
    View,
    Pack,
    Preview,
    Global,
}

impl Purpose {
    pub(super) const fn class(self) -> ChargeClass {
        match self {
            Self::View | Self::Overlay => ChargeClass::View,
            Self::Pack => ChargeClass::Pack,
            Self::Preview => ChargeClass::Preview,
            Self::Fact | Self::Segment | Self::Staging | Self::Payload => ChargeClass::Global,
        }
    }
}

/// Kind of payload. Reachability uses manifests and refs, not this label alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ObjectKind {
    Fact,
    Segment,
    Manifest,
    LegacyWal,
    Staging,
}

/// View, overlay, pack snapshot or preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ManifestKind {
    View,
    Overlay,
    Pack,
    Preview,
}

impl Physical {
    pub(super) const fn tag(self) -> u8 {
        match self {
            Self::Staged => 1,
            Self::Published => 2,
            Self::Retired => 3,
            Self::PendingUnlink => 4,
        }
    }

    pub(super) const fn from_tag(tag: u8) -> Option<Self> {
        Some(match tag {
            1 => Self::Staged,
            2 => Self::Published,
            3 => Self::Retired,
            4 => Self::PendingUnlink,
            _ => return None,
        })
    }
}

impl Purpose {
    pub(super) const fn tag(self) -> u8 {
        match self {
            Self::View => 1,
            Self::Overlay => 2,
            Self::Pack => 3,
            Self::Preview => 4,
            Self::Fact => 5,
            Self::Segment => 6,
            Self::Staging => 7,
            Self::Payload => 8,
        }
    }

    pub(super) const fn from_tag(tag: u8) -> Option<Self> {
        Some(match tag {
            1 => Self::View,
            2 => Self::Overlay,
            3 => Self::Pack,
            4 => Self::Preview,
            5 => Self::Fact,
            6 => Self::Segment,
            7 => Self::Staging,
            8 => Self::Payload,
            _ => return None,
        })
    }
}

impl ChargeClass {
    pub(super) const fn tag(self) -> u8 {
        match self {
            Self::View => 1,
            Self::Pack => 2,
            Self::Preview => 3,
            Self::Global => 4,
        }
    }

    pub(super) const fn from_tag(tag: u8) -> Option<Self> {
        Some(match tag {
            1 => Self::View,
            2 => Self::Pack,
            3 => Self::Preview,
            4 => Self::Global,
            _ => return None,
        })
    }
}

impl ObjectKind {
    pub(super) const fn tag(self) -> u8 {
        match self {
            Self::Fact => 1,
            Self::Segment => 2,
            Self::Manifest => 3,
            Self::LegacyWal => 4,
            Self::Staging => 5,
        }
    }

    pub(super) const fn from_tag(tag: u8) -> Option<Self> {
        Some(match tag {
            1 => Self::Fact,
            2 => Self::Segment,
            3 => Self::Manifest,
            4 => Self::LegacyWal,
            5 => Self::Staging,
            _ => return None,
        })
    }
}

impl ManifestKind {
    pub(super) const fn tag(self) -> u8 {
        match self {
            Self::View => 1,
            Self::Overlay => 2,
            Self::Pack => 3,
            Self::Preview => 4,
        }
    }

    pub(super) const fn from_tag(tag: u8) -> Option<Self> {
        Some(match tag {
            1 => Self::View,
            2 => Self::Overlay,
            3 => Self::Pack,
            4 => Self::Preview,
            _ => return None,
        })
    }
}

impl ManifestKind {
    pub(super) const fn purpose(self) -> Purpose {
        match self {
            Self::View => Purpose::View,
            Self::Overlay => Purpose::Overlay,
            Self::Pack => Purpose::Pack,
            Self::Preview => Purpose::Preview,
        }
    }

    pub(super) const fn keep(self) -> Option<usize> {
        match self {
            Self::View => Some(super::quota::MAX_VIEW_MANIFESTS),
            Self::Pack => Some(super::quota::MAX_PACK_SNAPSHOTS),
            Self::Preview => Some(super::quota::MAX_PREVIEW_GENERATIONS),
            Self::Overlay => None,
        }
    }
}
