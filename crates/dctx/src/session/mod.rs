// SPDX-License-Identifier: Apache-2.0
//! Session: one explicit source, one policy and the operations every surface calls.
//! Implements ARCHITECTURE-PROPOSAL §2 and §7 row DX-D1. The facade is synchronous.
//! Depends on the domain types, the index and the pack and template engines.
//! A cache root is shared: admission, snapshot leases and purge/GC use one ledger.
//! Handles keep that lease alive. Dropping the last one allows collection.
//! A plugin host is optional. The library default starts none and schedules no hook.

mod build;
mod cache_op;
mod dotted_op;
mod export_op;
mod export_order_op;
mod grep_op;
mod hooks;
mod hubs_op;
mod import_op;
mod ledger;
mod out_calls_op;
mod pack_op;
mod path_op;
mod query_op;
mod reuse;
mod sites_op;
mod template_op;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Weak};

use dctx_core::selection::SelectionPolicy;
use dctx_ports::{Cancellation, Clock, FactStore, Source};
use dctx_template::{ContextPlan, HostCeiling};

pub use build::SessionBuilder;
pub use build::StoragePolicy;

use self::ledger::{AccountingCache, PersistedCache, SnapshotLease};
use crate::domain::{
    Answer, BoundedSink, DctxError, HostGrants, MapRequest, OperationLimits, PolicyView,
    REQUEST_VERSION, RuleSources, SnapshotRequest, ancestors_include, check_deadline, seal_segment,
};
use crate::index::{Generation, WalkArgs, canonical_bytes, walk_source};
use crate::plugins::{HookWarning, PluginHost};

/// The stored generation. A query clones this `Arc` and borrows the rows.
struct SharedGeneration {
    generation: Generation,
    /// `Arc` owners while the latest query held its clone. `0` before the first query.
    borrowed_shares: AtomicUsize,
}

impl std::ops::Deref for SharedGeneration {
    type Target = Generation;

    fn deref(&self) -> &Self::Target {
        &self.generation
    }
}

/// Lease that keeps one generation reachable. The value is the allocation, not its fields.
struct Pin {
    /// Persisted reader pin. `None` in library isolation. Drop releases the ledger lease.
    release: Option<SnapshotLease>,
}

impl Drop for Pin {
    fn drop(&mut self) {
        drop(self.release.take());
    }
}

/// A pinned snapshot generation. Cloning increments the lease.
#[derive(Clone)]
pub struct SnapshotHandle {
    generation: u64,
    epoch: u64,
    digest: String,
    pin: Arc<Pin>,
    /// True when the generation was loaded after every stored content digest still matched.
    reused: bool,
}

impl SnapshotHandle {
    /// Generation id. The first snapshot in a session is 1.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Policy epoch observed when the snapshot was taken.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Own snapshot digest.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// True when this handle was installed from a persisted generation whose digests still matched.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn reused(&self) -> bool {
        self.reused
    }
}

