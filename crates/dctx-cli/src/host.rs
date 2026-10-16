//! Facade session for pack, map, template and the session cache ledger.
//! Implements ARCHITECTURE-PROPOSAL §2: the CLI injects the clock, policy and ephemeral storage.
//! Depends on the `dctx` facade. Pack, map, template and cache call [`open_dir`] or [`session_cache`].
//! Nothing here discovers a home directory or writes a source tree.

use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use dctx::memory::{CancelFlag, FixedClock, MemorySource};
use dctx::{
    CacheReport, FsSource, GcRequest, HostCeiling, HostGrants, PurgeRequest, PurgeScope, RuleTexts,
    SelectionPolicy, Session, Source, StoragePolicy, Timestamp,
};
use dctx_adapter_cache::HARD_CAP_BYTES;

/// Ephemeral session with no source files. Template compile and cache status use it.
pub fn open_memory() -> Result<Session, String> {
    build(MemorySource::new(), "memory", "default")
}

/// Opens `path` as a facade source with the product cache ceiling.
pub fn open_dir(path: &Path) -> Result<Session, String> {
    if path.as_os_str().is_empty() {
        return Err("source path is empty".to_string());
    }
    let source = FsSource::open(path).map_err(|error| error.to_string())?;
    let canonical = std::fs::canonicalize(path).map_err(|error| error.to_string())?;
    let place = canonical
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("place");
    build(source, place, "default")
}

/// Which session-ledger operation the cache command mirrors.
pub enum SessionCacheCall {
    /// Read the ephemeral session ledger.
    Status,
    /// Collect unreachable session generations. `apply` deletes them.
    Gc {
        /// False reports and deletes nothing.
        apply: bool,
    },
    /// Confirmed session purge. The managed root purge is a separate adapter call.
    Purge {
        /// False reports and deletes nothing.
        apply: bool,
    },
}

/// Runs one cache operation on a fresh ephemeral session so the CLI calls the facade.
pub fn session_cache(call: SessionCacheCall) -> Result<CacheReport, String> {
    let mut session = build(MemorySource::new(), "cache", "status")?;
    match call {
        SessionCacheCall::Status => Ok(session.cache_status()),
        SessionCacheCall::Gc { apply } => session.cache_gc(GcRequest { apply }).map_err(|error| error.to_string()),
        SessionCacheCall::Purge { apply } => session
            .cache_purge(PurgeRequest { confirm: true, apply, scope: PurgeScope::All })
            .map_err(|error| error.to_string()),
    }
}

fn build(source: impl Source + Send + 'static, place: &str, view: &str) -> Result<Session, String> {
    Session::builder(source)
        .clock(FixedClock(Timestamp(unix_millis())))
        .cancellation(Arc::new(CancelFlag::new()))
        .selection(SelectionPolicy::from_texts(&RuleTexts::default()))
        .ceiling(HostCeiling::base())
        .grants(HostGrants::PermitAll)
        .storage(StoragePolicy::Ephemeral { max_bytes: HARD_CAP_BYTES })
        .place(place)
        .view(view)
        .build()
        .map_err(|error| error.to_string())
}

/// Unix milliseconds for the injected clock. The facade does not read the clock itself.
pub fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_cache_status_is_a_facade_report() {
        let report = session_cache(SessionCacheCall::Status).expect("status");
        assert_eq!(report.reclaimed_bytes, 0);
        assert_eq!(report.generations, 0);
        let gc = session_cache(SessionCacheCall::Gc { apply: false }).expect("gc");
        assert_eq!(gc.reclaimed_bytes, 0);
        let purge = session_cache(SessionCacheCall::Purge { apply: false }).expect("purge");
        assert_eq!(purge.reclaimed_bytes, 0);
    }

    #[test]
    fn empty_path_is_refused_before_a_scan() {
        assert!(open_dir(Path::new("")).is_err());
    }
}
