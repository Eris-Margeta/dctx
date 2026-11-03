// SPDX-License-Identifier: Apache-2.0
//! Bounded measurement of the legacy marker expansion.
//! Tree and file bodies are measured as run-length documents, so an oversized answer is refused without allocation.
//! Depends on [`super::doc`]. Used by [`crate::legacy_output`] and the parity tests.

use super::doc::{too_big, with_vars, Doc};
use super::*;
use crate::TemplateError;

struct Built {
    tree: Doc,
    files: Doc,
    counter: usize,
}

impl Template {
    pub(crate) fn legacy_expansion_fits(&self, root: &MarkerNode, mode: &FileMode<'_>) -> bool {
        let mut files = 0u64;
        let mut tree = 0u64;
        if !self.fit_node(root, mode, 0, &mut files, &mut tree) {
            return false;
        }
        self.layout_upper(tree, files) <= RENDER_CEILING && files <= RENDER_CEILING && tree <= RENDER_CEILING
    }

    /// `(tree_raw, tree_trimmed, files_raw, layout_body)` byte lengths.
    ///
    /// A repeated file body stays one segment. The product is not allocated.
    pub(crate) fn measure_legacy(
        &self,
        root: &MarkerNode,
        mode: &FileMode<'_>,
    ) -> Result<(u64, u64, u64, u64), TemplateError> {
        let mut built = Built { tree: Doc::default(), files: Doc::default(), counter: 1 };
        self.measure_node(root, &mut built, 0, mode)?;
        let tree_raw = built.tree.len()?;
        let files_raw = built.files.len()?;
        let tree_trimmed = built.tree.trimmed()?;
        let files_trimmed = built.files.trimmed()?;
        let body = self.layout_doc(&tree_trimmed, &files_trimmed)?;
        Ok((tree_raw, tree_trimmed.len()?, files_raw, body.len()?))
    }

    fn fit_node(
        &self,
        node: &MarkerNode,
        mode: &FileMode<'_>,
        depth: usize,
        files: &mut u64,
        tree: &mut u64,
    ) -> bool {
        let entry = self.partial_tree_entry.len() as u64 + (depth.min(4096) as u64) * 2 + 64;
        *tree = tree.saturating_add(entry);
        if *tree > RENDER_CEILING {
            return false;
        }
        if !node.is_dir && !node.is_ignored && mode.includes(node) {
            let content = match mode.omission(node) {
                Some(message) => message.len() as u64,
                None => node.content.as_ref().map_or(0, |bytes| bytes.len() as u64).saturating_mul(3),
            };
            let block = block_upper(
                &self.partial_file_block,
                &[
                    ("{{{file.id}}}", 16),
                    ("{{{file.path}}}", node.relative_path.len() as u64 + 16),
                    ("{{{file.size}}}", 20),
                    ("{{{file.lines}}}", 20),
                    ("{{{file.content}}}", content),
                ],
            );
            *files = files.saturating_add(block);
            if block > RENDER_CEILING || *files > RENDER_CEILING {
                return false;
            }
        }
        node.children.iter().all(|child| self.fit_node(child, mode, depth + 1, files, tree))
    }

    fn layout_upper(&self, tree: u64, files: u64) -> u64 {
        let layout = &self.layout;
        let mut upper = layout.len() as u64;
        upper = upper.saturating_add((layout.matches("{{{comp.tree}}}").count() as u64).saturating_mul(tree));
        upper = upper.saturating_add((layout.matches("{{{comp.files}}}").count() as u64).saturating_mul(files));
        if let Some(project) = self.variables.get("project") {
            upper = upper.saturating_add((layout.matches("{{{project}}}").count() as u64).saturating_mul(project.len() as u64));
        }
        for (key, value) in &self.variables {
            let pattern = format!("{{{{{key}}}}}");
            upper = upper.saturating_add((layout.matches(&pattern).count() as u64).saturating_mul(value.len() as u64));
        }
        upper
    }

