//! High-level publication of views, overlays, packs, previews and facts.
//! Implements CACHE-v1 retention: three views, two pack snapshots, two previews.
//! Depends on [`super::Lifecycle`] admission and reachability. Startup GC reclaims what retention retired.
//! Publishing reserves the new bytes before the old generation is eligible to retire.

use super::ids::{ChargeClass, ManifestKind, ObjectKind, Physical, Purpose};
use super::{GenerationId, Lifecycle, Manifest, ObjectId, RootId, Spawn};
use crate::error::CacheError;

impl Lifecycle {
    /// Inserts a fact charged once globally. `roots` is attribution, not a per-root ceiling charge.
    pub fn insert_fact(&mut self, bytes: u64, roots: &[RootId]) -> Result<ObjectId, CacheError> {
        self.insert_payload(bytes, roots, ObjectKind::Fact, 0)
    }

    /// Inserts a segment. `dead_ppm` is parts per million of dead bytes, used only for compaction.
    pub fn insert_segment(
        &mut self,
        bytes: u64,
        dead_ppm: u32,
        roots: &[RootId],
    ) -> Result<ObjectId, CacheError> {
        self.insert_payload(bytes, roots, ObjectKind::Segment, dead_ppm.min(1_000_000))
    }

    /// Records an incomplete staged object with no writer. Startup reclaims it after 24 hours.
    pub fn load_staged(&mut self, bytes: u64, created_ms: u64) -> Result<ObjectId, CacheError> {
        if self.disk_full {
            return Err(CacheError::DiskFull);
        }
        let reservation = self.reserve_for(None, Purpose::Staging, bytes)?;
        self.reservations.remove(&reservation.id());
        let id = self.spawn(Spawn {
            bytes,
            kind: ObjectKind::Staging,
            state: Physical::Staged,
            roots: &[],
            refs: &[],
            class: ChargeClass::Global,
            charged_root: None,
            validated: false,
            complete: false,
            reservation: None,
            dead_ppm: 0,
        })?;
        if let Some(object) = self.objects.get_mut(&id) {
            object.created = dctx_core::ids::Timestamp(created_ms);
        }
        self.debug_check();
        Ok(id)
    }

    fn insert_payload(
        &mut self,
        bytes: u64,
        roots: &[RootId],
        kind: ObjectKind,
        dead_ppm: u32,
    ) -> Result<ObjectId, CacheError> {
        if roots.is_empty() || roots.iter().any(|root| !self.roots.contains_key(root)) {
            return Err(CacheError::InvalidRequest { reason: "fact root is not registered" });
        }
        if self.disk_full {
            return Err(CacheError::DiskFull);
        }
        let purpose = if kind == ObjectKind::Segment { Purpose::Segment } else { Purpose::Fact };
        let reservation = self.reserve_for(None, purpose, bytes)?;
        self.reservations.remove(&reservation.id());
        let id = self.spawn(Spawn {
            bytes,
            kind,
            state: Physical::Published,
            roots,
            refs: &[],
            class: ChargeClass::Global,
            charged_root: None,
            validated: true,
            complete: true,
            reservation: None,
            dead_ppm,
        })?;
        self.debug_check();
        Ok(id)
    }

    /// Publishes a validated view generation that references `refs`.
    pub fn publish_view(
        &mut self,
        root: RootId,
        bytes: u64,
        refs: &[ObjectId],
    ) -> Result<GenerationId, CacheError> {
        self.publish(root, ManifestKind::View, bytes, refs)
    }

    /// Publishes an overlay. Idle overlays expire after seven days.
    pub fn publish_overlay(
        &mut self,
        root: RootId,
        bytes: u64,
        refs: &[ObjectId],
    ) -> Result<GenerationId, CacheError> {
        self.publish(root, ManifestKind::Overlay, bytes, refs)
    }

    /// Publishes a pack snapshot. The third snapshot retires the oldest and sets `history_reset`.
    pub fn publish_pack(
        &mut self,
        root: RootId,
        bytes: u64,
        refs: &[ObjectId],
    ) -> Result<GenerationId, CacheError> {
        self.publish(root, ManifestKind::Pack, bytes, refs)
    }

    /// Publishes a managed preview. At most two generations and the preview ceiling apply.
    pub fn publish_preview(
        &mut self,
        root: RootId,
        bytes: u64,
        refs: &[ObjectId],
    ) -> Result<GenerationId, CacheError> {
        self.publish(root, ManifestKind::Preview, bytes, refs)
    }

    fn publish(
        &mut self,
        root: RootId,
        kind: ManifestKind,
        bytes: u64,
        refs: &[ObjectId],
    ) -> Result<GenerationId, CacheError> {
        if !self.roots.contains_key(&root) {
            return Err(CacheError::InvalidRequest { reason: "unknown root" });
        }
        self.ensure_refs(refs)?;
        if self.disk_full {
            return Err(CacheError::DiskFull);
        }
        let reservation = self.reserve_for(Some(root), kind.purpose(), bytes)?;
        let class = kind.purpose().class();
        self.reservations.remove(&reservation.id());
        let generation = self.alloc_generation()?;
        let object = self.spawn(Spawn {
            bytes,
            kind: ObjectKind::Manifest,
            state: Physical::Published,
            roots: &[root],
            refs,
            class,
            charged_root: Some(root),
            validated: true,
            complete: true,
            reservation: None,
            dead_ppm: 0,
        })?;
        let manifest_id = self.alloc_manifest()?;
        self.manifests.insert(
            manifest_id,
            Manifest {
                id: manifest_id,
                root,
                generation,
                kind,
                object,
                last_use: self.wall,
                pinned: false,
                available: true,
                refs: refs.to_vec(),
            },
        );
        if kind == ManifestKind::View {
            self.swap_pointer(root, generation);
            self.drop_previous(root);
        }
        self.note_use(root);
        self.enforce_retention(root);
        self.debug_check();
        Ok(generation)
    }

    fn enforce_retention(&mut self, root: RootId) {
        loop {
            let overflow = self.over_retention();
            let candidate = self
                .manifests
                .values()
                .filter(|manifest| {
                    manifest.root == root && overflow.contains(&manifest.id) && self.can_retire(manifest)
                })
                .map(|manifest| (manifest.last_use.0, manifest.root.raw(), manifest.id.raw(), manifest.id))
                .min();
            let Some((_, _, _, id)) = candidate else {
                break;
            };
            if !self.retire_manifest(id) {
                break;
            }
        }
    }
}
