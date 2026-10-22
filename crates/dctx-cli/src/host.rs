//! Facade session for pack, map, template and the persisted cache ledger.
//! Implements ARCHITECTURE-PROPOSAL §2 and §4: the CLI injects the clock, policy and cache root.
//! Depends on the `dctx` facade, `dctx-core` selection and `dctx-adapter-cache`.
//! Pack, map, template, plugin and the TUI call [`piggyback_auto_clean`].
//! A cache edit holds one ledger store across the load and the store.
//! Project ignore and omit texts are loaded here; `DCTX_INDEX_IGNORED=1` skips ignore texts.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use dctx::memory::{CancelFlag, FixedClock, MemorySource};
use dctx::{
    AutoCleanConfig, CacheReport, DctxError, FileHandlingDecision, FsSource, GcRequest, HostCeiling, HostGrants,
    PurgeRequest, PurgeScope, RuleSources, RuleTexts, SelectionPolicy, Session, Source, StoragePolicy, Timestamp,
};
use dctx_core::settings::APP_NAME;
use dctx_core::ItemContext;
use dctx_adapter_cache::adapters::FsLedgerStore;
use dctx_adapter_cache::ports::LedgerStore;
use dctx_adapter_cache::{Lifecycle, Quota, HARD_CAP_BYTES};

/// Ledger file the facade persists inside the cache root.
const LEDGER_FILE: &str = "ledger.v1";
/// CLI policy beside the ledger. Cache purge does not own this file.
const POLICY_FILE: &str = "policy.v1";
/// Root id the facade registers when a command piggy-backs auto-clean.
const CLI_ROOT_ID: u64 = 1;
/// One rule file. A larger file is refused, matching the desktop loader.
const MAX_RULE_FILE_BYTES: u64 = 1_048_576;
/// Nested `.gitignore` walk stops after this many directory levels.
const MAX_RULE_DEPTH: u32 = 256;

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
/// # Errors
/// Returns session builder or storage/cache setup failures.
pub fn open_memory() -> Result<Session, String> {
    build(MemorySource::new(), "memory", "default", RuleSources::default())
}

/// Opens `path` as a facade source with the product cache ceiling.
/// # Errors
/// Returns source-root, project-rule, session builder, or storage/cache setup failures.
pub fn open_dir(path: &Path) -> Result<Session, String> {
    let (source, place, _) = open_source(path)?;
    let rules = project_rules(path)?;
    build(source, &place, "default", rules)
}

/// Opens `path` for a map query and returns the canonical source key.
///
/// An absolute cache root is attached when one can be created. A relative `DCTX_CACHE_DIR`,
/// or a cache that will not open, falls back to an ephemeral session. Nothing is written
/// inside the scanned project.
/// # Errors
/// Returns source-root, project-rule, managed-cache, or session setup failures.
pub fn open_mapped(path: &Path) -> Result<(Session, String), String> {
    open_mapped_at(path, cache_root_dir().ok().as_deref())
}

/// Same map session with an explicit cache root, including isolated test caches.
/// # Errors
/// Returns source-root, project-rule, managed-cache, or session setup failures.
pub fn open_mapped_at(path: &Path, root: Option<&Path>) -> Result<(Session, String), String> {
    let (source, place, key) = open_source(path)?;
    let rules = project_rules(path)?;
    let Some(root) = root else {
        return Ok((build(source, &place, "default", rules)?, key));
    };
    if ensure_cache_dir(root).is_err() {
        return Ok((build(source, &place, "default", rules)?, key));
    }
    if let Ok(session) = build_cached(source, &place, root, rules.clone()) { Ok((session, key)) } else {
        let (source, place, key) = open_source(path)?;
        Ok((build(source, &place, "default", rules)?, key))
    }
}

/// True when map should print whether the generation was reused. Set `DCTX_REUSE_REPORT=1`.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
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

