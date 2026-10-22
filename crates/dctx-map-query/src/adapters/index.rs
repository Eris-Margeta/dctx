//! Content-hash incremental indexing with explicit stale diagnostics.
//! Implements SPEC-MAP-MODE sections 6 and 7 via public scan/parse/resolve/store APIs.
//! Retains parser retry facts and replaces only changed file-owned transactions.
use crate::{
    Overlay, ParseStatus, Result,
    domain::{cached::CachedFile, parse_status::PARSER_REVISION},
    ports::Snapshot,
};
use dctx_core::ids::Timestamp;
use dctx_map_model::{Edge, Node, StableId, Summary, SummarySource};
use dctx_map_parse::{Parser, TreeSitterParser};
use dctx_map_store::SqliteStore;
use dctx_ports::memory::{CancelFlag, FixedClock};
use dctx_ports::{Cancellation, Clock};
use std::{collections::BTreeMap, time::{Duration, Instant}};

/// Evidence from the latest source refresh.
#[derive(Debug, Clone, Default)]
pub struct IndexReport {
    /// Time capturing the complete authorized inventory.
    pub scan_time: Duration,
    /// Time parsing changed source files.
    pub parse_time: Duration,
    /// Time constructing and resolving the complete graph.
    pub resolve_time: Duration,
    /// Time comparing and committing file-owned storage facts.
    pub store_time: Duration,
    /// Files parsed because content or access changed.
    pub parsed: usize,
    /// File-owned transactions replaced, including dependent edges.
    pub replaced: usize,
    /// Files removed from the store.
    pub removed: usize,
    /// Stale paths and their explicit failure reasons.
    pub stale: BTreeMap<String, String>,
    /// Cached syntax failures and partial extraction diagnostics; these do not trigger retries.
    pub diagnostics: BTreeMap<String, ParseStatus>,
}

/// Stateful query index with a segment journal and a sqlite comparison store.
pub struct Index {
    pub(crate) store: Option<SqliteStore>,
    pub(crate) disk: Option<SqliteStore>,
    pub(crate) segments: Option<super::segment::Segments>,
    pub(crate) database: Option<std::path::PathBuf>,
    pub(crate) mirror: Option<std::path::PathBuf>,
    pub(crate) graph_loaded: bool,
    pub(crate) files: BTreeMap<String, CachedFile>,
    pub(crate) nodes: BTreeMap<StableId, Node>,
    pub(crate) edges: Vec<Edge>,
    pub(crate) moves: BTreeMap<StableId, StableId>,
    pub(crate) snapshot: Option<Snapshot>,
    pub(crate) report: IndexReport,
    pub(crate) place: String,
    pub(crate) ready: bool,
    pub(crate) writing: bool,
    pub(crate) generation: u64,
    pub(crate) overlay: Overlay,
    pub(crate) degree: BTreeMap<StableId, usize>,
    pub(crate) incoming: BTreeMap<StableId, Vec<usize>>,
    pub(crate) outgoing: BTreeMap<StableId, Vec<usize>>,
    cancel_flag: CancelFlag,
    clock: FixedClock,
}

impl Index {
    /// Creates an isolated in-memory index.
    ///
    /// # Errors
    /// Returns database initialization or schema migration errors.
    pub fn in_memory(place: impl Into<String>) -> Result<Self> {
        Ok(Self::with_store(
            &place.into(),
            Some(SqliteStore::in_memory()?),
        ))
    }

    pub(crate) fn with_store(place: &str, store: Option<SqliteStore>) -> Self {
        Self {
            store,
            disk: None,
            segments: None,
            database: None,
            mirror: None,
            graph_loaded: true,
            place: place.split_whitespace().collect::<Vec<_>>().join("_"),
            files: BTreeMap::new(),
            nodes: BTreeMap::new(),
            edges: vec![],
            moves: BTreeMap::new(),
            snapshot: None,
            report: IndexReport::default(),
            ready: false,
            writing: false,
            generation: 0,
            overlay: Overlay::default(),
            degree: BTreeMap::new(),
            incoming: BTreeMap::new(),
            outgoing: BTreeMap::new(),
            cancel_flag: CancelFlag::new(),
            clock: FixedClock(Timestamp(0)),
        }
    }

    /// Latest parse/transaction counts and explicit stale failures.
    pub fn report(&self) -> &IndexReport {
        &self.report
    }

    /// Paths added, edited, or removed since the clean base.
    pub fn overlay(&self) -> &Overlay {
        &self.overlay
    }

    /// Stops the next refresh or query on this index.
    pub fn cancel(&self) {
        self.cancel_flag.cancel();
    }

