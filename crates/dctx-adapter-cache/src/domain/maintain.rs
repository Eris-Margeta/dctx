// SPDX-License-Identifier: Apache-2.0
//! Startup and explicit maintenance. One slice, then the cursor is persisted.
//! Implements CACHE-v1: at most 128 objects and a 25 ms target, checked between objects.
//! Depends on retire and sweep in [`super::Lifecycle`]. The host calls this; nothing here starts a thread.
//! [`Lifecycle::auto_clean`] is the same bound, gated by [`super::AutoCleanPolicy`], still with no thread.

use super::{Blocker, Lifecycle};
use crate::error::CacheError;

pub use super::maintain_types::{GcRequest, MaintainReport, SliceBudget};

impl Lifecycle {
    /// One startup slice when daily work, pressure, quarantine or abandoned staging is due.
    /// # Errors
    /// Propagates startup maintenance planning/execution failures.
    pub fn startup(
        &mut self,
        clock: super::ClockSample,
        evidence: Option<&super::RestartEvidence>,
    ) -> Result<MaintainReport, CacheError> {
        self.startup_with(clock, evidence, SliceBudget::startup())
    }

    /// [`Self::startup`] with a host-supplied per-object cost so the 25 ms bound can be tested.
    /// # Errors
    /// Propagates maintenance planning/execution failures after reconciling process evidence.
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
    /// # Errors
    /// Refuses concurrent maintenance or an invalid/unexecutable collection request.
    pub fn gc(&mut self, request: GcRequest<'_>) -> Result<MaintainReport, CacheError> {
        if self.maintenance.is_some() {
            return Err(CacheError::Busy);
        }
        self.maintain(request)
    }

    /// Runs GC on a copy so the caller's cursor, clock and bytes stay put.
    /// # Errors
    /// Propagates collection planning failures from the simulated ledger.
    pub fn plan_gc(&self, request: GcRequest<'_>) -> Result<MaintainReport, CacheError> {
        let mut copy = self.clone();
        copy.maintenance = None;
        copy.maintain(request)
    }

    /// Holds the maintenance lock the way another session would. Startup then defers.
    /// # Errors
    /// Returns `CacheError::Busy` if maintenance is already held.
    pub fn hold_maintenance(&mut self, session: super::SessionId) -> Result<(), CacheError> {
        if self.maintenance.is_some() {
            return Err(CacheError::Busy);
        }
        self.maintenance = Some(session);
        self.bump();
        Ok(())
    }

    /// One bounded cleanup when `policy` is enabled and the interval has elapsed.
    /// A disabled policy does not touch the ledger. A system cap above the current hard cap is not applied.
    /// # Errors
    /// Refuses an invalid policy/cap transition or propagates maintenance planning/execution failures.
    pub fn auto_clean(
        &mut self,
        clock: super::ClockSample,
        policy: super::AutoCleanPolicy,
    ) -> Result<MaintainReport, CacheError> {
        policy.check()?;
        if !policy.enabled || !self.auto_clean_due(clock, policy.min_interval_ms) {
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
        self.tighten_cap(policy.system_cap_bytes)?;
        self.observe(clock);
        self.clean_ttl = Some(super::CleanTtl {
            overlay: policy.overlay_ttl_ms,
            preview: policy.preview_ttl_ms,
            root: policy.root_ttl_ms,
            staging: policy.staging_ttl_ms,
        });
        self.maintenance = Some(super::SessionId::new(0));
        let result = self.maintain(GcRequest {
            root: None,
            budget: SliceBudget::startup(),
            bypass_grace: false,
            max_bytes: None,
            cancel: None,
        });
        self.maintenance = None;
        self.clean_ttl = None;
        let report = result?;
        self.last_auto_clean = Some(self.wall);
        self.tighten_cap(policy.system_cap_bytes)?;
        Ok(report)
    }

    fn auto_clean_due(&self, clock: super::ClockSample, interval: u64) -> bool {
        match self.last_auto_clean {
            None => true,
            Some(stamp) => clock.wall.0.saturating_sub(stamp.0) >= interval,
        }
    }

    /// Lowers the hard cap. A looser cap, or one that must wait for staging, is left for later.
    fn tighten_cap(&mut self, cap: u64) -> Result<(), CacheError> {
        if cap >= self.quota.hard_cap || self.staged_bytes() != 0 || self.reserved_bytes() != 0 {
            return Ok(());
        }
        self.set_hard_cap(cap)
    }

    /// Releases a lock held by `session`.
    /// # Errors
    /// Refuses a session that does not hold the maintenance lock.
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
}

#[cfg(test)]
mod tests {
    use super::SliceBudget;
    use crate::{
        AutoCleanPolicy, FakeClock, Lifecycle, Physical, Quota, RootId, SessionId, HARD_CAP_BYTES,
        STARTUP_MAX_OBJECTS, STARTUP_TARGET_MILLIS,
    };

    fn policy() -> AutoCleanPolicy {
        let mut policy = AutoCleanPolicy::product_default();
        policy.min_interval_ms = 1;
        policy
    }

    fn state(life: &Lifecycle, root: RootId, generation: crate::GenerationId) -> Option<Physical> {
        life.manifest_of(root, generation).and_then(|id| life.manifest_state(id))
    }

