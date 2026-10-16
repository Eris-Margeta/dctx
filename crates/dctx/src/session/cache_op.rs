//! Cache status, reachability collection and explicit purge.
//! Implements contracts/API-PORTS-v1.md cache administration.
//! Depends on [`super::Session`] and the ledger. A live handle blocks destruction.
//! Purge does not schedule a hook. Cache collection runs its hook only after an applying reclaim.

use super::Session;
use crate::domain::{CacheReport, GcRequest, PurgeRequest, PurgeScope, DctxError};

impl Session {
    /// Current ledger totals. `reclaimed_bytes` is 0.
    pub fn cache_status(&self) -> CacheReport {
        self.report(0, 0)
    }

    /// Drops generations whose handle lease is gone. `apply: false` deletes nothing.
    pub fn cache_gc(&mut self, request: GcRequest) -> Result<CacheReport, DctxError> {
        if !cfg!(feature = "cache") {
            return Err(DctxError::unsupported("cache"));
        }
        self.reset_hook_warnings();
        let report = self.reclaim(request.apply, None);
        if request.apply {
            self.hooks_after_cache()?;
        }
        Ok(report)
    }

    /// Drops generations in `request.scope` when no handle pins them.
    pub fn cache_purge(&mut self, request: PurgeRequest) -> Result<CacheReport, DctxError> {
        if !cfg!(feature = "cache") {
            return Err(DctxError::unsupported("cache"));
        }
        if !request.confirm {
            return Err(DctxError::invalid("purge requires confirm"));
        }
        let only = match request.scope {
            PurgeScope::All => None,
            PurgeScope::Generation(id) => Some(id),
        };
        Ok(self.reclaim(request.apply, only))
    }

    fn reclaim(&mut self, apply: bool, only: Option<u64>) -> CacheReport {
        let ids: Vec<u64> = self
            .generations
            .keys()
            .copied()
            .filter(|id| only.is_none_or(|wanted| wanted == *id))
            .collect();
        let mut reclaimed = 0u64;
        let mut blocked = 0u64;
        for id in ids {
            let pinned = self.pins.get(&id).and_then(|weak| weak.upgrade()).is_some();
            if pinned {
                blocked += 1;
                continue;
            }
            if !apply {
                continue;
            }
            if let Some(generation) = self.generations.remove(&id) {
                self.cache.segment_release(generation.encoded_len);
                reclaimed = reclaimed.saturating_add(generation.encoded_len);
            }
            self.pins.remove(&id);
        }
        self.report(reclaimed, blocked)
    }

    fn report(&self, reclaimed_bytes: u64, blocked_generations: u64) -> CacheReport {
        let pinned_generations = self.pins.values().filter(|weak| weak.upgrade().is_some()).count();
        CacheReport {
            committed_bytes: self.cache.committed(),
            reserved_bytes: self.cache.reserved(),
            generations: u64::try_from(self.generations.len()).unwrap_or(u64::MAX),
            pinned_generations: u64::try_from(pinned_generations).unwrap_or(u64::MAX),
            reclaimed_bytes,
            blocked_generations,
        }
    }
}
