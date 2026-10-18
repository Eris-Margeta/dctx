//! Marker rendering for v1 partials and layouts.
//! Implements the dctx-core formatter's tree, file and layout substitution, without IO.
//! Depends on [`super::parse`]. Used by parity tests and by the bounded plan renderer.
//! Ignored, omitted and excluded nodes never copy file bytes into the answer.

use std::collections::HashMap;

use super::parse::{replace_vars, Template};
use crate::legacy_output::FileMode;
use crate::TemplateError;

const RENDER_CEILING: u64 = 16 * 1024 * 1024;

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

fn too_big() -> TemplateError {
    TemplateError::BudgetTooSmall { minimum_bytes: u64::MAX }
}

fn expanded_len(hay_len: u64, count: u64, pattern_len: u64, replacement_len: u64) -> Option<u64> {
    let removed = count.checked_mul(pattern_len)?;
    let added = count.checked_mul(replacement_len)?;
    hay_len.checked_sub(removed)?.checked_add(added)
}

fn seam_contains(text: &str, pattern: &str) -> bool {
    if pattern.len() < 2 || text.is_empty() {
        return false;
    }
    let window = pattern.len() - 1;
    let prefix = char_window_start(text, window);
    let suffix = char_window_end(text, window);
    let junction = format!("{suffix}{prefix}");
    match junction.find(pattern) {
        Some(index) => index < suffix.len() && index + pattern.len() > suffix.len(),
        None => false,
    }
}

fn char_window_start(text: &str, bytes: usize) -> &str {
    if bytes >= text.len() {
        return text;
    }
    let mut end = bytes;
    while end < text.len() && !text.is_char_boundary(end) {
        end += 1;
    }
    &text[..end]
}

fn char_window_end(text: &str, bytes: usize) -> &str {
    if bytes >= text.len() {
        return text;
    }
    let mut start = text.len() - bytes;
    while start > 0 && !text.is_char_boundary(start) {
        start -= 1;
    }
    &text[start..]
}

#[derive(Clone)]
struct Piece {
    text: String,
    times: u64,
}

#[derive(Clone, Default)]
struct Doc {
    pieces: Vec<Piece>,
}

impl Doc {
    fn len(&self) -> Result<u64, TemplateError> {
        let mut total = 0u64;
        for piece in &self.pieces {
            let one = u64::try_from(piece.text.len()).map_err(|_| too_big())?;
            let part = one.checked_mul(piece.times).ok_or_else(too_big)?;
            total = total.checked_add(part).ok_or_else(too_big)?;
        }
        Ok(total)
    }

    fn from_str(text: &str) -> Self {
        Self::from_string(text.to_string())
    }

    fn from_string(text: String) -> Self {
        if text.is_empty() {
            return Self::default();
        }
        Self { pieces: vec![Piece { text, times: 1 }] }
    }

    fn push_times(&mut self, text: &str, times: u64) -> Result<(), TemplateError> {
        if times == 0 || text.is_empty() {
            return Ok(());
        }
        if let Some(last) = self.pieces.last_mut()
            && last.text == text
        {
            last.times = last.times.checked_add(times).ok_or_else(too_big)?;
            return Ok(());
        }
        self.pieces.push(Piece { text: text.to_string(), times });
        Ok(())
    }

    fn push_str(&mut self, text: &str) -> Result<(), TemplateError> {
        self.push_times(text, 1)
    }

    fn push_repeated(&mut self, replacement: &Doc, times: u64) -> Result<(), TemplateError> {
        if times == 0 || replacement.pieces.is_empty() {
            return Ok(());
        }
        if replacement.pieces.len() == 1 {
            let piece = &replacement.pieces[0];
            let total = piece.times.checked_mul(times).ok_or_else(too_big)?;
            return self.push_times(&piece.text, total);
        }
        let unit = replacement.len()?;
        if times > 1 && unit.saturating_mul(times) > RENDER_CEILING {
            return Err(too_big());
        }
        for _ in 0..times {
            for piece in &replacement.pieces {
                self.push_times(&piece.text, piece.times)?;
            }
        }
        Ok(())
    }

