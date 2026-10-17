//! LRU chooses a manifest. Reachability then decides which facts can be unlinked.
//! Implements CACHE-v1 `Live = closure(R ∪ L ∪ recovery pointers)` and DX-A2 P3.
//! Depends on manifests, leases and pointers in [`super::Lifecycle`]. Maintenance applies one retire or sweep.
//! A fact referenced by any retained manifest stays, including a manifest from another root.

use super::ids::{GenerationId, ManifestKind, ObjectKind, Physical};
use super::{Lifecycle, Manifest, ObjectId, RootId};
use crate::error::CacheError;
use std::collections::BTreeSet;

impl Lifecycle {
    /// P3: every object reachable from a retained manifest or an active lease is still intact.
    pub fn p3_holds(&self) -> bool {
        if self.objects.values().any(|object| object.bytes != object.born) {
            return false;
        }
        let intact = |id: ObjectId| {
            self.objects.get(&id).is_some_and(|object| {
                object.bytes == object.born && object.state != Physical::PendingUnlink
            })
        };
        for lease in self.leases.values() {
            let super::lease::LeasePhase::Active { generation } = lease.phase else {
                continue;
            };
            let held = self.manifests.values().any(|manifest| {
                manifest.generation == generation && intact(manifest.object)
            });
            if !held {
                return false;
            }
        }
        for pointer in self.pointers.values() {
            for generation in [pointer.current, pointer.previous].into_iter().flatten() {
                let held = self.manifests.values().any(|manifest| {
                    manifest.generation == generation && intact(manifest.object)
                });
                if !held {
                    return false;
                }
            }
        }
        self.live_objects(true).iter().copied().all(intact)
    }

    pub(super) fn live_objects(&self, honor_leases: bool) -> BTreeSet<ObjectId> {
        let mut stack = Vec::new();
        for manifest in self.manifests.values() {
            if self.retained(manifest, honor_leases) {
                stack.push(manifest.object);
                stack.extend(manifest.refs.iter().copied());
            }
        }
        let mut live = BTreeSet::new();
        while let Some(id) = stack.pop() {
            if !live.insert(id) {
                continue;
            }
            if let Some(object) = self.objects.get(&id) {
                stack.extend(object.refs.iter().copied());
            }
        }
        live
    }

    pub(super) fn retained(&self, manifest: &Manifest, honor_leases: bool) -> bool {
        let Some(object) = self.objects.get(&manifest.object) else {
            return false;
        };
        if object.corrupt {
            return matches!(
                object.state,
                Physical::Published | Physical::Retired | Physical::Staged | Physical::PendingUnlink
            );
        }
        if !matches!(object.state, Physical::Published | Physical::Retired) {
            return false;
        }
        let leased = honor_leases && self.lease_holds(manifest.generation);
        let anchored = manifest.pinned
            || self.root_is_pinned(manifest.root)
            || self.is_current(manifest)
            || self.is_previous(manifest)
            || leased;
        if anchored {
            return true;
        }
        object.state == Physical::Published && manifest.available
    }

    pub(super) fn root_is_pinned(&self, root: RootId) -> bool {
        self.roots.get(&root).is_some_and(|record| record.pinned)
    }

    pub(super) fn is_current(&self, manifest: &Manifest) -> bool {
        self.pointers
            .get(&manifest.root)
            .and_then(|pointer| pointer.current)
            .is_some_and(|generation| generation == manifest.generation)
    }

    pub(super) fn is_previous(&self, manifest: &Manifest) -> bool {
        self.pointers
            .get(&manifest.root)
            .and_then(|pointer| pointer.previous)
            .is_some_and(|generation| generation == manifest.generation)
    }

    pub(super) fn unavailable_protected(&self, root: RootId) -> bool {
        let Some(since) = self.roots.get(&root).and_then(|record| record.unavailable_since) else {
            return false;
        };
        self.wall.0.saturating_sub(since.0) < super::quota::MISSING_GRACE_MS
    }

    pub(super) fn root_ttl_due(&self, root: RootId) -> bool {
        let Some(record) = self.roots.get(&root) else {
            return false;
        };
        !record.pinned && self.wall.0.saturating_sub(record.last_use.0) >= self.root_ttl()
    }

