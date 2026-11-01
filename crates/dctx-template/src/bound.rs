//! Bounded answer rendering and permission narrowing.
//! Implements PRODUCT-CONTRACTS P1, P5 and P6: denied bytes stay out, envelopes fit, order is canonical.
//! Depends on [`crate::model`], [`crate::opcode`] and dctx-core glob matching. Callers use [`render_plan`].
//! A template decision wider than the host grant is clamped on the render path and refused by [`authorize_path`].

use std::fmt::Write as _;
use std::collections::HashMap;

use dctx_core::budget::estimate_tokens;
use dctx_core::FileHandlingDecision;

use crate::error::TemplateError;
use crate::model::{BudgetPlan, ContextPlan, HostCeiling, Inst, NamedCode, RenderProgram, Selection};
use crate::opcode::execute;

mod policy;
use policy::{host_decision, template_decision};

const CONTINUED: &str = "[DCTX_CONTINUED]\n";
const MAX_WALK: usize = 100_000;

/// One caller-supplied record. Content is copied only when the final decision is include.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnswerRecord {
    /// Root-relative path.
    pub path: String,
    /// Directory when true.
    pub is_dir: bool,
    /// Nesting depth. Zero is the root.
    pub depth: u32,
    /// Size written into the tree entry.
    pub size: u64,
    /// Modification counter written into the tree entry.
    pub modified: u64,
    /// Caller-supplied bytes. Dropped unless the final decision is include.
    pub content: Vec<u8>,
}

/// Keeps `requested` when it is no wider than `host` in content or visibility.
///
/// # Errors
/// Returns a grant-expansion error if the requested content or visibility exceeds the host decision.
pub fn narrow_decision(
    host: FileHandlingDecision,
    requested: FileHandlingDecision,
) -> Result<FileHandlingDecision, TemplateError> {
    if content_rank(requested) > content_rank(host) || visibility_rank(requested) > visibility_rank(host) {
        Err(TemplateError::GrantExpansion { kind: "decision" })
    } else {
        Ok(requested)
    }
}

/// Intersects the host grant with the template selection. Expansion is an error.
///
/// # Errors
/// Returns a grant-expansion error when template selection widens the host decision.
pub fn authorize_path(
    host: &HostCeiling,
    selection: &Selection,
    path: &str,
) -> Result<FileHandlingDecision, TemplateError> {
    narrow_decision(host_decision(host, path), template_decision(selection, path))
}

/// Renders `view_name` for `records`. The answer is sorted, capped, and free of denied bytes.
///
/// # Errors
/// Refuses missing views/references, unavailable capabilities, exceeded ceilings, invalid render instructions or a budget too small for the answer.
pub fn render_plan(
    plan: &ContextPlan,
    view_name: &str,
    records: &[AnswerRecord],
    project: &str,
    host: &HostCeiling,
) -> Result<String, TemplateError> {
    if records.len() > MAX_WALK {
        return Err(TemplateError::Limit { what: "records", limit: MAX_WALK as u64 });
    }
    let view = plan
        .views
        .iter()
        .find(|view| view.name == view_name)
        .ok_or_else(|| TemplateError::UnresolvedRef { kind: "view", name: view_name.to_string() })?;
    let mode = format!("mode:{}", view.mode);
    if !host.capabilities.contains(&mode) {
        return Err(TemplateError::UnknownCapability { name: mode });
    }
    let selection = if view.selection.is_empty() {
        Selection { name: String::new(), roots: Vec::new(), languages: Vec::new(), kinds: Vec::new(), rules: Vec::new() }
    } else {
        plan.selections
            .iter()
            .find(|item| item.name == view.selection)
            .cloned()
            .ok_or_else(|| TemplateError::UnresolvedRef { kind: "selection", name: view.selection.clone() })?
    };
    let budget = plan
        .budgets
        .iter()
        .find(|item| item.name == view.budget)
        .cloned()
        .ok_or_else(|| TemplateError::UnresolvedRef { kind: "budget", name: view.budget.clone() })?;
    budget_within(&budget, host)?;
    if !view.render.is_empty() && !plan.render.layouts.iter().any(|layout| layout.name == view.render) {
        return Err(TemplateError::UnresolvedRef { kind: "render", name: view.render.clone() });
    }
    let prepared = prepare(records, host, &selection, budget.depth);
    if view.layout_label == "JSON" {
        return choose(prepared.len(), &budget, |count, truncated| Ok(json_document(&prepared, count, truncated)));
    }
    let end_marker = {
        let variables: HashMap<String, String> = plan.render.variables.iter().cloned().collect();
        crate::v1::end_marker_of(&variables)
    };
    let mut counter = 1usize;
    let mut fragments = Vec::with_capacity(prepared.len());
    for item in &prepared {
        fragments.push(render_record(&plan.render, item, &mut counter, &end_marker)?);
    }
    let layout_name = view.render.clone();
    choose(prepared.len(), &budget, |count, truncated| {
        let (tree, files) = join_fragments(&fragments, count);
        let body = apply_layout(&plan.render, &layout_name, &tree, &files, project)?;
        Ok(finish_marker(body, truncated))
    })
}