    fn materialize(&self) -> Result<String, TemplateError> {
        let len = self.len()?;
        if len > RENDER_CEILING {
            return Err(too_big());
        }
        let mut out = String::new();
        if let Ok(bytes) = usize::try_from(len) {
            out.reserve(bytes);
        }
        for piece in &self.pieces {
            for _ in 0..piece.times {
                out.push_str(&piece.text);
            }
        }
        Ok(out)
    }

    fn may_contain(&self, pattern: &str) -> bool {
        let Some(&first) = pattern.as_bytes().first() else {
            return true;
        };
        self.pieces.iter().any(|piece| piece.text.as_bytes().contains(&first))
    }

    fn replaced(&self, pattern: &str, replacement: &Doc) -> Result<Doc, TemplateError> {
        if pattern.is_empty() {
            return Err(too_big());
        }
        if self.len()? <= RENDER_CEILING {
            let haystack = self.materialize()?;
            return replace_text(&haystack, pattern, replacement);
        }
        if !self.may_contain(pattern) {
            return Ok(self.clone());
        }
        let mut out = Doc::default();
        for piece in &self.pieces {
            let first = pattern.as_bytes()[0];
            if !piece.text.as_bytes().contains(&first) && !seam_contains(&piece.text, pattern) {
                out.push_times(&piece.text, piece.times)?;
                continue;
            }
            if seam_contains(&piece.text, pattern) {
                return Err(too_big());
            }
            let replaced = replace_runs(&piece.text, pattern, replacement)?;
            out.push_repeated(&replaced, piece.times)?;
        }
        Ok(out)
    }

    fn trimmed(&self) -> Result<Doc, TemplateError> {
        if self.len()? <= RENDER_CEILING {
            let text = self.materialize()?;
            return Ok(Doc::from_string(text.trim().to_string()));
        }
        self.trim_large()
    }

    fn trim_large(&self) -> Result<Doc, TemplateError> {
        let mut pieces = self.pieces.clone();
        while pieces.first().is_some_and(|piece| whitespace_unit(&piece.text)) {
            pieces.remove(0);
        }
        while pieces.last().is_some_and(|piece| whitespace_unit(&piece.text)) {
            pieces.pop();
        }
        if pieces.is_empty() {
            return Ok(Doc::default());
        }
        trim_start_side(&mut pieces);
        trim_end_side(&mut pieces);
        pieces.retain(|piece| piece.times > 0 && !piece.text.is_empty());
        Ok(Doc { pieces })
    }
}

fn whitespace_unit(text: &str) -> bool {
    text.chars().all(char::is_whitespace)
}

fn trim_start_side(pieces: &mut Vec<Piece>) {
    if pieces.is_empty() {
        return;
    }
    let trimmed = pieces[0].text.trim_start().to_string();
    if trimmed.len() == pieces[0].text.len() {
        return;
    }
    let times = pieces[0].times;
    if times == 1 {
        if trimmed.is_empty() {
            pieces.remove(0);
        } else {
            pieces[0].text = trimmed;
        }
        return;
    }
    let original = pieces[0].text.clone();
    let mut rebuilt = Vec::new();
    if !trimmed.is_empty() {
        rebuilt.push(Piece { text: trimmed, times: 1 });
    }
    rebuilt.push(Piece { text: original, times: times - 1 });
    rebuilt.extend(pieces.drain(1..));
    *pieces = rebuilt;
}

fn trim_end_side(pieces: &mut Vec<Piece>) {
    if pieces.is_empty() {
        return;
    }
    let index = pieces.len() - 1;
    let trimmed = pieces[index].text.trim_end().to_string();
    if trimmed.len() == pieces[index].text.len() {
        return;
    }
    let times = pieces[index].times;
    if times == 1 {
        if trimmed.is_empty() {
            pieces.pop();
        } else {
            pieces[index].text = trimmed;
        }
        return;
    }
    pieces[index].times = times - 1;
    if !trimmed.is_empty() {
        pieces.push(Piece { text: trimmed, times: 1 });
    }
}

fn replace_text(haystack: &str, pattern: &str, replacement: &Doc) -> Result<Doc, TemplateError> {
    if pattern.is_empty() {
        return Err(too_big());
    }
    if !haystack.contains(pattern) {
        return Ok(Doc::from_str(haystack));
    }
    let count = haystack.matches(pattern).count() as u64;
    let repl_len = replacement.len()?;
    if let Some(new_len) = expanded_len(haystack.len() as u64, count, pattern.len() as u64, repl_len)
        && new_len <= RENDER_CEILING
        && repl_len <= RENDER_CEILING
    {
        let repl_text = replacement.materialize()?;
        return Ok(Doc::from_string(haystack.replace(pattern, &repl_text)));
    }
    replace_runs(haystack, pattern, replacement)
}