    pub(super) fn can_retire(&self, manifest: &Manifest) -> bool {
        if manifest.pinned || self.root_is_pinned(manifest.root) {
            return false;
        }
        if self.unavailable_protected(manifest.root) || self.lease_holds(manifest.generation) {
            return false;
        }
        if self.is_previous(manifest) {
            return false;
        }
        let Some(object) = self.objects.get(&manifest.object) else {
            return false;
        };
        if object.corrupt || object.state != Physical::Published {
            return false;
        }
        if self.is_current(manifest) && !self.root_ttl_due(manifest.root) {
            return false;
        }
        true
    }

    pub(super) fn ttl_due(&self, manifest: &Manifest) -> bool {
        if self.unavailable_protected(manifest.root) || self.root_is_pinned(manifest.root) {
            return false;
        }
        match manifest.kind {
            ManifestKind::Overlay => {
                self.wall.0.saturating_sub(manifest.last_use.0) >= self.overlay_ttl()
            }
            ManifestKind::Preview => {
                self.wall.0.saturating_sub(manifest.last_use.0) >= self.preview_ttl()
            }
            ManifestKind::View => self.root_ttl_due(manifest.root),
            ManifestKind::Pack => false,
        }
    }

    pub(super) fn over_retention(&self) -> BTreeSet<super::ManifestId> {
        let mut extra = BTreeSet::new();
        for root in self.roots.keys().copied() {
            for kind in [ManifestKind::View, ManifestKind::Pack, ManifestKind::Preview] {
                let Some(keep) = kind.keep() else {
                    continue;
                };
                extra.extend(self.overflow(root, kind, keep));
            }
        }
        extra
    }

    fn overflow(
        &self,
        root: RootId,
        kind: ManifestKind,
        keep: usize,
    ) -> Vec<super::ManifestId> {
        let mut rows: Vec<(u64, u64, super::ManifestId)> = self
            .manifests
            .values()
            .filter(|manifest| {
                manifest.root == root && manifest.kind == kind && self.is_published(manifest)
            })
            .map(|manifest| (manifest.last_use.0, manifest.id.raw(), manifest.id))
            .collect();
        rows.sort_by(|left, right| right.0.cmp(&left.0).then(right.1.cmp(&left.1)));
        rows.into_iter().skip(keep).map(|(_, _, id)| id).collect()
    }

    pub(super) fn is_published(&self, manifest: &Manifest) -> bool {
        self.objects.get(&manifest.object).is_some_and(|object| object.state == Physical::Published)
    }

    pub(super) fn next_retire_candidate(&self, only: Option<RootId>) -> Option<super::ManifestId> {
        let overflow = self.over_retention();
        let pressure = self.admitted() > self.quota.eviction_target;
        let mut rows = Vec::new();
        for manifest in self.manifests.values() {
            if only.is_some_and(|root| manifest.root != root) || !self.can_retire(manifest) {
                continue;
            }
            let due = self.ttl_due(manifest);
            let over = overflow.contains(&manifest.id);
            if !(due || over || pressure) {
                continue;
            }
            let rank = if due || over { 0 } else { 1 };
            rows.push((
                rank,
                manifest.last_use.0,
                manifest.root.raw(),
                manifest.id.raw(),
                manifest.id,
            ));
        }
        rows.sort_unstable();
        rows.first().map(|row| row.4)
    }

    pub(super) fn retire_manifest(&mut self, id: super::ManifestId) -> bool {
        let Some(manifest) = self.manifests.get(&id).cloned() else {
            return false;
        };
        if !self.can_retire(&manifest) {
            return false;
        }
        let was_current = self.is_current(&manifest);
        if let Some(object) = self.objects.get_mut(&manifest.object) {
            object.state = Physical::Retired;
            object.retired_at = Some(self.wall);
        }
        if manifest.kind == ManifestKind::Pack {
            self.history_reset.insert(manifest.root);
        }
        if was_current {
            if let Some(pointer) = self.pointers.get_mut(&manifest.root) {
                if pointer.current == Some(manifest.generation) {
                    pointer.current = None;
                }
                if pointer.previous == Some(manifest.generation) {
                    pointer.previous = None;
                }
            }
            if let Some(record) = self.roots.get_mut(&manifest.root) {
                record.expired = true;
                record.last_generation = Some(manifest.generation);
            }
        }
        self.bump();
        true
    }

    pub(super) fn mark_expired_roots(&mut self) {
        let due: Vec<RootId> = self
            .roots
            .keys()
            .copied()
            .filter(|id| self.root_ttl_due(*id) && !self.unavailable_protected(*id))
            .collect();
        let mut changed = false;
        for id in due {
            if let Some(record) = self.roots.get_mut(&id)
                && !record.expired
            {
                record.expired = true;
                changed = true;
            }
        }
        if changed {
            self.bump();
        }
    }