fn budget_within(budget: &BudgetPlan, host: &HostCeiling) -> Result<(), TemplateError> {
    within(budget.bytes, host.max_bytes, "bytes")?;
    within(budget.estimated_tokens, host.max_estimated_tokens, "estimated_tokens")?;
    within(budget.hard_estimated_tokens, host.max_hard_estimated_tokens, "hard_estimated_tokens")?;
    within(budget.records, host.max_records, "records")?;
    within(u64::from(budget.depth), u64::from(host.max_depth), "depth")?;
    within(budget.parse_time_ms, host.max_parse_time_ms, "parse_time_ms")?;
    within(budget.parse_memory, host.max_parse_memory, "parse_memory")?;
    within(budget.work, host.max_work, "work")?;
    if budget.hard_estimated_tokens < budget.estimated_tokens {
        return Err(TemplateError::InvalidSyntax { line: 0, detail: "hard budget".into() });
    }
    Ok(())
}

fn within(requested: u64, ceiling: u64, field: &'static str) -> Result<(), TemplateError> {
    if requested > ceiling {
        Err(TemplateError::BudgetAboveCeiling { field, requested, ceiling })
    } else {
        Ok(())
    }
}

struct Prepared {
    path: String,
    is_dir: bool,
    depth: u32,
    size: u64,
    modified: u64,
    decision: FileHandlingDecision,
    content: Vec<u8>,
}

fn prepare(records: &[AnswerRecord], host: &HostCeiling, selection: &Selection, max_depth: u32) -> Vec<Prepared> {
    let mut prepared = Vec::new();
    for record in records {
        if record.depth > max_depth {
            continue;
        }
        let host_decision = host_decision(host, &record.path);
        let requested = template_decision(selection, &record.path);
        let decision = match narrow_decision(host_decision, requested) {
            Ok(decision) => decision,
            Err(_) => host_decision,
        };
        if decision == FileHandlingDecision::Exclude {
            continue;
        }
        let content = if decision == FileHandlingDecision::Include { record.content.clone() } else { Vec::new() };
        prepared.push(Prepared {
            path: record.path.clone(),
            is_dir: record.is_dir,
            depth: record.depth,
            size: record.size,
            modified: record.modified,
            decision,
            content,
        });
    }
    prepared.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.depth.cmp(&right.depth))
            .then(left.is_dir.cmp(&right.is_dir))
            .then(left.size.cmp(&right.size))
            .then(left.modified.cmp(&right.modified))
            .then(left.content.cmp(&right.content))
    });
    prepared
}