fn replace_runs(haystack: &str, pattern: &str, replacement: &Doc) -> Result<Doc, TemplateError> {
    if pattern.is_empty() {
        return Err(too_big());
    }
    let mut out = Doc::default();
    let mut rest = haystack;
    loop {
        if rest.is_empty() {
            break;
        }
        match rest.find(pattern) {
            None => {
                out.push_str(rest)?;
                break;
            }
            Some(0) => {
                let mut run = 0u64;
                while let Some(stripped) = rest.strip_prefix(pattern) {
                    run = run.checked_add(1).ok_or_else(too_big)?;
                    rest = stripped;
                }
                out.push_repeated(replacement, run)?;
            }
            Some(index) => {
                out.push_str(&rest[..index])?;
                rest = &rest[index..];
            }
        }
    }
    Ok(out)
}

fn with_vars(doc: Doc, vars: &HashMap<String, String>) -> Result<Doc, TemplateError> {
    if doc.len()? <= RENDER_CEILING {
        let text = doc.materialize()?;
        return Ok(Doc::from_string(replace_vars(&text, vars)));
    }
    if !doc.may_contain("{{") {
        return Ok(doc);
    }
    let mut current = doc;
    for (key, value) in vars {
        let pattern = format!("{{{{{key}}}}}");
        current = current.replaced(&pattern, &Doc::from_str(value))?;
    }
    Ok(current)
}

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
                None => node.content.as_ref().map(|bytes| bytes.len() as u64).unwrap_or(0).saturating_mul(3),
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
        let size_attr = self.variables.get("attr_size").map(String::as_str).unwrap_or("SIZE");
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

#[cfg(test)]
mod measure_tests {
    use super::*;
    use crate::legacy_output::FileMode;
    use crate::Template;
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
        let mode = FileMode::Snapshot { tree_only: false, omit_svg_content: true };
        let (_tree_raw, _tree_trimmed, _files_raw, body) = template.measure_legacy(&root, &mode).expect("measure");
        let header = format!("[DCTX_LLM_CONTEXT_SNAPSHOT_{version}]\n");
        assert_eq!(header.len() as u64 + body, real.len() as u64);
        assert_eq!(real, format!("{header}{}", "ABC".repeat(5)));
    }

    #[test]
    fn repeated_content_over_the_ceiling_keeps_one_segment() {
        let times = 4096u64;
        let unit = 8192u64;
        let template = repeated(times as usize);
        let mut root = MarkerNode::dir("", 0);
        root.children.push(MarkerNode::file("a.txt", &vec![b'Q'; unit as usize], 1));
        let mode = FileMode::Snapshot { tree_only: false, omit_svg_content: true };
        assert!(!template.legacy_expansion_fits(&root, &mode));
        let (_tree_raw, _tree_trimmed, files_raw, body) = template.measure_legacy(&root, &mode).expect("measure");
        assert_eq!(body, times * unit);
        assert_eq!(files_raw, times * unit + 1);
    }

    #[test]
    fn measured_default_template_matches_the_renderer() {
        let mut template = Template::parse(include_str!("../../templates/default.dctx_template"));
        template.insert_variable("project", "Corpus");
        let mut root = MarkerNode::dir("", 0);
        root.children.push(MarkerNode::file("src/main.rs", b"fn main() {}\n", 1));
        let version = Version::new_v1();
        for tree_only in [false, true] {
            let real = crate::render_legacy_snapshot(&template, &root, &version, tree_only, true);
            let mode = FileMode::Snapshot { tree_only, omit_svg_content: true };
            let (_tree_raw, _tree_trimmed, _files_raw, body) = template.measure_legacy(&root, &mode).expect("measure");
            let header = format!("[DCTX_LLM_CONTEXT_SNAPSHOT_{version}]\n");
            assert_eq!(header.len() as u64 + body, real.len() as u64, "tree_only={tree_only}");
        }
    }
}
