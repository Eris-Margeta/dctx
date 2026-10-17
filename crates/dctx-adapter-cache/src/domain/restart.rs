//! Restart reconciliation and the versioned ledger image.
//! Implements CACHE-v1 and DX-A2 Recover: an OS lock plus session identity prove the holder is alive.
//! Depends on writer locks, leases and `dctx-hash`. Startup passes [`RestartEvidence`] on the first slice.
//! [`Lifecycle::encode_ledger`] is deterministic. Unknown versions and bad checksums are typed refusals.

use super::ids::{GenerationId, ManifestId, ObjectId, SessionId};
use super::lease::LeasePhase;
use super::writer::{Writer, WriterLock, WriterStep};
use super::{Lifecycle, Quota};
use crate::error::CacheError;
use dctx_core::ids::Timestamp;
use dctx_hash::Sha256;
use std::collections::{BTreeMap, BTreeSet};

/// Magic at the start of every ledger file. Bytes are ASCII `DCTXLEDG`.
pub const LEDGER_MAGIC: [u8; 8] = *b"DCTXLEDG";
/// Ledger body this crate reads. Any other version is [`CacheError::UnknownLedgerVersion`].
pub const LEDGER_VERSION: u32 = 1;
/// Largest accepted file, header and checksum included.
pub const LEDGER_MAX_BYTES: usize = 16 + 8 * 1024 * 1024 + 32;

const BODY_MAX: u32 = 8 * 1024 * 1024;
const COUNT_MAX: u32 = 100_000;
const TEXT_MAX: u32 = 4_096;
const FLAG_MASK: u32 = 0b111_1111;
const CLOCK_FAULT: u32 = 1 << 0;
const DEFERRED: u32 = 1 << 1;
const INCOMPATIBLE: u32 = 1 << 2;
const LEGACY_FROZEN: u32 = 1 << 3;
const DISK_FULL: u32 = 1 << 4;
const PASS_INCOMPLETE: u32 = 1 << 5;
const BOOT_ACCELERATES: u32 = 1 << 6;

/// Evidence from the host after a restart. There is no process-id field on purpose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartEvidence {
    /// Identity of the current boot. A change accelerates incomplete unowned staging only.
    pub boot_id: u64,
    /// Sessions whose OS locks are still held. A reused session id without a lock is dead.
    pub held_sessions: BTreeSet<SessionId>,
}

pub(super) fn corrupt(reason: &'static str) -> CacheError {
    CacheError::CorruptLedger { reason }
}

pub(super) struct Put {
    buf: Vec<u8>,
}

impl Put {
    pub(super) fn new() -> Self {
        Self { buf: Vec::new() }
    }

    pub(super) fn u8(&mut self, value: u8) {
        self.buf.push(value);
    }

    pub(super) fn u32(&mut self, value: u32) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    pub(super) fn u64(&mut self, value: u64) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    pub(super) fn opt_u64(&mut self, value: Option<u64>) {
        match value {
            None => self.u8(0),
            Some(inner) => {
                self.u8(1);
                self.u64(inner);
            }
        }
    }

    pub(super) fn count(&mut self, len: usize) -> Result<(), CacheError> {
        let len = u32::try_from(len).map_err(|_| corrupt("ledger is too large"))?;
        if len > COUNT_MAX {
            return Err(corrupt("ledger is too large"));
        }
        self.u32(len);
        Ok(())
    }

    pub(super) fn text(&mut self, value: &str) -> Result<(), CacheError> {
        let len = u32::try_from(value.len()).map_err(|_| corrupt("ledger is too large"))?;
        if len > TEXT_MAX {
            return Err(corrupt("ledger is too large"));
        }
        self.u32(len);
        self.buf.extend_from_slice(value.as_bytes());
        Ok(())
    }

    pub(super) fn finish(self) -> Vec<u8> {
        self.buf
    }
}

pub(super) struct Take<'a> {
    rest: &'a [u8],
}

