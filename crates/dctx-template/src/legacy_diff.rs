// SPDX-License-Identifier: Apache-2.0
//! Pure legacy diff envelopes over an injected tree and change report.
//! Implements D3-LEGACY-REMOVAL §5 D3-03, including the pinned closing-tag glue.
//! Depends on core diff/version types and the shared v1 component traversal.
//! Used by the facade; file ids follow the full tree even for unchanged files.

use dctx_core::datatypes::NodeType;
use dctx_core::diff::{ChangeType, DiffReport};
use dctx_core::version::Version;

use std::fmt::Write as _;
use crate::legacy_output::FileMode;
use crate::{MarkerNode, Template};

/// Renders the legacy diff signature, summary, updated tree and changed file blocks.
/// Summary entries retain caller order; blocks retain tree order and full-tree ids.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn render_legacy_diff(
    template: &Template,
    root: &MarkerNode,
    diff: &DiffReport,
    old_version: &Version,
    new_version: &Version,
    omit_svg_content: bool,
) -> String {
    let mut output = format!(
        "[DCTX_LLM_CONTEXT_DIFF_{new_version}]\nVersion Change: {old_version} -> {new_version}\n\n<CHANGES_SUMMARY>\n"
    );
    for entry in &diff.entries {
        let change = match entry.change_type {
            ChangeType::Added => "ADDED",
            ChangeType::Removed => "REMOVED",
            ChangeType::Modified => "MODIFIED",
        };
        let suffix = if entry.node_type == NodeType::Directory { "/" } else { "" };
        let _ = writeln!(output, "[{change}] {}{suffix}", entry.relative_path);
    }
    output.push_str("</CHANGES_SUMMARY>\n\n<UPDATED_DIRECTORY_TREE>\n");
    let changed = diff.entries.iter()
        .filter(|entry| entry.change_type != ChangeType::Removed)
        .map(|entry| entry.relative_path.as_str())
        .collect();
    let (tree, files) = template.marker_components(root, &FileMode::Diff { changed, omit_svg_content });
    output.push_str(tree.trim());
    output.push_str("</UPDATED_DIRECTORY_TREE>\n\n");
    output.push_str(&files);
    output
}
