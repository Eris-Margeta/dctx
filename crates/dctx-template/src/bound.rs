// SPDX-License-Identifier: Apache-2.0
//! Bounded answer rendering and permission narrowing.
//! Implements PRODUCT-CONTRACTS P1, P5 and P6: denied bytes stay out, envelopes fit, order is canonical.
//! Depends on [`crate::model`], [`crate::opcode`] and dctx-core glob matching. Callers use [`render_plan`].
//! A template decision wider than the host grant is clamped on the render path and refused by [`authorize_path`].

use dctx_core::FileHandlingDecision;

use crate::error::TemplateError;
use crate::model::{ContextPlan, HostCeiling, RenderProgram, Selection};
use crate::opcode::execute;

mod policy;
use policy::{host_decision, template_decision};

mod rank;
pub use rank::{authorize_path, narrow_decision};

mod json;
use json::{json_document, named};
mod record;
use record::{Fragment, render_record};

mod fit;
use fit::{budget_within, choose, finish_marker};

pub(super) const CONTINUED: &str = "[DCTX_CONTINUED]\n";
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

/// Renders `view_name` for `records`. The answer is sorted, capped, and free of denied bytes.
/// At most 100,000 input records are accepted. Duplicate records remain separate records;
/// ordering is canonical across paths, metadata and content, independent of caller order.
/// Invalid UTF-8 bodies use replacement characters, whose encoded bytes count toward the budget.
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
        return Err(TemplateError::Limit {
            what: "records",
            limit: MAX_WALK as u64,
        });
    }
    crate::opcode::validate_program(&plan.render)?;
    let view = plan
        .views
        .iter()
        .find(|view| view.name == view_name)
        .ok_or_else(|| TemplateError::UnresolvedRef {
            kind: "view",
            name: view_name.to_string(),
        })?;
    let mode = format!("mode:{}", view.mode);
    if !host.capabilities.contains(&mode) {
        return Err(TemplateError::UnknownCapability { name: mode });
    }
    let selection = if view.selection.is_empty() {
        Selection {
            name: String::new(),
            roots: Vec::new(),
            languages: Vec::new(),
            kinds: Vec::new(),
            rules: Vec::new(),
        }
    } else {
        plan.selections
            .iter()
            .find(|item| item.name == view.selection)
            .cloned()
            .ok_or_else(|| TemplateError::UnresolvedRef {
                kind: "selection",
                name: view.selection.clone(),
            })?
    };
    let budget = plan
        .budgets
        .iter()
        .find(|item| item.name == view.budget)
        .cloned()
        .ok_or_else(|| TemplateError::UnresolvedRef {
            kind: "budget",
            name: view.budget.clone(),
        })?;
    budget_within(&budget, host)?;
    if !view.render.is_empty()
        && !plan
            .render
            .layouts
            .iter()
            .any(|layout| layout.name == view.render)
    {
        return Err(TemplateError::UnresolvedRef {
            kind: "render",
            name: view.render.clone(),
        });
    }
    let prepared = prepare(records, host, &selection, budget.depth);
    if view.layout_label == "JSON" {
        return choose(prepared.len(), &budget, |count, truncated| {
            Ok(json_document(&prepared, count, truncated))
        });
    }
    let mut counter = 1usize;
    let mut fragments = Vec::with_capacity(prepared.len());
    for item in &prepared {
        fragments.push(render_record(&plan.render, item, &mut counter)?);
    }
    let layout_name = view.render.clone();
    choose(prepared.len(), &budget, |count, truncated| {
        let (tree, files) = join_fragments(&fragments, count);
        let body = apply_layout(&plan.render, &layout_name, &tree, &files, project)?;
        Ok(finish_marker(body, truncated))
    })
}

pub(super) struct Prepared {
    pub(super) path: String,
    pub(super) is_dir: bool,
    pub(super) depth: u32,
    pub(super) size: u64,
    pub(super) modified: u64,
    pub(super) decision: FileHandlingDecision,
    pub(super) content: Vec<u8>,
}

fn prepare(
    records: &[AnswerRecord],
    host: &HostCeiling,
    selection: &Selection,
    max_depth: u32,
) -> Vec<Prepared> {
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
        let content = if decision == FileHandlingDecision::Include {
            record.content.clone()
        } else {
            Vec::new()
        };
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

pub(super) fn node_value(
    field: u16,
    type_char: &str,
    path: &str,
    id: &str,
    modified: &str,
    size_part: &str,
) -> String {
    match field {
        1 => type_char.to_string(),
        2 => path.to_string(),
        3 => id.to_string(),
        4 => modified.to_string(),
        5 => size_part.to_string(),
        _ => String::new(),
    }
}

pub(super) fn file_value(
    field: u16,
    id: &str,
    path: &str,
    lines: &str,
    size: &str,
    content: &str,
) -> String {
    match field {
        6 => id.to_string(),
        7 => path.to_string(),
        8 => lines.to_string(),
        9 => size.to_string(),
        10 => content.to_string(),
        _ => String::new(),
    }
}

#[allow(
    clippy::naive_bytecount,
    reason = "Count newline bytes with standard iterators to preserve the renderer line-count convention without adding a byte-count dependency."
)]
pub(super) fn file_text(item: &Prepared) -> (usize, String) {
    if item.decision == FileHandlingDecision::OmitContent {
        return (0, "[Content omitted by pattern rules]".to_string());
    }
    let lines = item.content.iter().filter(|byte| **byte == b'\n').count() + 1;
    let text = String::from_utf8_lossy(&item.content);
    (lines, text.into_owned())
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
    execute(
        program,
        code,
        &mut |field| Ok(layout_value(field, tree, files, project)),
        0,
    )
}

fn layout_value(field: u16, tree: &str, files: &str, project: &str) -> String {
    match field {
        11 => tree.to_string(),
        12 => files.to_string(),
        13 => project.to_string(),
        _ => String::new(),
    }
}
