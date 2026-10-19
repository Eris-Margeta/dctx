//! Session: one explicit source, one policy and the operations every surface calls.
//! Implements ARCHITECTURE-PROPOSAL §2 and §7 row DX-D1. The facade is synchronous.
//! Depends on the domain types, the index and the pack and template engines.
//! A cache root is shared: admission, snapshot leases and purge/GC use one ledger.
//! Handles keep that lease alive. Dropping the last one allows collection.
//! A plugin host is optional. The library default starts none and schedules no hook.

mod build;
mod cache_op;
mod hooks;
mod import_op;
mod ledger;
mod pack_op;
mod query_op;
mod reuse;
mod template_op;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Weak};

use dctx_core::selection::SelectionPolicy;
use dctx_ports::{Cancellation, Clock, FactStore, Source};
use dctx_template::{ContextPlan, HostCeiling};

pub use build::StoragePolicy;
pub use build::SessionBuilder;

use self::ledger::{AccountingCache, PersistedCache, SnapshotLease};
use crate::domain::{
    ancestors_include, check_deadline, seal_segment, HostGrants, OperationLimits, PolicyView, RuleSources,
    SnapshotRequest, DctxError, REQUEST_VERSION,
};
use crate::index::{canonical_bytes, walk_source, Generation, WalkArgs};
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

    /// True when this handle was installed from a persisted generation whose digests still matched.
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

    /// Generations still stored.
    pub fn generation_count(&self) -> u64 {
        u64::try_from(self.generations.len()).unwrap_or(u64::MAX)
    }

    /// Live `Arc` owners of `handle`'s generation, and the owner count sampled by the latest query.
    ///
    /// The first number is the owners alive now. The second was sampled while the latest query
    /// borrowed this generation, and it is `0` before that query. After a query the sample is the
    /// live count plus one: the query shares the stored rows and then drops that share.
    pub fn generation_shares(&self, handle: &SnapshotHandle) -> Result<(usize, usize), DctxError> {
        let shared = self
            .generations
            .get(&handle.generation)
            .ok_or(DctxError::Stale { reason: "generation reclaimed" })?;
        Ok((Arc::strong_count(shared), shared.borrowed_shares.load(Ordering::Relaxed)))
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
        let (payload, digest) = canonical_bytes(&walked.files, &walked.nodes, &walked.edges)?;
        let id = self.next_generation;
        let mut generation = Generation {
            id,
            digest: digest.clone(),
            encoded_len: 0,
            files: walked.files,
            nodes: walked.nodes,
            edges: walked.edges,
            freshness: request.freshness,
            taken_at: started,
            epoch: self.epoch,
            coverage: walked.coverage,
            completion: walked.completion,
            parser: walked.parser,
        };
        let reuse_key = match source_key {
            Some(key) if self.managed.is_some() => Some(key),
            _ => None,
        };
        let stored = if let Some(key) = reuse_key {
            drop(payload);
            reuse::encode_generation(key, &self.view, &limits, &generation)?
        } else {
            let sealed = seal_segment(&payload)?;
            drop(payload);
            sealed
        };
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
        let handle = self.pin_generation(generation, ticket, false);
        self.hooks_after_index()?;
        Ok(handle)
    }

    fn reject_snapshot(
        &self,
        request: &SnapshotRequest,
    ) -> Result<(crate::domain::ScanLimits, crate::domain::ParserChoice, OperationLimits), DctxError> {
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
        Ok((limits, parser, scan_limits))
    }

    fn pin_generation(&mut self, generation: Generation, ticket: Option<SnapshotLease>, reused: bool) -> SnapshotHandle {
        let id = generation.id;
        let epoch = generation.epoch;
        let digest = generation.digest.clone();
        let pin = Arc::new(Pin { release: ticket });
        self.pins.insert(id, Arc::downgrade(&pin));
        let shared = Arc::new(SharedGeneration { generation, borrowed_shares: AtomicUsize::new(0) });
        if let Some(previous) = self.generations.insert(id, shared) {
            self.cache.segment_release(previous.encoded_len);
        }
        self.next_generation = id.saturating_add(1).max(self.next_generation);
        SnapshotHandle { generation: id, epoch, digest, pin, reused }
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

    fn generation(&self, handle: &SnapshotHandle) -> Result<Arc<SharedGeneration>, DctxError> {
        let shared = self
            .generations
            .get(&handle.generation)
            .map(Arc::clone)
            .ok_or(DctxError::Stale { reason: "generation reclaimed" })?;
        shared.borrowed_shares.store(Arc::strong_count(&shared), Ordering::Relaxed);
        Ok(shared)
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use dctx_core::selection::{RuleTexts, SelectionPolicy};
    use dctx_core::Timestamp;
    use dctx_ports::memory::{CancelFlag, FixedClock, MemorySource};
    use dctx_template::HostCeiling;

    use super::{Session, StoragePolicy};
    use crate::domain::{HostGrants, MapRequest, SnapshotRequest, VecSink, Verb};

    #[test]
    fn a_query_borrows_the_stored_generation() {
        let mut source = MemorySource::new();
        source.insert_file("src/a.rs", b"fn alpha() {}\n", 0).expect("a");
        source.insert_file("src/b.rs", b"fn beta() {}\n", 0).expect("b");
        let mut session = Session::builder(source)
            .clock(FixedClock(Timestamp(0)))
            .cancellation(Arc::new(CancelFlag::new()))
            .selection(SelectionPolicy::from_texts(&RuleTexts::default()))
            .ceiling(HostCeiling::base())
            .grants(HostGrants::PermitAll)
            .storage(StoragePolicy::Ephemeral { max_bytes: 8 * 1024 * 1024 })
            .place("fixture")
            .view("map")
            .build()
            .expect("session");
        let handle = session.snapshot(&SnapshotRequest::immutable()).expect("snapshot");
        let (live, seen) = session.generation_shares(&handle).expect("before");
        assert_eq!((live, seen), (1, 0));
        let mut request = MapRequest::new(Verb::Find);
        request.query = "alpha".to_string();
        let mut sink = VecSink::new();
        let first = session.query(&handle, &request, &mut sink).expect("find");
        assert_eq!(first.records.len(), 1);
        assert_eq!(first.records[0].path, "src/a.rs");
        assert_eq!(first.snapshot, handle.digest());
        let (live, seen) = session.generation_shares(&handle).expect("after");
        assert_eq!(live, 1);
        assert_eq!(seen, 2);
        let mut sink = VecSink::new();
        let second = session.query(&handle, &request, &mut sink).expect("again");
        assert_eq!(second.records, first.records);
        assert_eq!(second.snapshot, first.snapshot);
        let (live, seen) = session.generation_shares(&handle).expect("still");
        assert_eq!((live, seen), (1, 2));
    }
}
