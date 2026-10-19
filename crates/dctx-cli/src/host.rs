//! Facade session for pack, map, template and the persisted cache ledger.
//! Implements ARCHITECTURE-PROPOSAL §2 and §4: the CLI injects the clock, policy and cache root.
//! Depends on the `dctx` facade and `dctx-adapter-cache` for the ledger file the facade opens.
//! Pack, map, template, plugin and the TUI call [`piggyback_auto_clean`].
//! A cache edit holds one ledger store across the load and the store.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use dctx::memory::{CancelFlag, FixedClock, MemorySource};
use dctx::{
    AutoCleanConfig, CacheReport, DctxError, FsSource, GcRequest, HostCeiling, HostGrants, PurgeRequest, PurgeScope,
    RuleTexts, SelectionPolicy, Session, Source, StoragePolicy, Timestamp,
};
use dctx_adapter_cache::adapters::FsLedgerStore;
use dctx_adapter_cache::ports::LedgerStore;
use dctx_adapter_cache::{Lifecycle, Quota, HARD_CAP_BYTES};

/// Ledger file the facade persists inside the cache root.
const LEDGER_FILE: &str = "ledger.v1";
/// CLI policy beside the ledger. Cache purge does not own this file.
const POLICY_FILE: &str = "policy.v1";
/// Root id the facade registers when a command piggy-backs auto-clean.
const CLI_ROOT_ID: u64 = 1;

/// Set when this process ran piggy-back auto-clean before the map query.
static MAP_AUTO_CLEAN: AtomicBool = AtomicBool::new(false);

/// Persisted auto-clean policy and the last reported reclaim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CachePolicy {
    /// Policy passed to the facade on the next open.
    pub auto: AutoCleanConfig,
    /// Bytes the last due slice reported. Zero before the first reclaim.
    pub last_reclaimed_bytes: u64,
}

/// What `dctx cache auto` prints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoReport {
    /// Persisted enable flag.
    pub enabled: bool,
    /// Ledger stamp, when a slice has run.
    pub last_auto_clean_ms: Option<u64>,
    /// Bytes recorded for that slice.
    pub reclaimed_bytes: u64,
    /// Wall time the next slice becomes due.
    pub next_due_ms: u64,
}

/// Ephemeral session with no source files. Template compile uses it.
pub fn open_memory() -> Result<Session, String> {
    build(MemorySource::new(), "memory", "default")
}

/// Opens `path` as a facade source with the product cache ceiling.
pub fn open_dir(path: &Path) -> Result<Session, String> {
    let (source, place, _) = open_source(path)?;
    build(source, &place, "default")
}

/// Opens `path` for a map query and returns the canonical source key.
///
/// An absolute cache root is attached when one can be created. A relative `DCTX_CACHE_DIR`,
/// or a cache that will not open, falls back to an ephemeral session. Nothing is written
/// inside the scanned project.
pub fn open_mapped(path: &Path) -> Result<(Session, String), String> {
    let (source, place, key) = open_source(path)?;
    let Ok(root) = cache_root_dir() else {
        return Ok((build(source, &place, "default")?, key));
    };
    if ensure_cache_dir(&root).is_err() {
        return Ok((build(source, &place, "default")?, key));
    }
    match build_cached(source, &place, &root) {
        Ok(session) => Ok((session, key)),
        Err(_) => {
            let (source, place, key) = open_source(path)?;
            Ok((build(source, &place, "default")?, key))
        }
    }
}

/// True when map should print whether the generation was reused. Set `DCTX_REUSE_REPORT=1`.
pub fn reuse_report_enabled() -> bool {
    std::env::var("DCTX_REUSE_REPORT").ok().as_deref() == Some("1")
}

