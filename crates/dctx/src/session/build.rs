//! Session construction from explicit ports. Nothing is read from the home directory.
//! Implements ARCHITECTURE-PROPOSAL §2: source, storage, policy, limits and cancellation are arguments.
//! Depends on [`super::Session`] and the port traits. Hosts call [`Session::builder`].
//! A missing clock is [`crate::domain::DctxError::InvalidRequest`]. No cache root means library isolation.

use std::path::PathBuf;
use std::sync::Arc;

use dctx_core::selection::SelectionPolicy;
use dctx_ports::memory::{MemoryCache, MemoryFactStore};
use dctx_ports::{Cache, Cancellation, Clock, FactStore, Source};
use dctx_template::HostCeiling;

use super::ledger::{AccountingCache, PersistedCache};
use super::Session;
use crate::domain::{AutoCleanConfig, DctxError, HostGrants, OperationLimits, RuleSources};
use crate::plugins::{HookWarning, PluginHost};

/// Where bytes live, with an explicit ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoragePolicy {
    /// Memory store and cache, bounded by `max_bytes`.
    Ephemeral {
        /// Total pack plus segment bytes.
        max_bytes: u64,
    },
    /// Caller-supplied store and cache, still bounded by `max_bytes` in the facade ledger.
    CallerOwned {
        /// Total pack plus segment bytes.
        max_bytes: u64,
    },
}

/// Builds a [`Session`]. Every required input is a method; `build` does not invent one.
pub struct SessionBuilder {
    source: Box<dyn Source + Send>,
    store: Option<Box<dyn FactStore + Send>>,
    cache: Option<Box<dyn Cache + Send>>,
    clock: Option<Box<dyn Clock + Send>>,
    cancel: Option<Arc<dyn Cancellation + Send + Sync>>,
    selection: Option<SelectionPolicy>,
    ceiling: Option<HostCeiling>,
    grants: Option<HostGrants>,
    storage: Option<StoragePolicy>,
    plugins: Option<PluginHost>,
    rules: Option<RuleSources>,
    limits: OperationLimits,
    place: Option<String>,
    view: Option<String>,
    cache_root: Option<(PathBuf, u64)>,
    auto_clean: AutoCleanConfig,
}

impl SessionBuilder {
    pub(super) fn new(source: impl Source + Send + 'static) -> Self {
        Self {
            source: Box::new(source),
            store: None,
            cache: None,
            clock: None,
            cancel: None,
            selection: None,
            ceiling: None,
            grants: None,
            storage: None,
            plugins: None,
            rules: None,
            limits: OperationLimits::default(),
            place: None,
            view: None,
            cache_root: None,
            auto_clean: AutoCleanConfig::default(),
        }
    }

    /// Injected clock. There is no system clock.
    pub fn clock(mut self, clock: impl Clock + Send + 'static) -> Self {
        self.clock = Some(Box::new(clock));
        self
    }

    /// Cancellation checked at every directory and read.
    pub fn cancellation(mut self, cancel: Arc<dyn Cancellation + Send + Sync>) -> Self {
        self.cancel = Some(cancel);
        self
    }

    /// Selection policy. The facade does not read rule files.
    pub fn selection(mut self, selection: SelectionPolicy) -> Self {
        self.selection = Some(selection);
        self
    }

    /// Organization ceiling passed to template compile and to limit intersection.
    pub fn ceiling(mut self, ceiling: HostCeiling) -> Self {
        self.ceiling = Some(ceiling);
        self
    }

    /// `PermitAll` or the ceiling's grant list.
    pub fn grants(mut self, grants: HostGrants) -> Self {
        self.grants = Some(grants);
        self
    }

    /// Explicit storage. Ephemeral is not a fallback for a failed disk.
    pub fn storage(mut self, storage: StoragePolicy) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Optional caller store. Required for [`StoragePolicy::CallerOwned`].
    pub fn store(mut self, store: impl FactStore + Send + 'static) -> Self {
        self.store = Some(Box::new(store));
        self
    }

    /// Optional caller cache. Required for [`StoragePolicy::CallerOwned`].
    pub fn cache(mut self, cache: impl Cache + Send + 'static) -> Self {
        self.cache = Some(Box::new(cache));
        self
    }

