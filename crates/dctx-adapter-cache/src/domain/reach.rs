//! LRU chooses a manifest. Reachability then decides which facts can be unlinked.
//! Implements CACHE-v1 `Live = closure(R ∪ L ∪ recovery pointers)` and DX-A2 P3.
//! Depends on manifests, leases and pointers in [`super::Lifecycle`]. Maintenance applies one retire or sweep.
//! A fact referenced by any retained manifest stays, including a manifest from another root.

use super::ids::{GenerationId, ObjectKind, Physical};
use super::{Lifecycle, Manifest, ObjectId, RootId};
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
