// SPDX-License-Identifier: Apache-2.0
//! Marker rendering for v1 partials and layouts.
//! Implements the dctx-core formatter's tree, file and layout substitution, without IO.
//! Depends on [`super::parse`]. Used by parity tests and by the bounded plan renderer.
//! Ignored, omitted and excluded nodes never copy file bytes into the answer.

mod doc;
mod measure;

use super::parse::{Template, replace_vars};
use crate::legacy_output::FileMode;

const RENDER_CEILING: u64 = 16 * 1024 * 1024;

/// Splits template structure into literal spans and complete data placeholders.
/// Inserted values never pass through this iterator.
fn data_parts(mut text: &str) -> impl Iterator<Item = &str> {
    std::iter::from_fn(move || {
        if text.is_empty() {
            return None;
        }
        let end = match text.find("{{{") {
            Some(0) => text.find("}}}").map_or(text.len(), |end| end + 3),
            Some(start) => start,
            None => text.len(),
        };
        let (part, rest) = text.split_at(end);
        text = rest;
        Some(part)
    })
}

fn insert_data(text: &str, fields: &[(&str, &str)]) -> String {
    let mut output = String::new();
    for part in data_parts(text) {
        let value = fields.iter().find(|(field, _)| *field == part);
        output.push_str(value.map_or(part, |(_, value)| *value));
    }
    output
}

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
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
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
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
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
#[allow(
    clippy::must_use_candidate,
    reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
)]
pub fn render_markers(template: &Template, root: &MarkerNode, tree_only: bool) -> String {
    let (tree, files) = template.marker_components(root, &FileMode::Markers { tree_only });
    template.marker_layout(tree.trim(), files.trim())
}

impl Template {
    pub(crate) fn marker_components(
        &self,
        root: &MarkerNode,
        mode: &FileMode<'_>,
    ) -> (String, String) {
        let mut tree = String::new();
        let mut files = String::new();
        let mut counter = 1usize;
        render_node(root, self, &mut tree, &mut files, &mut counter, 0, mode);
        (tree, files)
    }

    pub(crate) fn marker_layout(&self, tree: &str, files: &str) -> String {
        let layout = replace_vars(&self.layout, &self.variables);
        let project = self
            .variables
            .get("project")
            .map_or("{{{project}}}", String::as_str);
        insert_data(
            &layout,
            &[
                ("{{{comp.tree}}}", tree),
                ("{{{comp.files}}}", files),
                ("{{{project}}}", project),
            ],
        )
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
    let size_attr = template
        .variables
        .get("attr_size")
        .map_or("SIZE", String::as_str);
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

    let partial = replace_vars(&template.partial_tree_entry, &template.variables);
    let entry = format!(
        "{indent}{}",
        insert_data(
            &partial,
            &[
                ("{{{node.type_char}}}", type_char),
                ("{{{node.path}}}", &display_path),
                ("{{{node.id}}}", &current_id),
                ("{{{node.mod}}}", &node.modified.to_string()),
                ("{{{node.size_part}}}", &size_part),
            ]
        )
    );
    tree_buf.push_str(&entry);
    tree_buf.push('\n');

    if !node.is_dir && !node.is_ignored && mode.includes(node) {
        let (line_count, content) = file_body(node, mode);
        let partial = replace_vars(&template.partial_file_block, &template.variables);
        let block = insert_data(
            &partial,
            &[
                ("{{{file.id}}}", &current_id),
                ("{{{file.path}}}", &node.relative_path),
                ("{{{file.size}}}", &node.content_size.to_string()),
                ("{{{file.lines}}}", &line_count.to_string()),
                ("{{{file.content}}}", &content),
            ],
        );
        files_buf.push_str(&block);
        files_buf.push('\n');
    }

    for child in &node.children {
        render_node(
            child,
            template,
            tree_buf,
            files_buf,
            counter,
            depth + 1,
            mode,
        );
    }
}

#[allow(
    clippy::naive_bytecount,
    reason = "Count newline bytes with standard iterators to preserve the renderer line-count convention without adding a byte-count dependency."
)]
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

#[cfg(test)]
mod measure_tests {
    use super::*;
    use crate::Template;
    use crate::legacy_output::FileMode;
    use dctx_core::version::Version;