    /// Persisted cache root and the caller root id to register on use.
    ///
    /// `path` must already be an absolute directory. The library does not create it and does not
    /// search a home directory. Omitting this method is library isolation: no ledger and no auto-clean.
    pub fn cache_root(mut self, path: impl Into<PathBuf>, root_id: u64) -> Self {
        self.cache_root = Some((path.into(), root_id));
        self
    }

    /// Auto-clean policy used when a cache root is set. The default is enabled, daily and bounded.
    /// Isolation ignores this value. `enabled: false` skips the open-time slice.
    pub fn auto_clean(mut self, policy: AutoCleanConfig) -> Self {
        self.auto_clean = policy;
        self
    }

    /// Extra caps intersected with verb caps. `None` fields do not raise anything.
    pub fn limits(mut self, limits: OperationLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Place name copied into every answer. Required.
    pub fn place(mut self, place: impl Into<String>) -> Self {
        self.place = Some(place.into());
        self
    }

    /// View name inside an installed plan. Required. This is not discovered from disk.
    pub fn view(mut self, view: impl Into<String>) -> Self {
        self.view = Some(view.into());
        self
    }

    /// Plugin ports. Absent means the session starts no plugin and no git child.
    /// Without `plugin-runtime`, the supplied host is unavailable: advisory hooks warn and required hooks refuse.
    pub fn plugins(mut self, plugins: PluginHost) -> Self {
        self.plugins = Some(plugins);
        self
    }

    /// Owned rule texts. Required to rebuild selection after a `.gitignore` edit.
    pub fn rule_sources(mut self, rules: RuleSources) -> Self {
        self.rules = Some(rules);
        self
    }

    /// Builds the session or reports the first missing input.
    pub fn build(self) -> Result<Session, DctxError> {
        let clock = self.clock.ok_or_else(|| DctxError::invalid("clock is required"))?;
        let cancel = self.cancel.ok_or_else(|| DctxError::invalid("cancellation is required"))?;
        let selection = self.selection.ok_or_else(|| DctxError::invalid("selection is required"))?;
        let ceiling = self.ceiling.ok_or_else(|| DctxError::invalid("ceiling is required"))?;
        let grants = self.grants.ok_or_else(|| DctxError::invalid("grants are required"))?;
        let storage = self.storage.ok_or_else(|| DctxError::invalid("storage policy is required"))?;
        let place = self.place.ok_or_else(|| DctxError::invalid("place is required"))?;
        let view = self.view.ok_or_else(|| DctxError::invalid("view is required"))?;
        if place.is_empty() || view.is_empty() {
            return Err(DctxError::invalid("place and view must be non-empty"));
        }
        let max_bytes = match storage {
            StoragePolicy::Ephemeral { max_bytes } => max_bytes,
            StoragePolicy::CallerOwned { max_bytes } => {
                if self.store.is_none() || self.cache.is_none() {
                    return Err(DctxError::invalid("caller-owned storage requires a store and a cache"));
                }
                max_bytes
            }
        };
        let store = self.store.unwrap_or_else(|| Box::new(MemoryFactStore::new()));
        let inner = self.cache.unwrap_or_else(|| Box::new(MemoryCache::new(u64::MAX)));
        let now_ms = clock.now().millis();
        let managed = open_managed(self.cache_root, self.auto_clean, now_ms)?;
        Ok(Session {
            source: self.source,
            store,
            cache: AccountingCache::new(max_bytes, inner),
            clock,
            cancel,
            selection,
            ceiling,
            grants,
            plan: None,
            plugins: self.plugins,
            rules: self.rules,
            warnings: Vec::<HookWarning>::new(),
            limits: self.limits,
            place,
            view,
            epoch: 1,
            next_generation: 1,
            generations: std::collections::BTreeMap::new(),
            pins: std::collections::BTreeMap::new(),
            content_reads: 0,
            cached_content_reads: 0,
            managed,
        })
    }
}

fn open_managed(
    root: Option<(PathBuf, u64)>,
    policy: AutoCleanConfig,
    now_ms: u64,
) -> Result<Option<PersistedCache>, DctxError> {
    let Some((path, root_id)) = root else {
        return Ok(None);
    };
    if !cfg!(feature = "cache") {
        return Err(DctxError::unsupported("cache"));
    }
    PersistedCache::open(&path, root_id, policy, now_ms).map(Some)
}
