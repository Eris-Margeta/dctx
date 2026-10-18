//! Field application for DTS selection, index, budget, view and plugin sections.
//! Implements ARCHITECTURE-PROPOSAL §2 and PLUGINS.md §3.5.
//! Depends on source drafts and scalar parsing; used by the DTS source parser.

use std::collections::BTreeSet;

use dctx_core::FileHandlingDecision;

use crate::error::TemplateError;
use crate::model::{SelectRule, Selection};

use super::draft::*;
use super::value::{once, parse_raw, split_key};

pub(super) fn apply_select(section: &mut Selection, trimmed: &str, line: u32, seen: &mut BTreeSet<String>) -> Result<(), TemplateError> {
    let (key, value) = split_key(trimmed, line)?;
    match key.as_str() {
        "include" | "exclude" | "deny" | "omit" | "debug" => {
            let decision = match key.as_str() {
                "include" => FileHandlingDecision::Include,
                "omit" => FileHandlingDecision::OmitContent,
                "debug" => FileHandlingDecision::DebugVisible,
                _ => FileHandlingDecision::Exclude,
            };
            for pattern in parse_raw(&value, line)?.list(line)? {
                section.rules.push(SelectRule { decision, pattern });
            }
            Ok(())
        }
        "roots" => {
            once(seen, &key)?;
            section.roots = parse_raw(&value, line)?.list(line)?;
            Ok(())
        }
        "languages" => {
            once(seen, &key)?;
            section.languages = parse_raw(&value, line)?.list(line)?;
            Ok(())
        }
        "kinds" => {
            once(seen, &key)?;
            section.kinds = parse_raw(&value, line)?.list(line)?;
            Ok(())
        }
        _ => Err(TemplateError::InvalidSyntax { line, detail: format!("unknown select field {key}") }),
    }
}

pub(super) fn apply_index(section: &mut IndexDraft, trimmed: &str, line: u32, seen: &mut BTreeSet<String>) -> Result<(), TemplateError> {
    let (key, value) = split_key(trimmed, line)?;
    once(seen, &key)?;
    match key.as_str() {
        "facts" => {
            section.facts = parse_raw(&value, line)?.list(line)?;
            Ok(())
        }
        "parser" => {
            section.parser = Some(parse_raw(&value, line)?.string(line)?);
            Ok(())
        }
        "language" => {
            section.language = parse_raw(&value, line)?.string(line)?;
            Ok(())
        }
        "dialect" => {
            section.dialect = parse_raw(&value, line)?.string(line)?;
            Ok(())
        }
        "max_bytes" => {
            section.max_bytes = Some(parse_raw(&value, line)?.int(line)?);
            Ok(())
        }
        _ => Err(TemplateError::InvalidSyntax { line, detail: format!("unknown index field {key}") }),
    }
}

pub(super) fn apply_budget(section: &mut BudgetDraft, trimmed: &str, line: u32, seen: &mut BTreeSet<String>) -> Result<(), TemplateError> {
    let (key, value) = split_key(trimmed, line)?;
    once(seen, &key)?;
    let number = parse_raw(&value, line)?.int(line)?;
    match key.as_str() {
        "bytes" => section.bytes = Some(number),
        "estimated_tokens" => section.estimated_tokens = Some(number),
        "hard_estimated_tokens" => section.hard_estimated_tokens = Some(number),
        "records" => section.records = Some(number),
        "depth" => {
            section.depth = Some(u32::try_from(number).map_err(|_| TemplateError::InvalidSyntax { line, detail: "depth".into() })?);
        }
        "parse_time_ms" => section.parse_time_ms = Some(number),
        "parse_memory" => section.parse_memory = Some(number),
        "work" => section.work = Some(number),
        _ => return Err(TemplateError::InvalidSyntax { line, detail: format!("unknown budget field {key}") }),
    }
    Ok(())
}

pub(super) fn apply_view(section: &mut ViewDraft, trimmed: &str, line: u32, seen: &mut BTreeSet<String>) -> Result<(), TemplateError> {
    let (key, value) = split_key(trimmed, line)?;
    once(seen, &key)?;
    let text = parse_raw(&value, line)?.string(line)?;
    match key.as_str() {
        "mode" => section.mode = Some(text),
        "selection" => section.selection = Some(text),
        "index" => section.index = Some(text),
        "budget" => section.budget = Some(text),
        "render" => section.render = Some(text),
        "layout" => section.layout_label = Some(text),
        _ => return Err(TemplateError::InvalidSyntax { line, detail: format!("unknown view field {key}") }),
    }
    Ok(())
}

pub(super) fn apply_group(section: &mut GroupDraft, trimmed: &str, line: u32, seen: &mut BTreeSet<String>) -> Result<(), TemplateError> {
    let (key, value) = split_key(trimmed, line)?;
    once(seen, &key)?;
    match key.as_str() {
        "enabled" => section.enabled = parse_raw(&value, line)?.list(line)?,
        "order" => section.order = parse_raw(&value, line)?.list(line)?,
        "network" => section.network = Some(parse_raw(&value, line)?.string(line)?),
        _ => return Err(TemplateError::InvalidSyntax { line, detail: format!("unknown plugins field {key}") }),
    }
    Ok(())
}

pub(super) fn apply_plugin(section: &mut PluginDecl, trimmed: &str, line: u32, seen: &mut BTreeSet<String>) -> Result<(), TemplateError> {
    let (key, value) = split_key(trimmed, line)?;
    if let Some(field) = key.strip_prefix("config.") {
        if field.is_empty() {
            return Err(TemplateError::InvalidSyntax { line, detail: "config field".into() });
        }
        once(seen, &key)?;
        section.config.push((field.to_string(), parse_raw(&value, line)?));
        return Ok(());
    }
    once(seen, &key)?;
    match key.as_str() {
        "version" => section.version = Some(parse_raw(&value, line)?.string(line)?),
        "digest" => section.digest = Some(parse_raw(&value, line)?.string(line)?),
        "hooks" => section.hooks = parse_raw(&value, line)?.list(line)?,
        "required" => section.required = parse_raw(&value, line)?.boolean(line)?,
        "alias" => section.alias = Some(parse_raw(&value, line)?.string(line)?),
        "allow" => section.allow = parse_raw(&value, line)?.list(line)?,
        "before" => section.before = parse_raw(&value, line)?.list(line)?,
        "after" => section.after = parse_raw(&value, line)?.list(line)?,
        _ => return Err(TemplateError::InvalidSyntax { line, detail: format!("unknown plugin field {key}") }),
    }
    Ok(())
}