/// Reads `policy.v1`. A missing file is the product default.
/// # Errors
/// Returns unreadable or malformed cache policy errors; a missing policy uses defaults.
pub fn load_policy(root: &Path) -> Result<CachePolicy, String> {
    let path = root.join(POLICY_FILE);
    match fs::read_to_string(&path) {
        Ok(text) => parse_policy(&text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(CachePolicy::default()),
        Err(_) => Err("corrupt cache policy: unreadable".to_string()),
    }
}

/// Shows the policy, or stores a tightened one. A show does not create the file.
/// # Errors
/// Returns policy loading/validation, cap application, ledger, or policy persistence failures.
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
/// # Errors
/// Returns policy or ledger loading failures.
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

fn build_cached(
    source: impl Source + Send + 'static,
    place: &str,
    root: &Path,
    rules: RuleSources,
) -> Result<Session, String> {
    let mut policy = load_policy(root).map_or_else(|_| AutoCleanConfig::default(), |item| item.auto);
    let env_off = std::env::var("DCTX_AUTO_CLEAN").ok().as_deref() == Some("0");
    if !MAP_AUTO_CLEAN.load(Ordering::Relaxed) || env_off {
        policy.enabled = false;
    }
    let selection = rules.policy();
    Session::builder(source)
        .clock(FixedClock(Timestamp(unix_millis())))
        .cancellation(Arc::new(CancelFlag::new()))
        .selection(selection)
        .rule_sources(rules)
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

fn build(source: impl Source + Send + 'static, place: &str, view: &str, rules: RuleSources) -> Result<Session, String> {
    let selection = rules.policy();
    Session::builder(source)
        .clock(FixedClock(Timestamp(unix_millis())))
        .cancellation(Arc::new(CancelFlag::new()))
        .selection(selection)
        .rule_sources(rules)
        .ceiling(HostCeiling::base())
        .grants(HostGrants::PermitAll)
        .storage(StoragePolicy::Ephemeral { max_bytes: HARD_CAP_BYTES })
        .place(place)
        .view(view)
        .build()
        .map_err(|error| error.to_string())
}

/// Ignore and omit texts for `root`. `DCTX_INDEX_IGNORED=1` loads omit texts only.
///
/// Local ignore order is the root `.gitignore`, then nested `.gitignore` files rewritten
/// to root-relative lines, then `.dctxignore`. An excluded directory is not listed.
pub(crate) fn project_rules(root: &Path) -> Result<RuleSources, String> {
    let root = fs::canonicalize(root).map_err(|error| error.to_string())?;
    let global_omit = optional_omit(config_file("omitcontent"));
    let local_omit = optional_omit(Some(root.join(".dctxomitcontent")));
    if index_ignored() {
        return Ok(RuleSources { global_ignore: None, global_omit, local_ignore: None, local_omit });
    }
    let global_ignore = required_ignore(config_file("ignore"))?;
    let local_ignore = local_ignore_text(&root, global_ignore.as_deref())?;
    Ok(RuleSources { global_ignore, global_omit, local_ignore, local_omit })
}

/// Local ignore text for `root`, or `None` when every rule file is missing or empty.
/// `global_ignore` is consulted only so an already-excluded directory is not listed.
pub(crate) fn local_ignore_text(root: &Path, global_ignore: Option<&str>) -> Result<Option<String>, String> {
    let mut local = String::new();
    if let Some(text) = read_rule_file(&root.join(".gitignore"))? {
        append_text(&mut local, &text);
    }
    collect_nested(root, "", &mut local, global_ignore, 0)?;
    if let Some(text) = read_rule_file(&root.join(".dctxignore"))? {
        append_text(&mut local, &text);
    }
    if local.is_empty() { Ok(None) } else { Ok(Some(local)) }
}

fn index_ignored() -> bool {
    std::env::var("DCTX_INDEX_IGNORED").ok().as_deref() == Some("1")
}

fn config_file(name: &str) -> Option<PathBuf> {
    config_dir().map(|dir| dir.join(name))
}

fn config_dir() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var("APPDATA").ok().map(PathBuf::from)
    } else {
        std::env::var("XDG_CONFIG_HOME")
            .ok()
            .map(PathBuf::from)
            .or_else(|| std::env::var("HOME").ok().map(|home| PathBuf::from(home).join(".config")))
    };
    base.map(|path| path.join(APP_NAME))
}

fn required_ignore(path: Option<PathBuf>) -> Result<Option<String>, String> {
    match path {
        Some(path) => read_rule_file(&path),
        None => Ok(None),
    }
}

