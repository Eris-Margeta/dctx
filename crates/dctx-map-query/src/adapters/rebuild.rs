//! Reconciles changed graph facts through the public per-file store API.
//! Implements SPEC-MAP-MODE section 7, retaining retry and move semantics.
//! Called only by the index refresh writer after content changes.
use super::Index;
use super::index::IndexReport;
use crate::{
    Result,
    domain::{bridge::bridge, cached::CachedFile},
};
use dctx_map_model::Node;
use dctx_map_store::{File, FileFacts, MapStore, Unresolved};
use std::{collections::BTreeMap, time::Instant};
impl Index {
    pub(super) fn rebuild(
        &mut self,
        next: &BTreeMap<String, CachedFile>,
        revision: &str,
        report: &mut IndexReport,
    ) -> Result<()> {
        let store = self
            .store
            .as_mut()
            .ok_or_else(|| crate::Error::Request("writer unavailable".into()))?;
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
        let mut old_nodes: BTreeMap<&str, Vec<&Node>> = BTreeMap::new();
        for node in self.nodes.values() {
            old_nodes.entry(&node.path).or_default().push(node);
        }
        let mut old_edges: BTreeMap<&str, Vec<&dctx_map_model::Edge>> = BTreeMap::new();
        for edge in &self.edges {
            old_edges.entry(&edge.owner_path).or_default().push(edge);
        }
        for file in facts.values_mut() {
            file.nodes.sort_by(|a, b| a.id.cmp(&b.id));
            file.unresolved
                .sort_by(|a, b| (&a.name, a.kind).cmp(&(&b.name, b.kind)));
            file.unresolved.dedup();
            if self.ready
                && self
                    .files
                    .get(&file.file.path)
                    .is_some_and(|old| old.hash == file.file.sha256)
                && old_nodes
                    .get(file.file.path.as_str())
                    .is_some_and(|old| old.iter().copied().eq(file.nodes.iter()))
                && old_edges
                    .get(file.file.path.as_str())
                    .into_iter()
                    .flatten()
                    .copied()
                    .eq(file.edges.iter())
            {
                continue;
            }
            if let Some(old) = store.file(&file.file.path)? {
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
            store.reindex(file)?;
            report.replaced += 1;
        }
        if !self.ready || self.files.keys().any(|path| !next.contains_key(path)) {
            report.removed = store.prune(&facts.keys().map(String::as_str).collect::<Vec<_>>())?;
        }
        let previous: Vec<_> = self
            .files
            .values()
            .flat_map(|f| f.nodes.iter().cloned())
            .collect();
        let current: Vec<_> = nodes.values().cloned().collect();
        for moved in dctx_map_resolve::detect_moves(&previous, &current, revision) {
            self.moves.insert(moved.old, moved.new);
        }
        self.dirty_nodes = self
            .nodes
            .keys()
            .chain(nodes.keys())
            .filter(|id| !self.ready || self.nodes.get(*id) != nodes.get(*id))
            .cloned()
            .collect();
        let mut new_edges: BTreeMap<&str, Vec<&dctx_map_model::Edge>> = BTreeMap::new();
        for edge in &edges {
            new_edges.entry(&edge.owner_path).or_default().push(edge);
        }
        self.dirty_edges = old_edges
            .keys()
            .chain(new_edges.keys())
            .filter(|owner| !self.ready || old_edges.get(*owner) != new_edges.get(*owner))
            .map(|owner| (*owner).to_owned())
            .collect();
        self.nodes = std::mem::take(&mut nodes);
        self.edges = edges;
        let old_degree = std::mem::take(&mut self.degree);
        self.adjacency();
        for id in old_degree.keys().chain(self.degree.keys()) {
            if old_degree.get(id) != self.degree.get(id) {
                self.dirty_nodes.insert(id.clone());
            }
        }
        self.graph_loaded = true;
        report.store_time = store_start.elapsed();
        Ok(())
    }
}
