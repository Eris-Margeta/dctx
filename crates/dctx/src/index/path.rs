//! Path: the shortest edge path between two nodes (the `dctx map path` verb).
//! Searches every stored edge (calls, imports, contains, ...) in both directions, at most [`MAX_HOPS`] hops.
//! A walk may not climb out of a container and straight down into a sibling, because that links every
//! symbol of a file through its module and says nothing about the code. Among equal paths the one with the
//! smaller node ids wins, so the page is deterministic. The page is its own shape, not an [`crate::domain::Answer`].

use std::fmt::Write as _;

use super::hubs::{quote, PageHead};
use super::{id_matches, write_node_id, Generation};

/// Longest path, in edges.
pub(crate) const MAX_HOPS: usize = 8;

/// Distance marking a state the search did not reach.
const UNSEEN: u8 = u8::MAX;

/// One node on the path and the edge that led to it. The first hop has no edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Hop {
    /// Edge name, or empty for the starting node.
    pub edge: String,
    /// `out` when the edge points from the previous node to this one, `in` when it points back. Empty for the start.
    pub direction: &'static str,
    /// Stable id of the node.
    pub id: String,
    /// Wire kind of the node.
    pub kind: String,
    /// Root-relative path of the node's file.
    pub path: String,
}

/// An edge seen from one of its ends.
#[derive(Debug, Clone, Copy)]
struct Incident {
    /// The node at the other end.
    other: u32,
    /// Index into the edge list.
    edge: usize,
    /// True when the edge leaves this node.
    leaves: bool,
}

/// Index of the node whose stable id is `target`.
fn locate(generation: &Generation, target: &str) -> Option<u32> {
    let index = generation.nodes.iter().position(|node| id_matches(&generation.text, node, target))?;
    u32::try_from(index).ok()
}

/// Edges by node, self-loops left out, in both directions.
fn adjacency(generation: &Generation) -> Vec<Vec<Incident>> {
    let mut by_node: Vec<Vec<Incident>> = vec![Vec::new(); generation.nodes.len()];
    for (edge, stored) in generation.edges.iter().enumerate() {
        let (src, dst) = (usize::try_from(stored.src).unwrap_or(usize::MAX), usize::try_from(stored.dst).unwrap_or(usize::MAX));
        if src == dst || src >= by_node.len() || dst >= by_node.len() {
            continue;
        }
        by_node[src].push(Incident { other: stored.dst, edge, leaves: true });
        by_node[dst].push(Incident { other: stored.src, edge, leaves: false });
    }
    by_node
}

/// The search state after taking `step` from a node: 1 when the walk just climbed to a container, else 0.
fn arrival(is_contains: bool, leaves: bool) -> usize {
    usize::from(is_contains && !leaves)
}

/// True when a walk in `state` may take a step with these properties. A climb may not be followed by a descent.
fn allowed(state: usize, is_contains: bool, leaves: bool) -> bool {
    !(state == 1 && is_contains && leaves)
}

