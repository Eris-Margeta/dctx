//! Cache ledger. Segment bytes live here because the cache port cannot release a commit.
//! Implements API-PORTS-v1 `Cache.reserve` and CACHE-v1 startup auto-clean for a configured root.
//! Depends on the [`dctx_ports::Cache`] port and `dctx-adapter-cache` ledger and object-store adapters.
//! The session is the only caller. Library isolation leaves the managed ledger unset.

use std::path::{Component, Path};

use dctx_adapter_cache::adapters::{auto_clean, FsLedgerStore, FsObjectStore};
use dctx_adapter_cache::ports::LedgerStore;
use dctx_adapter_cache::{CacheError, ClockSample, Lifecycle, Quota, RootId};
use dctx_core::budget::Reservation;
use dctx_core::error::DctxError as CoreError;
use dctx_core::ids::Timestamp;
use dctx_ports::Cache;

use super::Session;
use crate::domain::{AutoCleanConfig, CacheLedger, DctxError};

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

/// Persisted lifecycle at a host-supplied cache root. Isolation sessions do not hold one.
pub(crate) struct PersistedCache {
    ledger: FsLedgerStore,
    objects: FsObjectStore,
    life: Lifecycle,
    root_id: RootId,
    reclaimed: u64,
}

impl PersistedCache {
    /// Loads `path`, runs auto-clean once when `config` is enabled, then registers `root_id`.
    pub(crate) fn open(path: &Path, root_id: u64, config: AutoCleanConfig, now_ms: u64) -> Result<Self, DctxError> {
        if !path.is_absolute() || path.components().any(|component| matches!(component, Component::ParentDir)) {
            return Err(DctxError::invalid("cache root must be absolute"));
        }
        let policy = config.to_policy();
        policy.check().map_err(map_cache)?;
        let ledger = FsLedgerStore::open(path).map_err(map_cache)?;
        let objects = FsObjectStore::open(path).map_err(map_cache)?;
        let loaded = ledger.load(Quota::product_default()).map_err(map_cache)?;
        if let Some(fault) = loaded.fault {
            return Err(map_cache(fault));
        }
        let mut life = loaded.lifecycle;
        let sample = clock_sample(now_ms);
        let reclaimed = if config.enabled {
            auto_clean(&ledger, &objects, &mut life, sample, policy).map_err(map_cache)?.reclaimed_bytes
        } else {
            0
        };
        let mut cache = Self { ledger, objects, life, root_id: RootId::new(root_id), reclaimed };
        cache.register_and_touch(now_ms)?;
        Ok(cache)
    }

    /// Bytes reclaimed by the open-time slice. Zero when the policy was disabled or the slice was not due.
    pub(crate) fn reclaimed(&self) -> u64 {
        self.reclaimed
    }

    /// Point-in-time ledger figures. This does not touch the root.
    pub(crate) fn ledger_view(&self) -> CacheLedger {
        let status = self.life.status();
        let expired = status.roots.iter().filter(|root| root.expired).count();
        CacheLedger {
            admitted_bytes: status.admitted,
            hard_cap: status.hard_cap,
            roots: u64::try_from(status.roots.len()).unwrap_or(u64::MAX),
            expired_roots: u64::try_from(expired).unwrap_or(u64::MAX),
            last_auto_clean_ms: self.life.last_auto_clean().map(|stamp| stamp.0),
            reclaimed_bytes: self.reclaimed,
        }
    }

    /// Refreshes last use and persists the ledger. A root that auto-clean removed is registered again.
    pub(crate) fn register_and_touch(&mut self, now_ms: u64) -> Result<(), DctxError> {
        self.life.observe(clock_sample(now_ms));
        if !self.life.status().roots.iter().any(|root| root.id == self.root_id) {
            self.life.register_root(self.root_id).map_err(map_cache)?;
        }
        self.life.touch(self.root_id).map_err(map_cache)?;
        self.ledger.store(&self.life).map_err(map_cache)?;
        Ok(())
    }

    /// Records one segment for the registered root and writes its bytes under the cache root.
    pub(crate) fn admit_segment(&mut self, now_ms: u64, bytes: &[u8]) -> Result<(), DctxError> {
        self.register_and_touch(now_ms)?;
        if bytes.is_empty() {
            return Ok(());
        }
        let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        let id = self.life.insert_segment(len, 0, &[self.root_id]).map_err(map_cache)?;
        self.objects.put_object(id.raw(), bytes).map_err(map_cache)?;
        self.ledger.store(&self.life).map_err(map_cache)?;
        Ok(())
    }
}

impl Session {
    /// Registers and touches the configured root. Isolation is a no-op.
    pub(super) fn touch_cache_root(&mut self) -> Result<(), DctxError> {
        let now = self.clock.now().millis();
        if let Some(cache) = self.managed.as_mut() {
            cache.register_and_touch(now)?;
        }
        Ok(())
    }

    /// Persists `bytes` as a segment of the configured root. Isolation is a no-op.
    pub(super) fn store_cache_segment(&mut self, bytes: &[u8]) -> Result<(), DctxError> {
        let now = self.clock.now().millis();
        if let Some(cache) = self.managed.as_mut() {
            cache.admit_segment(now, bytes)?;
        }
        Ok(())
    }
}

fn clock_sample(now_ms: u64) -> ClockSample {
    ClockSample { wall: Timestamp(now_ms), monotonic_ms: now_ms, boot_id: 1 }
}

fn map_cache(error: CacheError) -> DctxError {
    DctxError::from_core(error.into_dctx())
}

#[cfg(test)]
mod persisted_tests {
    use super::PersistedCache;
    use crate::domain::{AutoCleanConfig, ErrorCode};
    use std::path::Path;

    #[test]
    fn relative_and_raised_caps_are_refused_and_two_opens_share_the_root() {
        let relative = PersistedCache::open(Path::new("cache"), 1, AutoCleanConfig::default(), 0);
        assert_eq!(relative.map(|_| ()).expect_err("relative").code(), ErrorCode::InvalidRequest);
        let mut raised = AutoCleanConfig::default();
        raised.system_cap_bytes = raised.system_cap_bytes.saturating_add(1);
        let dir = tempfile::tempdir().expect("temp");
        let root = dir.path().join("cache");
        std::fs::create_dir(&root).expect("dir");
        let raised = PersistedCache::open(&root, 4, raised, 0);
        assert_eq!(raised.map(|_| ()).expect_err("cap").code(), ErrorCode::InvalidRequest);

        let first = PersistedCache::open(&root, 4, AutoCleanConfig::default(), 5).expect("first");
        assert_eq!(first.ledger_view().roots, 1);
        assert_eq!(first.ledger_view().last_auto_clean_ms, Some(5));
        assert_eq!(first.reclaimed(), 0);
        drop(first);
        let second = PersistedCache::open(&root, 4, AutoCleanConfig::default(), 5).expect("second");
        assert_eq!(second.ledger_view().roots, 1);
        assert_eq!(second.ledger_view().last_auto_clean_ms, Some(5));
        assert_eq!(second.reclaimed(), 0);

        let mut disabled = AutoCleanConfig::default();
        disabled.enabled = false;
        let third = PersistedCache::open(&root, 4, disabled, 5 + 86_400_000).expect("disabled");
        assert_eq!(third.ledger_view().last_auto_clean_ms, Some(5));
        assert_eq!(third.reclaimed(), 0);
    }
}
