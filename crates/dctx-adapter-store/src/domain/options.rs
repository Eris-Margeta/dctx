// SPDX-License-Identifier: Apache-2.0
//! Requests, durability mode, and maintenance budgets for one store.
//! Implements CACHE-v1 admission (emergency metadata inside the cap) and SEGMENTS-v1 publication input.
//! Depends on [`super::error::StoreError`] and [`crate::SegmentRole`]. [`crate::SegmentStore`] consumes these.
//! Ephemeral mode is explicit. Durable mode refuses a filesystem that cannot sync or replace.

use super::error::StoreError;
use super::names::check_token;
use crate::SegmentRole;

/// Bytes held back inside the cap for pointer, lease, and directory metadata.
pub const EMERGENCY_METADATA: u64 = 4096;

/// Pin handshakes before [`super::StoreError::Busy`].
pub const MAX_ACQUIRE: u32 = 8;

/// Whether publication must survive a crash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DurabilityMode {
    /// Sync files and the directory, and require atomic replace.
    Durable,
    /// Caller-selected weaker mode. A crash may drop the new generation.
    Ephemeral,
}

/// Which mark set GC uses. [`MarkMode::OmitLeases`] is the P3 falsifying mutation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkMode {
    /// Retained pointers, writer protections, and active reader leases.
    Reachability,
    /// Mutation that drops leases from the mark set. Production GC does not use this.
    OmitLeases,
}

/// Caps and the namespace recorded in every manifest from this store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreOptions {
    /// Hard admission cap, in bytes, including emergency metadata.
    pub cap: u64,
    /// Durable or explicit ephemeral publication.
    pub mode: DurabilityMode,
    /// Trust namespace stored in kind 1. Not a filesystem path.
    pub namespace: String,
}

impl StoreOptions {
    /// Durable options with the given cap and namespace `local`.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn capped(cap: u64) -> Self {
        Self {
            cap,
            mode: DurabilityMode::Durable,
            namespace: "local".into(),
        }
    }

    pub(crate) fn check(&self) -> Result<(), StoreError> {
        if self.cap == 0 {
            return Err(StoreError::InvalidRequest { reason: "cap" });
        }
        check_token(&self.namespace)
    }
}

/// Revisions stored in manifest kind 5. They are not VCS identities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Revisions {
    /// Resolver revision text.
    pub resolver: String,
    /// Extraction revision text.
    pub extraction: String,
    /// Parser revision text.
    pub parser: String,
    /// Environment digest. Thirty-two bytes.
    pub environment: [u8; 32],
}

impl Revisions {
    /// The structural extractor's revision label.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn structural() -> Self {
        Self {
            resolver: "1".into(),
            extraction: "1".into(),
            parser: "structural".into(),
            environment: [0u8; 32],
        }
    }
}

/// One immutable segment to publish. Bytes are a `dctx-map-segment` file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegmentBlob {
    /// Fact or view role. Mixed roles inside one file are the codec's refusal.
    pub role: SegmentRole,
    /// Validated segment envelope.
    pub bytes: Vec<u8>,
}

/// One generation publish for a single root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublishRequest {
    /// Root id. Not a path and not `.git`.
    pub root: String,
    /// Monotonic generation. The first committed generation is 1.
    pub generation: u64,
    /// Source snapshot digest.
    pub snapshot: [u8; 32],
    /// View digest.
    pub view: [u8; 32],
    /// Policy epoch captured with the view.
    pub policy_epoch: u64,
    /// Segments named by the new manifest.
    pub segments: Vec<SegmentBlob>,
    /// Kind 5 revisions.
    pub revisions: Revisions,
}

/// A generation whose pointer directory entry is durable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Published {
    /// Root that owns the pointer.
    pub root: String,
    /// Committed generation number.
    pub generation: u64,
    /// `ManifestObject` digest.
    pub manifest: [u8; 32],
    /// `SegmentObject` digests named by that manifest, digest order.
    pub segments: Vec<[u8; 32]>,
}

/// What [`crate::SegmentStore::recover`] selected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recovery {
    /// Newest valid committed generation, when any pointer closure validated.
    pub generation: Option<u64>,
    /// Root of that generation.
    pub root: Option<String>,
    /// Temp bytes unlinked after validation.
    pub discarded_tmp: u64,
    /// True when an invalid current pointer was replaced from the previous one.
    pub repaired_pointer: bool,
}

/// How many unreachable objects one GC call may unlink.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GcBudget {
    /// Object cap for this call. Zero unlinks nothing.
    pub objects: u32,
}

/// Counts from one GC call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GcReport {
    /// Objects unlinked.
    pub unlinked: u64,
    /// Objects kept because a handle was still open.
    pub pending: u64,
    /// Objects still reachable.
    pub kept: u64,
    /// True when no unreachable object remains.
    pub done: bool,
}
