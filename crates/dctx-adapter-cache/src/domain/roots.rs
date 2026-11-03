// SPDX-License-Identifier: Apache-2.0
//! Root registration, last-use tracking, pinning and availability grace.
//! Implements the CACHE-v1 root table on [`super::Lifecycle`]. The facade registers roots and probes them.
//! In-memory use moves on every touch; the persisted stamp coalesces once per `LAST_USE_COALESCE_MS` window.
//! A repeated missing or offline probe does not restart the shared grace clock.

use super::ids::RootId;
use super::quota::LAST_USE_COALESCE_MS as COALESCE_MS;
use super::{Lifecycle, RootRecord};

impl Lifecycle {
    /// Registers a caller-opened root. A second registration of the same id is refused.
    /// # Errors
    /// Refuses an already registered root ID.
    pub fn register_root(&mut self, id: RootId) -> Result<(), crate::error::CacheError> {
        if self.roots.contains_key(&id) {
            return Err(crate::error::CacheError::InvalidRequest { reason: "root already registered" });
        }
        self.roots.insert(
            id,
            RootRecord {
                last_use: self.wall,
                persisted_use: self.wall,
                persist_writes: 1,
                unavailable_since: None,
                missing: false,
                offline: false,
                pinned: false,
                expired: false,
                last_generation: None,
            },
        );
        self.bump();
        Ok(())
    }

    /// In-memory last use always moves. The persisted stamp moves at most once per five minutes.
    /// # Errors
    /// Refuses an unknown root ID.
    pub fn touch(&mut self, root: RootId) -> Result<(), crate::error::CacheError> {
        if !self.roots.contains_key(&root) {
            return Err(crate::error::CacheError::InvalidRequest { reason: "unknown root" });
        }
        self.note_use(root);
        let wall = self.wall;
        for manifest in self.manifests.values_mut() {
            if manifest.root == root {
                manifest.last_use = wall;
            }
        }
        self.bump();
        Ok(())
    }

    /// Refreshes the root's in-memory last use and, at most once per five minutes, the persisted stamp.
    pub(super) fn note_use(&mut self, root: RootId) {
        let wall = self.wall;
        let Some(record) = self.roots.get_mut(&root) else {
            return;
        };
        record.last_use = wall;
        if wall.0.saturating_sub(record.persisted_use.0) >= COALESCE_MS {
            record.persisted_use = wall;
            record.persist_writes = record.persist_writes.saturating_add(1);
        }
    }

    /// An explicit pin opts out of expiry and LRU. It does not raise the hard cap.
    /// # Errors
    /// Refuses an unknown root ID.
    pub fn pin_root(&mut self, root: RootId, pinned: bool) -> Result<(), crate::error::CacheError> {
        let record = self.roots.get_mut(&root).ok_or(crate::error::CacheError::InvalidRequest {
            reason: "unknown root",
        })?;
        record.pinned = pinned;
        self.bump();
        Ok(())
    }

    /// Records a missing root. A repeated probe does not restart the seven-day grace clock.
    /// # Errors
    /// Refuses an unknown root ID.
    pub fn note_missing(&mut self, root: RootId) -> Result<(), crate::error::CacheError> {
        self.mark_unavailable(root, false)
    }

    /// Records an offline or permission failure. Grace is shared with [`Self::note_missing`].
    /// # Errors
    /// Refuses an unknown root ID.
    pub fn note_offline(&mut self, root: RootId) -> Result<(), crate::error::CacheError> {
        self.mark_unavailable(root, true)
    }

    /// The root is reachable again. Grace ends.
    /// # Errors
    /// Refuses an unknown root ID.
    pub fn note_online(&mut self, root: RootId) -> Result<(), crate::error::CacheError> {
        let record = self.roots.get_mut(&root).ok_or(crate::error::CacheError::InvalidRequest {
            reason: "unknown root",
        })?;
        record.missing = false;
        record.offline = false;
        record.unavailable_since = None;
        self.bump();
        Ok(())
    }

    fn mark_unavailable(
        &mut self,
        root: RootId,
        offline: bool,
    ) -> Result<(), crate::error::CacheError> {
        let wall = self.wall;
        let record = self.roots.get_mut(&root).ok_or(crate::error::CacheError::InvalidRequest {
            reason: "unknown root",
        })?;
        record.missing = true;
        if offline {
            record.offline = true;
        }
        if record.unavailable_since.is_none() {
            record.unavailable_since = Some(wall);
        }
        self.bump();
        Ok(())
    }
}
