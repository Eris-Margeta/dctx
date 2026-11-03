// SPDX-License-Identifier: Apache-2.0
//! Path: the shortest edge path between two nodes (the `dctx map path` verb).
//! Searches every stored edge (calls, imports, contains, ...) in both directions, at most [`MAX_HOPS`] hops.
//! A walk may not climb out of a container and straight down into a sibling, because that links every
//! symbol of a file through its module and says nothing about the code. Among equal paths the one with the
//! smaller node ids wins, so the page is deterministic. The page is its own shape, not an [`crate::domain::Answer`].

mod graph;
mod model;
mod render;
mod search;

pub(crate) use render::encode;
pub(crate) use search::shortest_path;

#[cfg(test)]
use super::Generation;
#[cfg(test)]
use super::hubs::PageHead;
#[cfg(test)]
use model::Hop;

#[cfg(test)]
#[path = "path_tests.rs"]
mod tests;
