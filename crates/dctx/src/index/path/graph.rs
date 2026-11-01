use super::super::{Generation, id_matches};
use super::model::Incident;

/// Index of the node whose stable id is `target`.
pub(super) fn locate(generation: &Generation, target: &str) -> Option<u32> {
    let index = generation
        .nodes
        .iter()
        .position(|node| id_matches(&generation.text, node, target))?;
    u32::try_from(index).ok()
}

/// Edges by node, self-loops left out, in both directions.
pub(super) fn adjacency(generation: &Generation) -> Vec<Vec<Incident>> {
    let mut by_node: Vec<Vec<Incident>> = vec![Vec::new(); generation.nodes.len()];
    for (edge, stored) in generation.edges.iter().enumerate() {
        let (src, dst) = (
            usize::try_from(stored.src).unwrap_or(usize::MAX),
            usize::try_from(stored.dst).unwrap_or(usize::MAX),
        );
        if src == dst || src >= by_node.len() || dst >= by_node.len() {
            continue;
        }
        by_node[src].push(Incident {
            other: stored.dst,
            edge,
            leaves: true,
        });
        by_node[dst].push(Incident {
            other: stored.src,
            edge,
            leaves: false,
        });
    }
    by_node
}

/// The search state after taking `step` from a node: 1 when the walk just climbed to a container, else 0.
pub(super) fn arrival(is_contains: bool, leaves: bool) -> usize {
    usize::from(is_contains && !leaves)
}

/// True when a walk in `state` may take a step with these properties. A climb may not be followed by a descent.
pub(super) fn allowed(state: usize, is_contains: bool, leaves: bool) -> bool {
    !(state == 1 && is_contains && leaves)
}
