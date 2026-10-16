//! Startup and explicit maintenance. One slice, then the cursor is persisted.
//! Implements CACHE-v1: at most 128 objects and a 25 ms target, checked between objects.
//! Depends on retire and sweep in [`super::Lifecycle`]. The host calls this; nothing here starts a thread.
//! A busy maintenance lock records deferred status and leaves readers on the published generation.

use super::ids::Physical;
use super::quota::{STARTUP_MAX_OBJECTS, STARTUP_TARGET_MILLIS};
use super::{Blocker, Lifecycle, ObjectId, RootId};
use crate::error::CacheError;
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

/// Explicit GC request. Dry-run uses [`Lifecycle::plan_gc`], which does not take `&mut self`.
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

enum Action {
    Retire(super::ManifestId),
    Sweep(ObjectId),
    Tombstone(RootId),
}

impl Lifecycle {
    /// One startup slice when daily work, pressure, quarantine or abandoned staging is due.
    pub fn startup(
        &mut self,
        clock: super::ClockSample,
        evidence: Option<&super::RestartEvidence>,
    ) -> Result<MaintainReport, CacheError> {
        self.startup_with(clock, evidence, SliceBudget::startup())
    }

    /// [`Self::startup`] with a host-supplied per-object cost so the 25 ms bound can be tested.
    pub fn startup_with(
        &mut self,
        clock: super::ClockSample,
        evidence: Option<&super::RestartEvidence>,
        budget: SliceBudget,
    ) -> Result<MaintainReport, CacheError> {
        self.observe(clock);
        if let Some(evidence) = evidence {
            self.reconcile(evidence);
        }
        if !self.cleanup_due() {
            return Ok(self.idle_report(false));
        }
        if self.maintenance.is_some() {
            self.deferred = true;
            self.bump();
            return Ok(MaintainReport {
                examined: 0,
                reclaimed_bytes: 0,
                pending_bytes: self.pending_bytes(),
                deferred: true,
                ran: false,
                completed_pass: false,
                blockers: vec![Blocker::MaintenanceBusy],
            });
        }
        self.maintenance = Some(super::SessionId::new(0));
        let request = GcRequest {
            root: None,
            budget,
            bypass_grace: false,
            max_bytes: None,
            cancel: None,
        };
        let result = self.maintain(request);
        self.maintenance = None;
        result
    }

    /// Explicit GC. Returns [`CacheError::Busy`] when a startup slice holds the lock.
    pub fn gc(&mut self, request: GcRequest<'_>) -> Result<MaintainReport, CacheError> {
        if self.maintenance.is_some() {
            return Err(CacheError::Busy);
        }
        self.maintain(request)
    }

    /// Runs GC on a copy so the caller's cursor, clock and bytes stay put.
    pub fn plan_gc(&self, request: GcRequest<'_>) -> Result<MaintainReport, CacheError> {
        let mut copy = self.clone();
        copy.maintenance = None;
        copy.maintain(request)
    }

    /// Holds the maintenance lock the way another session would. Startup then defers.
    pub fn hold_maintenance(&mut self, session: super::SessionId) -> Result<(), CacheError> {
        if self.maintenance.is_some() {
            return Err(CacheError::Busy);
        }
        self.maintenance = Some(session);
        self.bump();
        Ok(())
    }

    /// Releases a lock held by `session`.
    pub fn release_maintenance(&mut self, session: super::SessionId) -> Result<(), CacheError> {
        match self.maintenance {
            Some(current) if current == session => {
                self.maintenance = None;
                self.deferred = false;
                self.bump();
                Ok(())
            }
            _ => Err(CacheError::InvalidRequest { reason: "maintenance lock is not held by this session" }),
        }
    }

    fn idle_report(&self, deferred: bool) -> MaintainReport {
        MaintainReport {
            examined: 0,
            reclaimed_bytes: 0,
            pending_bytes: self.pending_bytes(),
            deferred,
            ran: false,
            completed_pass: false,
            blockers: Vec::new(),
        }
    }

    pub(super) fn cleanup_due(&self) -> bool {
        self.pass_incomplete
            || self.daily_due()
            || self.admitted() >= self.quota.soft_watermark
            || self.has_abandoned_staging()
            || self.incompatible
    }

    fn daily_due(&self) -> bool {
        match self.last_maintain {
            None => true,
            Some(stamp) => self.wall.0.saturating_sub(stamp.0) >= super::quota::DAY_MS,
        }
    }

    fn has_abandoned_staging(&self) -> bool {
        self.objects.keys().copied().any(|id| {
            self.objects.get(&id).is_some_and(|object| object.state == Physical::Staged)
                && self.unowned(id)
        })
    }

    fn maintain(&mut self, request: GcRequest<'_>) -> Result<MaintainReport, CacheError> {
        if !self.pass_incomplete {
            self.cursor_mark = 0;
        }
        self.mark_expired_roots();
        let mut examined = 0u32;
        let mut reclaimed = 0u64;
        loop {
            if examined >= request.budget.max_objects {
                break;
            }
            let elapsed = u64::from(examined).saturating_mul(request.budget.millis_per_object);
            if examined > 0 && elapsed >= request.budget.max_millis {
                break;
            }
            if let Some(max) = request.max_bytes
                && reclaimed >= max
            {
                break;
            }
            if let Some(cancel) = request.cancel
                && cancel.checkpoint().is_err()
            {
                self.debug_check();
                return Err(CacheError::Cancelled);
            }
            let Some(action) = self.next_action(request.root, request.bypass_grace) else {
                break;
            };
            reclaimed = reclaimed.saturating_add(self.apply_action(action, request.bypass_grace));
            examined = examined.saturating_add(1);
        }
        let more = self.next_action(request.root, request.bypass_grace).is_some();
        if more {
            self.pass_incomplete = true;
        } else {
            self.pass_incomplete = false;
            self.cursor_mark = 0;
            self.last_maintain = Some(self.wall);
        }
        self.bump();
        self.debug_check();
        Ok(MaintainReport {
            examined,
            reclaimed_bytes: reclaimed,
            pending_bytes: self.pending_bytes(),
            deferred: false,
            ran: true,
            completed_pass: !more,
            blockers: self.collect_blockers(),
        })
    }