    pub(super) fn drop_manifest(&mut self, id: super::ManifestId) {
        let Some(manifest) = self.manifests.remove(&id) else {
            return;
        };
        self.remove_object(manifest.object);
    }

    pub(super) fn drop_previous(&mut self, root: RootId) {
        let Some(pointer) = self.pointers.get(&root) else {
            return;
        };
        if pointer.previous.is_none() {
            return;
        }
        let Some(current) = pointer.current else {
            return;
        };
        let writer_idle = self.writers.get(&root).is_none_or(|writer| {
            writer.lock == super::writer::WriterLock::Free
                && matches!(writer.step, super::writer::WriterStep::Idle)
        });
        if writer_idle && self.generation_confirmable(root, current) {
            if let Some(pointer) = self.pointers.get_mut(&root) {
                pointer.previous = None;
            }
            self.bump();
        }
    }

    /// Drops the recovery pointer once the current generation is validated and the writer is idle.
    pub fn release_recovery(&mut self, root: RootId) -> Result<(), CacheError> {
        if !self.roots.contains_key(&root) {
            return Err(CacheError::InvalidRequest { reason: "unknown root" });
        }
        self.drop_previous(root);
        Ok(())
    }

    /// Generation the pointer names, if any.
    pub fn current_generation(&self, root: RootId) -> Option<GenerationId> {
        self.pointers.get(&root).and_then(|pointer| pointer.current)
    }

    /// Opens the current generation. Quarantine and revocation fail before any fact is served.
    pub fn open_current(&self, root: RootId) -> Result<GenerationId, CacheError> {
        let generation = self
            .current_generation(root)
            .ok_or(CacheError::InvalidRequest { reason: "no generation" })?;
        let manifest = self
            .manifests
            .values()
            .find(|manifest| manifest.root == root && manifest.generation == generation)
            .ok_or(CacheError::CorruptIndex { reason: "pointer has no manifest" })?;
        let object = self
            .objects
            .get(&manifest.object)
            .ok_or(CacheError::CorruptIndex { reason: "manifest object missing" })?;
        if object.corrupt {
            return Err(CacheError::CorruptIndex { reason: "generation is quarantined" });
        }
        if !manifest.available {
            return Err(CacheError::PolicyDenied);
        }
        if !object.validated || !object.complete || object.state != Physical::Published {
            return Err(CacheError::CorruptIndex { reason: "generation is incomplete" });
        }
        Ok(generation)
    }

    /// Quarantines an object. Maintenance becomes due. The bytes stay until an explicit rebuild.
    pub fn note_incompatible(&mut self, id: ObjectId) -> Result<(), CacheError> {
        let Some(object) = self.objects.get_mut(&id) else {
            return Err(CacheError::InvalidRequest { reason: "unknown object" });
        };
        object.corrupt = true;
        self.incompatible = true;
        self.bump();
        Ok(())
    }

    /// Stops serving a manifest immediately. Physical bytes stay while a pin or another root needs them.
    pub fn revoke_manifest(&mut self, id: super::ManifestId) -> Result<(), CacheError> {
        let Some(manifest) = self.manifests.get_mut(&id) else {
            return Err(CacheError::InvalidRequest { reason: "unknown manifest" });
        };
        manifest.available = false;
        self.bump();
        Ok(())
    }

    /// Pins one manifest. The pin blocks LRU and expiry for that manifest only.
    pub fn pin_manifest(&mut self, id: super::ManifestId, pinned: bool) -> Result<(), CacheError> {
        let Some(manifest) = self.manifests.get_mut(&id) else {
            return Err(CacheError::InvalidRequest { reason: "unknown manifest" });
        };
        manifest.pinned = pinned;
        self.bump();
        Ok(())
    }