fn choose(
    total: usize,
    budget: &BudgetPlan,
    mut render: impl FnMut(usize, bool) -> Result<String, TemplateError>,
) -> Result<String, TemplateError> {
    let cap = usize::try_from(budget.records).unwrap_or(usize::MAX);
    let max_count = total.min(cap);
    for count in (0..=max_count).rev() {
        let truncated = count < total;
        let text = render(count, truncated)?;
        if fits(&text, budget) {
            return Ok(text);
        }
    }
    let probe = render(0, total > 0)?;
    let minimum_bytes = u64::try_from(probe.len()).unwrap_or(u64::MAX);
    Err(TemplateError::BudgetTooSmall { minimum_bytes })
}

fn fits(text: &str, budget: &BudgetPlan) -> bool {
    let bytes = u64::try_from(text.len()).unwrap_or(u64::MAX);
    if bytes > budget.bytes {
        return false;
    }
    let tokens = estimate_tokens(bytes);
    tokens <= budget.estimated_tokens && tokens <= budget.hard_estimated_tokens
}

fn finish_marker(body: String, truncated: bool) -> String {
    if truncated {
        let mut out = body;
        out.push_str(CONTINUED);
        out
    } else {
        body
    }
}

struct Fragment {
    tree: String,
    file: String,
}

fn render_record(
    program: &RenderProgram,
    item: &Prepared,
    counter: &mut usize,
    end_marker: &str,
) -> Result<Fragment, TemplateError> {
    let id = if item.depth == 0 && item.is_dir {
        "ROOT".to_string()
    } else if item.is_dir {
        let id = format!("D{counter:03}");
        *counter += 1;
        id
    } else {
        let id = format!("F{counter:03}");
        *counter += 1;
        id
    };
    let type_char = if item.is_dir { "[D]" } else { "[F]" };
    let attr = variable(program, "attr_size").unwrap_or("SIZE");
    let size_part = if item.is_dir { String::new() } else { format!(", {attr}:{}", item.size) };
    let depth = usize::try_from(item.depth).unwrap_or(usize::MAX).min(4096);
    let indent = "  ".repeat(depth);
    let display = if item.decision == FileHandlingDecision::DebugVisible {
        format!("{} [IGNORED]", item.path)
    } else {
        item.path.clone()
    };
    let modified = item.modified.to_string();
    let mut tree = String::new();
    if let Some(code) = named(&program.partials, "TREE_ENTRY") {
        let body = execute(program, code, &mut |field| Ok(node_value(field, type_char, &display, &id, &modified, &size_part)), 0)?;
        tree.push_str(&indent);
        tree.push_str(&body);
        tree.push('\n');
    }
    let mut file = String::new();
    if !item.is_dir
        && item.decision != FileHandlingDecision::DebugVisible
        && let Some(code) = named(&program.partials, "FILE_BLOCK")
    {
        let (lines, text) = file_text(item, end_marker);
        let size = item.size.to_string();
        let lines = lines.to_string();
        let body = execute(
            program,
            code,
            &mut |field| Ok(file_value(field, &id, &item.path, &lines, &size, &text)),
            0,
        )?;
        file.push_str(&body);
        file.push('\n');
    }
    Ok(Fragment { tree, file })
}

fn node_value(field: u16, type_char: &str, path: &str, id: &str, modified: &str, size_part: &str) -> String {
    match field {
        1 => type_char.to_string(),
        2 => path.to_string(),
        3 => id.to_string(),
        4 => modified.to_string(),
        5 => size_part.to_string(),
        _ => String::new(),
    }
}

fn file_value(field: u16, id: &str, path: &str, lines: &str, size: &str, content: &str) -> String {
    match field {
        6 => id.to_string(),
        7 => path.to_string(),
        8 => lines.to_string(),
        9 => size.to_string(),
        10 => content.to_string(),
        _ => String::new(),
    }
}