fn optional_omit(path: Option<PathBuf>) -> Option<String> {
    path.and_then(|path| read_rule_file(&path).ok().flatten())
}

fn read_rule_file(path: &Path) -> Result<Option<String>, String> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    if meta.file_type().is_symlink() {
        return Err(format!("{}: is a symlink", path.display()));
    }
    if !meta.is_file() {
        return Err(format!("{}: is not a file", path.display()));
    }
    if meta.len() > MAX_RULE_FILE_BYTES {
        return Err(format!("{}: exceeds {MAX_RULE_FILE_BYTES} bytes", path.display()));
    }
    let file = fs::File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut buf = Vec::new();
    file.take(MAX_RULE_FILE_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if buf.len() as u64 > MAX_RULE_FILE_BYTES {
        return Err(format!("{}: exceeds {MAX_RULE_FILE_BYTES} bytes", path.display()));
    }
    let text = std::str::from_utf8(&buf).map_err(|_| format!("{}: invalid UTF-8", path.display()))?;
    Ok(Some(text.to_string()))
}

fn collect_nested(
    root: &Path,
    prefix: &str,
    rules: &mut String,
    global_ignore: Option<&str>,
    depth: u32,
) -> Result<(), String> {
    if depth > MAX_RULE_DEPTH {
        return Ok(());
    }
    let dir = if prefix.is_empty() { root.to_path_buf() } else { root.join(prefix) };
    let mut names = Vec::new();
    let entries = fs::read_dir(&dir).map_err(|error| format!("rule walk {}: {error}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("rule walk {}: {error}", dir.display()))?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if name.is_empty() || name == "." || name == ".." {
            continue;
        }
        let kind = entry.file_type().map_err(|error| format!("rule walk {}: {error}", dir.display()))?;
        if kind.is_symlink() || !kind.is_dir() {
            continue;
        }
        names.push(name);
    }
    names.sort();
    for name in names {
        let relative = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        if directory_excluded(global_ignore, rules, &relative, &name) {
            continue;
        }
        if let Some(text) = read_rule_file(&root.join(&relative).join(".gitignore"))? {
            append_nested(rules, &relative, &text);
        }
        collect_nested(root, &relative, rules, global_ignore, depth + 1)?;
    }
    Ok(())
}

fn directory_excluded(global_ignore: Option<&str>, local_so_far: &str, relative: &str, name: &str) -> bool {
    let local_ignore = if local_so_far.is_empty() { None } else { Some(local_so_far) };
    let policy = SelectionPolicy::from_texts(&RuleTexts { global_ignore, local_ignore, ..RuleTexts::default() });
    let context = ItemContext { relative_path: relative, name, is_directory: true, debug_mode: false };
    policy.decide(&context) == FileHandlingDecision::Exclude
}

fn append_text(rules: &mut String, text: &str) {
    if text.is_empty() {
        return;
    }
    if !rules.is_empty() && !rules.ends_with('\n') {
        rules.push('\n');
    }
    rules.push_str(text);
    if !rules.ends_with('\n') {
        rules.push('\n');
    }
}

fn append_nested(rules: &mut String, prefix: &str, text: &str) {
    for line in text.lines() {
        if !rules.is_empty() && !rules.ends_with('\n') {
            rules.push('\n');
        }
        rules.push_str(&rewrite_nested_line(prefix, line));
        rules.push('\n');
    }
}

/// Rewrites one line of a nested `.gitignore` so it matches from the repository root.
/// A pattern with no slash applies anywhere under `prefix`. An anchored pattern stays under it.
fn rewrite_nested_line(prefix: &str, line: &str) -> String {
    if prefix.is_empty() {
        return line.to_string();
    }
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return line.to_string();
    }
    let (body, comment) = match trimmed.find('#') {
        Some(pos) => (trimmed[..pos].trim_end(), Some(trimmed[pos..].trim())),
        None => (trimmed, None),
    };
    if body.is_empty() {
        return line.to_string();
    }
    let (negation, rest) = match body.strip_prefix('!') {
        Some(rest) => (true, rest),
        None => (false, body),
    };
    let (dir_only, rest) = match rest.strip_suffix('/') {
        Some(rest) => (true, rest),
        None => (false, rest),
    };
    let anchored = rest.starts_with('/');
    let rest = rest.strip_prefix('/').unwrap_or(rest);
    if rest.is_empty() {
        return line.to_string();
    }
    let rewritten = if anchored || rest.contains('/') {
        format!("{prefix}/{rest}")
    } else {
        format!("{prefix}/**/{rest}")
    };
    let mut out = String::new();
    if negation {
        out.push('!');
    }
    out.push_str(&rewritten);
    if dir_only {
        out.push('/');
    }
    if let Some(comment) = comment {
        out.push(' ');
        out.push_str(comment);
    }
    out
}