/// The shortest path from `from` to `to`, or `None` when none exists within [`MAX_HOPS`] edges.
/// # Errors
/// The id of the first endpoint that names no node.
pub(crate) fn shortest_path(generation: &Generation, from: &str, to: &str) -> Result<Option<Vec<Hop>>, String> {
    let start = locate(generation, from).ok_or_else(|| from.to_string())?;
    let goal = locate(generation, to).ok_or_else(|| to.to_string())?;
    let (text, edges) = (&generation.text, &generation.edges);
    let around = adjacency(generation);
    let contains = |edge: usize| text.get(edges[edge].edge) == "contains";
    let slot = |node: u32, state: usize| usize::try_from(node).unwrap_or(0) * 2 + state;
    // Distance to the goal, searched backwards one level at a time, from both states of the goal.
    let mut dist = vec![UNSEEN; generation.nodes.len() * 2];
    dist[slot(goal, 0)] = 0;
    dist[slot(goal, 1)] = 0;
    let mut level = vec![(goal, 0usize), (goal, 1usize)];
    let mut depth = 0u8;
    while dist[slot(start, 0)] == UNSEEN && !level.is_empty() && usize::from(depth) < MAX_HOPS {
        depth += 1;
        let mut next = Vec::new();
        for (node, state) in level {
            for seen in &around[usize::try_from(node).unwrap_or(0)] {
                // `seen.leaves` is true when the edge leaves `node`, so the walker coming from `seen.other` goes `in`.
                let walker_leaves = !seen.leaves;
                let is_contains = contains(seen.edge);
                if arrival(is_contains, walker_leaves) != state {
                    continue;
                }
                for before in 0..2 {
                    let at = slot(seen.other, before);
                    if allowed(before, is_contains, walker_leaves) && dist[at] == UNSEEN {
                        dist[at] = depth;
                        next.push((seen.other, before));
                    }
                }
            }
        }
        level = next;
    }
    if dist[slot(start, 0)] == UNSEEN {
        return Ok(None);
    }
    let node_hop = |node: u32, edge: usize, leaves: bool| {
        let stored = &generation.nodes[usize::try_from(node).unwrap_or(0)];
        let mut id = String::new();
        write_node_id(&mut id, text, stored);
        let name = if edge == usize::MAX { String::new() } else { text.get(edges[edge].edge).to_string() };
        let direction = if edge == usize::MAX { "" } else if leaves { "out" } else { "in" };
        Hop { edge: name, direction, id, kind: stored.kind.clone(), path: text.get(stored.path).to_string() }
    };
    let (mut node, mut state) = (start, 0usize);
    let mut hops = vec![node_hop(node, usize::MAX, true)];
    while dist[slot(node, state)] > 0 {
        let want = dist[slot(node, state)] - 1;
        let mut best: Option<Hop> = None;
        let mut best_at = (node, state);
        for step in &around[usize::try_from(node).unwrap_or(0)] {
            let is_contains = contains(step.edge);
            let after = arrival(is_contains, step.leaves);
            if !allowed(state, is_contains, step.leaves) || dist[slot(step.other, after)] != want {
                continue;
            }
            let hop = node_hop(step.other, step.edge, step.leaves);
            if best.as_ref().is_none_or(|kept| (&hop.id, &hop.edge, hop.direction) < (&kept.id, &kept.edge, kept.direction)) {
                best = Some(hop);
                best_at = (step.other, after);
            }
        }
        let Some(hop) = best else { break };
        hops.push(hop);
        (node, state) = best_at;
    }
    Ok(Some(hops))
}

/// Encodes one page. `hops` is `None` for "no path". `json` selects the JSON v2 style object, otherwise the text page.
pub(crate) fn encode(head: &PageHead<'_>, hops: Option<&[Hop]>, json: bool) -> String {
    if json { encode_json(head, hops) } else { encode_text(head, hops) }
}

/// Text page: `# <place> path`, then `<edge>:<dir>\t<id>\t<kind>\t<path>` per hop (`start` for the first), or `no path`.
fn encode_text(head: &PageHead<'_>, hops: Option<&[Hop]>) -> String {
    let mut out = format!("# {} path", head.place);
    if head.meta {
        let _ = write!(out, " {} g={}", head.snapshot, head.generation);
    }
    out.push('\n');
    let Some(hops) = hops else {
        out.push_str("no path\n");
        return out;
    };
    for hop in hops {
        let label = if hop.edge.is_empty() { "start".to_string() } else { format!("{}:{}", hop.edge, hop.direction) };
        let _ = writeln!(out, "{label}\t{}\t{}\t{}", hop.id, hop.kind, hop.path);
    }
    out
}

/// JSON page with the envelope of the other verbs, `found`, and `records` of hops.
fn encode_json(head: &PageHead<'_>, hops: Option<&[Hop]>) -> String {
    let mut out = String::from("{\"v\":2,\"place\":");
    quote(&mut out, head.place);
    if head.meta {
        out.push_str(",\"snapshot\":");
        quote(&mut out, head.snapshot);
        let _ = write!(out, ",\"generation\":{}", head.generation);
    }
    out.push_str(",\"verb\":\"path\",\"freshness\":{\"mode\":");
    quote(&mut out, head.mode);
    out.push_str(",\"state\":");
    quote(&mut out, head.state);
    if head.meta {
        let _ = write!(out, ",\"age_s\":{}", head.age_s);
    }
    out.push_str("},\"precision\":{\"coverage\":");
    quote(&mut out, head.coverage);
    out.push_str(",\"completion\":");
    quote(&mut out, head.completion);
    out.push_str(",\"parser\":");
    quote(&mut out, head.parser);
    let _ = write!(out, "}},\"found\":{},\"records\":[", hops.is_some());
    for (index, hop) in hops.unwrap_or_default().iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"id\":");
        quote(&mut out, &hop.id);
        out.push_str(",\"kind\":");
        quote(&mut out, &hop.kind);
        out.push_str(",\"path\":");
        quote(&mut out, &hop.path);
        if !hop.edge.is_empty() {
            out.push_str(",\"edge\":");
            quote(&mut out, &hop.edge);
            out.push_str(",\"direction\":");
            quote(&mut out, hop.direction);
        }
        out.push('}');
    }
    out.push_str("]}\n");
    out
}

#[cfg(test)]
#[path = "path_tests.rs"]
mod tests;
