//! Narrowing of the scanned tree to the selected filters.

use std::path::Path;

use dctx_core::datatypes::{DirContextTreeNode, IgnoreRule, NodeType};
use dctx_core::file_classifier::{FileContentHandling, classify_file};
use dctx_core::selection::rule_matches;

use super::base_name;
use super::filter::Filters;

/// Keeps the root. Drops files the ADR filters reject and prunes empty directories.
pub(super) fn narrow(node: &DirContextTreeNode, filters: &Filters) -> DirContextTreeNode {
    let mut root = shell(node);
    for child in &node.children {
        if let Some(kept) = narrow_child(child, filters) {
            root.children.push(kept);
        }
    }
    root
}

fn narrow_child(node: &DirContextTreeNode, filters: &Filters) -> Option<DirContextTreeNode> {
    let name = base_name(&node.relative_path);
    if node.node_type == NodeType::Directory {
        if !filters.config.should_include_directory(name)
            || pattern_hit(&filters.exclude, &node.relative_path, name, true)
        {
            return None;
        }
        let mut kept = shell(node);
        for child in &node.children {
            if let Some(next) = narrow_child(child, filters) {
                kept.children.push(next);
            }
        }
        return (!kept.children.is_empty()).then_some(kept);
    }
    let binary = treated_as_binary(node);
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
    Some(shell(node))
}

fn shell(node: &DirContextTreeNode) -> DirContextTreeNode {
    DirContextTreeNode {
        node_type: node.node_type,
        relative_path: node.relative_path.clone(),
        last_modified_timestamp: node.last_modified_timestamp,
        content_offset_in_data_section: node.content_offset_in_data_section,
        content_size: node.content_size,
        disk_path: node.disk_path.clone(),
        children: Vec::new(),
        generated_id_for_llm: node.generated_id_for_llm.clone(),
        file_content: node.file_content.clone(),
        is_binary_file: node.is_binary_file,
        binary_file_type: node.binary_file_type.clone(),
        is_ignored: node.is_ignored,
        handling_decision: node.handling_decision,
    }
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