    pub(crate) fn cancelled(&self) -> bool {
        self.cancel_flag.is_cancelled()
    }

    /// Age in seconds from the injected clock. The clock is frozen at the epoch.
    pub(crate) fn age_seconds(&self) -> u64 {
        self.clock.now().millis() / 1000
    }

    /// Applies a captured inventory, parsing only changed content.
    /// A finite deadline flags pending files stale; no obsolete source is served.
    #[allow(clippy::too_many_lines, reason = "Keep ordered incremental snapshot application and its stale/publication transitions together in one operation.")]
    pub(super) fn apply_snapshot(
        &mut self,
        mut snapshot: Snapshot,
        started: Instant,
        deadline: Option<Duration>,
    ) -> Result<&IndexReport> {
        if self.cancelled() {
            return Err(crate::Error::Request("cancelled".into()));
        }
        self.overlay = Overlay::observe(self.ready, self.snapshot.as_ref(), &snapshot);
        let mut report = IndexReport {
            scan_time: started.elapsed(),
            ..IndexReport::default()
        };
        let parse_start = Instant::now();
        let mut parsed_files =
            super::parsing::parse_changed(&snapshot, &self.files, parse_start, deadline)?;
        report.parse_time = parse_start.elapsed();
        let mut next = BTreeMap::new();
        for (path, current) in &snapshot.files {
            if self.cancelled() {
                return Err(crate::Error::Request("cancelled".into()));
            }
            let old = self.files.get(path);
            let readable = current.readable;
            if let Some(old) = old.filter(|old| {
                old.hash == current.hash
                    && old.readable == readable
                    && old.parser_revision == PARSER_REVISION
            }) {
                let mut file = old.clone();
                file.size = current.size;
                file.mtime = current.mtime;
                next.insert(path.clone(), file);
                continue;
            }
            let parsed = parsed_files
                .remove(path)
                .ok_or_else(|| crate::Error::Request("missing parser result".into()))?;
            match parsed {
                Ok((mut parsed, status)) => {
                    if !readable {
                        for node in &mut parsed.nodes {
                            node.summary = Summary {
                                text: "Content omitted by scan policy".into(),
                                source: SummarySource::Doc,
                            };
                        }
                    }
                    report.parsed += 1;
                    let mut file = CachedFile::parsed(
                        parsed,
                        current.hash.clone(),
                        current.size,
                        current.mtime,
                        readable,
                    );
                    file.status = status;
                    next.insert(path.clone(), file);
                }
                Err(reason) => {
                    report.stale.insert(path.clone(), reason);
                    if let Some(old) = old {
                        next.insert(path.clone(), old.clone());
                    } else {
                        let mut parsed = TreeSitterParser.parse(path, b"")?;
                        for node in &mut parsed.nodes {
                            node.summary.text = "stale: source could not be indexed".into();
                        }
                        next.insert(
                            path.clone(),
                            CachedFile::parsed(
                                parsed,
                                String::new(),
                                current.size,
                                current.mtime,
                                readable,
                            ),
                        );
                    }
                }
            }
        }
        let changed = !self.ready
            || next.len() != self.files.len()
            || next.iter().any(|(path, file)| {
                self.files.get(path).is_none_or(|old| {
                    old.hash != file.hash
                        || old.readable != file.readable
                        || old.parser_revision != file.parser_revision
                        || old.status != file.status
                })
            });
        if changed {
            self.rebuild(&next, &snapshot.revision, &mut report)?;
        }
        self.files = next;
        report.diagnostics = self
            .files
            .iter()
            .filter(|(_, file)| file.status != ParseStatus::Parsed)
            .map(|(path, file)| (path.clone(), file.status.clone()))
            .collect();
        for (path, file) in &mut snapshot.files {
            file.bytes = None;
            if report.stale.contains_key(path) {
                file.stamp = crate::ports::FileStamp::default();
            }
        }
        self.snapshot = Some(snapshot);
        self.report = report;
        self.ready = true;
        Ok(&self.report)
    }

    pub(super) fn adjacency(&mut self) {
        self.degree.clear();
        self.incoming.clear();
        self.outgoing.clear();
        for (i, edge) in self.edges.iter().enumerate() {
            *self.degree.entry(edge.dst.clone()).or_default() += 1;
            self.incoming.entry(edge.dst.clone()).or_default().push(i);
            self.outgoing.entry(edge.src.clone()).or_default().push(i);
        }
    }
}

impl Drop for Index {
    fn drop(&mut self) {
        self.release_mirror();
    }
}
