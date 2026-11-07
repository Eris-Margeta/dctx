// SPDX-License-Identifier: Apache-2.0
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
    AutoCleanConfig, CacheReport, DctxError, FileHandlingDecision, FsSource, GcRequest,
    HostCeiling, HostGrants, PurgeRequest, PurgeScope, RuleSources, RuleTexts, SelectionPolicy,
    Session, Source, StoragePolicy, Timestamp,
};
use dctx_adapter_cache::adapters::FsLedgerStore;
use dctx_adapter_cache::ports::LedgerStore;
use dctx_adapter_cache::{Lifecycle, Quota, HARD_CAP_BYTES};
use dctx_core::settings::APP_NAME;
use dctx_core::ItemContext;

mod cache;
mod policy;
mod rules;

pub use cache::*;

pub use policy::*;

pub(crate) use cache::with_ledger;

pub(crate) use rules::project_rules;

use cache::{CLI_ROOT_ID, MAP_AUTO_CLEAN};

#[cfg(test)]
use rules::{append_nested, append_text, local_ignore_text, rewrite_nested_line};

/// Ephemeral session with no source files. Template compile uses it.
/// # Errors
/// Returns session builder or storage/cache setup failures.
pub fn open_memory() -> Result<Session, String> {
    build(
        MemorySource::new(),
        "memory",
        "default",
        RuleSources::default(),
    )
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
    if let Ok(session) = build_cached(source, &place, root, rules.clone()) {
        Ok((session, key))
    } else {
        let (source, place, key) = open_source(path)?;
        Ok((build(source, &place, "default", rules)?, key))
    }
}

/// True when map should print whether the generation was reused. Set `DCTX_REUSE_REPORT=1`.
#[allow(
    clippy::must_use_candidate,
    reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
)]
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
    let mut policy =
        load_policy(root).map_or_else(|_| AutoCleanConfig::default(), |item| item.auto);
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
        .storage(StoragePolicy::Ephemeral {
            max_bytes: HARD_CAP_BYTES,
        })
        .place(place)
        .view("default")
        .cache_root(root, CLI_ROOT_ID)
        .auto_clean(policy)
        .build()
        .map_err(facade_error)
}

fn build(
    source: impl Source + Send + 'static,
    place: &str,
    view: &str,
    rules: RuleSources,
) -> Result<Session, String> {
    let selection = rules.policy();
    Session::builder(source)
        .clock(FixedClock(Timestamp(unix_millis())))
        .cancellation(Arc::new(CancelFlag::new()))
        .selection(selection)
        .rule_sources(rules)
        .ceiling(HostCeiling::base())
        .grants(HostGrants::PermitAll)
        .storage(StoragePolicy::Ephemeral {
            max_bytes: HARD_CAP_BYTES,
        })
        .place(place)
        .view(view)
        .build()
        .map_err(|error| error.to_string())
}

/// Unix milliseconds for the injected clock. The facade does not read the clock itself.
#[allow(
    clippy::must_use_candidate,
    reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
)]
pub fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

#[cfg(test)]
mod tests;
