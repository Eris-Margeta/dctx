//! Pure legacy snapshot envelopes and explicitly supplied output settings.
//! Implements D3-LEGACY-REMOVAL §5 D3-03 and the pinned D3-COMPATIBILITY bytes.
//! Depends on v1 marker rendering and core versions; no configuration discovery.
//! Used by the facade and by the legacy envelope parity tests.

use std::collections::BTreeSet;

use dctx_core::datatypes::NodeType;
use dctx_core::diff::{ChangeType, DiffReport};
use dctx_core::version::Version;

use crate::error::TemplateError;
use crate::{MarkerNode, Template};

/// Legacy document layout selected by a pack caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyLayout {
    /// Version header, template tree and all permitted file blocks.
    Snapshot,
    /// Snapshot header and template tree, with no file blocks.
    TreeOnly,
    /// Changes and changed file blocks; a first run or major reset emits a snapshot.
    Diff,
}

/// Injected settings for the facade's optional legacy document output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyOutput {
    /// Parsed template, including any runtime variables such as `project`.
    pub template: Template,
    /// Requested document layout.
    pub layout: LegacyLayout,
    /// Omit classified SVG content using the legacy sentence when true.
    /// Pattern-omitted bytes remain omitted regardless of this setting.
    pub omit_svg_content: bool,
    /// Include ignored metadata in the tree without reading ignored content.
    pub debug_ignored: bool,
}

impl LegacyOutput {
    /// Explicit template and layout, omitting SVG content and hiding ignored entries.
    pub fn new(template: Template, layout: LegacyLayout) -> Self {
        Self { template, layout, omit_svg_content: true, debug_ignored: false }
    }
}

/// Renders a legacy snapshot, including its version signature and optional file blocks.
/// The template already contains runtime variables; all bytes and decisions are injected.
pub fn render_legacy_snapshot(
    template: &Template,
    root: &MarkerNode,
    version: &Version,
    tree_only: bool,
    omit_svg_content: bool,
) -> String {
    let mode = FileMode::Snapshot { tree_only, omit_svg_content };
    let (tree, files) = template.marker_components(root, &mode);
    let body = template.marker_layout(tree.trim(), files.trim());
    format!("[DCTX_LLM_CONTEXT_SNAPSHOT_{version}]\n{body}")
}

pub(crate) enum FileMode<'a> {
    Markers { tree_only: bool },
    Snapshot { tree_only: bool, omit_svg_content: bool },
    Diff { changed: BTreeSet<&'a str>, omit_svg_content: bool },
}

impl FileMode<'_> {
    pub(crate) fn includes(&self, node: &MarkerNode) -> bool {
        match self {
            Self::Markers { tree_only } | Self::Snapshot { tree_only, .. } => !tree_only,
            Self::Diff { changed, .. } => changed.contains(node.relative_path.as_str()),
        }
    }

    pub(crate) fn omission(&self, node: &MarkerNode) -> Option<String> {
        if let Some(label) = &node.binary_label {
            if label == "SVG file" {
                match self {
                    Self::Snapshot { omit_svg_content: true, .. } => {
                        return Some("[SVG file (omit_svg_content=true), content omitted]".to_string());
                    }
                    Self::Diff { omit_svg_content: true, .. } => {
                        return Some("[SVG file, content omitted (omit_svg_content=true)]".to_string());
                    }
                    Self::Snapshot { .. } | Self::Diff { .. } => {}
                    Self::Markers { .. } => return Some(format!("[Binary file: {label}, content omitted]")),
                }
            } else {
                return Some(format!("[Binary file: {label}, content omitted]"));
            }
        }
        node.omit_content.then(|| "[Content omitted by pattern rules]".to_string())
    }
}

