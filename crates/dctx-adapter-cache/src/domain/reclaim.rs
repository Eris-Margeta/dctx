//! Reclaimable generations: superseded views retire at publish, admission evicts before it refuses.
//! Implements CACHE-v1 reclamation for P-C10 and P-C11: a view that a newer view replaced, and that no
//! reader lease holds, stops being retained at once, and its bytes may be evicted in age order across roots.
//! Depends on reachability, leases and retirement in [`super::Lifecycle`]. The adapter unlinks the files.
//! An evicted object leaves the ledger only together with its file: the adapter drains [`Lifecycle::take_evicted`].

use super::ids::{GenerationId, ManifestKind, Physical};
use super::{Lifecycle, ManifestId, ObjectId, RootId};
use std::collections::BTreeSet;

impl Lifecycle {
    /// Retires every view of `root` that is not the current one and that no lease or pin holds.
    pub(super) fn retire_superseded_views(&mut self, root: RootId) {
        let ids: Vec<ManifestId> = self
            .manifests
            .values()
            .filter(|manifest| {
                manifest.root == root
                    && manifest.kind == ManifestKind::View
                    && !self.is_current(manifest)
            })
            .map(|manifest| manifest.id)
            .collect();
        self.retire_unheld(ids);
    }

    /// Retires the superseded view of `generation` once its last lease is gone.
    pub(super) fn retire_superseded_generation(&mut self, generation: GenerationId) {
        let ids: Vec<ManifestId> = self
            .manifests
            .values()
            .filter(|manifest| {
                manifest.generation == generation
                    && manifest.kind == ManifestKind::View
                    && !self.is_current(manifest)
            })
            .map(|manifest| manifest.id)
            .collect();
        self.retire_unheld(ids);
    }

    fn retire_unheld(&mut self, ids: Vec<ManifestId>) {
        for id in ids {
            let Some(manifest) = self.manifests.get(&id).cloned() else {
                continue;
            };
            if self.can_retire(&manifest) && self.retire_manifest(id) {
                self.superseded.insert(id);
            }
        }
    }

    /// Objects that only a retired manifest names: its payload and the objects below it that no
    /// retained manifest, pointer or active lease reaches. In-flight inserts and pending unlinks are not in it.
    fn reclaimable_set(&self, superseded_only: bool) -> BTreeSet<ObjectId> {
        let live = self.live_objects(true);
        let mut stack = Vec::new();
        for manifest in self.manifests.values() {
            if superseded_only && !self.superseded.contains(&manifest.id) {
                continue;
            }
            if self
                .objects
                .get(&manifest.object)
                .is_some_and(|object| object.state == Physical::Retired)
            {
                stack.push(manifest.object);
                stack.extend(manifest.refs.iter().copied());
            }
        }
        let mut found = BTreeSet::new();
        while let Some(id) = stack.pop() {
            if live.contains(&id) || !found.insert(id) {
                continue;
            }
            if let Some(object) = self.objects.get(&id) {
                stack.extend(object.refs.iter().copied());
            }
        }
        found.retain(|id| {
            self.objects.get(id).is_some_and(|object| {
                matches!(object.state, Physical::Published | Physical::Retired)
                    && object.complete
                    && !object.corrupt
            }) && !self.unlink_failures.contains(id)
        });
        found
    }

    /// Reclaimable objects, oldest first. Roots do not matter: age decides.
    #[allow(
        clippy::must_use_candidate,
        reason = "Read-only report of the eviction order."
    )]
    pub fn reclaimable_ids(&self) -> Vec<ObjectId> {
        let mut rows: Vec<(u64, u64, ObjectId)> = self
            .reclaimable_set(false)
            .into_iter()
            .filter_map(|id| {
                let object = self.objects.get(&id)?;
                Some((object.retired_at.unwrap_or(object.created).0, id.raw(), id))
            })
            .collect();
        rows.sort_unstable();
        rows.into_iter().map(|row| row.2).collect()
    }

    /// Drops reclaimable objects, oldest first, until `fits` holds. `protect` is never touched.
    /// The ids go to [`Self::take_evicted`] so the adapter unlinks their files before it commits.
    pub(super) fn evict_until(
        &mut self,
        protect: &[ObjectId],
        fits: impl Fn(&Self) -> bool,
    ) -> bool {
        if fits(self) {
            return true;
        }
        for id in self.reclaimable_ids() {
            if protect.contains(&id) {
                continue;
            }
            self.remove_object(id);
            self.evicted.push(id);
            if fits(self) {
                return true;
            }
        }
        fits(self)
    }

    /// Evicts reclaimable objects, oldest first, until the admitted bytes are at most `limit`.
    /// Returns whether they are. The files go with the next ledger commit.
    pub fn evict_to_fit(&mut self, limit: u64) -> bool {
        self.evict_until(&[], |life| life.admitted() <= limit)
    }

    /// Objects admission evicted and whose files are not yet known to be unlinked.
    #[allow(
        clippy::must_use_candidate,
        reason = "Read-only view of the unlink hand-off list."
    )]
    pub fn evicted_ids(&self) -> &[ObjectId] {
        &self.evicted
    }

    /// Objects admission evicted since the last call. Their files must be unlinked before the ledger commits.
    pub fn take_evicted(&mut self) -> Vec<ObjectId> {
        std::mem::take(&mut self.evicted)
    }

    /// Marks the objects of views a newer view replaced as pending unlink: these skip the unlink grace, the way a
    /// publish reclaims its predecessor. A view that expired or was retired by pressure keeps its grace unless
    /// admission needs the room. The bytes stay charged until [`Self::finish_reclaim`]. Returns how many were marked.
    pub fn begin_reclaim(&mut self) -> usize {
        let ids = self.reclaimable_set(true);
        self.superseded.clear();
        for id in &ids {
            if let Some(object) = self.objects.get_mut(id) {
                object.state = Physical::PendingUnlink;
                // Persisted with the ledger: a restart finishes the unlink of what this call marked.
                object.deletion_requested = true;
            }
        }
        if !ids.is_empty() {
            self.bump();
        }
        ids.len()
    }

    /// Removes the pending objects that [`Self::begin_reclaim`] marked, or that a refused unlink put back, and returns
    /// them. Pending objects a gc slice marked are left to the next slice, which reports their bytes.
    /// The caller unlinks each file and calls `restore_pending_charge` for one that refused.
    pub fn finish_reclaim(&mut self) -> Vec<ObjectId> {
        let ids: Vec<ObjectId> = self
            .objects
            .iter()
            .filter(|(id, object)| {
                object.state == Physical::PendingUnlink
                    && object.deletion_requested
                    && !self.unlink_failures.contains(id)
            })
            .map(|(id, _)| *id)
            .collect();
        for id in &ids {
            self.remove_object(*id);
        }
        ids
    }
}
