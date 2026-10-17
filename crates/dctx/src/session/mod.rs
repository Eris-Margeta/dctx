//! Session: one explicit source, one policy and the operations every surface calls.
//! Implements ARCHITECTURE-PROPOSAL §2 and §7 row DX-D1. The facade is synchronous.
//! Depends on the domain types, the index and the pack and template engines.
//! Handles pin a generation with an [`std::sync::Arc`] lease. Dropping the last lease allows collection.
//! A plugin host is optional. The library default starts none and schedules no hook.

mod build;
mod cache_op;
mod hooks;
mod import_op;
mod ledger;
mod pack_op;
mod query_op;
mod template_op;

use std::collections::BTreeMap;
use std::sync::{Arc, Weak};

use dctx_core::selection::SelectionPolicy;
use dctx_ports::{Cancellation, Clock, FactStore, Source};
use dctx_template::{ContextPlan, HostCeiling};

pub use build::StoragePolicy;
pub use build::SessionBuilder;

use self::ledger::{AccountingCache, PersistedCache};
use crate::domain::{
    ancestors_include, check_deadline, seal_segment, HostGrants, OperationLimits, PolicyView, RuleSources,
    SnapshotRequest, DctxError, REQUEST_VERSION,
};
use crate::index::{canonical_bytes, walk_source, Generation, WalkArgs};
use crate::plugins::{HookWarning, PluginHost};

/// Lease that keeps one generation reachable. The value is the allocation, not its fields.
struct Pin;

/// A pinned snapshot generation. Cloning increments the lease.
#[derive(Clone)]
pub struct SnapshotHandle {
    generation: u64,
    epoch: u64,
    digest: String,
    pin: Arc<Pin>,
}

impl SnapshotHandle {
    /// Generation id. The first snapshot in a session is 1.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Policy epoch observed when the snapshot was taken.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Own snapshot digest.
    pub fn digest(&self) -> &str {
        &self.digest
    }
}

impl std::fmt::Debug for SnapshotHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SnapshotHandle")
            .field("generation", &self.generation)
            .field("epoch", &self.epoch)
            .field("digest", &self.digest)
            .field("lease", &std::sync::Arc::strong_count(&self.pin))
            .finish()
    }
}

/// The library entry point. It does not scan a home directory or write to stdout.
pub struct Session {
    source: Box<dyn Source + Send>,
    store: Box<dyn FactStore + Send>,
    cache: AccountingCache,
    clock: Box<dyn Clock + Send>,
    cancel: Arc<dyn Cancellation + Send + Sync>,
    selection: SelectionPolicy,
    ceiling: HostCeiling,
    grants: HostGrants,
    plan: Option<ContextPlan>,
    plugins: Option<PluginHost>,
    rules: Option<RuleSources>,
    warnings: Vec<HookWarning>,
    limits: OperationLimits,
    place: String,
    view: String,
    epoch: u64,
    next_generation: u64,
    generations: BTreeMap<u64, Generation>,
    pins: BTreeMap<u64, Weak<Pin>>,
    content_reads: u64,
    cached_content_reads: u64,
    managed: Option<PersistedCache>,
}

impl Session {
    /// Starts a builder. The source is the only implicit argument, and the caller supplies it.
    pub fn builder(source: impl Source + Send + 'static) -> SessionBuilder {
        SessionBuilder::new(source)
    }

    /// Build capabilities. Host grants do not change this value.
    pub fn capabilities(&self) -> crate::domain::Capabilities {
        crate::domain::Capabilities::current()
    }

    /// Content bytes requested from the source since construction.
    pub fn content_reads(&self) -> u64 {
        self.content_reads
    }

    /// Cached content reads. This facade never serves cached file bytes, so the value stays 0.
    pub fn cached_content_reads(&self) -> u64 {
        self.cached_content_reads
    }

    /// Generations still stored.
    pub fn generation_count(&self) -> u64 {
        u64::try_from(self.generations.len()).unwrap_or(u64::MAX)
    }

