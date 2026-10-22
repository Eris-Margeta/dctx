//! Restart reconciliation and the versioned ledger image.
//! Implements CACHE-v1 and DX-A2 Recover: an OS lock plus session identity prove the holder is alive.
//! Depends on writer locks, leases and `dctx-hash`. Startup passes [`RestartEvidence`] on the first slice.
//! [`Lifecycle::encode_ledger`] is deterministic. Unknown versions and bad checksums are typed refusals.

use super::ids::SessionId;
use super::lease::LeasePhase;
use super::Lifecycle;
use crate::error::CacheError;
use dctx_hash::Sha256;
use std::collections::BTreeSet;

pub(super) use super::ledger_io::{Put, Take, corrupt};

/// Magic at the start of every ledger file. Bytes are ASCII `DCTXLEDG`.
pub const LEDGER_MAGIC: [u8; 8] = *b"DCTXLEDG";
/// Ledger body this crate reads. Any other version is [`CacheError::UnknownLedgerVersion`].
pub const LEDGER_VERSION: u32 = 1;
/// Largest accepted file, header and checksum included.
pub const LEDGER_MAX_BYTES: usize = 16 + 8 * 1024 * 1024 + 32;

const BODY_MAX: u32 = 8 * 1024 * 1024;
pub(super) const CLOCK_FAULT: u32 = 1 << 0;
pub(super) const DEFERRED: u32 = 1 << 1;
pub(super) const INCOMPATIBLE: u32 = 1 << 2;
pub(super) const LEGACY_FROZEN: u32 = 1 << 3;
pub(super) const DISK_FULL: u32 = 1 << 4;
pub(super) const PASS_INCOMPLETE: u32 = 1 << 5;
pub(super) const BOOT_ACCELERATES: u32 = 1 << 6;

/// Evidence from the host after a restart. There is no process-id field on purpose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartEvidence {
    /// Identity of the current boot. A change accelerates incomplete unowned staging only.
    pub boot_id: u64,
    /// Sessions whose OS locks are still held. A reused session id without a lock is dead.
    pub held_sessions: BTreeSet<SessionId>,
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
    /// # Errors
    /// Refuses an oversized ledger, unencodable fields, or a checksum input above the hash length limit.
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
    /// # Errors
    /// Refuses unknown formats, malformed/truncated/oversized framing, checksum failures, or inconsistent ledger contents.
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
