//! Bounded outlines, directed graph neighborhoods and node explanations.
//! Implements SPEC-MAP-MODE section 4 using indexed adjacency lists.
//! Pure traversal over Index facts; query dispatch handles freshness and budgets.
use super::Index;
use crate::{
    Direction, Record, Request, Result,
    domain::{inside, validate_path},
};
use dctx_map_model::{EdgeKind, NodeKind, Relation, StableId};
use std::collections::{BTreeMap, BTreeSet};

impl Index {
    pub(super) fn outline(
        &self,
        request: &Request,
        target: Option<&StableId>,
    ) -> Result<Vec<Record>> {
        let mut roots = Vec::new();
        if let Some(id) = target {
            roots.push(id.clone());
        } else {
            let path = if request.target.is_empty() {
                "."
            } else {
                request.target.trim_end_matches('/')
            };
            validate_path(path)?;
            if path == "." {
                roots.push("p:.".parse()?);
            } else if let Some(node) = self
                .nodes
                .values()
                .find(|n| n.path == path && matches!(n.kind, NodeKind::Package | NodeKind::Module))
            {
                roots.push(node.id.clone());
            } else {
                roots.extend(
                    self.nodes
                        .values()
                        .filter(|n| n.kind == NodeKind::Module && inside(&n.path, path))
                        .map(|n| n.id.clone()),
                );
            }
        }
        let mut seen = BTreeSet::new();
        let mut records = Vec::new();
        let mut frontier = roots;
        for depth in 0..=request.depth {
            let mut next = Vec::new();
            for id in frontier {
                if !seen.insert(id.clone()) {
                    continue;
                }
                if let Some(node) = self.nodes.get(&id) {
                    if depth > 0 || target.is_some() || node.kind != NodeKind::Place {
                        let mut record = Record::from(node);
                        record.score = 10_000 - i64::from(depth) * 1_000 + i64::from(node.public);
                        records.push(record);
                    }
                    for index in self.outgoing.get(&id).into_iter().flatten() {
                        let edge = &self.edges[*index];
                        if edge.kind == EdgeKind::Contains {
                            next.push(edge.dst.clone());
                        }
                    }
                }
            }
            frontier = next;
        }
        Ok(records)
    }

    pub(super) fn neighbors(&self, request: &Request, target: &StableId) -> Vec<Record> {
        let mut visited = BTreeSet::new();
        let mut relations = BTreeMap::new();
        let mut frontier = vec![target.clone()];
        for _ in 0..request.depth {
            let mut next = Vec::new();
            for id in frontier {
                if !visited.insert(id.clone()) {
                    continue;
                }
                let directions = [(false, &self.outgoing), (true, &self.incoming)];
                for (incoming, adjacency) in directions {
                    if incoming && request.direction == Direction::Out
                        || !incoming && request.direction == Direction::In
                    {
                        continue;
                    }
                    for index in adjacency.get(&id).into_iter().flatten() {
                        let edge = &self.edges[*index];
                        if !request.edges.is_empty() && !request.edges.contains(&edge.kind) {
                            continue;
                        }
                        let other = if incoming { &edge.src } else { &edge.dst };
                        let Some(node) = self.nodes.get(other) else {
                            continue;
                        };
                        let mut record = Record::from(node);
                        record.relation = Some(Relation {
                            edge: edge.kind,
                            confidence: edge.conf,
                            incoming,
                        });
                        record.stale = self.report.stale.contains_key(&edge.owner_path);
                        record.score = 100_000 - edge.kind as i64 * 1_000
                            + self.degree.get(other).copied().unwrap_or(0).min(999) as i64;
                        relations.insert(
                            (edge.src.clone(), edge.dst.clone(), edge.kind, incoming),
                            record,
                        );
                        next.push(other.clone());
                    }
                }
            }
            frontier = next;
        }
        relations.into_values().collect()
    }

    pub(super) fn explain(&self, id: &StableId) -> Record {
        let node = &self.nodes[id];
        let mut record = Record::from(node);
        record.signature = Some(node.signature.clone());
        let mut incoming: Vec<_> = self
            .incoming
            .get(id)
            .into_iter()
            .flatten()
            .map(|i| self.edges[*i].clone())
            .collect();
        let mut outgoing: Vec<_> = self
            .outgoing
            .get(id)
            .into_iter()
            .flatten()
            .map(|i| self.edges[*i].clone())
            .collect();
        incoming.sort_by_key(|e| {
            (
                std::cmp::Reverse(self.degree.get(&e.src).copied().unwrap_or(0)),
                e.src.clone(),
                e.kind,
            )
        });
        outgoing.sort_by_key(|e| {
            (
                std::cmp::Reverse(self.degree.get(&e.dst).copied().unwrap_or(0)),
                e.dst.clone(),
                e.kind,
            )
        });
        record.tests = incoming
            .iter()
            .filter(|e| e.kind == EdgeKind::Tests)
            .take(5)
            .map(|e| e.src.clone())
            .collect();
        record.documents = incoming
            .iter()
            .filter(|e| e.kind == EdgeKind::Documents)
            .take(5)
            .map(|e| e.src.clone())
            .collect();
        record.stale = incoming
            .iter()
            .chain(&outgoing)
            .any(|e| self.report.stale.contains_key(&e.owner_path));
        record.incoming = incoming.into_iter().take(5).collect();
        record.outgoing = outgoing.into_iter().take(5).collect();
        record
    }
}