#[allow(clippy::naive_bytecount, reason = "Count newline bytes with standard iterators to preserve the renderer line-count convention without adding a byte-count dependency.")]
fn file_text(item: &Prepared, end_marker: &str) -> (usize, String) {
    if item.decision == FileHandlingDecision::OmitContent {
        return (0, "[Content omitted by pattern rules]".to_string());
    }
    let lines = item.content.iter().filter(|byte| **byte == b'\n').count() + 1;
    let text = String::from_utf8_lossy(&item.content);
    (lines, escape_content(&text, end_marker))
}

fn escape_content(text: &str, end_marker: &str) -> String {
    if !text.contains('\\') && (end_marker.is_empty() || !text.contains(end_marker)) {
        return text.to_string();
    }
    let marker = end_marker.as_bytes();
    let bytes = text.as_bytes();
    let mut out = String::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            out.push('\\');
            out.push('\\');
            index += 1;
            continue;
        }
        if !marker.is_empty() && bytes[index..].starts_with(marker) {
            out.push('\\');
        }
        let Some(ch) = text[index..].chars().next() else {
            break;
        };
        out.push(ch);
        index += ch.len_utf8();
    }
    out
}

fn join_fragments(fragments: &[Fragment], count: usize) -> (String, String) {
    let mut tree = String::new();
    let mut files = String::new();
    for fragment in fragments.iter().take(count) {
        tree.push_str(&fragment.tree);
        files.push_str(&fragment.file);
    }
    (tree.trim().to_string(), files.trim().to_string())
}

fn apply_layout(
    program: &RenderProgram,
    name: &str,
    tree: &str,
    files: &str,
    project: &str,
) -> Result<String, TemplateError> {
    let Some(code) = named(&program.layouts, name) else {
        return Ok(String::new());
    };
    execute(program, code, &mut |field| Ok(layout_value(field, tree, files, project)), 0)
}

fn layout_value(field: u16, tree: &str, files: &str, project: &str) -> String {
    match field {
        11 => tree.to_string(),
        12 => files.to_string(),
        13 => project.to_string(),
        _ => String::new(),
    }
}

fn json_document(items: &[Prepared], count: usize, truncated: bool) -> String {
    let mut out = String::from("{\"records\":[");
    for (index, item) in items.iter().take(count).enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"path\":");
        push_json(&mut out, &item.path);
        out.push_str(",\"content\":");
        push_json(&mut out, &json_content(item));
        out.push('}');
    }
    out.push_str("],\"truncated\":");
    out.push_str(if truncated { "true" } else { "false" });
    out.push('}');
    out
}

fn json_content(item: &Prepared) -> String {
    if item.is_dir {
        return String::new();
    }
    match item.decision {
        FileHandlingDecision::Include => String::from_utf8_lossy(&item.content).into_owned(),
        FileHandlingDecision::OmitContent => "[Content omitted by pattern rules]".to_string(),
        FileHandlingDecision::DebugVisible | FileHandlingDecision::Exclude => String::new(),
    }
}

fn push_json(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if (other as u32) < 0x20 => {
                let code = other as u32;
                let _ = write!(out,"\\u{code:04x}");
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

fn named<'a>(items: &'a [NamedCode], name: &str) -> Option<&'a [Inst]> {
    items.iter().find(|item| item.name == name).map(|item| item.instructions.as_slice())
}

fn variable<'a>(program: &'a RenderProgram, key: &str) -> Option<&'a str> {
    program.variables.iter().find(|(name, _)| name == key).map(|(_, value)| value.as_str())
}

fn content_rank(decision: FileHandlingDecision) -> u8 {
    match decision {
        FileHandlingDecision::Include => 2,
        FileHandlingDecision::OmitContent | FileHandlingDecision::DebugVisible => 1,
        FileHandlingDecision::Exclude => 0,
    }
}

fn visibility_rank(decision: FileHandlingDecision) -> u8 {
    match decision {
        FileHandlingDecision::Include | FileHandlingDecision::OmitContent => 2,
        FileHandlingDecision::DebugVisible => 1,
        FileHandlingDecision::Exclude => 0,
    }
}
