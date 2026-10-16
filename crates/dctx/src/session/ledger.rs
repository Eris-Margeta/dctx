//! Cache ledger. Segment bytes live here because the cache port cannot release a commit.
//! Implements contracts/API-PORTS-v1.md `Cache.reserve` for pack and an explicit segment quota.
//! Depends on the [`dctx_ports::Cache`] port. The session is the only caller.
//! The inner cache sees pack reservations only. Segment admission never calls `commit`.

use dctx_core::budget::Reservation;
use dctx_core::error::DctxError as CoreError;
use dctx_ports::Cache;

use crate::domain::DctxError;

/// Quota shared by pack publications and map segments.
pub(crate) struct AccountingCache {
    capacity: u64,
    pack_committed: u64,
    pack_reserved: u64,
    segment_committed: u64,
    inner: Box<dyn Cache + Send>,
}

impl AccountingCache {
    /// `inner` should be large enough that this wrapper is the real quota.
    pub(crate) fn new(capacity: u64, inner: Box<dyn Cache + Send>) -> Self {
        Self { capacity, pack_committed: 0, pack_reserved: 0, segment_committed: 0, inner }
    }

    /// Bytes still available for a new admission.
    pub(crate) fn available(&self) -> u64 {
        self.capacity
            .saturating_sub(self.pack_committed)
            .saturating_sub(self.pack_reserved)
            .saturating_sub(self.segment_committed)
    }

    /// Pack committed bytes plus segment bytes.
    pub(crate) fn committed(&self) -> u64 {
        self.pack_committed.saturating_add(self.segment_committed)
    }

    /// Outstanding pack reservations.
    pub(crate) fn reserved(&self) -> u64 {
        self.pack_reserved
    }

    /// Charges `bytes` of segment storage, or [`DctxError::CacheLimit`].
    pub(crate) fn segment_admit(&mut self, bytes: u64) -> Result<(), DctxError> {
        let available = self.available();
        if bytes > available {
            return Err(DctxError::CacheLimit { requested: bytes, available });
        }
        self.segment_committed = self.segment_committed.saturating_add(bytes);
        Ok(())
    }

    /// Returns segment bytes after a generation is dropped.
    pub(crate) fn segment_release(&mut self, bytes: u64) {
        self.segment_committed = self.segment_committed.saturating_sub(bytes);
    }
}

impl Cache for AccountingCache {
    fn reserve(&mut self, bytes: u64) -> Result<Reservation, CoreError> {
        let available = self.available();
        if bytes > available {
            return Err(CoreError::CacheLimit { requested: bytes, available });
        }
        let reservation = self.inner.reserve(bytes)?;
        self.pack_reserved = self.pack_reserved.saturating_add(bytes);
        Ok(reservation)
    }

    fn commit(&mut self, reservation: Reservation, used_bytes: u64) -> Result<(), CoreError> {
        let reserved = reservation.bytes();
        self.inner.commit(reservation, used_bytes)?;
        self.pack_reserved = self.pack_reserved.saturating_sub(reserved);
        self.pack_committed = self.pack_committed.saturating_add(used_bytes);
        Ok(())
    }

    fn abort(&mut self, reservation: Reservation) {
        let reserved = reservation.bytes();
        self.inner.abort(reservation);
        self.pack_reserved = self.pack_reserved.saturating_sub(reserved);
    }
}

impl std::fmt::Debug for AccountingCache {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AccountingCache")
            .field("capacity", &self.capacity)
            .field("committed", &self.committed())
            .field("reserved", &self.reserved())
            .finish()
    }
}

/// Largest file a later read will pull back for verification.
pub(crate) const READ_CAP: u64 = 8 * 1024 * 1024;