impl Template {
    /// Renders a legacy snapshot when the page and its file-block expansion fit in `limit`.
    ///
    /// The expansion is measured first. A larger page returns [`TemplateError::BudgetTooSmall`]
    /// with the exact byte length and does not allocate that expansion.
    pub fn legacy_snapshot_within(
        &self,
        root: &MarkerNode,
        version: &Version,
        tree_only: bool,
        omit_svg_content: bool,
        limit: u64,
    ) -> Result<String, TemplateError> {
        let mode = FileMode::Snapshot { tree_only, omit_svg_content };
        if self.legacy_expansion_fits(root, &mode) {
            return fit_page(render_legacy_snapshot(self, root, version, tree_only, omit_svg_content), limit);
        }
        let (tree_raw, _tree_trimmed, files_raw, layout_body) = self.measure_legacy(root, &mode)?;
        let header = format!("[DCTX_LLM_CONTEXT_SNAPSHOT_{version}]\n");
        let len = add_len(u64::try_from(header.len()).unwrap_or(u64::MAX), layout_body)?;
        if len > limit {
            return Err(TemplateError::BudgetTooSmall { minimum_bytes: len });
        }
        let intermediate = tree_raw.max(files_raw);
        if intermediate > limit {
            return Err(TemplateError::BudgetTooSmall { minimum_bytes: intermediate });
        }
        fit_page(render_legacy_snapshot(self, root, version, tree_only, omit_svg_content), limit)
    }

    /// Renders a legacy diff when the page and its changed file blocks fit in `limit`.
    ///
    /// File blocks are measured first. A larger page returns [`TemplateError::BudgetTooSmall`]
    /// with the exact byte length and does not allocate that expansion.
    pub fn legacy_diff_within(
        &self,
        root: &MarkerNode,
        diff: &DiffReport,
        old_version: &Version,
        new_version: &Version,
        omit_svg_content: bool,
        limit: u64,
    ) -> Result<String, TemplateError> {
        let changed = diff
            .entries
            .iter()
            .filter(|entry| entry.change_type != ChangeType::Removed)
            .map(|entry| entry.relative_path.as_str())
            .collect();
        let mode = FileMode::Diff { changed, omit_svg_content };
        if self.legacy_expansion_fits(root, &mode) {
            return fit_page(crate::render_legacy_diff(self, root, diff, old_version, new_version, omit_svg_content), limit);
        }
        let (tree_raw, tree_trimmed, files_raw, _layout_body) = self.measure_legacy(root, &mode)?;
        let len = diff_document_len(diff, old_version, new_version, tree_trimmed, files_raw)?;
        if len > limit {
            return Err(TemplateError::BudgetTooSmall { minimum_bytes: len });
        }
        let intermediate = tree_raw.max(files_raw);
        if intermediate > limit {
            return Err(TemplateError::BudgetTooSmall { minimum_bytes: intermediate });
        }
        fit_page(crate::render_legacy_diff(self, root, diff, old_version, new_version, omit_svg_content), limit)
    }
}

fn fit_page(text: String, limit: u64) -> Result<String, TemplateError> {
    let len = u64::try_from(text.len()).unwrap_or(u64::MAX);
    if len > limit {
        Err(TemplateError::BudgetTooSmall { minimum_bytes: len })
    } else {
        Ok(text)
    }
}

fn add_len(total: u64, extra: u64) -> Result<u64, TemplateError> {
    total.checked_add(extra).ok_or(TemplateError::BudgetTooSmall { minimum_bytes: u64::MAX })
}

fn diff_document_len(
    diff: &DiffReport,
    old_version: &Version,
    new_version: &Version,
    tree_trimmed: u64,
    files_raw: u64,
) -> Result<u64, TemplateError> {
    let mut prefix = format!(
        "[DCTX_LLM_CONTEXT_DIFF_{new_version}]\nVersion Change: {old_version} -> {new_version}\n\n<CHANGES_SUMMARY>\n"
    );
    for entry in &diff.entries {
        let change = match entry.change_type {
            ChangeType::Added => "ADDED",
            ChangeType::Removed => "REMOVED",
            ChangeType::Modified => "MODIFIED",
        };
        let suffix = if entry.node_type == NodeType::Directory { "/" } else { "" };
        prefix.push_str(&format!("[{change}] {}{suffix}\n", entry.relative_path));
    }
    prefix.push_str("</CHANGES_SUMMARY>\n\n<UPDATED_DIRECTORY_TREE>\n");
    let glued = add_len(u64::try_from(prefix.len()).unwrap_or(u64::MAX), tree_trimmed)?;
    let tail = "</UPDATED_DIRECTORY_TREE>\n\n".len() as u64;
    add_len(add_len(glued, tail)?, files_raw)
}
