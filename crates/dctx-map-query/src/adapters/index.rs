//! Content-hash incremental indexing with explicit stale diagnostics.
//! Implements SPEC-MAP-MODE sections 6 and 7 via public scan/parse/resolve/store APIs.
//! Retains parser retry facts and replaces only changed file-owned transactions.
use crate::{
    Result,
    domain::{bridge::bridge, cached::CachedFile},
    ports::{Snapshot, Source},
};
use dctx_map_model::{Edge, Node, StableId, Summary, SummarySource};
use dctx_map_parse::{Parser, TreeSitterParser};
use dctx_map_store::{File, FileFacts, MapStore, SqliteStore, Unresolved};
use std::{
    collections::BTreeMap,
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
    pub(crate) store: SqliteStore,
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
        Ok(Self::with_store(place.into(), SqliteStore::in_memory()?))
    }
    pub(crate) fn with_store(place: String, store: SqliteStore) -> Self {
        Self {
            store,
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

    /// Hashes authorized content and reparses only changed files.
    /// A finite deadline flags pending files stale; no obsolete source is served.
    pub fn refresh(
        &mut self,
        source: &impl Source,
        deadline: Option<Duration>,
    ) -> Result<&IndexReport> {
        let started = Instant::now();
        let snapshot = source.snapshot()?;
        let mut report = IndexReport {
            scan_time: started.elapsed(),
            ..IndexReport::default()
        };
        let parse_start = Instant::now();
        let mut parsed_files =
            super::parsing::parse_changed(&snapshot, &self.files, started, deadline)?;
        report.parse_time = parse_start.elapsed();
        let mut next = BTreeMap::new();
        for (path, current) in &snapshot.files {
            let old = self.files.get(path);
            let readable = current.bytes.is_some();
            if let Some(old) =
                old.filter(|old| old.hash == current.hash && old.readable == readable)
            {
                next.insert(path.clone(), old.clone());
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
        self.files = next;
        self.snapshot = Some(snapshot);
        self.report = report;
        self.ready = true;
        Ok(&self.report)
    }

    fn rebuild(
        &mut self,
        next: &BTreeMap<String, CachedFile>,
        revision: &str,
        report: &mut IndexReport,
    ) -> Result<()> {
        let resolve_start = Instant::now();
        let bridged = bridge(next)?;
        let resolution = super::resolving::resolve_components(bridged.facts)?;
        report.resolve_time = resolve_start.elapsed();
        let store_start = Instant::now();
        let mut nodes: BTreeMap<_, _> = next
            .values()
            .flat_map(|f| f.nodes.iter().cloned())
            .chain(bridged.structural)
            .map(|n| (n.id.clone(), n))
            .collect();
        let mut edges = resolution.edges;
        edges.extend(bridged.edges);
        edges.sort_by(|a, b| {
            (&a.owner_path, &a.src, &a.dst, a.kind).cmp(&(&b.owner_path, &b.src, &b.dst, b.kind))
        });
        edges.dedup();
        let mut facts: BTreeMap<String, FileFacts> = next
            .iter()
            .map(|(path, file)| {
                (
                    path.clone(),
                    FileFacts {
                        file: File {
                            path: path.clone(),
                            size: file.size,
                            mtime: file.mtime,
                            sha256: file.hash.clone(),
                            lang: if path.ends_with(".rs") {
                                "rust"
                            } else if path.ends_with(".md") {
                                "markdown"
                            } else {
                                "unknown"
                            }
                            .into(),
                        },
                        nodes: file.nodes.clone(),
                        edges: vec![],
                        unresolved: vec![],
                        summaries: vec![],
                    },
                )
            })
            .collect();
        for node in nodes.values().filter(|n| {
            matches!(
                n.kind,
                dctx_map_model::NodeKind::Place | dctx_map_model::NodeKind::Package
            )
        }) {
            facts
                .entry(node.path.clone())
                .or_insert_with(|| FileFacts {
                    file: File {
                        path: node.path.clone(),
                        size: 0,
                        mtime: 0,
                        sha256: String::new(),
                        lang: "structure".into(),
                    },
                    nodes: vec![],
                    edges: vec![],
                    unresolved: vec![],
                    summaries: vec![],
                })
                .nodes
                .push(node.clone());
        }
        for edge in &edges {
            if let Some(file) = facts.get_mut(&edge.owner_path) {
                file.edges.push(edge.clone());
            }
        }
        for unresolved in resolution.unresolved {
            if let Some(file) = facts.get_mut(&unresolved.owner_path) {
                let name = match unresolved.reference.target {
                    dctx_map_resolve::Target::Name(name) => name,
                    dctx_map_resolve::Target::Id(id) => id.to_string(),
                };
                file.unresolved.push(Unresolved {
                    name,
                    kind: unresolved.reference.kind,
                });
            }
        }
        for file in facts.values_mut() {
            file.nodes.sort_by(|a, b| a.id.cmp(&b.id));
            file.unresolved
                .sort_by(|a, b| (&a.name, a.kind).cmp(&(&b.name, b.kind)));
            file.unresolved.dedup();
            if let Some(old) = self.store.file(&file.file.path)? {
                file.summaries = old
                    .summaries
                    .into_iter()
                    .filter(|s| {
                        file.nodes
                            .iter()
                            .any(|n| n.id == s.id && n.fingerprint == s.fingerprint)
                    })
                    .collect();
                if old.file.sha256 == file.file.sha256
                    && old.nodes == file.nodes
                    && old.edges == file.edges
                    && old.unresolved == file.unresolved
                {
                    continue;
                }
            }
            self.store.reindex(file)?;
            report.replaced += 1;
        }
        report.removed = self
            .store
            .prune(&facts.keys().map(String::as_str).collect::<Vec<_>>())?;
        let previous: Vec<_> = self
            .files
            .values()
            .flat_map(|f| f.nodes.iter().cloned())
            .collect();
        let current: Vec<_> = nodes.values().cloned().collect();
        for moved in dctx_map_resolve::detect_moves(&previous, &current, revision) {
            self.moves.insert(moved.old, moved.new);
        }
        self.nodes = std::mem::take(&mut nodes);
        self.edges = edges;
        self.degree.clear();
        self.incoming.clear();
        self.outgoing.clear();
        for (i, edge) in self.edges.iter().enumerate() {
            *self.degree.entry(edge.dst.clone()).or_default() += 1;
            self.incoming.entry(edge.dst.clone()).or_default().push(i);
            self.outgoing.entry(edge.src.clone()).or_default().push(i);
        }
        report.store_time = store_start.elapsed();
        Ok(())
    }
}
