// SPDX-License-Identifier: Apache-2.0
//! Segment compaction reserves the copy before it retires the input.
//! Implements CACHE-v1: no headroom means `CacheLimit`, and the live input is not deleted to make room.
//! Depends on admission and reachability in [`super::Lifecycle`]. Explicit GC unlinks the old segment later.
//! Bytes of a surviving object never change; a failed unlink leaves the old segment pending.

use super::ids::{ChargeClass, ManifestKind, ObjectKind, Physical, Purpose};
use super::{Lifecycle, Manifest, ObjectId, Spawn};
use crate::error::CacheError;
use std::collections::BTreeMap;

/// Result of a compaction that had room for the old and new bytes together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactOutcome {
    /// Replacement segment.
    pub new_segment: ObjectId,
    /// Byte length of the input segment at the start of the call.
    pub old_bytes: u64,
    /// Admitted bytes while both the input and the copy were in the ledger.
    pub peak_admitted: u64,
    /// True when a lease or a remaining reference kept the input segment.
    pub old_retained: bool,
}

impl Lifecycle {
    /// Copies a segment, then retires unpinned inputs. Refuses before any write when the copy will not fit.
    /// # Errors
    /// Refuses unknown/non-segment/quarantined objects, disk/quota exhaustion, or invalid references/identity during compaction.
    pub fn compact_segment(&mut self, id: ObjectId) -> Result<CompactOutcome, CacheError> {
        let Some(object) = self.objects.get(&id) else {
            return Err(CacheError::InvalidRequest {
                reason: "unknown object",
            });
        };
        if object.kind != ObjectKind::Segment {
            return Err(CacheError::InvalidRequest {
                reason: "object is not a segment",
            });
        }
        if object.corrupt {
            return Err(CacheError::CorruptIndex {
                reason: "segment is quarantined",
            });
        }
        let old_bytes = object.bytes;
        let born = object.born;
        let new_bytes = live_portion(old_bytes, object.dead_ppm);
        let fact_refs = object.refs.clone();
        let roots = object.roots.clone();
        let manifest_ids: Vec<super::ManifestId> = self
            .manifests
            .values()
            .filter(|manifest| manifest.refs.contains(&id))
            .map(|manifest| manifest.id)
            .collect();
        let leased = manifest_ids.iter().any(|manifest_id| {
            self.manifests
                .get(manifest_id)
                .is_some_and(|manifest| self.lease_holds(manifest.generation))
        });
        let mut copies = Vec::new();
        if !leased {
            for manifest_id in &manifest_ids {
                let Some(manifest) = self.manifests.get(manifest_id) else {
                    continue;
                };
                let bytes = self
                    .objects
                    .get(&manifest.object)
                    .map_or(0, |object| object.bytes);
                copies.push((*manifest_id, manifest.root, manifest.kind, bytes));
            }
        }
        let copy_bytes = self.sum_iter(copies.iter().map(|(_, _, _, bytes)| *bytes));
        let need = new_bytes.saturating_add(copy_bytes);
        if self.disk_full {
            return Err(CacheError::DiskFull);
        }
        if need > 0 && !self.admission_allows(need) {
            return Err(self.limit(need));
        }
        let mut charged: BTreeMap<(super::RootId, ChargeClass), u64> = BTreeMap::new();
        for (_, root, kind, bytes) in &copies {
            if *bytes == 0 {
                continue;
            }
            let slot = charged.entry((*root, kind.purpose().class())).or_default();
            *slot = slot.saturating_add(*bytes);
        }
        for ((root, class), bytes) in charged {
            if !self.ceiling_allows(root, class, bytes) {
                return Err(self.limit(bytes));
            }
        }
        if self.object_bytes(id) != Some(born) {
            return Err(CacheError::CorruptIndex {
                reason: "segment changed before compaction",
            });
        }
        let new_segment = if new_bytes == 0 {
            id
        } else {
            let reservation = self.reserve_for(None, Purpose::Segment, new_bytes)?;
            self.reservations.remove(&reservation.id());
            self.spawn(Spawn {
                bytes: new_bytes,
                kind: ObjectKind::Segment,
                state: Physical::Published,
                roots: &roots,
                refs: &fact_refs,
                class: ChargeClass::Global,
                charged_root: None,
                validated: true,
                complete: true,
                reservation: None,
                dead_ppm: 0,
            })?
        };
        if !leased {
            for (manifest_id, root, kind, bytes) in copies {
                self.retarget(manifest_id, root, kind, bytes, id, new_segment)?;
            }
        }
        let peak_admitted = self.admitted();
        let old_retained = if leased
            || self.live_objects(true).contains(&id)
            || self.unlink_failures.contains(&id)
        {
            true
        } else if id != new_segment {
            self.remove_object(id);
            false
        } else {
            true
        };
        self.debug_check();
        Ok(CompactOutcome {
            new_segment,
            old_bytes,
            peak_admitted,
            old_retained,
        })
    }

    fn retarget(
        &mut self,
        manifest_id: super::ManifestId,
        root: super::RootId,
        kind: ManifestKind,
        bytes: u64,
        old: ObjectId,
        new_segment: ObjectId,
    ) -> Result<(), CacheError> {
        let Some(manifest) = self.manifests.get(&manifest_id).cloned() else {
            return Ok(());
        };
        let mut refs = manifest.refs.clone();
        for reference in &mut refs {
            if *reference == old {
                *reference = new_segment;
            }
        }
        if bytes > 0 {
            let reservation = self.reserve_for(Some(root), kind.purpose(), bytes)?;
            self.reservations.remove(&reservation.id());
        }
        let generation = self.alloc_generation()?;
        let object = if bytes == 0 {
            manifest.object
        } else {
            self.spawn(Spawn {
                bytes,
                kind: ObjectKind::Manifest,
                state: Physical::Published,
                roots: &[root],
                refs: &refs,
                class: kind.purpose().class(),
                charged_root: Some(root),
                validated: true,
                complete: true,
                reservation: None,
                dead_ppm: 0,
            })?
        };
        let new_id = self.alloc_manifest()?;
        self.manifests.insert(
            new_id,
            Manifest {
                id: new_id,
                root,
                generation,
                kind,
                object,
                last_use: self.wall,
                pinned: false,
                available: true,
                refs,
            },
        );
        let was_current = self.current_generation(root) == Some(manifest.generation);
        if was_current {
            self.swap_pointer(root, generation);
            self.drop_previous(root);
        }
        if self.can_retire(&manifest) {
            self.retire_manifest(manifest_id);
        }
        Ok(())
    }
}

fn live_portion(bytes: u64, dead_ppm: u32) -> u64 {
    let dead = u64::from(dead_ppm.min(1_000_000));
    let live_ppm = 1_000_000u64.saturating_sub(dead);
    let product = u128::from(bytes).saturating_mul(u128::from(live_ppm)) / 1_000_000;
    u64::try_from(product).unwrap_or(u64::MAX)
}