/// Unix milliseconds for the injected clock. The facade does not read the clock itself.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
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

    #[test]
    fn nested_gitignore_lines_become_root_relative() {
        assert_eq!(rewrite_nested_line("src", "generated.rs"), "src/**/generated.rs");
        assert_eq!(rewrite_nested_line("src", "/only.rs"), "src/only.rs");
        assert_eq!(rewrite_nested_line("src", "build/"), "src/**/build/");
        assert_eq!(rewrite_nested_line("src", "!keep.rs"), "!src/**/keep.rs");
        assert_eq!(rewrite_nested_line("", "*.log"), "*.log");
        assert_eq!(rewrite_nested_line("src", "# note"), "# note");
        assert_eq!(rewrite_nested_line("src", "foo/bar"), "src/foo/bar");
    }

    #[test]
    fn nested_rules_anchor_at_their_directory_and_a_leading_slash_is_root_only() {
        let mut local = String::new();
        append_text(&mut local, "/corpus/\n*.secret\n!keep.secret\n");
        append_nested(&mut local, "src", "generated.rs\n");
        let policy = SelectionPolicy::from_texts(&RuleTexts { local_ignore: Some(&local), ..RuleTexts::default() });
        let decision = |path: &str, dir: bool| {
            let name = path.rsplit('/').next().unwrap_or(path);
            policy.decide(&ItemContext { relative_path: path, name, is_directory: dir, debug_mode: false })
        };
        let excluded = |path: &str, dir: bool| decision(path, dir) == FileHandlingDecision::Exclude;
        let described: Vec<String> = policy
            .ignore_rules()
            .iter()
            .map(|rule| format!("{:?} {} dir={} neg={}", rule.pattern_type, rule.pattern, rule.is_dir_only, rule.is_negation))
            .collect();
        assert!(
            excluded("corpus", true),
            "decision={:?} local={local:?} rules={described:?}",
            decision("corpus", true)
        );
        assert!(!excluded("src/corpus", true));
        assert!(!excluded("corpus/ready.json", false));
        assert!(excluded("drop.secret", false));
        assert!(!excluded("keep.secret", false));
        assert!(excluded("src/generated.rs", false));
        assert!(excluded("src/a/generated.rs", false));
        assert!(!excluded("generated.rs", false));
        assert!(!excluded("src/lib.rs", false));
    }

    #[cfg(unix)]
    #[test]
    fn an_ignored_unreadable_directory_is_not_listed() {
        struct Restore(PathBuf);
        impl Drop for Restore {
            fn drop(&mut self) {
                let Ok(meta) = fs::metadata(&self.0) else {
                    return;
                };
                let mut perms = meta.permissions();
                perms.set_mode(0o755);
                let _ = fs::set_permissions(&self.0, perms);
            }
        }
        use std::os::unix::fs::PermissionsExt;
        let root = scratch("ignored-corpus");
        fs::write(root.join(".gitignore"), "/corpus/\n").expect("ignore");
        let corpus = root.join("corpus");
        fs::create_dir(&corpus).expect("corpus");
        fs::write(corpus.join("ready.json"), b"{}\n").expect("ready");
        let mut perms = fs::metadata(&corpus).expect("meta").permissions();
        perms.set_mode(0o0);
        fs::set_permissions(&corpus, perms).expect("chmod");
        {
            let _restore = Restore(corpus);
            let text = local_ignore_text(&root, None).expect("rules").expect("text");
            assert!(text.contains("/corpus/"), "{text}");
        }
        let _ = fs::remove_dir_all(&root);
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