    #[test]
    fn disabled_and_illegal_cap_do_not_touch_the_ledger() {
        let mut life = Lifecycle::new(Quota::scaled(80)).expect("quota");
        let root = RootId::new(1);
        life.register_root(root).expect("root");
        let staged = life.load_staged(8, 0).expect("staged");
        let revision = life.logical_revision();
        let mut disabled = policy();
        disabled.enabled = false;
        disabled.staging_ttl_ms = 1;
        let report = life.auto_clean(FakeClock::at(10_000).sample(), disabled).expect("clean");
        assert!(!report.ran);
        assert_eq!(life.logical_revision(), revision);
        assert_eq!(life.object_state(staged), Some(Physical::Staged));
        assert!(life.last_auto_clean().is_none());
        disabled.system_cap_bytes = HARD_CAP_BYTES + 1;
        assert!(life.auto_clean(FakeClock::at(10_000).sample(), disabled).is_err());
        assert_eq!(life.logical_revision(), revision);
        let legal = AutoCleanPolicy::product_default();
        assert!(legal.check().is_ok());
    }

    #[test]
    fn interval_gates_a_second_slice_until_the_gap_elapses() {
        let mut life = Lifecycle::new(Quota::scaled(100)).expect("quota");
        let mut policy = policy();
        policy.min_interval_ms = 5_000;
        policy.system_cap_bytes = 100;
        let mut clock = FakeClock::at(0);
        let first = life.auto_clean(clock.sample(), policy).expect("clean");
        assert!(first.ran);
        assert_eq!(life.last_auto_clean().expect("stamp").0, 0);
        let again = life.auto_clean(clock.sample(), policy).expect("clean");
        assert!(!again.ran);
        clock.advance(4_999);
        assert!(!life.auto_clean(clock.sample(), policy).expect("clean").ran);
        clock.advance(1);
        assert!(life.auto_clean(clock.sample(), policy).expect("clean").ran);
    }

    #[test]
    fn system_cap_only_tightens_and_ttl_overrides_expire_idle_manifests() {
        let mut life = Lifecycle::new(Quota::scaled(100)).expect("quota");
        let mut loose = policy();
        loose.min_interval_ms = 0;
        loose.system_cap_bytes = HARD_CAP_BYTES;
        life.auto_clean(FakeClock::at(0).sample(), loose).expect("clean");
        assert_eq!(life.status().hard_cap, 100);
        loose.system_cap_bytes = 40;
        life.auto_clean(FakeClock::at(1).sample(), loose).expect("clean");
        assert_eq!(life.status().hard_cap, 40);
        loose.system_cap_bytes = 80;
        life.auto_clean(FakeClock::at(2).sample(), loose).expect("clean");
        assert_eq!(life.status().hard_cap, 40);

        let mut life = Lifecycle::new(Quota::scaled(10_000)).expect("quota");
        let root = RootId::new(1);
        life.register_root(root).expect("root");
        let overlay = life.publish_overlay(root, 8, &[]).expect("overlay");
        let preview = life.publish_preview(root, 4, &[]).expect("preview");
        let mut ttl = policy();
        ttl.overlay_ttl_ms = 1_000;
        ttl.preview_ttl_ms = 1_000;
        life.auto_clean(FakeClock::at(999).sample(), ttl).expect("early");
        assert_eq!(state(&life, root, overlay), Some(Physical::Published));
        assert_eq!(state(&life, root, preview), Some(Physical::Published));
        assert!(!life.root_expired(root));
        life.auto_clean(FakeClock::at(1_000).sample(), ttl).expect("due");
        assert_eq!(state(&life, root, overlay), Some(Physical::Retired));
        assert_eq!(state(&life, root, preview), Some(Physical::Retired));
        assert!(!life.root_expired(root));
    }

    #[test]
    fn staging_override_reclaims_at_the_policy_age_and_the_slice_stops_at_128() {
        let mut life = Lifecycle::new(Quota::scaled(100)).expect("quota");
        let staged = life.load_staged(10, 0).expect("staged");
        let mut policy = policy();
        policy.min_interval_ms = 0;
        policy.staging_ttl_ms = 5;
        policy.system_cap_bytes = 100;
        life.auto_clean(FakeClock::at(4).sample(), policy).expect("early");
        assert_eq!(life.object_state(staged), Some(Physical::Staged));
        life.auto_clean(FakeClock::at(5).sample(), policy).expect("due");
        assert_eq!(life.object_state(staged), None);

        let mut life = Lifecycle::new(Quota::scaled(4_000)).expect("quota");
        let root = RootId::new(1);
        life.register_root(root).expect("root");
        for _ in 0..129 {
            life.insert_fact(8, &[root]).expect("fact");
        }
        let admitted = life.admitted();
        let report = life.auto_clean(FakeClock::at(0).sample(), AutoCleanPolicy::product_default()).expect("clean");
        assert!(report.ran);
        assert_eq!(report.examined, STARTUP_MAX_OBJECTS);
        assert_eq!(SliceBudget::startup().max_objects, STARTUP_MAX_OBJECTS);
        assert_eq!(SliceBudget::startup().max_millis, STARTUP_TARGET_MILLIS);
        assert_eq!(life.admitted(), admitted);
        assert_eq!(life.object_ids().len(), 129);
    }

    #[test]
    fn a_held_maintenance_lock_defers_auto_clean() {
        let mut life = Lifecycle::new(Quota::scaled(50)).expect("quota");
        life.hold_maintenance(SessionId::new(3)).expect("hold");
        let report = life.auto_clean(FakeClock::at(0).sample(), policy()).expect("clean");
        assert!(report.deferred);
        assert!(!report.ran);
        assert!(life.last_auto_clean().is_none());
    }
}
