// SPDX-License-Identifier: Apache-2.0
//! Narrowing of the scanned tree to the selected filters.

use std::path::Path;

use dctx_core::datatypes::{DirContextTreeNode, IgnoreRule, NodeType};
use dctx_core::file_classifier::{classify_file, FileContentHandling};
use dctx_core::selection::rule_matches;

use super::base_name;
use super::filter::Filters;

/// Keeps the root. Drops files the ADR filters reject and prunes empty directories.
pub(super) fn narrow(mut node: DirContextTreeNode, filters: &Filters) -> DirContextTreeNode {
    node.children = node
        .children
        .into_iter()
        .filter_map(|child| narrow_child(child, filters))
        .collect();
    node
}

fn narrow_child(mut node: DirContextTreeNode, filters: &Filters) -> Option<DirContextTreeNode> {
    let name = base_name(&node.relative_path);
    if node.node_type == NodeType::Directory {
        if !filters.config.should_include_directory(name)
            || pattern_hit(&filters.exclude, &node.relative_path, name, true)
        {
            return None;
        }
        node.children = node
            .children
            .into_iter()
            .filter_map(|child| narrow_child(child, filters))
            .collect();
        return (!node.children.is_empty()).then_some(node);
    }
    let binary = treated_as_binary(&node);
    if !filters.config.should_include_file(
        name,
        node.content_size,
        binary,
        Path::new(&node.relative_path),
    ) {
        return None;
    }
    if !filters.include.is_empty()
        && !pattern_hit(&filters.include, &node.relative_path, name, false)
    {
        return None;
    }
    if pattern_hit(&filters.exclude, &node.relative_path, name, false) {
        return None;
    }
    Some(node)
}

/// Extension binaries stay omitted when the scan withheld their bytes.
fn treated_as_binary(node: &DirContextTreeNode) -> bool {
    node.is_binary_file
        || matches!(
            classify_file(Path::new(&node.relative_path), b""),
            FileContentHandling::OmitBinary { .. }
        )
}

fn pattern_hit(rules: &[IgnoreRule], path: &str, name: &str, is_dir: bool) -> bool {
    rules
        .iter()
        .any(|rule| rule_matches(rule, path, name, is_dir))
}

pub(super) fn file_count(node: &DirContextTreeNode) -> u64 {
    let own = u64::from(node.node_type == NodeType::File);
    node.children
        .iter()
        .fold(own, |sum, child| sum.saturating_add(file_count(child)))
}