    /// Policy epoch. Installing a plan or revising the selection increments it.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Place copied into answers.
    pub fn place(&self) -> &str {
        &self.place
    }

    /// Takes a snapshot. Cancellation and a cache refusal leave `generation_count` unchanged.
    pub fn snapshot(&mut self, request: &SnapshotRequest) -> Result<SnapshotHandle, DctxError> {
        if !cfg!(feature = "map") {
            return Err(DctxError::unsupported("map"));
        }
        if request.request_version != REQUEST_VERSION {
            return Err(DctxError::UnsupportedVersion { found: request.request_version });
        }
        self.cancel.checkpoint().map_err(DctxError::from_core)?;
        let scan_limits = self.limits.intersect(OperationLimits::from_ceiling(&self.ceiling)).intersect(request.limits);
        let limits = crate::domain::resolve_scan(&scan_limits)?;
        let parser = {
            let view = self.policy_view();
            view.check()?;
            view.parser()?
        };
        self.hooks_before_index()?;
        self.touch_cache_root()?;
        let started = self.clock.now().millis();
        check_deadline(started, started, scan_limits.deadline_ms)?;
        let walked = walk_source(&WalkArgs {
            source: self.source.as_ref(),
            cancel: self.cancel.as_ref(),
            policy: PolicyView {
                selection: &self.selection,
                ceiling: &self.ceiling,
                grants: self.grants,
                plan: self.plan.as_ref(),
                view: &self.view,
            },
            limits: &limits,
            freshness: request.freshness,
            parser,
            deadline_ms: scan_limits.deadline_ms,
            started_ms: started,
            now_ms: started,
        })?;
        self.content_reads += walked.reads;
        let (payload, digest) = canonical_bytes(&walked.files, &walked.nodes, &walked.edges)?;
        let segment = seal_segment(&payload)?;
        let encoded_len = if cfg!(feature = "segments") {
            u64::try_from(segment.len()).unwrap_or(u64::MAX)
        } else {
            0
        };
        if cfg!(feature = "segments") {
            self.cache.segment_admit(encoded_len)?;
        }
        if let Err(error) = self.store_cache_segment(&segment) {
            if cfg!(feature = "segments") {
                self.cache.segment_release(encoded_len);
            }
            return Err(error);
        }
        let id = self.next_generation;
        let pin = Arc::new(Pin);
        self.pins.insert(id, Arc::downgrade(&pin));
        self.generations.insert(
            id,
            Generation {
                id,
                digest: digest.clone(),
                encoded_len,
                files: walked.files,
                nodes: walked.nodes,
                edges: walked.edges,
                freshness: request.freshness,
                taken_at: started,
                epoch: self.epoch,
                coverage: walked.coverage,
                completion: walked.completion,
                parser: walked.parser,
            },
        );
        self.next_generation = self.next_generation.saturating_add(1);
        let handle = SnapshotHandle { generation: id, epoch: self.epoch, digest, pin };
        self.hooks_after_index()?;
        Ok(handle)
    }

    fn policy_view(&self) -> PolicyView<'_> {
        PolicyView {
            selection: &self.selection,
            ceiling: &self.ceiling,
            grants: self.grants,
            plan: self.plan.as_ref(),
            view: &self.view,
        }
    }

    fn authorize(&self, path: &str) -> Result<(), DctxError> {
        let allowed = ancestors_include(|item, is_dir| self.policy_view().decide(item, is_dir), path)?;
        if allowed { Ok(()) } else { Err(DctxError::PolicyDenied) }
    }

    fn generation(&self, handle: &SnapshotHandle) -> Result<Generation, DctxError> {
        self.generations
            .get(&handle.generation)
            .cloned()
            .ok_or(DctxError::Stale { reason: "generation reclaimed" })
    }
}

impl std::fmt::Debug for Session {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Session")
            .field("place", &self.place)
            .field("view", &self.view)
            .field("epoch", &self.epoch)
            .field("generations", &self.generations.len())
            .finish()
    }
}