fn open_source(path: &Path) -> Result<(FsSource, String, String), String> {
    if path.as_os_str().is_empty() {
        return Err("source path is empty".to_string());
    }
    let source = FsSource::open(path).map_err(|error| error.to_string())?;
    let canonical = std::fs::canonicalize(path).map_err(|error| error.to_string())?;
    let place = canonical
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("place")
        .to_string();
    let key = match canonical.to_str() {
        Some(text) => text.to_string(),
        None => canonical.to_string_lossy().into_owned(),
    };
    Ok((source, place, key))
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

/// `DCTX_CACHE_DIR` when it is set, otherwise the platform cache directory.
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
pub fn auto_clean_allowed(args: &[String]) -> bool {
    auto_clean_enabled(args, std::env::var("DCTX_AUTO_CLEAN").ok().as_deref())
}

/// Pure form of [`auto_clean_allowed`]. `env_value` of `"0"` disables the slice.
pub fn auto_clean_enabled(args: &[String], env_value: Option<&str>) -> bool {
    if args.iter().any(|arg| arg == "--no-auto-clean") {
        return false;
    }
    env_value != Some("0")
}

/// Drops `--no-auto-clean` so command parsers keep their existing flag sets.
pub fn without_auto_flag(args: Vec<String>) -> Vec<String> {
    args.into_iter().filter(|arg| arg != "--no-auto-clean").collect()
}

/// Creates the cache root when it is missing. A symlink root is refused.
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
pub fn load_ledger(root: &Path) -> Result<Lifecycle, String> {
    let store = FsLedgerStore::open(root).map_err(|error| error.to_string())?;
    read_opened(&store)
}

/// Atomically replaces an absent facade ledger.
/// A fresh store has not observed a committed revision, so an existing `ledger.v1` is `stale ledger`.
/// A load-modify-store cycle uses [`with_ledger`] on the store that observed the revision.
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

/// Reads `policy.v1`. A missing file is the product default.
pub fn load_policy(root: &Path) -> Result<CachePolicy, String> {
    let path = root.join(POLICY_FILE);
    match fs::read_to_string(&path) {
        Ok(text) => parse_policy(&text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(CachePolicy::default()),
        Err(_) => Err("corrupt cache policy: unreadable".to_string()),
    }
}

/// Shows the policy, or stores a tightened one. A show does not create the file.
pub fn configure(
    root: &Path,
    auto: Option<bool>,
    max_bytes: Option<u64>,
    ttl_days: Option<u64>,
) -> Result<CachePolicy, String> {
    let mut policy = load_policy(root)?;
    if auto.is_none() && max_bytes.is_none() && ttl_days.is_none() {
        return Ok(policy);
    }
    if let Some(enabled) = auto {
        policy.auto.enabled = enabled;
    }
    if let Some(days) = ttl_days {
        let millis = days.saturating_mul(dctx_adapter_cache::DAY_MS);
        policy.auto.overlay_ttl_ms = millis;
        policy.auto.preview_ttl_ms = millis;
    }
    if let Some(cap) = max_bytes {
        apply_cap(root, &mut policy, cap)?;
    }
    store_policy(root, &policy)?;
    Ok(policy)
}

/// Last slice, recorded reclaim and the next due time. This does not run a slice.
pub fn auto_report(root: &Path) -> Result<AutoReport, String> {
    let policy = load_policy(root)?;
    let last = load_ledger(root)?.last_auto_clean().map(|stamp| stamp.0);
    let next_due_ms = match last {
        Some(stamp) => stamp.saturating_add(policy.auto.min_interval_ms),
        None => unix_millis(),
    };
    Ok(AutoReport {
        enabled: policy.auto.enabled,
        last_auto_clean_ms: last,
        reclaimed_bytes: policy.last_reclaimed_bytes,
        next_due_ms,
    })
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

fn apply_cap(root: &Path, policy: &mut CachePolicy, cap: u64) -> Result<(), String> {
    if cap > HARD_CAP_BYTES {
        return Err("system cap cannot exceed the product hard cap".to_string());
    }
    with_ledger(root, |life| {
        let current = life.status().hard_cap;
        if cap > current {
            return Err("max-bytes can only tighten the system cap".to_string());
        }
        if cap < current {
            life.set_hard_cap(cap).map_err(|error| error.to_string())?;
        }
        Ok(((), cap < current))
    })?;
    policy.auto.system_cap_bytes = cap;
    Ok(())
}

fn store_policy(root: &Path, policy: &CachePolicy) -> Result<(), String> {
    let body = format!(
        "enabled={}\nmin_interval_ms={}\noverlay_ttl_ms={}\npreview_ttl_ms={}\nroot_ttl_ms={}\nstaging_ttl_ms={}\nsystem_cap_bytes={}\nlast_reclaimed_bytes={}\n",
        u8::from(policy.auto.enabled),
        policy.auto.min_interval_ms,
        policy.auto.overlay_ttl_ms,
        policy.auto.preview_ttl_ms,
        policy.auto.root_ttl_ms,
        policy.auto.staging_ttl_ms,
        policy.auto.system_cap_bytes,
        policy.last_reclaimed_bytes
    );
    let temp = root.join("policy.v1.tmp");
    write_private(&temp, body.as_bytes())?;
    fs::rename(&temp, root.join(POLICY_FILE)).map_err(|_| "failed to commit the cache policy".to_string())
}

fn parse_policy(text: &str) -> Result<CachePolicy, String> {
    let mut policy = CachePolicy::default();
    let mut seen = [false; 8];
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (key, value) = line.split_once('=').ok_or_else(|| "corrupt cache policy: line".to_string())?;
        let index = policy_key(key)?;
        if seen[index] {
            return Err("corrupt cache policy: duplicate".to_string());
        }
        seen[index] = true;
        assign_policy(&mut policy, index, value)?;
    }
    if seen.iter().any(|present| !present) {
        return Err("corrupt cache policy: missing field".to_string());
    }
    Ok(policy)
}

fn policy_key(key: &str) -> Result<usize, String> {
    let index = [
        "enabled",
        "min_interval_ms",
        "overlay_ttl_ms",
        "preview_ttl_ms",
        "root_ttl_ms",
        "staging_ttl_ms",
        "system_cap_bytes",
        "last_reclaimed_bytes",
    ]
    .iter()
    .position(|name| *name == key)
    .ok_or_else(|| "corrupt cache policy: field".to_string())?;
    Ok(index)
}

fn assign_policy(policy: &mut CachePolicy, index: usize, value: &str) -> Result<(), String> {
    let number: u64 = value.parse().map_err(|_| "corrupt cache policy: number".to_string())?;
    match index {
        0 => {
            policy.auto.enabled = match number {
                0 => false,
                1 => true,
                _ => return Err("corrupt cache policy: enabled".to_string()),
            };
        }
        1 => policy.auto.min_interval_ms = number,
        2 => policy.auto.overlay_ttl_ms = number,
        3 => policy.auto.preview_ttl_ms = number,
        4 => policy.auto.root_ttl_ms = number,
        5 => policy.auto.staging_ttl_ms = number,
        6 => policy.auto.system_cap_bytes = number,
        _ => policy.last_reclaimed_bytes = number,
    }
    Ok(())
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

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|_| "failed to write the cache policy".to_string())?;
    file.write_all(bytes).map_err(|_| "failed to write the cache policy".to_string())?;
    Ok(())
}

fn facade_error(error: DctxError) -> String {
    match error {
        DctxError::CorruptIndex { reason } => format!("corrupt ledger: {reason}"),
        other => other.to_string(),
    }
}

fn build_cached(source: impl Source + Send + 'static, place: &str, root: &Path) -> Result<Session, String> {
    let mut policy = load_policy(root).map(|item| item.auto).unwrap_or_else(|_| AutoCleanConfig::default());
    let env_off = std::env::var("DCTX_AUTO_CLEAN").ok().as_deref() == Some("0");
    if !MAP_AUTO_CLEAN.load(Ordering::Relaxed) || env_off {
        policy.enabled = false;
    }
    Session::builder(source)
        .clock(FixedClock(Timestamp(unix_millis())))
        .cancellation(Arc::new(CancelFlag::new()))
        .selection(SelectionPolicy::from_texts(&RuleTexts::default()))
        .ceiling(HostCeiling::base())
        .grants(HostGrants::PermitAll)
        .storage(StoragePolicy::Ephemeral { max_bytes: HARD_CAP_BYTES })
        .place(place)
        .view("default")
        .cache_root(root, CLI_ROOT_ID)
        .auto_clean(policy)
        .build()
        .map_err(facade_error)
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

    #[test]
    fn relative_cache_roots_and_the_auto_clean_switch_are_refused() {
        assert!(ensure_cache_dir(Path::new("cache")).is_err());
        assert!(ensure_cache_dir(Path::new("/tmp/../dctx-cache")).is_err());
        assert!(!auto_clean_enabled(&["--no-auto-clean".to_string()], None));
        assert!(!auto_clean_enabled(&[], Some("0")));
        assert!(auto_clean_enabled(&[], Some("1")));
        let kept = without_auto_flag(vec!["dctx".to_string(), "--no-auto-clean".to_string(), ".".to_string()]);
        assert_eq!(kept, vec!["dctx".to_string(), ".".to_string()]);
    }

    #[test]
    fn disabled_auto_clean_keeps_staged_bytes_and_an_enabled_slice_reclaims_them() {
        let root = scratch("auto");
        let mut life = Lifecycle::product_default();
        life.load_staged(1_000, 0).expect("staged");
        life.load_staged(1_000, 0).expect("staged");
        store_ledger(&root, &life).expect("store");
        let before = fs::read(root.join("ledger.v1")).expect("ledger");
        configure(&root, Some(false), None, None).expect("off");
        let skipped = piggyback_at(&root, true).expect("disabled");
        assert_eq!(skipped, 0);
        assert_eq!(fs::read(root.join("ledger.v1")).expect("ledger"), before);
        assert!(load_ledger(&root).expect("load").last_auto_clean().is_none());
        let blocked = piggyback_at(&root, false).expect("flag");
        assert_eq!(blocked, 0);
        configure(&root, Some(true), None, None).expect("on");
        let reclaimed = piggyback_at(&root, true).expect("clean");
        assert_eq!(reclaimed, 2_000);
        let restored = load_ledger(&root).expect("load");
        assert_eq!(restored.admitted(), 0);
        assert!(restored.last_auto_clean().is_some());
        let report = auto_report(&root).expect("auto");
        assert_eq!(report.reclaimed_bytes, 2_000);
        assert!(report.enabled);
        configure(&root, None, Some(1_000_000), None).expect("first cap");
        configure(&root, None, Some(500_000), None).expect("second cap");
        assert_eq!(load_ledger(&root).expect("load").status().hard_cap, 500_000);
        let page = crate::cache::run_in(&args(&["cache", "auto"]), Some(&root)).expect("page");
        assert!(page.contains("reclaimed_bytes: 2000"));
        assert!(page.contains("last_auto_clean_ms: "));
        assert!(!page.contains("last_auto_clean_ms: none"));
    }

    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("dctx-cli-host-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("dir");
        path
    }

    fn args(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_string()).collect()
    }
}
