// SPDX-License-Identifier: Apache-2.0
//! Cache ledger. Segment bytes and snapshot leases live in one shared root.
//! Implements API-PORTS-v1 `Cache.reserve` and CACHE-v1 auto-clean, purge, and collection.
//! Depends on the [`dctx_ports::Cache`] port, `dctx-adapter-cache`, and [`cache_bind`].
//! The session is the only caller. Library isolation leaves the managed ledger unset.
//! Two sessions on one root reload before they admit, pin, or collect.

mod cache_bind;

use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use dctx_adapter_cache::adapters::{FsLedgerStore, FsObjectStore, auto_clean};
use dctx_adapter_cache::ports::LedgerStore;
use dctx_adapter_cache::{CacheError, ClockSample, Lifecycle, Quota, RootId, SessionId};

pub(in crate::session) use cache_bind::{DiskReclaim, SnapshotLease};
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
        Self {
            capacity,
            pack_committed: 0,
            pack_reserved: 0,
            segment_committed: 0,
            inner,
        }
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

    /// Session cap and pack bytes that are not yet in the shared ledger.
    fn admission_budget(&self) -> (u64, u64, u64) {
        (self.capacity, self.pack_committed, self.pack_reserved)
    }

    /// Charges `bytes` of segment storage, or [`DctxError::CacheLimit`].
    pub(crate) fn segment_admit(&mut self, bytes: u64) -> Result<(), DctxError> {
        let available = self.available();
        if bytes > available {
            return Err(DctxError::CacheLimit {
                requested: bytes,
                available,
            });
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
            return Err(CoreError::CacheLimit {
                requested: bytes,
                available,
            });
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

#[allow(
    clippy::missing_fields_in_debug,
    reason = "Debug intentionally reports aggregate accounting without exposing the wrapped cache implementation."
)]
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
    root_path: PathBuf,
    session: SessionId,
    reclaimed: u64,
    /// One bound for persisted object admission and reads. Never below an admitted object.
    object_read_cap: u64,
}

static SESSION_IDS: AtomicU64 = AtomicU64::new(1);

fn next_session() -> SessionId {
    SessionId::new(SESSION_IDS.fetch_add(1, Ordering::Relaxed))
}

impl PersistedCache {
    /// Loads `path`, runs auto-clean once when `config` is enabled, then registers `root_id`.
    pub(crate) fn open(
        path: &Path,
        root_id: u64,
        config: AutoCleanConfig,
        now_ms: u64,
    ) -> Result<Self, DctxError> {
        if !path.is_absolute()
            || path
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(DctxError::invalid("cache root must be absolute"));
        }
        let policy = config.to_policy();
        policy.check().map_err(map_cache)?;
        let ledger = FsLedgerStore::open(path).map_err(map_cache)?;
        let objects = FsObjectStore::open(path).map_err(map_cache)?;
        let sample = clock_sample(now_ms);
        // Another session may commit between this load and the clean slice's commit; reload and run the slice again.
        let mut attempt = 0;
        let (life, reclaimed) = loop {
            let loaded = ledger.load(Quota::product_default()).map_err(map_cache)?;
            if let Some(fault) = loaded.fault {
                return Err(map_cache(fault));
            }
            let mut life = loaded.lifecycle;
            if !config.enabled {
                break (life, 0);
            }
            match auto_clean(&ledger, &objects, &mut life, sample, policy) {
                Ok(report) => break (life, report.reclaimed_bytes),
                Err(error)
                    if cache_bind::is_conflict(&error)
                        && attempt + 1 < cache_bind::COMMIT_ATTEMPTS =>
                {
                    attempt += 1
                }
                Err(error) => return Err(map_cache(error)),
            }
        };
        let mut cache = Self {
            ledger,
            objects,
            life,
            root_id: RootId::new(root_id),
            root_path: path.to_path_buf(),
            session: next_session(),
            reclaimed,
            object_read_cap: dctx_adapter_cache::HARD_CAP_BYTES,
        };
        cache.register_and_touch(now_ms)?;
        Ok(cache)
    }

    fn reload(&mut self) -> Result<(), DctxError> {
        let loaded = self
            .ledger
            .load(Quota::product_default())
            .map_err(map_cache)?;
        if let Some(fault) = loaded.fault {
            return Err(map_cache(fault));
        }
        self.life = loaded.lifecycle;
        Ok(())
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

    /// Reloads, refreshes last use, and persists. A root that auto-clean removed is registered again.
    /// The load, the edit and the commit happen under one exclusive lock, so a concurrent session cannot make it stale.
    pub(crate) fn register_and_touch(&mut self, now_ms: u64) -> Result<(), DctxError> {
        let root_id = self.root_id;
        let (_, life) = self
            .ledger
            .update_reclaiming(Quota::product_default(), &self.objects, |life| {
                life.observe(clock_sample(now_ms));
                if !life.status().roots.iter().any(|root| root.id == root_id) {
                    life.register_root(root_id).map_err(map_cache)?;
                }
                life.touch(root_id).map_err(map_cache)?;
                Ok::<_, DctxError>(((), true))
            })
            .map_err(cache_bind::update_error)?;
        self.life = life;
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

    /// Persists `bytes` as a leased view of the configured root. Isolation stores nothing.
    pub(super) fn store_cache_segment(
        &mut self,
        bytes: &[u8],
    ) -> Result<Option<SnapshotLease>, DctxError> {
        let now = self.clock.now().millis();
        let Some(cache) = self.managed.as_mut() else {
            return Ok(None);
        };
        let (capacity, pack_committed, pack_reserved) = self.cache.admission_budget();
        cache.admit_snapshot(now, bytes, capacity, pack_committed, pack_reserved)
    }
}

fn clock_sample(now_ms: u64) -> ClockSample {
    ClockSample {
        wall: Timestamp(now_ms),
        monotonic_ms: now_ms,
        boot_id: 1,
    }
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
        assert_eq!(
            relative.map(|_| ()).expect_err("relative").code(),
            ErrorCode::InvalidRequest
        );
        let mut raised = AutoCleanConfig::default();
        raised.system_cap_bytes = raised.system_cap_bytes.saturating_add(1);
        let dir = tempfile::tempdir().expect("temp");
        let root = dir.path().join("cache");
        std::fs::create_dir(&root).expect("dir");
        let raised = PersistedCache::open(&root, 4, raised, 0);
        assert_eq!(
            raised.map(|_| ()).expect_err("cap").code(),
            ErrorCode::InvalidRequest
        );

        let first = PersistedCache::open(&root, 4, AutoCleanConfig::default(), 5).expect("first");
        assert_eq!(first.ledger_view().roots, 1);
        assert_eq!(first.ledger_view().last_auto_clean_ms, Some(5));
        assert_eq!(first.reclaimed(), 0);
        drop(first);
        let second = PersistedCache::open(&root, 4, AutoCleanConfig::default(), 5).expect("second");
        assert_eq!(second.ledger_view().roots, 1);
        assert_eq!(second.ledger_view().last_auto_clean_ms, Some(5));
        assert_eq!(second.reclaimed(), 0);

        let disabled = AutoCleanConfig {
            enabled: false,
            ..AutoCleanConfig::default()
        };
        let third = PersistedCache::open(&root, 4, disabled, 5 + 86_400_000).expect("disabled");
        assert_eq!(third.ledger_view().last_auto_clean_ms, Some(5));
        assert_eq!(third.reclaimed(), 0);
    }
}
