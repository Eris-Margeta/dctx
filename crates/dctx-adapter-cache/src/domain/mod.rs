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
mod publish;
mod purge;
mod quota;
mod reach;
mod reserve;
mod restart;
mod retire;
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
use quota::LAST_USE_COALESCE_MS as COALESCE_MS;
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

    /// Registers a caller-opened root. A second registration of the same id is refused.
    /// # Errors
    /// Refuses an already registered root ID.
    pub fn register_root(&mut self, id: RootId) -> Result<(), crate::error::CacheError> {
        if self.roots.contains_key(&id) {
            return Err(crate::error::CacheError::InvalidRequest { reason: "root already registered" });
        }
        self.roots.insert(
            id,
            RootRecord {
                last_use: self.wall,
                persisted_use: self.wall,
                persist_writes: 1,
                unavailable_since: None,
                missing: false,
                offline: false,
                pinned: false,
                expired: false,
                last_generation: None,
            },
        );
        self.bump();
        Ok(())
    }

    /// In-memory last use always moves. The persisted stamp moves at most once per five minutes.
    /// # Errors
    /// Refuses an unknown root ID.
    pub fn touch(&mut self, root: RootId) -> Result<(), crate::error::CacheError> {
        if !self.roots.contains_key(&root) {
            return Err(crate::error::CacheError::InvalidRequest { reason: "unknown root" });
        }
        self.note_use(root);
        let wall = self.wall;
        for manifest in self.manifests.values_mut() {
            if manifest.root == root {
                manifest.last_use = wall;
            }
        }
        self.bump();
        Ok(())
    }

    /// Refreshes the root's in-memory last use and, at most once per five minutes, the persisted stamp.
    pub(super) fn note_use(&mut self, root: RootId) {
        let wall = self.wall;
        let Some(record) = self.roots.get_mut(&root) else {
            return;
        };
        record.last_use = wall;
        if wall.0.saturating_sub(record.persisted_use.0) >= COALESCE_MS {
            record.persisted_use = wall;
            record.persist_writes = record.persist_writes.saturating_add(1);
        }
    }

    /// An explicit pin opts out of expiry and LRU. It does not raise the hard cap.
    /// # Errors
    /// Refuses an unknown root ID.
    pub fn pin_root(&mut self, root: RootId, pinned: bool) -> Result<(), crate::error::CacheError> {
        let record = self.roots.get_mut(&root).ok_or(crate::error::CacheError::InvalidRequest {
            reason: "unknown root",
        })?;
        record.pinned = pinned;
        self.bump();
        Ok(())
    }

    /// Records a missing root. A repeated probe does not restart the seven-day grace clock.
    /// # Errors
    /// Refuses an unknown root ID.
    pub fn note_missing(&mut self, root: RootId) -> Result<(), crate::error::CacheError> {
        self.mark_unavailable(root, false)
    }

    /// Records an offline or permission failure. Grace is shared with [`Self::note_missing`].
    /// # Errors
    /// Refuses an unknown root ID.
    pub fn note_offline(&mut self, root: RootId) -> Result<(), crate::error::CacheError> {
        self.mark_unavailable(root, true)
    }

    /// The root is reachable again. Grace ends.
    /// # Errors
    /// Refuses an unknown root ID.
    pub fn note_online(&mut self, root: RootId) -> Result<(), crate::error::CacheError> {
        let record = self.roots.get_mut(&root).ok_or(crate::error::CacheError::InvalidRequest {
            reason: "unknown root",
        })?;
        record.missing = false;
        record.offline = false;
        record.unavailable_since = None;
        self.bump();
        Ok(())
    }

    fn mark_unavailable(
        &mut self,
        root: RootId,
        offline: bool,
    ) -> Result<(), crate::error::CacheError> {
        let wall = self.wall;
        let record = self.roots.get_mut(&root).ok_or(crate::error::CacheError::InvalidRequest {
            reason: "unknown root",
        })?;
        record.missing = true;
        if offline {
            record.offline = true;
        }
        if record.unavailable_since.is_none() {
            record.unavailable_since = Some(wall);
        }
        self.bump();
        Ok(())
    }

    pub(super) fn bump(&mut self) {
        self.logical_revision = self.logical_revision.saturating_add(1);
    }

    #[allow(clippy::needless_pass_by_value, reason = "Object creation consumes an assembled Spawn record at the ledger mutation boundary.")]
    fn spawn(&mut self, spec: Spawn<'_>) -> Result<ObjectId, crate::error::CacheError> {
        let id = ObjectId::new(self.ids.object);
        self.ids.object = self.ids.object.checked_add(1).ok_or(
            crate::error::CacheError::InvalidRequest { reason: "object id space exhausted" },
        )?;
        self.objects.insert(
            id,
            CacheObject {
                bytes: spec.bytes,
                born: spec.bytes,
                kind: spec.kind,
                state: spec.state,
                roots: spec.roots.to_vec(),
                refs: spec.refs.to_vec(),
                class: spec.class,
                charged_root: spec.charged_root,
                validated: spec.validated,
                complete: spec.complete,
                created: self.wall,
                retired_at: None,
                unreachable_since: None,
                deletion_requested: false,
                corrupt: false,
                reservation: spec.reservation,
                dead_ppm: spec.dead_ppm,
            },
        );
        self.bump();
        Ok(id)
    }

    pub(super) fn remove_object(&mut self, id: ObjectId) {
        if self.objects.remove(&id).is_none() {
            return;
        }
        self.manifests.retain(|_, manifest| manifest.object != id);
        for manifest in self.manifests.values_mut() {
            manifest.refs.retain(|ref_id| *ref_id != id);
        }
        for object in self.objects.values_mut() {
            object.refs.retain(|ref_id| *ref_id != id);
        }
        for writer in self.writers.values_mut() {
            if writer.segment == Some(id) {
                writer.segment = None;
            }
            if writer.manifest_object == Some(id) {
                writer.manifest_object = None;
            }
        }
        self.unlink_failures.remove(&id);
        self.bump();
        self.refresh_freeze();
    }

    pub(super) fn refresh_freeze(&mut self) {
        if self.legacy_frozen
            && self.admitted() <= self.payload_ceiling()
            && self.staged_bytes() == 0
            && self.reserved_bytes() == 0
        {
            self.legacy_frozen = false;
        }
    }

    #[allow(clippy::unused_self, reason = "Keep saturating ledger accounting on Lifecycle beside the quota and reachability computations that use it.")]
    pub(super) fn sum_iter(&self, values: impl Iterator<Item = u64>) -> u64 {
        values.fold(0u64, u64::saturating_add)
    }

    /// Ledger revision. Dry-runs do not move it.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn logical_revision(&self) -> u64 {
        self.logical_revision
    }

    /// How many legacy apply removals have run.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn touches(&self) -> u64 {
        self.touches
    }

    /// Wall time the ledger is using. A rejected backward sample does not change this.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn wall(&self) -> Timestamp {
        self.wall
    }

    /// True after a backward wall sample.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn clock_fault(&self) -> bool {
        self.clock_fault
    }

    /// Boot identity last accepted by reconciliation.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn boot_id(&self) -> u64 {
        self.boot_id
    }

    /// True while a maintenance slice holds the non-blocking lock.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn maintenance_held(&self) -> bool {
        self.maintenance.is_some()
    }

    /// Cursor for the next startup sweep. Zero means the next pass starts at the first object.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn cursor(&self) -> u64 {
        self.cursor_mark
    }

    /// Objects currently in the ledger, in id order.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn object_ids(&self) -> BTreeSet<ObjectId> {
        self.objects.keys().copied().collect()
    }

    /// Wall time of the last auto-clean slice that ran. `None` until the first one.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn last_auto_clean(&self) -> Option<Timestamp> {
        self.last_auto_clean
    }

    pub(super) fn overlay_ttl(&self) -> u64 {
        self.clean_ttl.map_or(quota::OVERLAY_TTL_MS, |ttl| ttl.overlay)
    }

    pub(super) fn preview_ttl(&self) -> u64 {
        self.clean_ttl.map_or(quota::PREVIEW_TTL_MS, |ttl| ttl.preview)
    }

    pub(super) fn root_ttl(&self) -> u64 {
        self.clean_ttl.map_or(quota::ROOT_TTL_MS, |ttl| ttl.root)
    }

    pub(super) fn staging_ttl(&self) -> u64 {
        self.clean_ttl.map_or(quota::STAGING_TTL_MS, |ttl| ttl.staging)
    }

    pub(super) fn debug_check(&self) {
        debug_assert!(self.p4_holds(), "P4 admitted={} cap={}", self.admitted(), self.quota.hard_cap);
        debug_assert!(self.p3_holds(), "P3");
    }
}
