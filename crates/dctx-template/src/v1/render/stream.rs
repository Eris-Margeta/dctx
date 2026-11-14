// SPDX-License-Identifier: Apache-2.0
//! Writer-based legacy rendering. Component trimming uses two traversals and constant space.

use std::borrow::Cow;
use std::io::{self, Write};

use super::{MarkerNode, Template, data_parts, escape_path, replace_vars};
use crate::legacy_output::FileMode;
use dctx_core::version::Version;

/// Writes the exact legacy snapshot bytes without allocating the expanded document.
/// Memory beyond the input tree is bounded by template size, traversal depth and one decoded file.
/// Writes stop on the first sink error; the caller owns flushing and cancellation.
/// # Errors
/// Returns the first writer error, including a closed pipe or interrupted client.
pub fn write_legacy_snapshot(
    output: &mut dyn Write,
    template: &Template,
    root: &MarkerNode,
    version: &Version,
    tree_only: bool,
    omit_svg_content: bool,
) -> io::Result<()> {
    snapshot(
        output,
        template,
        root,
        version,
        tree_only,
        omit_svg_content,
        None,
    )
}

/// Writes a snapshot using externally held file bodies. The loader is called only for
/// included file blocks, at most one body is retained, and inserted values stay literal.
/// Returning `None` represents a withheld body; an empty body is `Some(Vec::new())`.
/// Bodies must remain byte-stable across the measurement and emission passes.
/// # Errors
/// Returns the first loader or writer error.
pub fn write_legacy_snapshot_with(
    output: &mut dyn Write,
    template: &Template,
    root: &MarkerNode,
    version: &Version,
    tree_only: bool,
    omit_svg_content: bool,
    load: &dyn Fn(&MarkerNode) -> io::Result<Option<Vec<u8>>>,
) -> io::Result<()> {
    snapshot(
        output,
        template,
        root,
        version,
        tree_only,
        omit_svg_content,
        Some(load),
    )
}

type ContentLoader<'a> = dyn Fn(&MarkerNode) -> io::Result<Option<Vec<u8>>> + 'a;

struct Partials<'a> {
    tree: String,
    files: String,
    size_attr: &'a str,
}

impl<'a> Partials<'a> {
    fn new(template: &'a Template) -> Self {
        Self {
            tree: replace_vars(&template.partial_tree_entry, &template.variables),
            files: replace_vars(&template.partial_file_block, &template.variables),
            size_attr: template
                .variables
                .get("attr_size")
                .map_or("SIZE", String::as_str),
        }
    }
}

fn snapshot(
    output: &mut dyn Write,
    template: &Template,
    root: &MarkerNode,
    version: &Version,
    tree_only: bool,
    omit_svg_content: bool,
    load: Option<&ContentLoader<'_>>,
) -> io::Result<()> {
    let partials = Partials::new(template);
    let mode = FileMode::Snapshot {
        tree_only,
        omit_svg_content,
    };
    writeln!(output, "[DCTX_LLM_CONTEXT_SNAPSHOT_{version}]")?;
    let layout = replace_vars(&template.layout, &template.variables);
    for part in data_parts(&layout) {
        match part {
            "{{{comp.tree}}}" => component(output, &partials, root, &mode, false, load)?,
            "{{{comp.files}}}" => component(output, &partials, root, &mode, true, load)?,
            "{{{project}}}" => output.write_all(
                template
                    .variables
                    .get("project")
                    .map_or(part, String::as_str)
                    .as_bytes(),
            )?,
            _ => output.write_all(part.as_bytes())?,
        }
    }
    Ok(())
}