    pub(super) fn pinned_bytes(&self) -> u64 {
        let mut stack = Vec::new();
        for manifest in self.manifests.values() {
            if manifest.pinned || self.root_is_pinned(manifest.root) || self.lease_holds(manifest.generation)
            {
                stack.push(manifest.object);
                stack.extend(manifest.refs.iter().copied());
            }
        }
        let mut seen = BTreeSet::new();
        let mut total = 0u64;
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            if let Some(object) = self.objects.get(&id) {
                total = total.saturating_add(object.bytes);
                stack.extend(object.refs.iter().copied());
            }
        }
        total
    }

    pub(super) fn manifest_for(
        &self,
        root: RootId,
        generation: GenerationId,
    ) -> Option<super::ManifestId> {
        self.manifests
            .values()
            .find(|manifest| manifest.root == root && manifest.generation == generation)
            .map(|manifest| manifest.id)
    }

    /// Physical state, if the object is still in the ledger.
    pub fn object_state(&self, id: ObjectId) -> Option<Physical> {
        self.objects.get(&id).map(|object| object.state)
    }

    /// Byte length recorded at creation. Missing means unlinked or never admitted.
    pub fn object_bytes(&self, id: ObjectId) -> Option<u64> {
        self.objects.get(&id).map(|object| object.bytes)
    }

    /// Manifest that currently publishes `generation` for `root`.
    pub fn manifest_of(&self, root: RootId, generation: GenerationId) -> Option<super::ManifestId> {
        self.manifest_for(root, generation)
    }

    /// Payload object recorded for a manifest.
    pub fn manifest_object(&self, id: super::ManifestId) -> Option<ObjectId> {
        self.manifests.get(&id).map(|manifest| manifest.object)
    }

    /// Physical state of a manifest's payload.
    pub fn manifest_state(&self, id: super::ManifestId) -> Option<Physical> {
        let object = self.manifests.get(&id)?.object;
        self.object_state(object)
    }

    /// True when this object is a segment whose dead bytes exceed 25%, or the soft cap is crossed.
    pub fn compaction_candidate(&self, id: ObjectId) -> bool {
        let Some(object) = self.objects.get(&id) else {
            return false;
        };
        object.kind == ObjectKind::Segment
            && (object.dead_ppm > super::quota::DEAD_BYTE_THRESHOLD_PPM
                || self.admitted() >= self.quota.soft_watermark)
    }

    /// How many times this root has persisted a last-use stamp.
    pub fn persist_writes(&self, root: RootId) -> Option<u64> {
        self.roots.get(&root).map(|record| record.persist_writes)
    }

    /// In-memory last use.
    pub fn last_use(&self, root: RootId) -> Option<dctx_core::ids::Timestamp> {
        self.roots.get(&root).map(|record| record.last_use)
    }

    /// True once root TTL has fired. The record stays until tombstone compaction.
    pub fn root_expired(&self, root: RootId) -> bool {
        self.roots.get(&root).is_some_and(|record| record.expired)
    }

    /// When the root was first observed missing or offline.
    pub fn unavailable_since(&self, root: RootId) -> Option<dctx_core::ids::Timestamp> {
        self.roots.get(&root).and_then(|record| record.unavailable_since)
    }

    /// Pack eviction wants the next diff answer to say `history_reset`.
    pub fn history_reset(&self, root: RootId) -> bool {
        self.history_reset.contains(&root)
    }

    /// Clears the history-reset flag after the caller has reported it.
    pub fn acknowledge_history(&mut self, root: RootId) {
        if self.history_reset.remove(&root) {
            self.bump();
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::{Lifecycle, ObjectId, Physical, Quota, RootId, SessionId};

    #[test]
    fn omit_leases_mutation_drops_a_pinned_object() {
        let mut life = Lifecycle::new(Quota::scaled(1_000)).unwrap();
        let root = RootId::new(1);
        life.register_root(root).unwrap();
        let fact = life.insert_fact(10, &[root]).unwrap();
        let generation = life.publish_view(root, 5, &[fact]).unwrap();
        let lease = life.reader_begin(root, SessionId::new(3)).unwrap();
        life.reader_confirm(lease).unwrap();
        life.publish_view(root, 5, &[]).unwrap();
        life.publish_view(root, 5, &[]).unwrap();
        life.publish_view(root, 5, &[]).unwrap();
        let payload = life.manifests.get(&life.manifest_of(root, generation).unwrap()).unwrap().object;
        life.objects.get_mut(&payload).unwrap().state = Physical::Retired;
        let with_leases = life.live_objects(true);
        let without_leases = life.live_objects(false);
        assert!(with_leases.contains(&fact));
        assert!(!without_leases.contains(&fact) || life.lease_holds(generation));
        let only_because_of_lease: Vec<ObjectId> =
            with_leases.difference(&without_leases).copied().collect();
        assert!(!only_because_of_lease.is_empty());
        for id in only_because_of_lease {
            life.remove_object(id);
        }
        assert!(!life.p3_holds());
    }
}