impl std::fmt::Debug for SnapshotHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SnapshotHandle")
            .field("generation", &self.generation)
            .field("epoch", &self.epoch)
            .field("digest", &self.digest)
            .field("reused", &self.reused)
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
    generations: BTreeMap<u64, Arc<SharedGeneration>>,
    pins: BTreeMap<u64, Weak<Pin>>,
    content_reads: u64,
    cached_content_reads: u64,
    stamp_skipped_reads: u64,
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

    /// Content reads that confirmed a retained digest and did not rebuild the generation.
    /// A cold snapshot leaves this at 0.
    pub fn cached_content_reads(&self) -> u64 {
        self.cached_content_reads
    }

    /// Files a warm start accepted from an equal stat stamp (size, mtime ns, inode) without reading their bytes.
    /// 0 on a cold snapshot, on a file inside the racy window, and under `DCTX_FRESHNESS=strict_live`.
    pub fn stamp_skipped_reads(&self) -> u64 {
        self.stamp_skipped_reads
    }

    /// Generations still stored.
    pub fn generation_count(&self) -> u64 {
        u64::try_from(self.generations.len()).unwrap_or(u64::MAX)
    }

    /// Live `Arc` owners of `handle`'s generation, and the owner count sampled by the latest query.
    ///
    /// The first number is the owners alive now. The second was sampled while the latest query
    /// borrowed this generation, and it is `0` before that query. After a query the sample is the
    /// live count plus one: the query shares the stored rows and then drops that share.
    /// # Errors
    /// Returns a stale-generation error if the generation has been reclaimed.
    pub fn generation_shares(&self, handle: &SnapshotHandle) -> Result<(usize, usize), DctxError> {
        let shared = self
            .generations
            .get(&handle.generation)
            .ok_or(DctxError::Stale {
                reason: "generation reclaimed",
            })?;
        Ok((
            Arc::strong_count(shared),
            shared.borrowed_shares.load(Ordering::Relaxed),
        ))
    }

    /// Policy epoch. Installing a plan or revising the selection increments it.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Place copied into answers.
    pub fn place(&self) -> &str {
        &self.place
    }

    /// Starts per-stage and per-file counters for this thread. Off until this is called.
    pub fn enable_profile(&mut self) {
        crate::index::profile::enable();
    }

    /// Drops counters after a failed snapshot or query.
    pub fn discard_profile(&mut self) {
        crate::index::profile::discard();
    }

    /// Stderr report, then clears the counters. `None` when [`Self::enable_profile`] was not called.
    pub fn profile_report(&mut self) -> Option<String> {
        crate::index::profile::report()
    }

    /// Same as [`Self::snapshot_reusable`]. When profiling is on, an unaccounted reuse path is charged here.
    ///
    /// A cold build records canonicalize and persist inside the snapshot. This wall is not added
    /// on top of those stages. A reused generation never enters that path, so the remainder after
    /// its metadata walk is the persist/reuse stage.
    /// # Errors
    /// Propagates request/capability validation, source/store/cache failures, cancellation, deadlines, and snapshot resource-limit errors.
    pub fn snapshot_reusable_for_profile(
        &mut self,
        request: &SnapshotRequest,
        source_key: &str,
    ) -> Result<SnapshotHandle, DctxError> {
        let start = crate::index::profile::start();
        let handle = self.snapshot_reusable(request, source_key)?;
        if let Some(start) = start {
            crate::index::profile::observe_snapshot_wall(start.elapsed());
        }
        Ok(handle)
    }

    /// Same as [`Self::query`]. The sample starts after file watches have closed, so it is not parse time.
    /// # Errors
    /// Refuses invalid/stale handles or requests, denied source access, unsupported capabilities, exceeded budgets/deadlines, or sink failures.
    pub fn query_for_profile(
        &mut self,
        handle: &SnapshotHandle,
        request: &MapRequest,
        sink: &mut dyn BoundedSink,
    ) -> Result<Answer, DctxError> {
        let _span = crate::index::profile::Elapsed::begin(crate::index::profile::Stage::Query);
        self.query(handle, request, sink)
    }

    /// Charges a snapshot wall that this session did not already account. Cold builds ignore it.
    pub fn observe_profile_snapshot(&mut self, elapsed: std::time::Duration) {
        crate::index::profile::observe_snapshot_wall(elapsed);
    }

    /// Takes a snapshot. Cancellation and a cache refusal leave `generation_count` unchanged.
    /// # Errors
    /// Propagates request/capability validation, source/store/cache failures, cancellation, deadlines, and snapshot resource-limit errors.
    pub fn snapshot(&mut self, request: &SnapshotRequest) -> Result<SnapshotHandle, DctxError> {
        self.snapshot_inner(request, None, true)
    }

    /// Walks `request` and stores the generation. `hooks` false means the caller already ran the before-index hook.
    fn snapshot_inner(
        &mut self,
        request: &SnapshotRequest,
        source_key: Option<&str>,
        hooks: bool,
    ) -> Result<SnapshotHandle, DctxError> {
        let (limits, parser, scan_limits) = self.reject_snapshot(request)?;
        if hooks {
            self.hooks_before_index()?;
        }
        self.touch_cache_root()?;
        let started = self.clock.now().millis();
        check_deadline(started, started, scan_limits.deadline_ms)?;
        let walked = walk_source(&WalkArgs {
            source: self.source.as_ref(),
            cancel: self.cancel.as_ref(),
            policy: self.policy_view(),
            limits: &limits,
            freshness: request.freshness,
            parser,
            deadline_ms: scan_limits.deadline_ms,
            started_ms: started,
            now_ms: started,
        })?;
        self.content_reads += walked.reads;
        let (payload, digest) = {
            let _span = crate::index::profile::Elapsed::begin(
                crate::index::profile::Stage::CanonicalizeSeal,
            );
            canonical_bytes(&walked.files, &walked.nodes, &walked.edges, &walked.text)?
        };
        let id = self.next_generation;
        let mut generation = Generation {
            id,
            digest: digest.clone(),
            encoded_len: 0,
            files: walked.files,
            nodes: walked.nodes,
            edges: walked.edges,
            text: walked.text,
            freshness: request.freshness,
            taken_at: started,
            epoch: self.epoch,
            coverage: walked.coverage,
            completion: walked.completion,
            parser: walked.parser,
            ids: crate::index::IdIndex::default(),
        };
        let reuse_key = match source_key {
            Some(key) if self.managed.is_some() => Some(key),
            _ => None,
        };
        let stored = if let Some(key) = reuse_key {
            let encoded = {
                let _span = crate::index::profile::Elapsed::begin(
                    crate::index::profile::Stage::PersistReuse,
                );
                reuse::encode_generation(key, &self.view, &limits, &generation)?
            };
            drop(payload);
            encoded
        } else {
            let sealed = {
                let _span = crate::index::profile::Elapsed::begin(
                    crate::index::profile::Stage::CanonicalizeSeal,
                );
                seal_segment(&payload)?
            };
            drop(payload);
            sealed
        };
        let handle = {
            let _span =
                crate::index::profile::Elapsed::begin(crate::index::profile::Stage::PersistReuse);
            let encoded_len = if cfg!(feature = "segments") {
                u64::try_from(stored.len()).unwrap_or(u64::MAX)
            } else {
                0
            };
            generation.encoded_len = encoded_len;
            if cfg!(feature = "segments") {
                self.cache.segment_admit(encoded_len)?;
            }
            let ticket = match self.store_cache_segment(&stored) {
                Ok(ticket) => ticket,
                Err(error) => {
                    if cfg!(feature = "segments") {
                        self.cache.segment_release(encoded_len);
                    }
                    return Err(error);
                }
            };
            self.pin_generation(generation, ticket, false)
        };
        self.hooks_after_index()?;
        crate::index::profile::mark_snapshot_accounted();
        Ok(handle)
    }

    fn reject_snapshot(
        &self,
        request: &SnapshotRequest,
    ) -> Result<
        (
            crate::domain::ScanLimits,
            crate::domain::ParserChoice,
            OperationLimits,
        ),
        DctxError,
    > {
        if !cfg!(feature = "map") {
            return Err(DctxError::unsupported("map"));
        }
        if request.request_version != REQUEST_VERSION {
            return Err(DctxError::UnsupportedVersion {
                found: request.request_version,
            });
        }
        self.cancel.checkpoint().map_err(DctxError::from_core)?;
        let scan_limits = self
            .limits
            .intersect(OperationLimits::from_ceiling(&self.ceiling))
            .intersect(request.limits);
        let limits = crate::domain::resolve_scan(&scan_limits)?;
        let parser = {
            let view = self.policy_view();
            view.check()?;
            view.parser()?
        };
        Ok((limits, parser, scan_limits))
    }

    fn pin_generation(
        &mut self,
        generation: Generation,
        ticket: Option<SnapshotLease>,
        reused: bool,
    ) -> SnapshotHandle {
        let id = generation.id;
        let epoch = generation.epoch;
        let digest = generation.digest.clone();
        let pin = Arc::new(Pin { release: ticket });
        self.pins.insert(id, Arc::downgrade(&pin));
        let shared = Arc::new(SharedGeneration {
            generation,
            borrowed_shares: AtomicUsize::new(0),
        });
        if let Some(previous) = self.generations.insert(id, shared) {
            self.cache.segment_release(previous.encoded_len);
        }
        self.next_generation = id.saturating_add(1).max(self.next_generation);
        SnapshotHandle {
            generation: id,
            epoch,
            digest,
            pin,
            reused,
        }
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
        let allowed =
            ancestors_include(|item, is_dir| self.policy_view().decide(item, is_dir), path)?;
        if allowed {
            Ok(())
        } else {
            Err(DctxError::PolicyDenied)
        }
    }

    fn generation(&self, handle: &SnapshotHandle) -> Result<Arc<SharedGeneration>, DctxError> {
        let shared = self
            .generations
            .get(&handle.generation)
            .map(Arc::clone)
            .ok_or(DctxError::Stale {
                reason: "generation reclaimed",
            })?;
        shared
            .borrowed_shares
            .store(Arc::strong_count(&shared), Ordering::Relaxed);
        Ok(shared)
    }
}

#[allow(
    clippy::missing_fields_in_debug,
    reason = "Debug intentionally reports session identity and generation counts without exposing source, storage, grants or callback internals."
)]
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

#[cfg(test)]
mod tests;