/// Writes a legacy diff, preserving full-tree ids and the pinned closing-tag glue.
/// Memory beyond the input is bounded by changed-path references, depth, template and one decoded file.
/// # Errors
/// Returns the first writer error.
#[allow(
    clippy::too_many_arguments,
    reason = "Match the legacy diff API with one additional output writer."
)]
pub fn write_legacy_diff(
    output: &mut dyn Write,
    template: &Template,
    root: &MarkerNode,
    diff: &dctx_core::diff::DiffReport,
    old_version: &Version,
    new_version: &Version,
    omit_svg_content: bool,
) -> io::Result<()> {
    use dctx_core::datatypes::NodeType;
    use dctx_core::diff::ChangeType;
    write!(
        output,
        "[DCTX_LLM_CONTEXT_DIFF_{new_version}]\nVersion Change: {old_version} -> {new_version}\n\n<CHANGES_SUMMARY>\n"
    )?;
    for entry in &diff.entries {
        let change = match entry.change_type {
            ChangeType::Added => "ADDED",
            ChangeType::Removed => "REMOVED",
            ChangeType::Modified => "MODIFIED",
        };
        let suffix = if entry.node_type == NodeType::Directory {
            "/"
        } else {
            ""
        };
        writeln!(
            output,
            "[{change}] {}{suffix}",
            escape_path(&entry.relative_path)
        )?;
    }
    output.write_all(b"</CHANGES_SUMMARY>\n\n<UPDATED_DIRECTORY_TREE>\n")?;
    let changed = diff
        .entries
        .iter()
        .filter(|entry| entry.change_type != ChangeType::Removed)
        .map(|entry| entry.relative_path.as_str())
        .collect();
    let mode = FileMode::Diff {
        changed,
        omit_svg_content,
    };
    let partials = Partials::new(template);
    component(output, &partials, root, &mode, false, None)?;
    output.write_all(b"</UPDATED_DIRECTORY_TREE>\n\n")?;
    walk(output, &partials, root, &mode, true, &mut 1, 0, None)
}

// Measure only the non-whitespace range. This preserves str::trim even when whitespace
// spans many files, without retaining an arbitrarily long pending whitespace suffix.
#[derive(Default)]
struct TrimRange {
    total: usize,
    start: Option<usize>,
    end: usize,
}

impl Write for TrimRange {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let text = std::str::from_utf8(bytes).map_err(io::Error::other)?;
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            let leading = text.len() - text.trim_start().len();
            self.start.get_or_insert(self.total + leading);
            self.end = self.total + text.trim_end().len();
        }
        self.total = self.total.checked_add(bytes.len()).ok_or_else(|| {
            io::Error::other("render size overflow; reduce the template expansion or file bodies")
        })?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct Window<'a> {
    output: &'a mut dyn Write,
    position: usize,
    start: usize,
    end: usize,
}

impl Write for Window<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let start = self.start.saturating_sub(self.position).min(bytes.len());
        let end = self.end.saturating_sub(self.position).min(bytes.len());
        if start < end {
            // Bound each transport write, including a repeated multi-megabyte file field.
            for chunk in bytes[start..end].chunks(64 * 1024) {
                self.output.write_all(chunk)?;
            }
        }
        self.position += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.output.flush()
    }
}

fn component(
    output: &mut dyn Write,
    template: &Partials<'_>,
    root: &MarkerNode,
    mode: &FileMode<'_>,
    files: bool,
    load: Option<&ContentLoader<'_>>,
) -> io::Result<()> {
    let mut range = TrimRange::default();
    walk(&mut range, template, root, mode, files, &mut 1, 0, load)?;
    let mut window = Window {
        output,
        position: 0,
        start: range.start.unwrap_or(0),
        end: range.end,
    };
    walk(&mut window, template, root, mode, files, &mut 1, 0, load)
}

fn fields(output: &mut dyn Write, partial: &str, fields: &[(&str, &str)]) -> io::Result<()> {
    for part in data_parts(partial) {
        let value = fields
            .iter()
            .find(|(key, _)| *key == part)
            .map_or(part, |(_, value)| *value);
        write_text(output, value)?;
    }
    Ok(())
}

fn write_text(output: &mut dyn Write, mut text: &str) -> io::Result<()> {
    while text.len() > 64 * 1024 {
        let mut end = 64 * 1024;
        // Measurement writers consume UTF-8 spans, so keep scalar boundaries intact.
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        output.write_all(&text.as_bytes()[..end])?;
        text = &text[end..];
    }
    output.write_all(text.as_bytes())
}