    fn measure_node(&self, node: &MarkerNode, built: &mut Built, depth: usize, mode: &FileMode<'_>) -> Result<(), TemplateError> {
        let current_id = if depth == 0 && node.is_dir {
            "ROOT".to_string()
        } else if node.is_dir {
            let id = format!("D{:03}", built.counter);
            built.counter += 1;
            id
        } else {
            let id = format!("F{:03}", built.counter);
            built.counter += 1;
            id
        };
        let entry = self.tree_entry(node, &current_id, depth)?;
        built.tree.push_str(&entry)?;
        built.tree.push_str("\n")?;
        if !node.is_dir && !node.is_ignored && mode.includes(node) {
            let block = self.file_block(node, &current_id, mode)?;
            built.files.push_repeated(&block, 1)?;
            built.files.push_str("\n")?;
        }
        for child in &node.children {
            self.measure_node(child, built, depth + 1, mode)?;
        }
        Ok(())
    }

    fn tree_entry(&self, node: &MarkerNode, current_id: &str, depth: usize) -> Result<String, TemplateError> {
        let type_char = if node.is_dir { "[D]" } else { "[F]" };
        let size_attr = self.variables.get("attr_size").map_or("SIZE", String::as_str);
        let size_part = if node.is_dir {
            String::new()
        } else {
            let mut part = String::from(", ");
            part.push_str(size_attr);
            part.push(':');
            part.push_str(&node.content_size.to_string());
            part
        };
        let display_path = if node.is_ignored {
            format!("{} [IGNORED]", node.relative_path)
        } else {
            node.relative_path.clone()
        };
        let mut entry = String::new();
        for _ in 0..depth.min(4096) {
            entry.push_str("  ");
        }
        entry.push_str(&self.partial_tree_entry);
        let mut doc = Doc::from_string(entry);
        doc = doc.replaced("{{{node.type_char}}}", &Doc::from_str(type_char))?;
        doc = doc.replaced("{{{node.path}}}", &Doc::from_str(&display_path))?;
        doc = doc.replaced("{{{node.id}}}", &Doc::from_str(current_id))?;
        doc = doc.replaced("{{{node.mod}}}", &Doc::from_str(&node.modified.to_string()))?;
        doc = doc.replaced("{{{node.size_part}}}", &Doc::from_str(&size_part))?;
        doc = with_vars(doc, &self.variables)?;
        if doc.len()? > RENDER_CEILING {
            return Err(too_big());
        }
        doc.materialize()
    }

    fn file_block(&self, node: &MarkerNode, current_id: &str, mode: &FileMode<'_>) -> Result<Doc, TemplateError> {
        let (line_count, content) = measured_body(node, mode);
        let mut block = Doc::from_str(&self.partial_file_block);
        block = block.replaced("{{{file.id}}}", &Doc::from_str(current_id))?;
        block = block.replaced("{{{file.path}}}", &Doc::from_str(&node.relative_path))?;
        block = block.replaced("{{{file.size}}}", &Doc::from_str(&node.content_size.to_string()))?;
        block = block.replaced("{{{file.lines}}}", &Doc::from_str(&line_count.to_string()))?;
        block = block.replaced("{{{file.content}}}", &Doc::from_string(content))?;
        with_vars(block, &self.variables)
    }

    fn layout_doc(&self, tree: &Doc, files: &Doc) -> Result<Doc, TemplateError> {
        let mut output = Doc::from_str(&self.layout);
        output = output.replaced("{{{comp.tree}}}", tree)?;
        output = output.replaced("{{{comp.files}}}", files)?;
        if let Some(project) = self.variables.get("project") {
            output = output.replaced("{{{project}}}", &Doc::from_str(project))?;
        }
        with_vars(output, &self.variables)
    }
}

fn block_upper(partial: &str, replacements: &[(&str, u64)]) -> u64 {
    let mut upper = partial.len() as u64;
    for (pattern, repl_len) in replacements {
        let count = partial.matches(pattern).count() as u64;
        upper = upper.saturating_add(count.saturating_mul(*repl_len));
    }
    upper.saturating_add(partial.len() as u64)
}

#[allow(clippy::naive_bytecount, reason = "Count newline bytes with standard iterators to preserve the renderer line-count convention without adding a byte-count dependency.")]
fn measured_body(node: &MarkerNode, mode: &FileMode<'_>) -> (usize, String) {
    if let Some(message) = mode.omission(node) {
        return (0, message);
    }
    match &node.content {
        Some(bytes) => {
            let lines = bytes.iter().filter(|byte| **byte == b'\n').count() + 1;
            (lines, String::from_utf8_lossy(bytes).into_owned())
        }
        None => (0, String::new()),
    }
}
