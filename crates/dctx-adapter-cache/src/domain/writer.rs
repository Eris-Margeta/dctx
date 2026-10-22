//! One writer per root: reserve, stage, validate, swap, crash, recover.
//! Implements the DX-A2 publication steps. The swap is the only visibility point.
//! Depends on [`super::Lifecycle`] reservations and objects. Restart reconciliation calls recover.
//! Recovery keeps a swapped pointer and drops every unpublished temp. It never reads a process id.

use super::ids::{GenerationId, ManifestKind, ObjectKind, Physical, SessionId};
use super::{Lifecycle, Manifest, Spawn};
use crate::error::CacheError;
use dctx_core::budget::Reservation;

/// Lock as observed by recovery. `Dead` means the holder crashed; the OS lock decides if it is gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriterPhase {
    /// No writer.
    Free,
    /// A live session holds the lock.
    Held,
    /// The session crashed. Recovery runs only when the OS lock is gone.
    Dead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WriterLock {
    Free,
    Held,
    Dead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WriterStep {
    Idle,
    Held,
    Reserved,
    Staged,
    SegmentValid,
    Manifest,
    Ready,
    Swapped,
    #[allow(dead_code)]
    Refused,
}

#[derive(Debug, Clone)]
pub(super) struct Writer {
    pub lock: WriterLock,
    pub step: WriterStep,
    pub session: Option<SessionId>,
    pub reservation: Option<u64>,
    pub segment: Option<super::ObjectId>,
    pub manifest: Option<super::ManifestId>,
    pub manifest_object: Option<super::ObjectId>,
    pub generation: Option<GenerationId>,
}

impl Writer {
    pub(super) const fn fresh() -> Self {
        Self {
            lock: WriterLock::Free,
            step: WriterStep::Idle,
            session: None,
            reservation: None,
            segment: None,
            manifest: None,
            manifest_object: None,
            generation: None,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct Pointer {
    pub current: Option<GenerationId>,
    pub previous: Option<GenerationId>,
}

impl Lifecycle {
    /// Takes the per-root writer lock.
    /// # Errors
    /// Refuses an unknown root or an already held writer.
    pub fn writer_acquire(
        &mut self,
        root: super::RootId,
        session: SessionId,
    ) -> Result<(), CacheError> {
        if !self.roots.contains_key(&root) {
            return Err(CacheError::InvalidRequest { reason: "unknown root" });
        }
        let writer = self.writers.entry(root).or_insert_with(Writer::fresh);
        if writer.lock != WriterLock::Free || !matches!(writer.step, WriterStep::Idle | WriterStep::Refused)
        {
            return Err(CacheError::Busy);
        }
        writer.lock = WriterLock::Held;
        writer.step = WriterStep::Held;
        writer.session = Some(session);
        self.bump();
        Ok(())
    }

    /// Reserves the generation's byte budget. Refusal releases the lock and admits nothing.
    /// # Errors
    /// Refuses the wrong writer phase, disk-full state, quota exhaustion, or invalid reservation allocation.
    pub fn writer_reserve(
        &mut self,
        root: super::RootId,
        bytes: u64,
    ) -> Result<Reservation, CacheError> {
        self.require_step(root, WriterStep::Held)?;
        if self.disk_full {
            self.fail_writer(root);
            return Err(CacheError::DiskFull);
        }
        match self.reserve_for(Some(root), super::Purpose::View, bytes) {
            Ok(reservation) => {
                if let Some(writer) = self.writers.get_mut(&root) {
                    writer.step = WriterStep::Reserved;
                    writer.reservation = Some(reservation.id());
                }
                Ok(reservation)
            }
            Err(error) => {
                self.fail_writer(root);
                Err(error)
            }
        }
    }

    /// Materialises a temp segment from the outstanding reservation.
    /// # Errors
    /// Refuses the wrong writer phase, bytes beyond the reservation, or exhausted object IDs.
    pub fn writer_stage_segment(
        &mut self,
        root: super::RootId,
        bytes: u64,
    ) -> Result<super::ObjectId, CacheError> {
        self.require_step(root, WriterStep::Reserved)?;
        let id = self.materialize(root, bytes, ObjectKind::Segment, &[])?;
        if let Some(writer) = self.writers.get_mut(&root) {
            writer.step = WriterStep::Staged;
            writer.segment = Some(id);
        }
        Ok(id)
    }

    /// Marks the temp segment complete. The pointer does not move.
    /// # Errors
    /// Refuses the wrong writer phase or a missing staged segment.
    pub fn writer_validate_segment(&mut self, root: super::RootId) -> Result<(), CacheError> {
        self.require_step(root, WriterStep::Staged)?;
        let Some(segment) = self.writers.get(&root).and_then(|writer| writer.segment) else {
            return Err(CacheError::InvalidRequest { reason: "writer has no segment" });
        };
        let Some(object) = self.objects.get_mut(&segment) else {
            return Err(CacheError::CorruptIndex { reason: "staged segment missing" });
        };
        object.validated = true;
        object.complete = true;
        if let Some(writer) = self.writers.get_mut(&root) {
            writer.step = WriterStep::SegmentValid;
        }
        self.bump();
        Ok(())
    }

    /// Materialises the temp manifest. It stays invisible until swap.
    /// # Errors
    /// Refuses the wrong writer phase, invalid references, bytes beyond the reservation, or exhausted publication IDs.
    pub fn writer_stage_manifest(
        &mut self,
        root: super::RootId,
        bytes: u64,
        refs: &[super::ObjectId],
    ) -> Result<super::ManifestId, CacheError> {
        self.require_step(root, WriterStep::SegmentValid)?;
        self.ensure_refs(refs)?;
        let generation = self.alloc_generation()?;
        let object_id = self.materialize(root, bytes, ObjectKind::Manifest, refs)?;
        let manifest_id = self.alloc_manifest()?;
        self.manifests.insert(
            manifest_id,
            Manifest {
                id: manifest_id,
                root,
                generation,
                kind: ManifestKind::View,
                object: object_id,
                last_use: self.wall,
                pinned: false,
                available: true,
                refs: refs.to_vec(),
            },
        );
        if let Some(writer) = self.writers.get_mut(&root) {
            writer.step = WriterStep::Manifest;
            writer.manifest = Some(manifest_id);
            writer.manifest_object = Some(object_id);
            writer.generation = Some(generation);
        }
        self.bump();
        Ok(manifest_id)
    }

    /// Marks the temp manifest complete. Swap is still required before readers can observe it.
    /// # Errors
    /// Refuses the wrong writer phase or a missing staged manifest.
    pub fn writer_validate_manifest(&mut self, root: super::RootId) -> Result<(), CacheError> {
        self.require_step(root, WriterStep::Manifest)?;
        let Some(object_id) = self.writers.get(&root).and_then(|writer| writer.manifest_object) else {
            return Err(CacheError::InvalidRequest { reason: "writer has no manifest" });
        };
        let Some(object) = self.objects.get_mut(&object_id) else {
            return Err(CacheError::CorruptIndex { reason: "staged manifest missing" });
        };
        object.validated = true;
        object.complete = true;
        if let Some(writer) = self.writers.get_mut(&root) {
            writer.step = WriterStep::Ready;
        }
        self.bump();
        Ok(())
    }

    /// Atomic visibility point. Both objects must already be validated.
    /// # Errors
    /// Refuses the wrong writer phase, a missing generation/object, or unvalidated publication data.
    pub fn writer_swap(&mut self, root: super::RootId) -> Result<(), CacheError> {
        self.require_step(root, WriterStep::Ready)?;
        let generation = self
            .writers
            .get(&root)
            .and_then(|writer| writer.generation)
            .ok_or(CacheError::InvalidRequest { reason: "writer has no generation" })?;
        let segment = self.writers.get(&root).and_then(|writer| writer.segment);
        let manifest_object = self.writers.get(&root).and_then(|writer| writer.manifest_object);
        for id in [segment, manifest_object].into_iter().flatten() {
            let Some(object) = self.objects.get_mut(&id) else {
                return Err(CacheError::CorruptIndex { reason: "publication object missing" });
            };
            if !object.validated || !object.complete {
                return Err(CacheError::InvalidRequest { reason: "publication is not validated" });
            }
            object.state = Physical::Published;
        }
        self.swap_pointer(root, generation);
        self.note_use(root);
        if let Some(writer) = self.writers.get_mut(&root) {
            writer.step = WriterStep::Swapped;
        }
        self.bump();
        self.debug_check();
        Ok(())
    }

    /// Releases the lock after a successful swap and returns unused reservation bytes.
    /// # Errors
    /// Refuses unless the writer has completed the pointer swap.
    pub fn writer_release(&mut self, root: super::RootId) -> Result<(), CacheError> {
        self.require_step(root, WriterStep::Swapped)?;
        let reservation = self.writers.get(&root).and_then(|writer| writer.reservation);
        self.release_hold(reservation);
        if let Some(writer) = self.writers.get_mut(&root) {
            writer.lock = WriterLock::Free;
            writer.step = WriterStep::Idle;
            writer.reservation = None;
            writer.session = None;
            writer.segment = None;
            writer.manifest_object = None;
        }
        self.bump();
        self.debug_check();
        Ok(())
    }

    /// The session died. The lock stays dead until reconciliation sees the OS lock released.
    /// # Errors
    /// Refuses an absent writer or one that is not held.
    pub fn crash_writer(&mut self, root: super::RootId) -> Result<(), CacheError> {
        let Some(writer) = self.writers.get_mut(&root) else {
            return Err(CacheError::InvalidRequest { reason: "no writer" });
        };
        if writer.lock != WriterLock::Held {
            return Err(CacheError::InvalidRequest { reason: "writer is not held" });
        }
        writer.lock = WriterLock::Dead;
        self.bump();
        Ok(())
    }

    /// Lock phase for tests and status.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn writer_phase(&self, root: super::RootId) -> WriterPhase {
        match self.writers.get(&root).map(|writer| writer.lock) {
            Some(WriterLock::Held) => WriterPhase::Held,
            Some(WriterLock::Dead) => WriterPhase::Dead,
            _ => WriterPhase::Free,
        }
    }

    pub(super) fn writer_owns(&self, id: super::ObjectId) -> bool {
        self.writers.values().any(|writer| {
            writer.lock != WriterLock::Free
                && (writer.segment == Some(id) || writer.manifest_object == Some(id))
        })
    }

    pub(super) fn recover_writer(&mut self, root: super::RootId, os_lock_held: bool) {
        let Some(writer) = self.writers.get(&root) else {
            return;
        };
        // Abrupt death leaves Held. A missing session OS lock, not the stored tag, means the holder is gone.
        // A changed boot id does not override a lock that is still held; reconcile uses it for unowned staging.
        let restart_dead = matches!(writer.lock, WriterLock::Held | WriterLock::Dead) && !os_lock_held;
        if !restart_dead {
            return;
        }
        let swapped = writer.step == WriterStep::Swapped;
        let segment = writer.segment;
        let manifest = writer.manifest;
        let reservation = writer.reservation;
        if swapped {
            self.release_hold(reservation);
            if let Some(writer) = self.writers.get_mut(&root) {
                writer.lock = WriterLock::Free;
                writer.step = WriterStep::Idle;
                writer.reservation = None;
                writer.session = None;
                writer.segment = None;
                writer.manifest_object = None;
            }
            return;
        }
        if let Some(segment) = segment {
            self.remove_object(segment);
        }
        if let Some(manifest) = manifest {
            self.drop_manifest(manifest);
        }
        self.release_hold(reservation);
        if let Some(writer) = self.writers.get_mut(&root) {
            *writer = Writer::fresh();
        }
        self.bump();
    }

    fn fail_writer(&mut self, root: super::RootId) {
        if let Some(writer) = self.writers.get_mut(&root) {
            writer.lock = WriterLock::Free;
            writer.step = WriterStep::Idle;
            writer.session = None;
            writer.reservation = None;
        }
    }

    fn require_step(&self, root: super::RootId, expected: WriterStep) -> Result<(), CacheError> {
        let Some(writer) = self.writers.get(&root) else {
            return Err(CacheError::InvalidRequest { reason: "no writer" });
        };
        if writer.lock != WriterLock::Held || writer.step != expected {
            return Err(CacheError::InvalidRequest { reason: "writer step is not ready" });
        }
        Ok(())
    }

    fn materialize(
        &mut self,
        root: super::RootId,
        bytes: u64,
        kind: ObjectKind,
        refs: &[super::ObjectId],
    ) -> Result<super::ObjectId, CacheError> {
        if self.disk_full {
            return Err(CacheError::DiskFull);
        }
        let reservation_id = self
            .writers
            .get(&root)
            .and_then(|writer| writer.reservation)
            .ok_or(CacheError::InvalidRequest { reason: "writer has no reservation" })?;
        let (class, charged_root) = {
            let Some(hold) = self.reservations.get_mut(&reservation_id) else {
                return Err(CacheError::InvalidRequest { reason: "unknown reservation" });
            };
            if bytes > hold.left {
                return Err(CacheError::InvalidRequest { reason: "stage exceeds reservation" });
            }
            hold.left -= bytes;
            (hold.class, hold.root)
        };
        self.spawn(Spawn {
            bytes,
            kind,
            state: Physical::Staged,
            roots: &[root],
            refs,
            class,
            charged_root,
            validated: false,
            complete: false,
            reservation: Some(reservation_id),
            dead_ppm: 0,
        })
    }

    pub(super) fn alloc_generation(&mut self) -> Result<GenerationId, CacheError> {
        let id = GenerationId::new(self.ids.generation);
        self.ids.generation = self.ids.generation.checked_add(1).ok_or(CacheError::InvalidRequest {
            reason: "generation id space exhausted",
        })?;
        Ok(id)
    }

    pub(super) fn alloc_manifest(&mut self) -> Result<super::ManifestId, CacheError> {
        let id = super::ManifestId::new(self.ids.manifest);
        self.ids.manifest = self.ids.manifest.checked_add(1).ok_or(CacheError::InvalidRequest {
            reason: "manifest id space exhausted",
        })?;
        Ok(id)
    }

    pub(super) fn swap_pointer(&mut self, root: super::RootId, generation: GenerationId) {
        let pointer = self.pointers.entry(root).or_insert(Pointer { current: None, previous: None });
        let previous = pointer.current;
        pointer.previous = previous;
        pointer.current = Some(generation);
    }

    pub(super) fn ensure_refs(&self, refs: &[super::ObjectId]) -> Result<(), CacheError> {
        if refs.iter().any(|id| !self.objects.contains_key(id)) {
            return Err(CacheError::InvalidRequest { reason: "reference is not in the ledger" });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Lifecycle, Quota, RootId, SessionId};

    #[test]
    fn swap_before_validation_does_not_move_the_pointer() {
        let mut life = Lifecycle::new(Quota::scaled(100)).unwrap();
        let root = RootId::new(1);
        life.register_root(root).unwrap();
        life.writer_acquire(root, SessionId::new(7)).unwrap();
        life.writer_reserve(root, 10).unwrap();
        life.writer_stage_segment(root, 8).unwrap();
        assert!(life.writer_swap(root).is_err());
        assert!(life.current_generation(root).is_none());
        life.writer_validate_segment(root).unwrap();
        life.writer_stage_manifest(root, 2, &[]).unwrap();
        life.writer_validate_manifest(root).unwrap();
        life.writer_swap(root).unwrap();
        life.writer_release(root).unwrap();
        assert!(life.current_generation(root).is_some());
        assert_eq!(life.writer_phase(root), WriterPhase::Free);
        assert!(life.p4_holds());
    }
}
