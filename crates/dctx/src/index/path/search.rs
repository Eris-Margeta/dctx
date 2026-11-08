// SPDX-License-Identifier: Apache-2.0
use super::super::{Generation, write_node_id};
use super::graph::{adjacency, allowed, arrival, locate};
use super::model::{Hop, MAX_HOPS, UNSEEN};

/// The shortest path from `from` to `to`, or `None` when none exists within [`MAX_HOPS`] edges.
/// # Errors
/// The id of the first endpoint that names no node.
pub(crate) fn shortest_path(
    generation: &Generation,
    from: &str,
    to: &str,
) -> Result<Option<Vec<Hop>>, String> {
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
        let name = if edge == usize::MAX {
            String::new()
        } else {
            text.get(edges[edge].edge).to_string()
        };
        let direction = if edge == usize::MAX {
            ""
        } else if leaves {
            "out"
        } else {
            "in"
        };
        Hop {
            edge: name,
            direction,
            id,
            kind: stored.kind.to_string(),
            path: text.get(stored.path).to_string(),
        }
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
            if best.as_ref().is_none_or(|kept| {
                (&hop.id, &hop.edge, hop.direction) < (&kept.id, &kept.edge, kept.direction)
            }) {
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
