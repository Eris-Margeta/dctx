//! High-level publication of views, overlays, packs, previews and facts.
//! Implements CACHE-v1 retention: three views, two pack snapshots, two previews.
//! Depends on [`super::Lifecycle`] admission and reachability. Startup GC reclaims what retention retired.
//! Publishing reserves the new bytes before the old generation is eligible to retire.

use super::ids::{ChargeClass, ManifestKind, ObjectKind, Physical, Purpose};
use super::restart::{Put, Take};
use super::{GenerationId, Lifecycle, Manifest, ObjectId, RootId, Spawn};
use crate::error::CacheError;
use dctx_core::ids::Timestamp;
use std::collections::BTreeMap;

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

const ROOT_MISSING: u8 = 1;
const ROOT_OFFLINE: u8 = 2;
const ROOT_PINNED: u8 = 4;
const ROOT_EXPIRED: u8 = 8;
const OBJECT_VALIDATED: u8 = 1;
const OBJECT_COMPLETE: u8 = 2;
const OBJECT_DELETION: u8 = 4;
const OBJECT_CORRUPT: u8 = 8;
const MANIFEST_PINNED: u8 = 1;
const MANIFEST_AVAILABLE: u8 = 2;

pub(super) struct Maps {
    pub roots: BTreeMap<RootId, super::RootRecord>,
    pub objects: BTreeMap<ObjectId, super::CacheObject>,
    pub manifests: BTreeMap<super::ManifestId, Manifest>,
}

pub(super) fn write_maps(life: &Lifecycle, out: &mut Put) -> Result<(), CacheError> {
    out.count(life.roots.len())?;
    for (id, record) in &life.roots {
        out.u64(id.raw());
        out.u64(record.last_use.0);
        out.u64(record.persisted_use.0);
        out.u64(record.persist_writes);
        out.opt_u64(record.unavailable_since.map(|stamp| stamp.0));
        out.u8(root_flags(record));
        out.opt_u64(record.last_generation.map(GenerationId::raw));
    }
    out.count(life.objects.len())?;
    for (id, object) in &life.objects {
        out.u64(id.raw());
        write_object(out, object)?;
    }
    out.count(life.manifests.len())?;
    for (id, manifest) in &life.manifests {
        out.u64(id.raw());
        out.u64(manifest.root.raw());
        out.u64(manifest.generation.raw());
        out.u8(manifest.kind.tag());
        out.u64(manifest.object.raw());
        out.u64(manifest.last_use.0);
        out.u8(manifest_flags(manifest));
        write_ids(out, manifest.refs.iter().copied().map(ObjectId::raw))?;
    }
    Ok(())
}

pub(super) fn read_maps(inp: &mut Take<'_>) -> Result<Maps, CacheError> {
    let mut roots = BTreeMap::new();
    for _ in 0..inp.count()? {
        let id = RootId::new(inp.u64()?);
        let flags = inp_root(inp)?;
        if roots.insert(id, flags).is_some() {
            return Err(super::restart::corrupt("duplicate"));
        }
    }
    let mut objects = BTreeMap::new();
    for _ in 0..inp.count()? {
        let id = ObjectId::new(inp.u64()?);
        let object = read_object(inp)?;
        if objects.insert(id, object).is_some() {
            return Err(super::restart::corrupt("duplicate"));
        }
    }
    let mut manifests = BTreeMap::new();
    for _ in 0..inp.count()? {
        let id = super::ManifestId::new(inp.u64()?);
        let manifest = read_manifest(inp, id)?;
        if manifests.insert(id, manifest).is_some() {
            return Err(super::restart::corrupt("duplicate"));
        }
    }
    Ok(Maps { roots, objects, manifests })
}

fn inp_root(inp: &mut Take<'_>) -> Result<super::RootRecord, CacheError> {
    let last_use = Timestamp(inp.u64()?);
    let persisted_use = Timestamp(inp.u64()?);
    let persist_writes = inp.u64()?;
    let unavailable_since = inp.opt_u64()?.map(Timestamp);
    let flags = inp.u8()?;
    if (flags & !0b1111) != 0 {
        return Err(super::restart::corrupt("root"));
    }
    let last_generation = inp.opt_u64()?.map(GenerationId::new);
    Ok(super::RootRecord {
        last_use,
        persisted_use,
        persist_writes,
        unavailable_since,
        missing: flags & ROOT_MISSING != 0,
        offline: flags & ROOT_OFFLINE != 0,
        pinned: flags & ROOT_PINNED != 0,
        expired: flags & ROOT_EXPIRED != 0,
        last_generation,
    })
}