impl<'a> Take<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Self {
        Self { rest: bytes }
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], CacheError> {
        if self.rest.len() < len {
            return Err(corrupt("truncated ledger"));
        }
        let (head, tail) = self.rest.split_at(len);
        self.rest = tail;
        Ok(head)
    }

    pub(super) fn u8(&mut self) -> Result<u8, CacheError> {
        let bytes = self.take(1)?;
        Ok(bytes[0])
    }

    pub(super) fn u32(&mut self) -> Result<u32, CacheError> {
        let mut buf = [0u8; 4];
        buf.copy_from_slice(self.take(4)?);
        Ok(u32::from_le_bytes(buf))
    }

    pub(super) fn u64(&mut self) -> Result<u64, CacheError> {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(self.take(8)?);
        Ok(u64::from_le_bytes(buf))
    }

    pub(super) fn opt_u64(&mut self) -> Result<Option<u64>, CacheError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.u64()?)),
            _ => Err(corrupt("optional")),
        }
    }

    pub(super) fn count(&mut self) -> Result<usize, CacheError> {
        let len = self.u32()?;
        if len > COUNT_MAX {
            return Err(corrupt("ledger is too large"));
        }
        Ok(len as usize)
    }

    pub(super) fn u64s(&mut self) -> Result<Vec<u64>, CacheError> {
        let len = self.count()?;
        let mut values = Vec::with_capacity(len);
        for _ in 0..len {
            values.push(self.u64()?);
        }
        Ok(values)
    }

    pub(super) fn text(&mut self) -> Result<String, CacheError> {
        let len = self.u32()?;
        if len > TEXT_MAX {
            return Err(corrupt("ledger is too large"));
        }
        let bytes = self.take(len as usize)?;
        std::str::from_utf8(bytes).map(str::to_owned).map_err(|_| corrupt("text"))
    }

    pub(super) fn finish(self) -> Result<(), CacheError> {
        if self.rest.is_empty() { Ok(()) } else { Err(corrupt("trailing bytes")) }
    }
}

impl Lifecycle {
    /// Applies lock and boot evidence. Unpublished temps of a dead writer are dropped.
    pub fn reconcile(&mut self, evidence: &RestartEvidence) {
        let boot_changed = evidence.boot_id != self.boot_id;
        if boot_changed {
            self.boot_id = evidence.boot_id;
            self.boot_accelerates = true;
        }
        let roots: Vec<super::RootId> = self.writers.keys().copied().collect();
        for root in roots {
            let session = self.writers.get(&root).and_then(|writer| writer.session);
            let held = session.is_some_and(|session| evidence.held_sessions.contains(&session));
            self.recover_writer(root, held);
        }
        let lease_ids: Vec<super::LeaseId> = self.leases.keys().copied().collect();
        for id in lease_ids {
            let Some(lease) = self.leases.get(&id) else {
                continue;
            };
            let open = matches!(
                lease.phase,
                LeasePhase::Active { .. } | LeasePhase::Acquiring { .. }
            );
            if open
                && !evidence.held_sessions.contains(&lease.session)
                && let Some(lease) = self.leases.get_mut(&id)
            {
                lease.phase = LeasePhase::OwnerProvenDead;
            }
        }
        if boot_changed {
            self.reclaim_abandoned_now();
            self.boot_accelerates = false;
        }
        self.bump();
        self.debug_check();
    }

    /// Canonical ledger file: magic, version, length, body, then a dctx-hash SHA-256.
    pub fn encode_ledger(&self) -> Result<Vec<u8>, CacheError> {
        let body = self.encode_body()?;
        let len = u32::try_from(body.len()).map_err(|_| CacheError::InvalidRequest {
            reason: "ledger is too large",
        })?;
        if len > BODY_MAX {
            return Err(CacheError::InvalidRequest { reason: "ledger is too large" });
        }
        let mut framed = Vec::with_capacity(16 + body.len() + 32);
        framed.extend_from_slice(&LEDGER_MAGIC);
        framed.extend_from_slice(&LEDGER_VERSION.to_le_bytes());
        framed.extend_from_slice(&len.to_le_bytes());
        framed.extend_from_slice(&body);
        let digest = Sha256::digest(&framed).map_err(|_| CacheError::InvalidRequest {
            reason: "ledger is too large",
        })?;
        framed.extend_from_slice(digest.as_bytes());
        Ok(framed)
    }

    /// Reads [`Self::encode_ledger`]. A bad checksum is corrupt. A foreign version is unknown.
    pub fn decode_ledger(bytes: &[u8]) -> Result<Self, CacheError> {
        if bytes.len() < 12 || bytes[..8] != LEDGER_MAGIC[..] {
            return Err(corrupt("ledger magic"));
        }
        let mut version_bytes = [0u8; 4];
        version_bytes.copy_from_slice(&bytes[8..12]);
        let version = u32::from_le_bytes(version_bytes);
        if version != LEDGER_VERSION {
            return Err(CacheError::UnknownLedgerVersion { found: version });
        }
        if bytes.len() < 48 {
            return Err(corrupt("truncated ledger"));
        }
        let mut len_bytes = [0u8; 4];
        len_bytes.copy_from_slice(&bytes[12..16]);
        let body_len = u32::from_le_bytes(len_bytes);
        if body_len > BODY_MAX {
            return Err(corrupt("ledger is too large"));
        }
        let body_len = body_len as usize;
        let total = 16usize.checked_add(body_len).and_then(|len| len.checked_add(32)).ok_or_else(|| {
            corrupt("ledger length")
        })?;
        if bytes.len() != total {
            return Err(corrupt("ledger length"));
        }
        let framed = &bytes[..16 + body_len];
        let digest = Sha256::digest(framed).map_err(|_| corrupt("checksum"))?;
        if digest.as_bytes().as_slice() != &bytes[16 + body_len..] {
            return Err(corrupt("checksum"));
        }
        Self::decode_body(&bytes[16..16 + body_len])
    }