#[allow(
    clippy::too_many_arguments,
    reason = "Traversal state is explicit and bounded by the scan depth cap."
)]
#[allow(
    clippy::naive_bytecount,
    reason = "Preserve the legacy newline count convention."
)]
fn walk(
    output: &mut dyn Write,
    template: &Partials<'_>,
    node: &MarkerNode,
    mode: &FileMode<'_>,
    files: bool,
    counter: &mut usize,
    depth: usize,
    load: Option<&ContentLoader<'_>>,
) -> io::Result<()> {
    let id = if depth == 0 && node.is_dir {
        "ROOT".to_string()
    } else {
        let id = format!("{}{:03}", if node.is_dir { 'D' } else { 'F' }, counter);
        *counter += 1;
        id
    };
    if !files {
        for _ in 0..depth.min(4096) {
            output.write_all(b"  ")?;
        }
        let path = escape_path(&node.relative_path);
        let path = if node.is_ignored {
            Cow::Owned(format!("{path} [IGNORED]"))
        } else {
            path
        };
        let size = if node.is_dir {
            String::new()
        } else {
            format!(", {}:{}", template.size_attr, node.content_size)
        };
        fields(
            output,
            &template.tree,
            &[
                (
                    "{{{node.type_char}}}",
                    if node.is_dir { "[D]" } else { "[F]" },
                ),
                ("{{{node.path}}}", &path),
                ("{{{node.id}}}", &id),
                ("{{{node.mod}}}", &node.modified.to_string()),
                ("{{{node.size_part}}}", &size),
            ],
        )?;
        output.write_all(b"\n")?;
    } else if !node.is_dir && !node.is_ignored && mode.includes(node) {
        let omission = mode.omission(node);
        let loaded = if let Some(load) = load.filter(|_| omission.is_none()) {
            load(node)?
        } else {
            None
        };
        let body = if load.is_some() {
            loaded.as_deref()
        } else {
            node.content.as_deref()
        };
        let (lines, content) = if let Some(message) = omission {
            (0, Cow::Owned(message))
        } else if let Some(bytes) = body {
            (
                bytes.iter().filter(|byte| **byte == b'\n').count() + 1,
                String::from_utf8_lossy(bytes),
            )
        } else {
            (0, Cow::Borrowed(""))
        };
        fields(
            output,
            &template.files,
            &[
                ("{{{file.id}}}", &id),
                ("{{{file.path}}}", &escape_path(&node.relative_path)),
                ("{{{file.size}}}", &node.content_size.to_string()),
                ("{{{file.lines}}}", &lines.to_string()),
                ("{{{file.content}}}", &content),
            ],
        )?;
        output.write_all(b"\n")?;
    }
    for child in &node.children {
        walk(
            output,
            template,
            child,
            mode,
            files,
            counter,
            depth + 1,
            load,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_body_writes_are_bounded_even_for_large_utf8_content() {
        use dctx_core::datatypes::NodeType;
        use dctx_core::diff::{ChangeType, DiffEntry, DiffReport};
        struct Ceiling(usize);
        impl Write for Ceiling {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                assert!(bytes.len() <= 64 * 1024);
                self.0 += bytes.len();
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let template =
            Template::parse("[PARTIAL:FILE_BLOCK]\n{{{file.content}}}\n[LAYOUT]\n{{{comp.files}}}");
        let root = MarkerNode::file("a", "€".repeat(1024 * 1024).as_bytes(), 0);
        let mut diff = DiffReport::default();
        diff.entries.push(DiffEntry {
            change_type: ChangeType::Added,
            node_type: NodeType::File,
            relative_path: "a".to_string(),
        });
        let mut output = Ceiling(0);
        write_legacy_diff(
            &mut output,
            &template,
            &root,
            &diff,
            &Version::new_v1(),
            &Version::new_v1(),
            false,
        )
        .unwrap();
        assert!(output.0 > 3 * 1024 * 1024);
    }

    #[test]
    fn external_bodies_match_capture_without_retaining_body_bytes_in_the_tree() {
        let template = Template::parse(
            "[PARTIAL:TREE_ENTRY]\n{{{node.path}}}\n[PARTIAL:FILE_BLOCK]\n{{{file.id}}} {{{file.lines}}} {{{file.content}}}{{{file.content}}}\n[LAYOUT]\n{{{comp.files}}}|{{{comp.tree}}}|{{{comp.files}}}",
        );
        let mut root = MarkerNode::dir("", 0);
        root.children = vec![
            MarkerNode::file("text", b" \xff\n", 0),
            MarkerNode::file("empty", b"", 0),
            MarkerNode::file("omitted", b"secret", 0),
        ];
        root.children[2].omit_content = true;
        let expected =
            crate::render_legacy_snapshot(&template, &root, &Version::new_v1(), false, false);
        let bodies: std::collections::HashMap<_, _> = root
            .children
            .iter_mut()
            .map(|node| (node.relative_path.clone(), node.content.take()))
            .collect();
        let mut output = Vec::new();
        write_legacy_snapshot_with(
            &mut output,
            &template,
            &root,
            &Version::new_v1(),
            false,
            false,
            &|node| {
                assert_ne!(node.relative_path, "omitted");
                Ok(bodies[&node.relative_path].clone())
            },
        )
        .unwrap();
        assert_eq!(output, expected.as_bytes());
        assert!(root.children.iter().all(|node| node.content.is_none()));
    }

    #[test]
    fn snapshot_matches_capture_with_repeated_fields_unicode_trimming_and_invalid_utf8() {
        let mut root = MarkerNode::dir("", 0);
        root.children
            .push(MarkerNode::file("{{{file.content}}}.txt", b" \xff\n\t", 1));
        root.children
            .push(MarkerNode::file("blank", "\u{2003}  ".as_bytes(), 2));
        root.children.push(MarkerNode::file("empty", b"", 3));
        root.children.push(MarkerNode::dir("ignored", 4));
        root.children[3].is_ignored = true;
        let version = Version::new_v1();
        let templates = [
            crate::TreeStamps::Off.template(),
            crate::TreeStamps::On.template(),
            Template::parse(
                "[PARTIAL:TREE_ENTRY]\n {{{node.path}}} \n[PARTIAL:FILE_BLOCK]\n{{{file.content}}}{{{file.content}}}\n[LAYOUT]\n{{{comp.files}}}|{{{comp.tree}}}|{{{comp.files}}}|{{{project}}}",
            ),
        ];
        for mut template in templates {
            template.insert_variable("project", "{{{comp.files}}}");
            for tree_only in [false, true] {
                let expected =
                    crate::render_legacy_snapshot(&template, &root, &version, tree_only, true);
                let mut output = Vec::new();
                write_legacy_snapshot(&mut output, &template, &root, &version, tree_only, true)
                    .unwrap();
                assert_eq!(output, expected.as_bytes());
            }
        }
    }

    #[test]
    fn expanded_content_is_written_in_bounded_chunks_and_stops_on_failure() {
        struct Probe {
            calls: usize,
        }
        impl Write for Probe {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                assert!(bytes.len() <= 64 * 1024);
                self.calls += 1;
                if self.calls == 8 {
                    return Err(io::ErrorKind::BrokenPipe.into());
                }
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let template =
            Template::parse("[PARTIAL:FILE_BLOCK]\n{{{file.content}}}\n[LAYOUT]\n{{{comp.files}}}");
        let root = MarkerNode::file("a", &vec![b'x'; 4 * 1024 * 1024], 0);
        let mut probe = Probe { calls: 0 };
        let error = write_legacy_snapshot(
            &mut probe,
            &template,
            &root,
            &Version::new_v1(),
            false,
            false,
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
        assert_eq!(probe.calls, 8);
    }
}
