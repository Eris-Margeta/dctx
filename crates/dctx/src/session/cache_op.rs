//! Cache status, reachability collection and explicit purge.
//! Implements contracts/API-PORTS-v1.md cache administration.
//! Depends on [`super::Session`] and the shared ledger. A live lease blocks destruction.
//! Applying purge and collection unlink unleased bytes. A dry-run writes nothing.

use super::ledger::{DiskReclaim, PersistedCache};
use super::Session;
use crate::domain::{CacheLedger, CacheReport, GcRequest, PurgeRequest, PurgeScope, DctxError};

impl Session {
    /// Current ledger totals. `reclaimed_bytes` is the open-time auto-clean total, or zero in isolation.
    pub fn cache_status(&self) -> CacheReport {
        self.report(self.managed.as_ref().map_or(0, PersistedCache::reclaimed), 0)
    }

    /// Managed lifecycle figures. `None` in library isolation, where auto-clean does not run.
    pub fn cache_ledger(&self) -> Option<CacheLedger> {
        self.managed.as_ref().map(PersistedCache::ledger_view)
    }

    /// Drops generations whose handle lease is gone. `apply: false` deletes nothing.
    /// # Errors
    /// Refuses an unsupported cache capability or propagates disk reclamation and post-cache hook failures.
    pub fn cache_gc(&mut self, request: GcRequest) -> Result<CacheReport, DctxError> {
        if !cfg!(feature = "cache") {
            return Err(DctxError::unsupported("cache"));
        }
        self.reset_hook_warnings();
        let disk = self.disk_reclaim(request.apply)?;
        let mut report = self.reclaim(request.apply, None);
        apply_disk(&mut report, disk);
        if request.apply {
            self.hooks_after_cache()?;
        }
        Ok(report)
    }

    /// Drops generations in `request.scope` when no handle pins them.
    /// # Errors
    /// Refuses an unsupported cache capability, unconfirmed applying purge, or disk reclamation failures.
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
        let disk = if only.is_none() { self.disk_reclaim(request.apply)? } else { None };
        let mut report = self.reclaim(request.apply, only);
        apply_disk(&mut report, disk);
        Ok(report)
    }

    /// Shared-root purge and unlink. Isolation has no ledger, so the effect is `None`.
    fn disk_reclaim(&mut self, apply: bool) -> Result<Option<DiskReclaim>, DctxError> {
        let now = self.clock.now().millis();
        match self.managed.as_mut() {
            Some(cache) => cache.reclaim_unleased(now, apply).map(Some),
            None => Ok(None),
        }
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
            let pinned = self.pins.get(&id).and_then(std::sync::Weak::upgrade).is_some();
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

fn apply_disk(report: &mut CacheReport, disk: Option<DiskReclaim>) {
    let Some(disk) = disk else {
        return;
    };
    report.reclaimed_bytes = disk.reclaimed_bytes;
    if disk.held_by_lease > report.blocked_generations {
        report.blocked_generations = disk.held_by_lease;
    }
}