    fn repeated(times: usize) -> Template {
        let partial = "{{{file.content}}}".repeat(times);
        let mut raw = String::from("[PARTIAL:FILE_BLOCK]\n");
        raw.push_str(&partial);
        raw.push_str("\n[LAYOUT]\n{{{comp.files}}}");
        Template::parse(&raw)
    }

    #[test]
    fn repeated_content_length_matches_the_renderer() {
        let template = repeated(5);
        let mut root = MarkerNode::dir("", 0);
        root.children.push(MarkerNode::file("a.txt", b"ABC", 1));
        let version = Version::new_v1();
        let real = crate::render_legacy_snapshot(&template, &root, &version, false, true);
        let mode = FileMode::Snapshot {
            tree_only: false,
            omit_svg_content: true,
        };
        let (_tree_raw, _tree_trimmed, _files_raw, body) =
            template.measure_legacy(&root, &mode).expect("measure");
        let header = format!("[DCTX_LLM_CONTEXT_SNAPSHOT_{version}]\n");
        assert_eq!(header.len() as u64 + body, real.len() as u64);
        assert_eq!(real, format!("{header}{}", "ABC".repeat(5)));
    }

    #[test]
    fn repeated_content_over_the_ceiling_keeps_one_segment() {
        let times = 4096u64;
        let unit = 8192u64;
        let template = repeated(usize::try_from(times).unwrap());
        let mut root = MarkerNode::dir("", 0);
        root.children.push(MarkerNode::file(
            "a.txt",
            &vec![b'Q'; usize::try_from(unit).unwrap()],
            1,
        ));
        let mode = FileMode::Snapshot {
            tree_only: false,
            omit_svg_content: true,
        };
        assert!(!template.legacy_expansion_fits(&root, &mode));
        let (_tree_raw, _tree_trimmed, files_raw, body) =
            template.measure_legacy(&root, &mode).expect("measure");
        assert_eq!(body, times * unit);
        assert_eq!(files_raw, times * unit + 1);
    }

    #[test]
    fn measured_literals_match_rendering_even_above_the_ceiling() {
        let body = "{{name}} {{start_marker_open}} {{ unknown }} {{{triple}}} \\{{escaped}} {{{project}}} {{{comp.files}}}";
        let mut template = repeated(5);
        template.insert_variable("name", "expanded");
        template.insert_variable("start_marker_open", "<");
        template.insert_variable("project", "{{{comp.files}}}");
        let mut root = MarkerNode::dir("", 0);
        root.children.push(MarkerNode::file(
            "{{name}}-{{{file.content}}}",
            body.as_bytes(),
            1,
        ));
        let mode = FileMode::Snapshot {
            tree_only: false,
            omit_svg_content: true,
        };
        let (_, _, files, measured) = template.measure_legacy(&root, &mode).expect("measure");
        assert_eq!(measured, (body.len() * 5) as u64);
        assert_eq!(files, measured + 1);
        assert_eq!(template.marker_layout("", &body.repeat(5)), body.repeat(5));

        template.partial_file_block = "{{{file.content}}}".repeat(4096);
        root.children[0] = MarkerNode::file("{{name}}", &body.repeat(128).into_bytes(), 1);
        assert!(!template.legacy_expansion_fits(&root, &mode));
        let (_, _, files, measured) = template
            .measure_legacy(&root, &mode)
            .expect("large measure");
        assert_eq!(measured, (body.len() * 128 * 4096) as u64);
        assert_eq!(files, measured + 1);
    }

    #[test]
    fn measured_default_template_matches_the_renderer() {
        let mut template = Template::parse(include_str!("../../templates/default.dctx_template"));
        template.insert_variable("project", "Corpus");
        let mut root = MarkerNode::dir("", 0);
        root.children
            .push(MarkerNode::file("src/main.rs", b"fn main() {}\n", 1));
        let version = Version::new_v1();
        for tree_only in [false, true] {
            let real = crate::render_legacy_snapshot(&template, &root, &version, tree_only, true);
            let mode = FileMode::Snapshot {
                tree_only,
                omit_svg_content: true,
            };
            let (_tree_raw, _tree_trimmed, _files_raw, body) =
                template.measure_legacy(&root, &mode).expect("measure");
            let header = format!("[DCTX_LLM_CONTEXT_SNAPSHOT_{version}]\n");
            assert_eq!(
                header.len() as u64 + body,
                real.len() as u64,
                "tree_only={tree_only}"
            );
        }
    }
}
