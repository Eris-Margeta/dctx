//! Object creation, removal and saturating byte accounting.
//! Implements the CACHE-v1 object table mutations on [`super::Lifecycle`]. Writers, reserve and compact spawn here.
//! Removal drops the object's manifests and back-references, then re-checks the legacy freeze.
//! Byte sums saturate instead of panicking when a pre-existing ledger is over the cap.

use super::ids::ObjectId;
use super::{CacheObject, Lifecycle, Spawn};

impl Lifecycle {
    #[allow(clippy::needless_pass_by_value, reason = "Object creation consumes an assembled Spawn record at the ledger mutation boundary.")]
    pub(super) fn spawn(&mut self, spec: Spawn<'_>) -> Result<ObjectId, crate::error::CacheError> {
        let id = ObjectId::new(self.ids.object);
        self.ids.object = self.ids.object.checked_add(1).ok_or(
            crate::error::CacheError::InvalidRequest { reason: "object id space exhausted" },
        )?;
        self.objects.insert(
            id,
            CacheObject {
                bytes: spec.bytes,
                born: spec.bytes,
                kind: spec.kind,
                state: spec.state,
                roots: spec.roots.to_vec(),
                refs: spec.refs.to_vec(),
                class: spec.class,
                charged_root: spec.charged_root,
                validated: spec.validated,
                complete: spec.complete,
                created: self.wall,
                retired_at: None,
                unreachable_since: None,
                deletion_requested: false,
                corrupt: false,
                reservation: spec.reservation,
                dead_ppm: spec.dead_ppm,
            },
        );
        self.bump();
        Ok(id)
    }

    pub(super) fn remove_object(&mut self, id: ObjectId) {
        if self.objects.remove(&id).is_none() {
            return;
        }
        self.manifests.retain(|_, manifest| manifest.object != id);
        for manifest in self.manifests.values_mut() {
            manifest.refs.retain(|ref_id| *ref_id != id);
        }
        for object in self.objects.values_mut() {
            object.refs.retain(|ref_id| *ref_id != id);
        }
        for writer in self.writers.values_mut() {
            if writer.segment == Some(id) {
                writer.segment = None;
            }
            if writer.manifest_object == Some(id) {
                writer.manifest_object = None;
            }
        }
        self.unlink_failures.remove(&id);
        self.bump();
        self.refresh_freeze();
    }

    pub(super) fn refresh_freeze(&mut self) {
        if self.legacy_frozen
            && self.admitted() <= self.payload_ceiling()
            && self.staged_bytes() == 0
            && self.reserved_bytes() == 0
        {
            self.legacy_frozen = false;
        }
    }

    #[allow(clippy::unused_self, reason = "Keep saturating ledger accounting on Lifecycle beside the quota and reachability computations that use it.")]
    pub(super) fn sum_iter(&self, values: impl Iterator<Item = u64>) -> u64 {
        values.fold(0u64, u64::saturating_add)
    }
}