fn root_flags(record: &super::RootRecord) -> u8 {
    let mut flags = 0u8;
    if record.missing {
        flags |= ROOT_MISSING;
    }
    if record.offline {
        flags |= ROOT_OFFLINE;
    }
    if record.pinned {
        flags |= ROOT_PINNED;
    }
    if record.expired {
        flags |= ROOT_EXPIRED;
    }
    flags
}

fn write_object(out: &mut Put, object: &super::CacheObject) -> Result<(), CacheError> {
    out.u64(object.bytes);
    out.u64(object.born);
    out.u8(object.kind.tag());
    out.u8(object.state.tag());
    out.u8(object.class.tag());
    write_ids(out, object.roots.iter().copied().map(RootId::raw))?;
    write_ids(out, object.refs.iter().copied().map(ObjectId::raw))?;
    out.opt_u64(object.charged_root.map(RootId::raw));
    out.u8(object_flags(object));
    out.u64(object.created.0);
    out.opt_u64(object.retired_at.map(|stamp| stamp.0));
    out.opt_u64(object.unreachable_since.map(|stamp| stamp.0));
    out.opt_u64(object.reservation);
    out.u32(object.dead_ppm);
    Ok(())
}

fn read_object(inp: &mut Take<'_>) -> Result<super::CacheObject, CacheError> {
    let bytes = inp.u64()?;
    let born = inp.u64()?;
    let kind = ObjectKind::from_tag(inp.u8()?).ok_or_else(|| super::restart::corrupt("object"))?;
    let state = Physical::from_tag(inp.u8()?).ok_or_else(|| super::restart::corrupt("object"))?;
    let class = ChargeClass::from_tag(inp.u8()?).ok_or_else(|| super::restart::corrupt("object"))?;
    let roots = read_roots(inp)?;
    let refs = read_objects(inp)?;
    let charged_root = inp.opt_u64()?.map(RootId::new);
    let flags = inp.u8()?;
    if (flags & !0b1111) != 0 {
        return Err(super::restart::corrupt("object"));
    }
    Ok(super::CacheObject {
        bytes,
        born,
        kind,
        state,
        roots,
        refs,
        class,
        charged_root,
        validated: flags & OBJECT_VALIDATED != 0,
        complete: flags & OBJECT_COMPLETE != 0,
        created: Timestamp(inp.u64()?),
        retired_at: inp.opt_u64()?.map(Timestamp),
        unreachable_since: inp.opt_u64()?.map(Timestamp),
        deletion_requested: flags & OBJECT_DELETION != 0,
        corrupt: flags & OBJECT_CORRUPT != 0,
        reservation: inp.opt_u64()?,
        dead_ppm: inp.u32()?,
    })
}

fn object_flags(object: &super::CacheObject) -> u8 {
    let mut flags = 0u8;
    if object.validated {
        flags |= OBJECT_VALIDATED;
    }
    if object.complete {
        flags |= OBJECT_COMPLETE;
    }
    if object.deletion_requested {
        flags |= OBJECT_DELETION;
    }
    if object.corrupt {
        flags |= OBJECT_CORRUPT;
    }
    flags
}

fn read_manifest(inp: &mut Take<'_>, id: super::ManifestId) -> Result<Manifest, CacheError> {
    let root = RootId::new(inp.u64()?);
    let generation = GenerationId::new(inp.u64()?);
    let kind = ManifestKind::from_tag(inp.u8()?).ok_or_else(|| super::restart::corrupt("manifest"))?;
    let object = ObjectId::new(inp.u64()?);
    let last_use = Timestamp(inp.u64()?);
    let flags = inp.u8()?;
    if (flags & !0b11) != 0 {
        return Err(super::restart::corrupt("manifest"));
    }
    Ok(Manifest {
        id,
        root,
        generation,
        kind,
        object,
        last_use,
        pinned: flags & MANIFEST_PINNED != 0,
        available: flags & MANIFEST_AVAILABLE != 0,
        refs: read_objects(inp)?,
    })
}

fn manifest_flags(manifest: &Manifest) -> u8 {
    let mut flags = 0u8;
    if manifest.pinned {
        flags |= MANIFEST_PINNED;
    }
    if manifest.available {
        flags |= MANIFEST_AVAILABLE;
    }
    flags
}

fn write_ids(out: &mut Put, ids: impl Iterator<Item = u64>) -> Result<(), CacheError> {
    let ids: Vec<u64> = ids.collect();
    out.count(ids.len())?;
    for id in ids {
        out.u64(id);
    }
    Ok(())
}

fn read_roots(inp: &mut Take<'_>) -> Result<Vec<RootId>, CacheError> {
    Ok(inp.u64s()?.into_iter().map(RootId::new).collect())
}

fn read_objects(inp: &mut Take<'_>) -> Result<Vec<ObjectId>, CacheError> {
    Ok(inp.u64s()?.into_iter().map(ObjectId::new).collect())
}