    fn reclaim_abandoned_now(&mut self) {
        let doomed: Vec<super::ObjectId> = self
            .objects
            .keys()
            .copied()
            .filter(|id| self.staged_reclaimable(*id))
            .collect();
        for id in doomed {
            self.remove_object(id);
        }
    }

    fn encode_body(&self) -> Result<Vec<u8>, CacheError> {
        let mut out = Put::new();
        self.write_scalars(&mut out);
        super::publish::write_maps(self, &mut out)?;
        super::lease::write_holds(self, &mut out)?;
        self.write_locks(&mut out)?;
        super::legacy::write_exports(self, &mut out)?;
        self.write_sets(&mut out)?;
        Ok(out.finish())
    }

    fn write_scalars(&self, out: &mut Put) {
        let quota = &self.quota;
        out.u64(quota.hard_cap);
        out.u64(quota.soft_watermark);
        out.u64(quota.eviction_target);
        out.u64(quota.root_view_ceiling);
        out.u64(quota.root_pack_ceiling);
        out.u64(quota.root_preview_ceiling);
        out.u64(quota.emergency_metadata);
        out.u64(self.wall.0);
        out.u64(self.monotonic_ms);
        out.u64(self.boot_id);
        out.u32(self.pack_flags());
        out.u64(self.legacy_ceiling);
        out.u64(self.ids.object);
        out.u64(self.ids.manifest);
        out.u64(self.ids.generation);
        out.u64(self.ids.lease);
        out.u64(self.ids.reservation);
        out.u64(self.cursor_mark);
        out.u64(self.logical_revision);
        out.u64(self.touches);
        out.opt_u64(self.last_maintain.map(|stamp| stamp.0));
        out.opt_u64(self.last_auto_clean.map(|stamp| stamp.0));
        out.opt_u64(self.maintenance.map(SessionId::raw));
    }

    fn pack_flags(&self) -> u32 {
        let mut flags = 0u32;
        if self.clock_fault {
            flags |= CLOCK_FAULT;
        }
        if self.deferred {
            flags |= DEFERRED;
        }
        if self.incompatible {
            flags |= INCOMPATIBLE;
        }
        if self.legacy_frozen {
            flags |= LEGACY_FROZEN;
        }
        if self.disk_full {
            flags |= DISK_FULL;
        }
        if self.pass_incomplete {
            flags |= PASS_INCOMPLETE;
        }
        if self.boot_accelerates {
            flags |= BOOT_ACCELERATES;
        }
        flags
    }

    fn write_locks(&self, out: &mut Put) -> Result<(), CacheError> {
        out.count(self.writers.len())?;
        for (root, writer) in &self.writers {
            out.u64(root.raw());
            out.u8(lock_tag(writer.lock));
            out.u8(step_tag(writer.step));
            out.opt_u64(writer.session.map(SessionId::raw));
            out.opt_u64(writer.reservation);
            out.opt_u64(writer.segment.map(ObjectId::raw));
            out.opt_u64(writer.manifest.map(ManifestId::raw));
            out.opt_u64(writer.manifest_object.map(ObjectId::raw));
            out.opt_u64(writer.generation.map(GenerationId::raw));
        }
        out.count(self.pointers.len())?;
        for (root, pointer) in &self.pointers {
            out.u64(root.raw());
            out.opt_u64(pointer.current.map(GenerationId::raw));
            out.opt_u64(pointer.previous.map(GenerationId::raw));
        }
        Ok(())
    }

    fn write_sets(&self, out: &mut Put) -> Result<(), CacheError> {
        out.count(self.history_reset.len())?;
        for id in &self.history_reset {
            out.u64(id.raw());
        }
        out.count(self.unlink_failures.len())?;
        for id in &self.unlink_failures {
            out.u64(id.raw());
        }
        Ok(())
    }

