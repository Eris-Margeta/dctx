//! Global reservation coordinator.
//! Implements CACHE-v1 `admitted = C + Sg + R + P` and `admit(n)` only when that sum stays ≤ H.
//! Depends on [`super::Lifecycle`] buckets. Publication, compaction and the `Cache` port all reserve here.
//! A pre-existing oversize ledger admits nothing until reclamation brings it back under the cap.

use super::ids::{ChargeClass, ObjectKind, Physical, Purpose};
use super::quota::UNLINK_GRACE_MS;
use super::{CacheObject, Hold, Lifecycle, Spawn};
use crate::error::CacheError;
use dctx_core::budget::Reservation;

impl Lifecycle {
    /// `C + Sg + R + P`. Shared facts are stored once, so they contribute once.
    pub fn admitted(&self) -> u64 {
        self.stored_bytes().saturating_add(self.reserved_bytes())
    }

    pub(super) fn stored_bytes(&self) -> u64 {
        self.sum_iter(self.objects.values().map(|object| object.bytes))
    }

    pub(super) fn reserved_bytes(&self) -> u64 {
        self.sum_iter(self.reservations.values().map(|hold| hold.left))
    }

    pub(super) fn staged_bytes(&self) -> u64 {
        self.bytes_in(&[Physical::Staged])
    }

    pub(super) fn pending_bytes(&self) -> u64 {
        self.bytes_in(&[Physical::PendingUnlink])
    }

    pub(super) fn committed_bytes(&self) -> u64 {
        self.bytes_in(&[Physical::Published, Physical::Retired])
    }

    fn bytes_in(&self, states: &[Physical]) -> u64 {
        self.sum_iter(
            self.objects.values().filter(|object| states.contains(&object.state)).map(|object| object.bytes),
        )
    }

    pub(super) fn payload_ceiling(&self) -> u64 {
        self.quota.hard_cap.saturating_sub(self.quota.emergency_metadata)
    }

    /// P4: under the cap, or frozen with no staged growth and no new reservation.
    pub fn p4_holds(&self) -> bool {
        let admitted = self.admitted();
        if self.legacy_frozen {
            admitted <= self.legacy_ceiling && self.staged_bytes() == 0 && self.reserved_bytes() == 0
        } else {
            admitted <= self.payload_ceiling() && admitted <= self.quota.hard_cap
        }
    }

    pub(super) fn admission_allows(&self, bytes: u64) -> bool {
        if self.legacy_frozen {
            return false;
        }
        self.admitted().checked_add(bytes).is_some_and(|sum| sum <= self.payload_ceiling())
    }

    pub(super) fn class_charge(&self, root: super::RootId, class: ChargeClass) -> u64 {
        let objects = self.sum_iter(
            self.objects
                .values()
                .filter(|object| object.charged_root == Some(root) && object.class == class)
                .map(|object| object.bytes),
        );
        let holds = self.sum_iter(
            self.reservations
                .values()
                .filter(|hold| hold.root == Some(root) && hold.class == class)
                .map(|hold| hold.left),
        );
        objects.saturating_add(holds)
    }

    pub(super) fn ceiling_for(&self, class: ChargeClass) -> u64 {
        match class {
            ChargeClass::View => self.quota.root_view_ceiling,
            ChargeClass::Pack => self.quota.root_pack_ceiling,
            ChargeClass::Preview => self.quota.root_preview_ceiling,
            ChargeClass::Global => u64::MAX,
        }
    }

    pub(super) fn ceiling_allows(&self, root: super::RootId, class: ChargeClass, bytes: u64) -> bool {
        if matches!(class, ChargeClass::Global) {
            return true;
        }
        self.class_charge(root, class)
            .checked_add(bytes)
            .is_some_and(|sum| sum <= self.ceiling_for(class))
    }

    pub(super) fn limit(&self, requested: u64) -> CacheError {
        CacheError::CacheLimit {
            requested,
            available: if self.legacy_frozen {
                0
            } else {
                self.payload_ceiling().saturating_sub(self.admitted())
            },
            pinned: self.pinned_bytes(),
            pending: self.pending_bytes(),
        }
    }

