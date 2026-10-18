//! Manifest retirement, root expiry and generation availability.
//! Implements CACHE-v1 retention limits, TTL and recovery pointer protection.
//! Depends on lifecycle reachability; maintenance and generation readers use these rules.

use super::ids::{GenerationId, ManifestKind, Physical};
use super::{Lifecycle, Manifest, ObjectId, RootId};
use crate::error::CacheError;
use std::collections::BTreeSet;

impl Lifecycle {
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

}
