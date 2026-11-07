// SPDX-License-Identifier: Apache-2.0
//! In-memory `FactStore` and `Cache`: ephemeral storage and quota doubles with spy counters.
//! Implements the `FactStore` publish-is-commit rule and the `Cache` settle-once accounting
//! (committed + reserved ≤ capacity) so engine tests can observe publishes, reservations and refusals.
//! Depends on [`crate::fact_store`], [`crate::cache`] and dctx-core. Used by dctx-pack tests.

use crate::cache::Cache;
use crate::fact_store::FactStore;
use dctx_core::budget::Reservation;
use dctx_core::error::DctxError;
use dctx_core::facts::PackSnapshot;
use dctx_core::ids::ViewKey;
use std::collections::BTreeMap;

/// Snapshots kept in memory, keyed by view.
#[derive(Debug, Default)]
pub struct MemoryFactStore {
    snapshots: BTreeMap<String, PackSnapshot>,
    publishes: u64,
    fail_publish: bool,
}

impl MemoryFactStore {
    /// An empty store.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the memory adapter API's existing caller lint behavior."
    )]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of successful publishes.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the memory adapter API's existing caller lint behavior."
    )]
    pub fn publishes(&self) -> u64 {
        self.publishes
    }

    /// Makes every later publish fail with an IO error.
    pub fn fail_publishes(&mut self) {
        self.fail_publish = true;
    }
}

impl FactStore for MemoryFactStore {
    fn load(&self, view: &ViewKey) -> Result<Option<PackSnapshot>, DctxError> {
        Ok(self.snapshots.get(view.as_str()).cloned())
    }

    fn publish(&mut self, view: &ViewKey, snapshot: &PackSnapshot) -> Result<(), DctxError> {
        if self.fail_publish {
            return Err(DctxError::io("publish refused"));
        }
        self.snapshots
            .insert(view.as_str().to_string(), snapshot.clone());
        self.publishes += 1;
        Ok(())
    }
}

/// A quota of `capacity` bytes with exact reservation accounting.
#[derive(Debug)]
pub struct MemoryCache {
    capacity: u64,
    committed: u64,
    outstanding: BTreeMap<u64, u64>,
    next_id: u64,
}

impl MemoryCache {
    /// A cache admitting at most `capacity` bytes.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the memory adapter API's existing caller lint behavior."
    )]
    pub fn new(capacity: u64) -> Self {
        Self {
            capacity,
            committed: 0,
            outstanding: BTreeMap::new(),
            next_id: 1,
        }
    }

    /// Bytes committed so far.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the memory adapter API's existing caller lint behavior."
    )]
    pub fn committed(&self) -> u64 {
        self.committed
    }

    /// Bytes reserved and not yet settled.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the memory adapter API's existing caller lint behavior."
    )]
    pub fn reserved(&self) -> u64 {
        self.outstanding.values().sum()
    }
}

impl Cache for MemoryCache {
    fn reserve(&mut self, bytes: u64) -> Result<Reservation, DctxError> {
        let available = self
            .capacity
            .saturating_sub(self.committed.saturating_add(self.reserved()));
        if bytes > available {
            return Err(DctxError::CacheLimit {
                requested: bytes,
                available,
            });
        }
        let id = self.next_id;
        self.next_id += 1;
        self.outstanding.insert(id, bytes);
        Ok(Reservation::new(id, bytes))
    }

    fn commit(&mut self, reservation: Reservation, used_bytes: u64) -> Result<(), DctxError> {
        let reserved =
            self.outstanding
                .remove(&reservation.id())
                .ok_or(DctxError::InvalidRequest {
                    reason: "unknown reservation",
                })?;
        if used_bytes > reserved {
            self.outstanding.insert(reservation.id(), reserved);
            return Err(DctxError::InvalidRequest {
                reason: "commit exceeds reservation",
            });
        }
        self.committed += used_bytes;
        Ok(())
    }

    fn abort(&mut self, reservation: Reservation) {
        self.outstanding.remove(&reservation.id());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dctx_core::datatypes::{DirContextTreeNode, NodeType};
    use dctx_core::facts::ContentDigests;
    use dctx_core::ids::Timestamp;
    use dctx_core::version::Version;
    use std::path::PathBuf;

    fn snapshot() -> PackSnapshot {
        let root = DirContextTreeNode::new(NodeType::Directory, "", PathBuf::new());
        PackSnapshot::new(root, Version::new_v1(), Timestamp(1), ContentDigests::new()).unwrap()
    }

    #[test]
    fn store_publishes_and_loads_by_view() {
        let view = ViewKey::parse("v").unwrap();
        let mut store = MemoryFactStore::new();
        assert!(store.load(&view).unwrap().is_none());
        store.publish(&view, &snapshot()).unwrap();
        assert_eq!(store.publishes(), 1);
        assert_eq!(
            store.load(&view).unwrap().unwrap().digest(),
            snapshot().digest()
        );
        store.fail_publishes();
        assert!(store.publish(&view, &snapshot()).is_err());
        assert_eq!(store.publishes(), 1);
    }

    #[test]
    fn cache_admits_within_capacity_and_settles_once() {
        let mut cache = MemoryCache::new(10);
        let first = cache.reserve(6).unwrap();
        assert_eq!(
            cache.reserve(5),
            Err(DctxError::CacheLimit {
                requested: 5,
                available: 4
            })
        );
        let second = cache.reserve(4).unwrap();
        cache.commit(first, 5).unwrap();
        cache.abort(second);
        assert_eq!((cache.committed(), cache.reserved()), (5, 0));
        assert!(cache.reserve(5).is_ok());
    }

    #[test]
    fn cache_refuses_overcommit_and_unknown_reservations() {
        let mut cache = MemoryCache::new(10);
        let reservation = cache.reserve(3).unwrap();
        let copy = Reservation::new(reservation.id(), reservation.bytes());
        assert!(cache.commit(reservation, 4).is_err());
        assert_eq!(cache.reserved(), 3);
        cache.commit(copy, 3).unwrap();
        assert!(cache.commit(Reservation::new(99, 1), 1).is_err());
    }
}
