// SPDX-License-Identifier: Apache-2.0
/// Longest path, in edges.
pub(crate) const MAX_HOPS: usize = 8;

/// Distance marking a state the search did not reach.
pub(super) const UNSEEN: u8 = u8::MAX;

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
pub(super) struct Incident {
    /// The node at the other end.
    pub(super) other: u32,
    /// Index into the edge list.
    pub(super) edge: usize,
    /// True when the edge leaves this node.
    pub(super) leaves: bool,
}
