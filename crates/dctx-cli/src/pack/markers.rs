// SPDX-License-Identifier: Apache-2.0
//! Source document and legacy marker tree construction for `dctx pack`.

use std::path::Path;

use super::base_name;

/// Source-bearing page. Scan caps in the facade bound which files are packed.
pub(super) fn source_document(
    path: &Path,
    stamps: dctx_template::TreeStamps,
) -> Result<dctx_template::LegacyOutput, String> {
    let mut template = stamps.template();
    template.insert_variable("project", &project_name(path));
    Ok(dctx_template::LegacyOutput::new(
        template,
        dctx_template::LegacyLayout::Snapshot,
    ))
}

pub(super) fn project_name(path: &Path) -> String {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    canonical
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty() && *name != "." && *name != "..")
        .unwrap_or("project")
        .to_string()
}

/// Project the scanned facts into pure renderer input, preserving presentation-only omit rules.
#[allow(
    clippy::must_use_candidate,
    reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
)]
pub fn legacy_markers(
    node: &dctx_core::datatypes::DirContextTreeNode,
    policy: &dctx_core::selection::SelectionPolicy,
) -> dctx_template::MarkerNode {
    let name = base_name(&node.relative_path);
    let is_dir = node.node_type == dctx_core::datatypes::NodeType::Directory;
    let omit_content = if node.is_binary_file || node.binary_file_type.is_some() {
        false
    } else {
        node.handling_decision == dctx_core::FileHandlingDecision::OmitContent
            || dctx_core::selection::rule::last_match(
                policy.omit_rules(),
                &node.relative_path,
                name,
                is_dir,
            )
    };
    dctx_template::MarkerNode {
        relative_path: node.relative_path.clone(),
        is_dir,
        is_ignored: node.is_ignored,
        omit_content,
        binary_label: if node.is_binary_file {
            Some(
                node.binary_file_type
                    .clone()
                    .unwrap_or_else(|| "Unknown".into()),
            )
        } else {
            node.binary_file_type
                .clone()
                .filter(|label| label == "SVG file")
        },
        content: if node.is_ignored || omit_content || node.is_binary_file {
            None
        } else {
            node.file_content.clone()
        },
        content_size: node.content_size,
        modified: node.last_modified_timestamp,
        children: node
            .children
            .iter()
            .map(|child| legacy_markers(child, policy))
            .collect(),
    }
}
