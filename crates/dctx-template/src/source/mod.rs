//! DTS v2 source grammar. Version comes only from `[DTS] version`.
//! Implements ARCHITECTURE-PROPOSAL §2 and PLUGINS.md §3.5, which this crate freezes.
//! Depends on [`crate::v1`] and [`crate::model`]. [`crate::lower`] checks the draft against the host.
//! Unknown headers are refused. A SELECT section never selects v2 by itself.

use std::collections::BTreeSet;

use crate::error::{TemplateError, MAX_PLAN_BYTES};
use crate::v1::Template;

mod builder;
mod draft;
mod fields;
mod value;

pub(crate) use builder::parse_v2;
pub(crate) use draft::{BudgetDraft, Draft, GroupDraft, IndexDraft, PluginDecl, RawValue, ViewDraft};
use value::{header, parse_version, split_key};

/// How [`crate::compile`] chooses a language version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VersionDecl {
    /// No `[DTS]` section. The source is v1.
    V1,
    /// `[DTS] version = 2`.
    V2,
}

/// Refuses an oversized source or a NUL byte before parsing.
pub(crate) fn guard_source(source: &str) -> Result<(), TemplateError> {
    if source.len() > usize::try_from(MAX_PLAN_BYTES).unwrap_or(usize::MAX) {
        return Err(TemplateError::Limit { what: "source", limit: u64::from(MAX_PLAN_BYTES) });
    }
    if source.as_bytes().contains(&0) {
        return Err(TemplateError::InvalidSyntax { line: 0, detail: "nul".into() });
    }
    Ok(())
}

/// Reads only the `[DTS]` section. A `version` key anywhere else is not a version declaration.
pub(crate) fn version_decl(source: &str) -> Result<VersionDecl, TemplateError> {
    let mut in_dts = false;
    let mut saw_dts = false;
    let mut version: Option<u32> = None;
    for (idx, line) in source.lines().enumerate() {
        let line_no = u32::try_from(idx + 1).unwrap_or(u32::MAX);
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((kind, name)) = header(trimmed) {
            in_dts = kind == "DTS";
            if in_dts {
                if !name.is_empty() || saw_dts {
                    return Err(TemplateError::InvalidSyntax { line: line_no, detail: "dts".into() });
                }
                saw_dts = true;
            }
            continue;
        }
        if in_dts {
            let (key, value) = split_key(trimmed, line_no)?;
            if key != "version" {
                return Err(TemplateError::InvalidSyntax { line: line_no, detail: format!("unknown dts field {key}") });
            }
            if version.is_some() {
                return Err(TemplateError::Duplicate { kind: "field", name: "version".into() });
            }
            version = Some(parse_version(&value, line_no)?);
        }
    }
    match (saw_dts, version) {
        (false, _) => Ok(VersionDecl::V1),
        (true, Some(2)) => Ok(VersionDecl::V2),
        (true, Some(found)) => Err(TemplateError::UnsupportedVersion { found }),
        (true, None) => Err(TemplateError::InvalidSyntax { line: 0, detail: "missing dts version".into() }),
    }
}

/// Include names written as `{{>name}}`, used to drop those unresolved-variable errors.
pub(crate) fn include_names(template: &Template, partials: &[(String, String)], layouts: &[(String, String)]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for text in [&template.partial_tree_entry, &template.partial_file_block, &template.layout] {
            for name in crate::opcode::include_targets(text) {
            names.insert(format!(">{name}"));
        }
    }
    for (_, body) in partials.iter().chain(layouts.iter()) {
        for name in crate::opcode::include_targets(body) {
            names.insert(format!(">{name}"));
        }
    }
    names
}

