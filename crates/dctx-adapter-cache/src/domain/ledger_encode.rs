// SPDX-License-Identifier: Apache-2.0
//! Deterministic encoding of lifecycle ledger state.
//! Implements CACHE-v1 scalar flags, writer locks, recovery pointers and id sets.
//! Depends on ledger primitives and domain map codecs; restart frames this body.

use super::Lifecycle;
use super::ids::{GenerationId, ManifestId, ObjectId, SessionId};
use super::restart::{
    BOOT_ACCELERATES, CLOCK_FAULT, DEFERRED, DISK_FULL, INCOMPATIBLE, LEGACY_FROZEN,
    PASS_INCOMPLETE, Put,
};
use super::writer::{WriterLock, WriterStep};
use crate::error::CacheError;

impl Lifecycle {
    pub(super) fn encode_body(&self) -> Result<Vec<u8>, CacheError> {
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
}

fn lock_tag(lock: WriterLock) -> u8 {
    match lock {
        WriterLock::Free => 1,
        WriterLock::Held => 2,
        WriterLock::Dead => 3,
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