    /// Admits `bytes` against the global cap and, when `root` is set, that root's ceiling.
    pub fn reserve_for(
        &mut self,
        root: Option<super::RootId>,
        purpose: Purpose,
        bytes: u64,
    ) -> Result<Reservation, CacheError> {
        if bytes == 0 {
            return Err(CacheError::InvalidRequest { reason: "reservation is empty" });
        }
        if root.is_some_and(|id| !self.roots.contains_key(&id)) {
            return Err(CacheError::InvalidRequest { reason: "unknown root" });
        }
        self.reclaim_safe();
        if !self.admission_allows(bytes) {
            return Err(self.limit(bytes));
        }
        let class = purpose.class();
        if let Some(root_id) = root
            && !self.ceiling_allows(root_id, class, bytes)
        {
            return Err(self.limit(bytes));
        }
        let id = self.ids.reservation;
        self.ids.reservation = self.ids.reservation.checked_add(1).ok_or(CacheError::InvalidRequest {
            reason: "reservation id space exhausted",
        })?;
        self.reservations.insert(
            id,
            Hold { left: bytes, original: bytes, root, class, purpose },
        );
        self.bump();
        self.debug_check();
        Ok(Reservation::new(id, bytes))
    }

    /// Settles a rootless or rooted reservation. `used` bytes become a committed object.
    pub fn commit_reservation(
        &mut self,
        reservation: Reservation,
        used: u64,
    ) -> Result<(), CacheError> {
        let Some(hold) = self.reservations.get(&reservation.id()) else {
            return Err(CacheError::InvalidRequest { reason: "unknown reservation" });
        };
        if used > hold.left || used > hold.original {
            return Err(CacheError::InvalidRequest { reason: "commit exceeds reservation" });
        }
        let class = hold.class;
        let root = hold.root;
        let purpose = hold.purpose;
        self.reservations.remove(&reservation.id());
        if used > 0 {
            let roots: &[super::RootId] = &[];
            let charged_root = if matches!(class, ChargeClass::Global) { None } else { root };
            let kind = match purpose {
                Purpose::Fact => ObjectKind::Fact,
                Purpose::Segment => ObjectKind::Segment,
                Purpose::Staging => ObjectKind::Staging,
                Purpose::View | Purpose::Overlay | Purpose::Pack | Purpose::Preview => {
                    ObjectKind::Manifest
                }
                Purpose::Payload => ObjectKind::Fact,
            };
            self.spawn(Spawn {
                bytes: used,
                kind,
                state: Physical::Published,
                roots,
                refs: &[],
                class,
                charged_root,
                validated: true,
                complete: true,
                reservation: None,
                dead_ppm: 0,
            })?;
        }
        self.bump();
        self.debug_check();
        Ok(())
    }

    /// Drops the reservation and any staged objects still tied to it.
    pub fn abort_reservation(&mut self, reservation: Reservation) {
        let id = reservation.id();
        let staged: Vec<super::ObjectId> = self
            .objects
            .iter()
            .filter(|(_, object)| object.reservation == Some(id) && object.state == Physical::Staged)
            .map(|(id, _)| *id)
            .collect();
        for object_id in staged {
            self.remove_object(object_id);
        }
        if self.reservations.remove(&id).is_some() {
            self.bump();
        }
        self.debug_check();
    }

    pub(super) fn release_hold(&mut self, id: Option<u64>) {
        if let Some(id) = id {
            self.reservations.remove(&id);
        }
    }

    /// Frees pending and grace-elapsed garbage before a new admission. Does not retire manifests.
    pub(super) fn reclaim_safe(&mut self) {
        for _ in 0..super::quota::STARTUP_MAX_OBJECTS {
            let Some(id) = self.objects.keys().copied().find(|id| self.immediately_reclaimable(*id)) else {
                break;
            };
            self.remove_object(id);
        }
    }

    fn immediately_reclaimable(&self, id: super::ObjectId) -> bool {
        if self.unlink_failures.contains(&id) || self.live_objects(true).contains(&id) {
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
            Physical::Published | Physical::Retired => self.grace_due(object, false),
        }
    }

    pub(super) fn grace_due(&self, object: &CacheObject, bypass: bool) -> bool {
        if bypass && object.complete && !object.corrupt {
            return true;
        }
        let since = object.retired_at.or(object.unreachable_since);
        since.is_some_and(|stamp| self.wall.0.saturating_sub(stamp.0) >= UNLINK_GRACE_MS)
    }

    pub(super) fn staged_reclaimable(&self, id: super::ObjectId) -> bool {
        let Some(object) = self.objects.get(&id) else {
            return false;
        };
        if object.state != Physical::Staged || !self.unowned(id) {
            return false;
        }
        let age = self.wall.0.saturating_sub(object.created.0);
        age >= self.staging_ttl() || self.boot_accelerates
    }

    pub(super) fn unowned(&self, id: super::ObjectId) -> bool {
        !self.writer_owns(id) && !self.live_objects(true).contains(&id)
    }

