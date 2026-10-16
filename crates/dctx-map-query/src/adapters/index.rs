//! Content-hash incremental indexing with explicit stale diagnostics.
//! Implements SPEC-MAP-MODE sections 6 and 7 via public scan/parse/resolve/store APIs.
//! Retains parser retry facts and replaces only changed file-owned transactions.
use crate::{Result, domain::cached::CachedFile, ports::Snapshot};
use dctx_map_model::{Edge, Node, StableId, Summary, SummarySource};
use dctx_map_parse::{Parser, TreeSitterParser};
use dctx_map_store::SqliteStore;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

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
}

/// Stateful query index with independently selected storage.
pub struct Index {
    pub(crate) store: Option<SqliteStore>,
    pub(crate) database: Option<std::path::PathBuf>,
    pub(crate) reader: Option<rusqlite::Connection>,
    pub(crate) graph_loaded: bool,
    pub(crate) dirty_files: BTreeSet<String>,
    pub(crate) dirty_nodes: BTreeSet<StableId>,
    pub(crate) dirty_edges: BTreeSet<String>,
    pub(crate) files: BTreeMap<String, CachedFile>,
    pub(crate) nodes: BTreeMap<StableId, Node>,
    pub(crate) edges: Vec<Edge>,
    pub(crate) moves: BTreeMap<StableId, StableId>,
    pub(crate) snapshot: Option<Snapshot>,
    pub(crate) report: IndexReport,
    pub(crate) place: String,
    pub(crate) ready: bool,
    pub(crate) degree: BTreeMap<StableId, usize>,
    pub(crate) incoming: BTreeMap<StableId, Vec<usize>>,
    pub(crate) outgoing: BTreeMap<StableId, Vec<usize>>,
}
impl Index {
    /// Creates an isolated in-memory index.
    pub fn in_memory(place: impl Into<String>) -> Result<Self> {
        Ok(Self::with_store(
            place.into(),
            Some(SqliteStore::in_memory()?),
        ))
    }
    pub(crate) fn with_store(place: String, store: Option<SqliteStore>) -> Self {
        Self {
            store,
            database: None,
            reader: None,
            graph_loaded: true,
            dirty_files: BTreeSet::new(),
            dirty_nodes: BTreeSet::new(),
            dirty_edges: BTreeSet::new(),
            place: place.split_whitespace().collect::<Vec<_>>().join("_"),
            files: BTreeMap::new(),
            nodes: BTreeMap::new(),
            edges: vec![],
            moves: BTreeMap::new(),
            snapshot: None,
            report: IndexReport::default(),
            ready: false,
            degree: BTreeMap::new(),
            incoming: BTreeMap::new(),
            outgoing: BTreeMap::new(),
        }
    }
    /// Latest parse/transaction counts and explicit stale failures.
    pub fn report(&self) -> &IndexReport {
        &self.report
    }

    /// Applies a captured inventory, parsing only changed content.
    /// A finite deadline flags pending files stale; no obsolete source is served.
    pub(super) fn apply_snapshot(
        &mut self,
        mut snapshot: Snapshot,
        started: Instant,
        deadline: Option<Duration>,
    ) -> Result<&IndexReport> {
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
            let old = self.files.get(path);
            let readable = current.readable;
            if let Some(old) =
                old.filter(|old| old.hash == current.hash && old.readable == readable)
            {
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
                Ok(mut parsed) => {
                    if !readable {
                        for node in &mut parsed.nodes {
                            node.summary = Summary {
                                text: "Content omitted by scan policy".into(),
                                source: SummarySource::Doc,
                            };
                        }
                    }
                    report.parsed += 1;
                    next.insert(
                        path.clone(),
                        CachedFile::parsed(
                            parsed,
                            current.hash.clone(),
                            current.size,
                            current.mtime,
                            readable,
                        ),
                    );
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
                self.files
                    .get(path)
                    .is_none_or(|old| old.hash != file.hash || old.readable != file.readable)
            });
        if changed {
            self.rebuild(&next, &snapshot.revision, &mut report)?;
        }
        self.dirty_files = self
            .files
            .keys()
            .chain(next.keys())
            .filter(|path| {
                self.snapshot.is_none()
                    || match (self.files.get(*path), next.get(*path)) {
                        (Some(old), Some(new)) => {
                            old.hash != new.hash
                                || old.readable != new.readable
                                || old.size != new.size
                                || old.mtime != new.mtime
                        }
                        _ => true,
                    }
            })
            .cloned()
            .collect();
        self.files = next;
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
