// SPDX-License-Identifier: Apache-2.0
//! Cache root discovery, the persisted facade ledger and the piggy-back auto-clean slice.
//! The session entry points that use these helpers stay in the host module.

use super::*;

use super::policy::store_policy;

/// Ledger file the facade persists inside the cache root.
const LEDGER_FILE: &str = "ledger.v1";

/// Root id the facade registers when a command piggy-backs auto-clean.
pub(super) const CLI_ROOT_ID: u64 = 1;

/// Set when this process ran piggy-back auto-clean before the map query.
pub(super) static MAP_AUTO_CLEAN: AtomicBool = AtomicBool::new(false);

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
/// # Errors
/// Returns cache-session setup or collection/purge failures.
#[allow(clippy::needless_pass_by_value, reason = "The public cache dispatcher consumes one command value; preserve its existing ownership contract.")]
pub fn session_cache(call: SessionCacheCall) -> Result<CacheReport, String> {
    let mut session = build(MemorySource::new(), "cache", "status", RuleSources::default())?;
    match call {
        SessionCacheCall::Status => Ok(session.cache_status()),
        SessionCacheCall::Gc { apply } => session.cache_gc(GcRequest { apply }).map_err(|error| error.to_string()),
        SessionCacheCall::Purge { apply } => session
            .cache_purge(PurgeRequest { confirm: true, apply, scope: PurgeScope::All })
            .map_err(|error| error.to_string()),
    }
}

/// `DCTX_CACHE_DIR` when it is set, otherwise the platform cache directory.
/// # Errors
/// Returns an invalid explicit cache root or failure to determine a supported system cache path.
pub fn cache_root_dir() -> Result<PathBuf, String> {
    if let Ok(raw) = std::env::var("DCTX_CACHE_DIR") {
        if !raw.is_empty() {
            let path = PathBuf::from(raw);
            absolute_root(&path)?;
            return Ok(path);
        }
    }
    platform_cache_dir()
}

/// True when this argv and `DCTX_AUTO_CLEAN` allow a piggy-back slice.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn auto_clean_allowed(args: &[String]) -> bool {
    auto_clean_enabled(args, std::env::var("DCTX_AUTO_CLEAN").ok().as_deref())
}

/// Pure form of [`auto_clean_allowed`]. `env_value` of `"0"` disables the slice.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn auto_clean_enabled(args: &[String], env_value: Option<&str>) -> bool {
    if args.iter().any(|arg| arg == "--no-auto-clean") {
        return false;
    }
    env_value != Some("0")
}

/// Drops `--no-auto-clean` so command parsers keep their existing flag sets.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn without_auto_flag(args: Vec<String>) -> Vec<String> {
    args.into_iter().filter(|arg| arg != "--no-auto-clean").collect()
}

/// Creates the cache root when it is missing. A symlink root is refused.
/// # Errors
/// Refuses relative/unsafe/non-directory roots or directory creation/metadata failures.
pub fn ensure_cache_dir(root: &Path) -> Result<(), String> {
    absolute_root(root)?;
    match fs::symlink_metadata(root) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
            Err("cache root is not a directory".to_string())
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => create_private_dir(root),
        Err(_) => Err("cache root metadata failed".to_string()),
    }
}

/// Loads the facade ledger. A missing file is an empty product ledger. Corruption is returned.
/// # Errors
/// Returns cache-root, lock, ledger I/O, or invalid/corrupt ledger failures.
pub fn load_ledger(root: &Path) -> Result<Lifecycle, String> {
    let store = FsLedgerStore::open(root).map_err(|error| error.to_string())?;
    read_opened(&store)
}

/// Atomically replaces an absent facade ledger.
/// A fresh store has not observed a committed revision, so an existing `ledger.v1` is `stale ledger`.
/// A load-modify-store cycle must reuse the store that observed the revision,
/// so stale writes can be refused.
/// # Errors
/// Returns cache-root, lock, ledger encoding, or durable persistence failures.
pub fn store_ledger(root: &Path, life: &Lifecycle) -> Result<(), String> {
    debug_assert_eq!(LEDGER_FILE, dctx_adapter_cache::adapters::LEDGER_FILE);
    FsLedgerStore::open(root).and_then(|store| store.store(life)).map_err(|error| error.to_string())
}

