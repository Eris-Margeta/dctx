//! Ledger body parsing and id-cursor validation.
//! Implements CACHE-v1 reconstruction with duplicate, flag and cursor refusals.
//! Depends on domain map codecs and ledger primitives; restart verifies the frame first.

use super::ids::{GenerationId, ManifestId, ObjectId, SessionId};
use super::restart::{
    BOOT_ACCELERATES, CLOCK_FAULT, DEFERRED, DISK_FULL, INCOMPATIBLE, LEGACY_FROZEN, PASS_INCOMPLETE,
    Take, corrupt,
};
use super::writer::{Writer, WriterLock, WriterStep};
use super::{Lifecycle, Quota};
use crate::error::CacheError;
use dctx_core::ids::Timestamp;
use std::collections::{BTreeMap, BTreeSet};

const FLAG_MASK: u32 = 0b111_1111;

impl Lifecycle {
    pub(super) fn decode_body(bytes: &[u8]) -> Result<Self, CacheError> {
        let mut inp = Take::new(bytes);
        let quota = Quota {
            hard_cap: inp.u64()?,
            soft_watermark: inp.u64()?,
            eviction_target: inp.u64()?,
            root_view_ceiling: inp.u64()?,
            root_pack_ceiling: inp.u64()?,
            root_preview_ceiling: inp.u64()?,
            emergency_metadata: inp.u64()?,
        };
        quota.check().map_err(|_| corrupt("quota"))?;
        let wall = Timestamp(inp.u64()?);
        let monotonic_ms = inp.u64()?;
        let boot_id = inp.u64()?;
        let flags = inp.u32()?;
        if (flags & !FLAG_MASK) != 0 {
            return Err(corrupt("flags"));
        }
        let legacy_ceiling = inp.u64()?;
        let ids = super::IdGen {
            object: inp.u64()?,
            manifest: inp.u64()?,
            generation: inp.u64()?,
            lease: inp.u64()?,
            reservation: inp.u64()?,
        };
        let cursor_mark = inp.u64()?;
        let logical_revision = inp.u64()?;
        let touches = inp.u64()?;
        let last_maintain = inp.opt_u64()?.map(Timestamp);
        let last_auto_clean = inp.opt_u64()?.map(Timestamp);
        let maintenance = inp.opt_u64()?.map(SessionId::new);
        let maps = super::publish::read_maps(&mut inp)?;
        let holds = super::lease::read_holds(&mut inp)?;
        let writers = read_writers(&mut inp)?;
        let pointers = read_pointers(&mut inp)?;
        let exports = super::legacy::read_exports(&mut inp)?;
        let history_reset = read_id_set(&mut inp, super::RootId::new)?;
        let unlink_failures = read_id_set(&mut inp, ObjectId::new)?;
        inp.finish()?;
        let life = Self {
            quota,
            wall,
            monotonic_ms,
            boot_id,
            clock_fault: flags & CLOCK_FAULT != 0,
            ids,
            roots: maps.roots,
            objects: maps.objects,
            manifests: maps.manifests,
            leases: holds.leases,
            reservations: holds.reservations,
            writers,
            pointers,
            exports,
            maintenance,
            cursor_mark,
            last_maintain,
            deferred: flags & DEFERRED != 0,
            incompatible: flags & INCOMPATIBLE != 0,
            legacy_frozen: flags & LEGACY_FROZEN != 0,
            legacy_ceiling,
            history_reset,
            unlink_failures,
            disk_full: flags & DISK_FULL != 0,
            pass_incomplete: flags & PASS_INCOMPLETE != 0,
            boot_accelerates: flags & BOOT_ACCELERATES != 0,
            logical_revision,
            touches,
            last_auto_clean,
            clean_ttl: None,
        };
        if !life.ids_cover() {
            return Err(corrupt("id cursor"));
        }
        life.debug_check();
        Ok(life)
    }

    fn ids_cover(&self) -> bool {
        let generations = self.manifests.values().all(|manifest| manifest.generation.raw() < self.ids.generation)
            && self.writers.values().all(|writer| {
                writer.generation.is_none_or(|id| id.raw() < self.ids.generation)
            })
            && self.pointers.values().all(|pointer| {
                pointer.current.is_none_or(|id| id.raw() < self.ids.generation)
                    && pointer.previous.is_none_or(|id| id.raw() < self.ids.generation)
            });
        self.ids.object > 0
            && self.ids.manifest > 0
            && self.ids.generation > 0
            && self.ids.lease > 0
            && self.ids.reservation > 0
            && self.objects.keys().all(|id| id.raw() < self.ids.object)
            && self.manifests.keys().all(|id| id.raw() < self.ids.manifest)
            && self.leases.keys().all(|id| id.raw() < self.ids.lease)
            && self.reservations.keys().all(|id| *id < self.ids.reservation)
            && generations
    }
}

fn read_id_set<T: Ord>(inp: &mut Take<'_>, wrap: fn(u64) -> T) -> Result<BTreeSet<T>, CacheError> {
    let len = inp.count()?;
    let mut set = BTreeSet::new();
    for _ in 0..len {
        if !set.insert(wrap(inp.u64()?)) {
            return Err(corrupt("duplicate"));
        }
    }
    Ok(set)
}

fn read_writers(inp: &mut Take<'_>) -> Result<BTreeMap<super::RootId, Writer>, CacheError> {
    let len = inp.count()?;
    let mut writers = BTreeMap::new();
    for _ in 0..len {
        let root = super::RootId::new(inp.u64()?);
        let writer = Writer {
            lock: lock_from(inp.u8()?)?,
            step: step_from(inp.u8()?)?,
            session: inp.opt_u64()?.map(SessionId::new),
            reservation: inp.opt_u64()?,
            segment: inp.opt_u64()?.map(ObjectId::new),
            manifest: inp.opt_u64()?.map(ManifestId::new),
            manifest_object: inp.opt_u64()?.map(ObjectId::new),
            generation: inp.opt_u64()?.map(GenerationId::new),
        };
        if writers.insert(root, writer).is_some() {
            return Err(corrupt("duplicate"));
        }
    }
    Ok(writers)
}

fn read_pointers(inp: &mut Take<'_>) -> Result<BTreeMap<super::RootId, super::writer::Pointer>, CacheError> {
    let len = inp.count()?;
    let mut pointers = BTreeMap::new();
    for _ in 0..len {
        let root = super::RootId::new(inp.u64()?);
        let pointer = super::writer::Pointer {
            current: inp.opt_u64()?.map(GenerationId::new),
            previous: inp.opt_u64()?.map(GenerationId::new),
        };
        if pointers.insert(root, pointer).is_some() {
            return Err(corrupt("duplicate"));
        }
    }
    Ok(pointers)
}

fn lock_from(tag: u8) -> Result<WriterLock, CacheError> {
    match tag {
        1 => Ok(WriterLock::Free),
        2 => Ok(WriterLock::Held),
        3 => Ok(WriterLock::Dead),
        _ => Err(corrupt("writer lock")),
    }
}

fn step_from(tag: u8) -> Result<WriterStep, CacheError> {
    match tag {
        1 => Ok(WriterStep::Idle),
        2 => Ok(WriterStep::Held),
        3 => Ok(WriterStep::Reserved),
        4 => Ok(WriterStep::Staged),
        5 => Ok(WriterStep::SegmentValid),
        6 => Ok(WriterStep::Manifest),
        7 => Ok(WriterStep::Ready),
        8 => Ok(WriterStep::Swapped),
        9 => Ok(WriterStep::Refused),
        _ => Err(corrupt("writer step")),
    }
}