    /// Records bytes that were already on disk above the cap. Later admissions freeze.
    pub fn observe_legacy_wal(&mut self, bytes: u64) -> Result<super::ObjectId, CacheError> {
        if bytes == 0 {
            return Err(CacheError::InvalidRequest { reason: "legacy record is empty" });
        }
        if self.legacy_frozen {
            return Err(self.limit(bytes));
        }
        if self.staged_bytes() != 0 || self.reserved_bytes() != 0 {
            return Err(CacheError::InvalidRequest {
                reason: "legacy oversize is reconciled before new staging",
            });
        }
        let id = self.spawn(Spawn {
            bytes,
            kind: ObjectKind::LegacyWal,
            state: Physical::Published,
            roots: &[],
            refs: &[],
            class: ChargeClass::Global,
            charged_root: None,
            validated: true,
            complete: true,
            reservation: None,
            dead_ppm: 0,
        })?;
        if self.admitted() > self.payload_ceiling() {
            self.legacy_frozen = true;
            self.legacy_ceiling = self.admitted();
        }
        self.debug_check();
        Ok(id)
    }

    /// Drops one observed legacy wal object. Freeze lifts once the ledger fits again.
    pub fn reclaim_legacy(&mut self, id: super::ObjectId) -> Result<u64, CacheError> {
        let Some(object) = self.objects.get(&id) else {
            return Err(CacheError::InvalidRequest { reason: "unknown object" });
        };
        if object.kind != ObjectKind::LegacyWal {
            return Err(CacheError::InvalidRequest { reason: "object is not legacy wal" });
        }
        if self.live_objects(true).contains(&id) {
            return Err(CacheError::InvalidRequest { reason: "legacy object is still retained" });
        }
        let bytes = object.bytes;
        self.remove_object(id);
        self.debug_check();
        Ok(bytes)
    }

    /// Lowers the hard cap. Growth freezes when committed bytes already sit above it.
    pub fn set_hard_cap(&mut self, hard: u64) -> Result<(), CacheError> {
        if hard < self.quota.emergency_metadata {
            return Err(CacheError::InvalidRequest {
                reason: "hard cap is smaller than the metadata reserve",
            });
        }
        if self.staged_bytes() != 0 || self.reserved_bytes() != 0 {
            return Err(CacheError::InvalidRequest { reason: "cap changes wait until staging settles" });
        }
        if hard < self.quota.eviction_target {
            self.quota.eviction_target = hard;
            self.quota.soft_watermark = hard;
        } else if hard < self.quota.soft_watermark {
            self.quota.soft_watermark = hard;
        }
        self.quota.hard_cap = hard;
        if self.admitted() > self.payload_ceiling() {
            self.legacy_frozen = true;
            self.legacy_ceiling = self.admitted();
        }
        self.bump();
        self.debug_check();
        Ok(())
    }

    /// The next materialising write fails and leaves admission unchanged.
    pub fn inject_disk_full(&mut self, full: bool) {
        self.disk_full = full;
    }

    /// A pending unlink of `id` stays pending until the fault is cleared.
    pub fn fail_unlink(&mut self, id: super::ObjectId) {
        self.unlink_failures.insert(id);
    }

    /// Allows a later sweep to finish the unlink.
    pub fn clear_unlink_fault(&mut self, id: super::ObjectId) {
        self.unlink_failures.remove(&id);
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::{Lifecycle, Purpose, Quota};

    #[test]
    fn commit_settles_once_and_unknown_reservations_fail() {
        let mut life = Lifecycle::new(Quota::scaled(10)).unwrap();
        let reservation = life.reserve_for(None, Purpose::Payload, 6).unwrap();
        assert!(life.reserve_for(None, Purpose::Payload, 5).is_err());
        life.commit_reservation(reservation, 4).unwrap();
        assert_eq!(life.admitted(), 4);
        assert!(life.commit_reservation(dctx_core::budget::Reservation::new(1, 6), 1).is_err());
        let again = life.reserve_for(None, Purpose::Payload, 6).unwrap();
        life.abort_reservation(again);
        assert_eq!(life.reserved_bytes(), 0);
        assert!(life.p4_holds());
    }

    #[test]
    fn dropping_reservation_from_the_sum_would_over_admit() {
        let mut life = Lifecycle::new(Quota::scaled(10)).unwrap();
        let _hold = life.reserve_for(None, Purpose::Payload, 8).unwrap();
        let without_reservations = life.committed_bytes() + life.staged_bytes() + life.pending_bytes();
        assert!(without_reservations + 4 <= 10);
        assert!(life.reserve_for(None, Purpose::Payload, 4).is_err());
        assert!(life.p4_holds());
    }
}