/// Loads, runs `body`, and stores through one [`FsLedgerStore`] while that store holds the cache lock.
/// `body` returns `(value, commit)`. A false `commit` leaves the committed file unchanged.
/// The store observes the revision on load. A different store that missed that observation still gets `stale ledger`.
pub(crate) fn with_ledger<T>(
    root: &Path,
    body: impl FnOnce(&mut Lifecycle) -> Result<(T, bool), String>,
) -> Result<T, String> {
    let store = FsLedgerStore::open(root).map_err(|error| error.to_string())?;
    store.update(Quota::product_default(), body).map_err(|error| error.to_string())
}

fn read_opened(store: &FsLedgerStore) -> Result<Lifecycle, String> {
    let loaded = store.load(Quota::product_default()).map_err(|error| error.to_string())?;
    match loaded.fault {
        Some(error) => Err(error.to_string()),
        None => Ok(loaded.lifecycle),
    }
}

/// One bounded facade auto-clean. Failures are ignored so the command still runs.
pub fn piggyback_auto_clean() {
    MAP_AUTO_CLEAN.store(true, Ordering::Relaxed);
    let Ok(root) = cache_root_dir() else {
        return;
    };
    let _ = piggyback_at(&root, true);
}

/// Runs auto-clean at `root` when `allow` and the persisted policy are both on.
/// Returns bytes reclaimed by this open. Zero when the slice is disabled or not due.
/// # Errors
/// Returns cache-root, policy, ledger, auto-clean/session, or policy persistence failures.
pub fn piggyback_at(root: &Path, allow: bool) -> Result<u64, String> {
    if !allow {
        return Ok(0);
    }
    ensure_cache_dir(root)?;
    let mut policy = load_policy(root)?;
    if !policy.auto.enabled {
        return Ok(0);
    }
    let before = load_ledger(root)?.last_auto_clean().map(|stamp| stamp.0);
    let session = open_cache_session(root, policy.auto)?;
    let view = session.cache_ledger().ok_or_else(|| "cache ledger is absent".to_string())?;
    if view.last_auto_clean_ms != before {
        policy.last_reclaimed_bytes = view.reclaimed_bytes;
        store_policy(root, &policy)?;
    }
    if view.reclaimed_bytes > 0 {
        eprintln!("cache auto-clean reclaimed {} bytes", view.reclaimed_bytes);
    }
    Ok(view.reclaimed_bytes)
}

fn open_cache_session(root: &Path, policy: AutoCleanConfig) -> Result<Session, String> {
    Session::builder(MemorySource::new())
        .clock(FixedClock(Timestamp(unix_millis())))
        .cancellation(Arc::new(CancelFlag::new()))
        .selection(SelectionPolicy::from_texts(&RuleTexts::default()))
        .ceiling(HostCeiling::base())
        .grants(HostGrants::PermitAll)
        .storage(StoragePolicy::Ephemeral { max_bytes: HARD_CAP_BYTES })
        .place("cache")
        .view("auto")
        .cache_root(root, CLI_ROOT_ID)
        .auto_clean(policy)
        .build()
        .map_err(facade_error)
}

fn platform_cache_dir() -> Result<PathBuf, String> {
    if cfg!(target_os = "windows") {
        let base = std::env::var("LOCALAPPDATA").map_err(|_| "LOCALAPPDATA is not set".to_string())?;
        return Ok(PathBuf::from(base).join("dctx").join("Cache").join("v2"));
    }
    if cfg!(target_os = "macos") {
        let home = std::env::var("HOME").map_err(|_| "HOME is not set".to_string())?;
        return Ok(PathBuf::from(home).join("Library").join("Caches").join("dctx").join("v2"));
    }
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        if !xdg.is_empty() {
            return Ok(PathBuf::from(xdg).join("dctx").join("v2"));
        }
    }
    let home = std::env::var("HOME").map_err(|_| "HOME is not set".to_string())?;
    Ok(PathBuf::from(home).join(".cache").join("dctx").join("v2"))
}

fn absolute_root(root: &Path) -> Result<(), String> {
    if !root.is_absolute() {
        return Err("cache root must be absolute".to_string());
    }
    if root.components().any(|component| matches!(component, Component::ParentDir)) {
        return Err("cache root must not contain ..".to_string());
    }
    Ok(())
}

fn create_private_dir(root: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        builder.mode(0o700);
        builder.create(root).map_err(|_| "failed to create the cache root".to_string())
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(root).map_err(|_| "failed to create the cache root".to_string())
    }
}
