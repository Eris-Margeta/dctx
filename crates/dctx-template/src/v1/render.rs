//! Marker rendering for v1 partials and layouts.
//! Implements the dctx-core formatter's tree, file and layout substitution, without IO.
//! Depends on [`super::parse`]. Used by parity tests and by the bounded plan renderer.
//! Ignored, omitted and excluded nodes never copy file bytes into the answer.

use std::collections::HashMap;

use super::parse::{replace_vars, Template};
use crate::legacy_output::FileMode;

/// One tree node. Content is ignored unless the node is an included file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkerNode {
    /// Root-relative path. Empty for the root directory.
    pub relative_path: String,
    /// Directory when true.
    pub is_dir: bool,
    /// Debug-visible: the path is marked and the content is not rendered.
    pub is_ignored: bool,
    /// Metadata stays; content is replaced with an omission marker.
    pub omit_content: bool,
    /// When set, content is replaced with a binary omission marker.
    pub binary_label: Option<String>,
    /// File bytes. Not read by this crate; the caller already holds them.
    pub content: Option<Vec<u8>>,
    /// Size written into the tree entry.
    pub content_size: u64,
    /// Modification counter written into the tree entry.
    pub modified: u64,
    /// Children in caller order.
    pub children: Vec<MarkerNode>,
}

impl MarkerNode {
    /// A directory node with no children.
    pub fn dir(path: &str, modified: u64) -> Self {
        Self {
            relative_path: path.to_string(),
            is_dir: true,
            is_ignored: false,
            omit_content: false,
            binary_label: None,
            content: None,
            content_size: 0,
            modified,
            children: Vec::new(),
        }
    }

    /// A file node. `content` is rendered only when the node is included.
    pub fn file(path: &str, content: &[u8], modified: u64) -> Self {
        Self {
            relative_path: path.to_string(),
            is_dir: false,
            is_ignored: false,
            omit_content: false,
            binary_label: None,
            content: Some(content.to_vec()),
            content_size: u64::try_from(content.len()).unwrap_or(u64::MAX),
            modified,
            children: Vec::new(),
        }
    }
}

/// Renders `root` with the v1 marker rules. `template.variables` must already hold `project` when the layout uses it.
pub fn render_markers(template: &Template, root: &MarkerNode, tree_only: bool) -> String {
    let (tree, files) = template.marker_components(root, &FileMode::Markers { tree_only });
    template.marker_layout(tree.trim(), files.trim())
}

impl Template {
    pub(crate) fn marker_components(&self, root: &MarkerNode, mode: &FileMode<'_>) -> (String, String) {
        let mut tree = String::new();
        let mut files = String::new();
        let mut counter = 1usize;
        render_node(root, self, &mut tree, &mut files, &mut counter, 0, mode);
        (tree, files)
    }

    pub(crate) fn marker_layout(&self, tree: &str, files: &str) -> String {
        let mut output = self.layout.clone();
        output = output.replace("{{{comp.tree}}}", tree);
        output = output.replace("{{{comp.files}}}", files);
        if let Some(project) = self.variables.get("project") {
            output = output.replace("{{{project}}}", project);
        }
        replace_vars(&output, &self.variables)
    }
}

fn render_node(
    node: &MarkerNode,
    template: &Template,
    tree_buf: &mut String,
    files_buf: &mut String,
    counter: &mut usize,
    depth: usize,
    mode: &FileMode<'_>,
) {
    let current_id = if depth == 0 && node.is_dir {
        "ROOT".to_string()
    } else if node.is_dir {
        let id = format!("D{counter:03}");
        *counter += 1;
        id
    } else {
        let id = format!("F{counter:03}");
        *counter += 1;
        id
    };

    let type_char = if node.is_dir { "[D]" } else { "[F]" };
    let size_attr = template.variables.get("attr_size").map(String::as_str).unwrap_or("SIZE");
    let size_part = if node.is_dir {
        String::new()
    } else {
        format!(", {size_attr}:{}", node.content_size)
    };
    let indent = "  ".repeat(depth.min(4096));
    let display_path = if node.is_ignored {
        format!("{} [IGNORED]", node.relative_path)
    } else {
        node.relative_path.clone()
    };

    let mut entry = format!("{indent}{}", template.partial_tree_entry);
    entry = entry.replace("{{{node.type_char}}}", type_char);
    entry = entry.replace("{{{node.path}}}", &display_path);
    entry = entry.replace("{{{node.id}}}", &current_id);
    entry = entry.replace("{{{node.mod}}}", &node.modified.to_string());
    entry = entry.replace("{{{node.size_part}}}", &size_part);
    entry = replace_vars(&entry, &template.variables);
    tree_buf.push_str(&entry);
    tree_buf.push('\n');

    if !node.is_dir && !node.is_ignored && mode.includes(node) {
        let (line_count, content) = file_body(node, mode);
        let mut block = template.partial_file_block.clone();
        block = block.replace("{{{file.id}}}", &current_id);
        block = block.replace("{{{file.path}}}", &node.relative_path);
        block = block.replace("{{{file.size}}}", &node.content_size.to_string());
        block = block.replace("{{{file.lines}}}", &line_count.to_string());
        block = block.replace("{{{file.content}}}", &content);
        block = replace_vars(&block, &template.variables);
        files_buf.push_str(&block);
        files_buf.push('\n');
    }

    for child in &node.children {
        render_node(child, template, tree_buf, files_buf, counter, depth + 1, mode);
    }
}

fn file_body(node: &MarkerNode, mode: &FileMode<'_>) -> (usize, String) {
    if let Some(message) = mode.omission(node) {
        return (0, message);
    }
    match &node.content {
        Some(bytes) => {
            let lines = bytes.iter().filter(|byte| **byte == b'\n').count() + 1;
            let text = String::from_utf8_lossy(bytes).into_owned();
            (lines, text)
        }
        None => (0, String::new()),
    }
}

/// End-marker prefix used when a plan render escapes file content.
pub fn end_marker_of(variables: &HashMap<String, String>) -> String {
    let open = variables.get("end_marker_open").map(String::as_str).unwrap_or("</");
    let name = variables.get("file_end_marker").map(String::as_str).unwrap_or("FILE");
    let close = variables.get("end_marker_close").map(String::as_str).unwrap_or(">");
    format!("{open}{name}{close}")
}