    fn next_action(&self, only: Option<RootId>, bypass: bool) -> Option<Action> {
        if let Some(id) = self.next_retire_candidate(only) {
            return Some(Action::Retire(id));
        }
        if let Some(id) = self.next_sweep(only, bypass) {
            return Some(Action::Sweep(id));
        }
        self.next_tombstone(only).map(Action::Tombstone)
    }

    fn next_sweep(&self, only: Option<RootId>, bypass: bool) -> Option<ObjectId> {
        self.objects.keys().copied().find(|id| {
            id.raw() >= self.cursor_mark && self.in_scope(*id, only) && self.sweep_ready(*id, bypass)
        })
    }

    fn in_scope(&self, id: ObjectId, only: Option<RootId>) -> bool {
        let Some(root) = only else {
            return true;
        };
        self.objects.get(&id).is_some_and(|object| object.roots.is_empty() || object.roots.contains(&root))
    }

    fn sweep_ready(&self, id: ObjectId, bypass: bool) -> bool {
        if self.live_objects(true).contains(&id) || self.unlink_failures.contains(&id) {
            return false;
        }
        let Some(object) = self.objects.get(&id) else {
            return false;
        };
        if object.corrupt {
            return false;
        }
        match object.state {
            Physical::PendingUnlink => true,
            Physical::Staged => self.staged_reclaimable(id),
            Physical::Published | Physical::Retired => {
                if !object.complete {
                    return false;
                }
                if self.grace_due(object, bypass) {
                    return true;
                }
                object.retired_at.is_none() && object.unreachable_since.is_none()
            }
        }
    }

    fn apply_action(&mut self, action: Action, bypass: bool) -> u64 {
        match action {
            Action::Retire(id) => {
                self.retire_manifest(id);
                0
            }
            Action::Sweep(id) => self.apply_sweep(id, bypass),
            Action::Tombstone(root) => {
                self.roots.remove(&root);
                self.history_reset.remove(&root);
                self.bump();
                0
            }
        }
    }

    fn apply_sweep(&mut self, id: ObjectId, bypass: bool) -> u64 {
        let live = self.live_objects(true);
        if live.contains(&id) || self.unlink_failures.contains(&id) {
            self.cursor_mark = id.raw().saturating_add(1);
            return 0;
        }
        let Some(object) = self.objects.get(&id) else {
            self.cursor_mark = id.raw().saturating_add(1);
            return 0;
        };
        let state = object.state;
        let grace = self.grace_due(object, bypass);
        let observed = object.unreachable_since.is_some() || object.retired_at.is_some();
        match state {
            Physical::PendingUnlink | Physical::Staged => {
                let bytes = self.objects.get(&id).map(|object| object.bytes).unwrap_or(0);
                self.remove_object(id);
                self.cursor_mark = id.raw().saturating_add(1);
                bytes
            }
            Physical::Published | Physical::Retired if grace => {
                if let Some(object) = self.objects.get_mut(&id) {
                    object.state = Physical::PendingUnlink;
                }
                self.cursor_mark = id.raw().saturating_add(1);
                self.bump();
                0
            }
            Physical::Published | Physical::Retired if !observed => {
                if let Some(object) = self.objects.get_mut(&id) {
                    object.unreachable_since = Some(self.wall);
                }
                self.cursor_mark = id.raw().saturating_add(1);
                self.bump();
                0
            }
            Physical::Published | Physical::Retired => {
                self.cursor_mark = id.raw().saturating_add(1);
                0
            }
        }
    }

    fn next_tombstone(&self, only: Option<RootId>) -> Option<RootId> {
        self.roots.keys().copied().find(|id| {
            only.is_none_or(|root| root == *id)
                && self.roots.get(id).is_some_and(|record| record.expired)
                && !self.manifests.values().any(|manifest| manifest.root == *id)
                && !self.objects.values().any(|object| object.roots.contains(id))
                && !self.root_has_open_lease(*id)
        })
    }

    pub(super) fn collect_blockers(&self) -> Vec<Blocker> {
        let mut blockers = Vec::new();
        for manifest in self.manifests.values() {
            if !self.is_published(manifest) || self.is_current(manifest) {
                continue;
            }
            if manifest.pinned || self.root_is_pinned(manifest.root) {
                blockers.push(Blocker::Pinned);
            }
            if self.lease_holds(manifest.generation) {
                blockers.push(Blocker::ActiveLease);
            }
            if self.unavailable_protected(manifest.root) {
                blockers.push(Blocker::UnavailableGrace);
            }
        }
        if self.unlink_failures.iter().any(|id| self.objects.contains_key(id)) {
            blockers.push(Blocker::FailedUnlink);
        }
        let live = self.live_objects(true);
        if self.objects.iter().any(|(id, object)| {
            !object.corrupt
                && matches!(object.state, Physical::Published | Physical::Retired)
                && (object.unreachable_since.is_some() || object.retired_at.is_some())
                && !self.grace_due(object, false)
                && !live.contains(id)
        }) {
            blockers.push(Blocker::UnlinkGrace);
        }
        blockers.sort_unstable();
        blockers.dedup();
        blockers
    }
}