    fn decode_body(bytes: &[u8]) -> Result<Self, CacheError> {
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

fn lock_tag(lock: WriterLock) -> u8 {
    match lock {
        WriterLock::Free => 1,
        WriterLock::Held => 2,
        WriterLock::Dead => 3,
    }
}

fn lock_from(tag: u8) -> Result<WriterLock, CacheError> {
    match tag {
        1 => Ok(WriterLock::Free),
        2 => Ok(WriterLock::Held),
        3 => Ok(WriterLock::Dead),
        _ => Err(corrupt("writer lock")),
    }
}

fn step_tag(step: WriterStep) -> u8 {
    match step {
        WriterStep::Idle => 1,
        WriterStep::Held => 2,
        WriterStep::Reserved => 3,
        WriterStep::Staged => 4,
        WriterStep::SegmentValid => 5,
        WriterStep::Manifest => 6,
        WriterStep::Ready => 7,
        WriterStep::Swapped => 8,
        WriterStep::Refused => 9,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ExportEvidence, FakeClock, KnownExport, Lifecycle, OutputPlace, Quota, RootId, SessionId,
        WriterPhase,
    };

    fn rich() -> Lifecycle {
        let mut life = Lifecycle::new(Quota::scaled(500)).expect("quota");
        life.reconcile(&RestartEvidence { boot_id: 42, held_sessions: BTreeSet::new() });
        life.follow(&FakeClock::at(10));
        let root = RootId::new(3);
        life.register_root(root).expect("root");
        life.publish_view(root, 20, &[]).expect("view");
        let lease = life.reader_begin(root, SessionId::new(4)).expect("begin");
        life.reader_confirm(lease).expect("confirm");
        let other = RootId::new(8);
        life.register_root(other).expect("root");
        life.writer_acquire(other, SessionId::new(9)).expect("acquire");
        life.writer_reserve(other, 30).expect("reserve");
        life.crash_writer(other).expect("crash");
        life.adopt_legacy(&KnownExport {
            place: OutputPlace::Registry,
            label: "out".to_string(),
            bytes: 4,
            evidence: ExportEvidence::OwnedText { identity: 3 },
        })
        .expect("adopt");
        life
    }

    #[test]
    fn ledger_bytes_round_trip_and_keep_lock_evidence() {
        let life = rich();
        let bytes = life.encode_ledger().expect("encode");
        assert_eq!(life.encode_ledger().expect("again"), bytes);
        let restored = Lifecycle::decode_ledger(&bytes).expect("decode");
        assert_eq!(restored.encode_ledger().expect("reencode"), bytes);
        assert_eq!(restored.boot_id(), 42);
        assert_eq!(restored.writer_phase(RootId::new(8)), WriterPhase::Dead);
        assert_eq!(restored.status().active_leases, 1);
        let mut kept = Lifecycle::decode_ledger(&bytes).expect("decode");
        let mut held = BTreeSet::new();
        held.insert(SessionId::new(4));
        held.insert(SessionId::new(9));
        kept.reconcile(&RestartEvidence { boot_id: 42, held_sessions: held });
        assert_eq!(kept.writer_phase(RootId::new(8)), WriterPhase::Dead);
        assert_eq!(kept.status().active_leases, 1);
        let mut dropped = Lifecycle::decode_ledger(&bytes).expect("decode");
        dropped.reconcile(&RestartEvidence { boot_id: 42, held_sessions: BTreeSet::new() });
        assert_eq!(dropped.writer_phase(RootId::new(8)), WriterPhase::Free);
        assert_eq!(dropped.status().active_leases, 0);
        assert_eq!(dropped.admitted(), life.admitted() - 30);
    }

    #[test]
    fn unknown_version_and_bad_checksum_are_distinct() {
        let bytes = Lifecycle::new(Quota::scaled(20)).expect("quota").encode_ledger().expect("encode");
        let mut unknown = bytes.clone();
        unknown[8..12].copy_from_slice(&99u32.to_le_bytes());
        assert!(matches!(
            Lifecycle::decode_ledger(&unknown),
            Err(CacheError::UnknownLedgerVersion { found: 99 })
        ));
        let mut broken = bytes.clone();
        let last = broken.len() - 1;
        broken[last] ^= 0xff;
        assert!(matches!(
            Lifecycle::decode_ledger(&broken),
            Err(CacheError::CorruptLedger { reason: "checksum" })
        ));
        assert!(matches!(
            Lifecycle::decode_ledger(&bytes[..20]),
            Err(CacheError::CorruptLedger { .. })
        ));
        assert!(matches!(
            Lifecycle::decode_ledger(b"not-a-ledger-file!!"),
            Err(CacheError::CorruptLedger { reason: "ledger magic" })
        ));
    }
}
