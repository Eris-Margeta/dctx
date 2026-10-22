//! Serializable generation of the query graph.
//! Implements DX-C5 persistence: one journal, chunked into fact summaries.
//! Segment publication and cache load are the only callers.
use super::Index;
use crate::{domain::cached::CachedFile, ports::Snapshot};
use crate::Overlay;
use dctx_map_model::{Edge, Node, StableId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const VERSION: u32 = 3;
const CHUNK: usize = 200_000;
const EMPTY_REVISION: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// One committed or interrupted query generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Journal {
    pub version: u32,
    pub place: String,
    pub writing: bool,
    pub snapshot: Snapshot,
    pub stale: BTreeMap<String, String>,
    pub moves: BTreeMap<StableId, StableId>,
    pub files: BTreeMap<String, CachedFile>,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub overlay: Overlay,
    pub generation: u64,
}

impl Journal {
    pub(super) fn capture(index: &Index) -> Self {
        let mut nodes: Vec<_> = index.nodes.values().cloned().collect();
        nodes.sort_by(|left, right| left.id.cmp(&right.id));
        let mut edges = index.edges.clone();
        edges.sort_by(|left, right| {
            (&left.owner_path, &left.src, &left.dst, left.kind).cmp(&(
                &right.owner_path,
                &right.src,
                &right.dst,
                right.kind,
            ))
        });
        Self {
            version: VERSION,
            place: index.place.clone(),
            writing: index.writing,
            snapshot: index.snapshot.clone().unwrap_or_else(empty_snapshot),
            stale: index.report.stale.clone(),
            moves: index.moves.clone(),
            files: index.files.clone(),
            nodes,
            edges,
            overlay: index.overlay.clone(),
            generation: index.generation,
        }
    }

}

pub(super) fn chunk(text: &str) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    let mut parts = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        if rest.len() <= CHUNK {
            parts.push(rest.to_string());
            break;
        }
        let mut end = CHUNK;
        while !rest.is_char_boundary(end) {
            end -= 1;
        }
        if end == 0 {
            end = rest.chars().next().map_or(1, char::len_utf8);
        }
        let (head, tail) = rest.split_at(end);
        parts.push(head.to_string());
        rest = tail;
    }
    parts
}

fn empty_snapshot() -> Snapshot {
    Snapshot {
        files: BTreeMap::new(),
        revision: EMPTY_REVISION.into(),
        dirty: BTreeSet::new(),
        recent: BTreeSet::new(),
        directories: BTreeMap::new(),
        rules: BTreeMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_join_without_splitting_a_character() {
        let mut text = "a".repeat(199_999);
        text.push('β');
        text.push_str(&"b".repeat(20));
        let parts = chunk(&text);
        assert_eq!(parts.concat(), text);
        assert!(parts.iter().all(|part| part.is_char_boundary(part.len())));
    }
}
