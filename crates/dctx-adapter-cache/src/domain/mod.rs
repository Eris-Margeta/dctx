//! In-memory cache ledger: quotas, roots, objects, manifests, leases and cursors.
//! Implements ARCHITECTURE-PROPOSAL §4, CACHE-v1 and the DX-A2 lifecycle model (P3, P4, P8).
//! Depends on dctx-core timestamps and reservations. The facade drives startup, `auto_clean`, gc, status and purge.
//! No daemon and no version-control identity. [`Lifecycle::encode_ledger`] is the versioned image of this state.

mod clock;
mod compact;
mod ids;
mod lease;
mod ledger_decode;
mod ledger_encode;
mod ledger_io;
mod legacy;
mod maintain;
mod maintain_slice;
mod maintain_types;
mod objects;
mod publish;
mod purge;
mod queries;
mod quota;
mod reach;
mod reserve;
mod restart;
mod retire;
mod roots;
mod status;
mod writer;

pub use clock::{ClockSample, FakeClock};
pub use compact::CompactOutcome;
pub use ids::{GenerationId, LeaseId, ManifestId, ObjectId, Physical, Purpose, RootId, SessionId};
pub use legacy::{
    ExportEvidence, KnownExport, LeaveReason, LegacyApplyReport, LegacyCandidate, LegacyDryRun,
    OutputPlace, ReclaimedExport, UntouchedExport,
};
pub use maintain::{GcRequest, MaintainReport, SliceBudget};
pub use purge::{PurgeReport, PurgeRequest, PurgeScope};
pub use quota::{
    AutoCleanPolicy, Quota, ACCOUNTING_UNIT, DAY_MS, DEAD_BYTE_THRESHOLD_PPM,
    EMERGENCY_METADATA_BYTES, EVICTION_TARGET_BYTES, HARD_CAP_BYTES, LAST_USE_COALESCE_MS,
    MAX_PACK_SNAPSHOTS, MAX_PREVIEW_GENERATIONS, MAX_VIEW_MANIFESTS, MISSING_GRACE_MS,
    OVERLAY_TTL_MS, PREVIEW_TTL_MS, RECOGNIZED_LEGACY_VERSION, ROOT_PACK_CEILING_BYTES,
    ROOT_PREVIEW_CEILING_BYTES, ROOT_TTL_MS, ROOT_VIEW_CEILING_BYTES, SOFT_WATERMARK_BYTES,
    STAGING_TTL_MS, STARTUP_MAX_OBJECTS, STARTUP_TARGET_MILLIS, UNLINK_GRACE_MS,
};
pub use restart::{RestartEvidence, LEDGER_MAGIC, LEDGER_MAX_BYTES, LEDGER_VERSION};
pub use status::{Blocker, Buckets, RootStatus, Status};
pub use writer::WriterPhase;

use dctx_core::ids::Timestamp;
use ids::{ChargeClass, ManifestKind, ObjectKind};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
struct IdGen {
    object: u64,
    manifest: u64,
    generation: u64,
    lease: u64,
    reservation: u64,
}

impl IdGen {
    const fn fresh() -> Self {
        Self { object: 1, manifest: 1, generation: 1, lease: 1, reservation: 1 }
    }
}

#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub(super) struct RootRecord {
    last_use: Timestamp,
    persisted_use: Timestamp,
    persist_writes: u64,
    unavailable_since: Option<Timestamp>,
    missing: bool,
    offline: bool,
    pinned: bool,
    expired: bool,
    last_generation: Option<GenerationId>,
}

#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub(super) struct CacheObject {
    bytes: u64,
    born: u64,
    kind: ObjectKind,
    state: Physical,
    roots: Vec<RootId>,
    refs: Vec<ObjectId>,
    class: ChargeClass,
    charged_root: Option<RootId>,
    validated: bool,
    complete: bool,
    created: Timestamp,
    retired_at: Option<Timestamp>,
    unreachable_since: Option<Timestamp>,
    deletion_requested: bool,
    corrupt: bool,
    reservation: Option<u64>,
    dead_ppm: u32,
}

#[derive(Debug, Clone)]
pub(super) struct Manifest {
    id: ManifestId,
    root: RootId,
    generation: GenerationId,
    kind: ManifestKind,
    object: ObjectId,
    last_use: Timestamp,
    pinned: bool,
    available: bool,
    refs: Vec<ObjectId>,
}

#[derive(Debug, Clone)]
pub(super) struct Hold {
    left: u64,
    original: u64,
    root: Option<RootId>,
    class: ChargeClass,
    purpose: Purpose,
}

