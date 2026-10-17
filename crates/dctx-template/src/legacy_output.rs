//! Pure legacy snapshot envelopes and explicitly supplied output settings.
//! Implements D3-LEGACY-REMOVAL §5 D3-03 and the pinned D3-COMPATIBILITY bytes.
//! Depends on v1 marker rendering and core versions; no configuration discovery.
//! Used by the facade and by the legacy envelope parity tests.

use std::collections::BTreeSet;

use dctx_core::version::Version;

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
