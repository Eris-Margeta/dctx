// SPDX-License-Identifier: Apache-2.0
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
    #[allow(clippy::case_sensitive_file_extension_comparisons, reason = "Preserve the existing literal suffix policy for source language dispatch and privacy exclusions; case folding would change which paths are parsed or denied.")]
    #[allow(clippy::too_many_lines, reason = "Keep graph reconciliation and file-owned storage writes in their transaction order.")]
    pub(super) fn rebuild(
        &mut self,
        next: &BTreeMap<String, CachedFile>,
        revision: &str,
        report: &mut IndexReport,
    ) -> Result<()> {
        if self.store.is_none() {
            return Err(crate::Error::Request("writer unavailable".into()));
        }
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
        let mut old_nodes: BTreeMap<String, Vec<Node>> = BTreeMap::new();
        for node in self.nodes.values() {
            old_nodes
                .entry(node.path.clone())
                .or_default()
                .push(node.clone());
        }
        let mut old_edges: BTreeMap<String, Vec<dctx_map_model::Edge>> = BTreeMap::new();
        for edge in &self.edges {
            old_edges
                .entry(edge.owner_path.clone())
                .or_default()
                .push(edge.clone());
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
                    .get(&file.file.path)
                    .is_some_and(|old| old.iter().eq(file.nodes.iter()))
                && old_edges
                    .get(&file.file.path)
                    .into_iter()
                    .flatten()
                    .eq(file.edges.iter())
            {
                continue;
            }
            if let Some(old) = self.persisted(&file.file.path)? {
                file.summaries = old
                    .summaries
                    .into_iter()
                    .filter(|summary| {
                        file.nodes
                            .iter()
                            .any(|node| node.id == summary.id && node.fingerprint == summary.fingerprint)
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
            self.write_facts(file)?;
            report.replaced += 1;
        }
        if !self.ready || self.files.keys().any(|path| !next.contains_key(path)) {
            let live = facts.keys().map(String::as_str).collect::<Vec<_>>();
            report.removed = self.prune_paths(&live)?;
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
        self.nodes = std::mem::take(&mut nodes);
        self.edges = edges;
        self.adjacency();
        self.graph_loaded = true;
        report.store_time = store_start.elapsed();
        Ok(())
    }

    fn persisted(&self, path: &str) -> Result<Option<FileFacts>> {
        let store = if let Some(disk) = &self.disk {
            disk
        } else {
            self.store
                .as_ref()
                .ok_or_else(|| crate::Error::Request("writer unavailable".into()))?
        };
        Ok(store.file(path)?)
    }

    fn write_facts(&mut self, file: &FileFacts) -> Result<()> {
        if let Some(disk) = &mut self.disk {
            disk.reindex(file)?;
        }
        self.store
            .as_mut()
            .ok_or_else(|| crate::Error::Request("writer unavailable".into()))?
            .reindex(file)?;
        Ok(())
    }

    fn prune_paths(&mut self, live: &[&str]) -> Result<usize> {
        let removed = if let Some(disk) = &mut self.disk {
            Some(disk.prune(live)?)
        } else {
            None
        };
        let mirror = self
            .store
            .as_mut()
            .ok_or_else(|| crate::Error::Request("writer unavailable".into()))?
            .prune(live)?;
        Ok(removed.unwrap_or(mirror))
    }
}