/// Inputs for [`Lifecycle::spawn`].
struct Spawn<'a> {
    bytes: u64,
    kind: ObjectKind,
    state: Physical,
    roots: &'a [RootId],
    refs: &'a [ObjectId],
    class: ChargeClass,
    charged_root: Option<RootId>,
    validated: bool,
    complete: bool,
    reservation: Option<u64>,
    dead_ppm: u32,
}

/// Quotas, ownership and reclamation for one managed cache.
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct Lifecycle {
    quota: Quota,
    wall: Timestamp,
    monotonic_ms: u64,
    boot_id: u64,
    clock_fault: bool,
    ids: IdGen,
    roots: BTreeMap<RootId, RootRecord>,
    objects: BTreeMap<ObjectId, CacheObject>,
    manifests: BTreeMap<ManifestId, Manifest>,
    leases: BTreeMap<LeaseId, lease::Lease>,
    reservations: BTreeMap<u64, Hold>,
    writers: BTreeMap<RootId, writer::Writer>,
    pointers: BTreeMap<RootId, writer::Pointer>,
    exports: BTreeMap<String, legacy::AdoptedExport>,
    maintenance: Option<SessionId>,
    cursor_mark: u64,
    last_maintain: Option<Timestamp>,
    deferred: bool,
    incompatible: bool,
    legacy_frozen: bool,
    legacy_ceiling: u64,
    history_reset: BTreeSet<RootId>,
    unlink_failures: BTreeSet<ObjectId>,
    disk_full: bool,
    pass_incomplete: bool,
    boot_accelerates: bool,
    logical_revision: u64,
    touches: u64,
    last_auto_clean: Option<Timestamp>,
    clean_ttl: Option<CleanTtl>,
}

/// TTLs used for one [`Lifecycle::auto_clean`] slice. Absent means the product constants.
#[derive(Debug, Clone, Copy)]
pub(super) struct CleanTtl {
    pub overlay: u64,
    pub preview: u64,
    pub root: u64,
    pub staging: u64,
}

impl Lifecycle {
    /// Empty ledger under `quota`. Watermarks must be ordered and the metadata reserve must fit.
    /// # Errors
    /// Refuses quota watermarks or ceilings that fail quota validation.
    #[allow(clippy::needless_pass_by_value, reason = "Lifecycle construction takes ownership of the public quota configuration; preserve the existing value-taking API.")]
    pub fn new(quota: Quota) -> Result<Self, crate::error::CacheError> {
        quota.check()?;
        Ok(Self::open(quota))
    }

    fn open(quota: Quota) -> Self {
        Self {
            quota,
            wall: Timestamp(0),
            monotonic_ms: 0,
            boot_id: 0,
            clock_fault: false,
            ids: IdGen::fresh(),
            roots: BTreeMap::new(),
            objects: BTreeMap::new(),
            manifests: BTreeMap::new(),
            leases: BTreeMap::new(),
            reservations: BTreeMap::new(),
            writers: BTreeMap::new(),
            pointers: BTreeMap::new(),
            exports: BTreeMap::new(),
            maintenance: None,
            cursor_mark: 0,
            last_maintain: None,
            deferred: false,
            incompatible: false,
            legacy_frozen: false,
            legacy_ceiling: 0,
            history_reset: BTreeSet::new(),
            unlink_failures: BTreeSet::new(),
            disk_full: false,
            pass_incomplete: false,
            boot_accelerates: false,
            logical_revision: 0,
            touches: 0,
            last_auto_clean: None,
            clean_ttl: None,
        }
    }

    /// Product defaults: 2 GiB cap, 1.5 GiB soft watermark, 1.2 GiB target.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn product_default() -> Self {
        Self::open(Quota::product_default())
    }

    /// Applies a clock sample. A backward wall time sets the diagnostic and leaves expiry clocks put.
    pub fn observe(&mut self, sample: ClockSample) {
        if sample.monotonic_ms >= self.monotonic_ms {
            self.monotonic_ms = sample.monotonic_ms;
        }
        if sample.wall < self.wall {
            self.clock_fault = true;
            return;
        }
        self.wall = sample.wall;
    }

    /// Observes [`FakeClock::sample`](FakeClock::sample).
    pub fn follow(&mut self, clock: &FakeClock) {
        self.observe(clock.sample());
    }

    pub(super) fn bump(&mut self) {
        self.logical_revision = self.logical_revision.saturating_add(1);
    }
}
