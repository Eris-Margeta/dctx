// SPDX-License-Identifier: Apache-2.0
//! Explicit purge. Confirmation is part of the request; a dry-run does not take the ledger `&mut`.
//! Implements CACHE-v1 purge: managed references drop, active handles keep their physical bytes.
//! A leased generation keeps those bytes and loses logical retention when the last lease releases.
//! Depends on manifests and leases in [`super::Lifecycle`]. Exports, config and template sources are not in scope.

use super::{Lifecycle, ManifestId, RootId};
use crate::error::CacheError;

/// What a purge is allowed to name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PurgeScope {
    /// One registered root.
    Root(RootId),
    /// Every registered root.
    All,
}

/// Explicit destructive request. `confirm` is required unless `dry_run` is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PurgeRequest {
    /// Root or all managed roots.
    pub scope: PurgeScope,
    /// API confirmation. The CLI's `--yes` maps to this.
    pub confirm: bool,
    /// Report only.
    pub dry_run: bool,
}

/// How many manifests a purge marked, and how many stayed because a lease holds them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurgeReport {
    /// Manifests whose retention was dropped.
    pub dropped: u64,
    /// Manifests marked for deletion but kept intact for an active lease.
    pub held_by_lease: u64,
    /// True when nothing was written.
    pub dry_run: bool,
}

impl Lifecycle {
    /// Drops managed manifests in `scope`. Adopted exports are not manifests and stay.
    /// # Errors
    /// Refuses an applying purge without explicit confirmation; dry runs need no confirmation.
    pub fn purge(&mut self, request: PurgeRequest) -> Result<PurgeReport, CacheError> {
        if request.dry_run {
            return Ok(self.plan_purge(request.scope));
        }
        if !request.confirm {
            return Err(CacheError::InvalidRequest { reason: "purge requires explicit confirmation" });
        }
        Ok(self.apply_purge(request.scope))
    }

    fn plan_purge(&self, scope: PurgeScope) -> PurgeReport {
        let mut dropped = 0u64;
        let mut held_by_lease = 0u64;
        for manifest in self.manifests.values() {
            if !self.in_purge(manifest.root, scope) {
                continue;
            }
            if self.lease_holds(manifest.generation) {
                held_by_lease += 1;
            } else {
                dropped += 1;
            }
        }
        PurgeReport { dropped, held_by_lease, dry_run: true }
    }

    fn apply_purge(&mut self, scope: PurgeScope) -> PurgeReport {
        let ids: Vec<ManifestId> = self
            .manifests
            .values()
            .filter(|manifest| self.in_purge(manifest.root, scope))
            .map(|manifest| manifest.id)
            .collect();
        let mut dropped = 0u64;
        let mut held_by_lease = 0u64;
        for id in ids {
            let Some(manifest) = self.manifests.get(&id).cloned() else {
                continue;
            };
            if self.lease_holds(manifest.generation) {
                if let Some(object) = self.objects.get_mut(&manifest.object) {
                    object.deletion_requested = true;
                }
                held_by_lease += 1;
                continue;
            }
            if self.drop_logical_retention(id) {
                dropped += 1;
            }
        }
        self.bump();
        self.debug_check();
        PurgeReport { dropped, held_by_lease, dry_run: false }
    }

    /// Finishes a purge that only marked `deletion_requested` while a lease held the generation.
    pub(super) fn finish_requested_deletion(&mut self, generation: super::GenerationId) {
        let ids: Vec<ManifestId> = self
            .manifests
            .values()
            .filter(|manifest| manifest.generation == generation)
            .filter(|manifest| {
                self.objects.get(&manifest.object).is_some_and(|object| object.deletion_requested)
            })
            .map(|manifest| manifest.id)
            .collect();
        for id in ids {
            self.drop_logical_retention(id);
        }
    }

    fn drop_logical_retention(&mut self, id: ManifestId) -> bool {
        let Some(manifest) = self.manifests.get(&id).cloned() else {
            return false;
        };
        if let Some(record) = self.manifests.get_mut(&id) {
            record.available = false;
            record.pinned = false;
        }
        self.clear_generation(manifest.root, manifest.generation);
        let Some(manifest) = self.manifests.get(&id).cloned() else {
            return false;
        };
        if self.can_retire(&manifest) {
            self.retire_manifest(id);
        } else if let Some(record) = self.manifests.get(&id) {
            let object_id = record.object;
            if let Some(object) = self.objects.get_mut(&object_id) {
                if object.state == super::Physical::Published {
                    object.state = super::Physical::Retired;
                    object.retired_at = Some(self.wall);
                }
                object.deletion_requested = true;
            }
        }
        true
    }

    fn clear_generation(&mut self, root: RootId, generation: super::GenerationId) {
        let Some(pointer) = self.pointers.get_mut(&root) else {
            return;
        };
        if pointer.current == Some(generation) {
            pointer.current = None;
        }
        if pointer.previous == Some(generation) {
            pointer.previous = None;
        }
    }

    #[allow(clippy::unused_self, reason = "Keep purge-scope selection on Lifecycle alongside purge planning and application.")]
    fn in_purge(&self, root: RootId, scope: PurgeScope) -> bool {
        match scope {
            PurgeScope::All => true,
            PurgeScope::Root(selected) => root == selected,
        }
    }
}
