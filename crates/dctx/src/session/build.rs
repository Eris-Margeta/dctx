//! Session construction from explicit ports. Nothing is read from the home directory.
//! Implements ARCHITECTURE-PROPOSAL §2: source, storage, policy, limits and cancellation are arguments.
//! Depends on [`super::Session`] and the port traits. Hosts call [`Session::builder`].
//! A missing clock or any other required input is [`crate::domain::DctxError::InvalidRequest`].

use std::sync::Arc;

use dctx_core::selection::SelectionPolicy;
use dctx_ports::memory::{MemoryCache, MemoryFactStore};
use dctx_ports::{Cache, Cancellation, Clock, FactStore, Source};
use dctx_template::HostCeiling;

use super::ledger::AccountingCache;
use super::Session;
use crate::domain::{DctxError, HostGrants, OperationLimits};

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
    limits: OperationLimits,
    place: Option<String>,
    view: Option<String>,
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
            limits: OperationLimits::default(),
            place: None,
            view: None,
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
            limits: self.limits,
            place,
            view,
            epoch: 1,
            next_generation: 1,
            generations: std::collections::BTreeMap::new(),
            pins: std::collections::BTreeMap::new(),
            content_reads: 0,
            cached_content_reads: 0,
        })
    }
}
