//! DTS v2 source grammar. Version comes only from `[DTS] version`.
//! Implements ARCHITECTURE-PROPOSAL §2 and PLUGINS.md §3.5, which this crate freezes.
//! Depends on [`crate::v1`] and [`crate::model`]. [`crate::lower`] checks the draft against the host.
//! Unknown headers are refused. A SELECT section never selects v2 by itself.

use std::collections::{BTreeSet, HashMap};

use dctx_core::FileHandlingDecision;

use crate::error::{TemplateError, MAX_PLAN_BYTES};
use crate::model::{SelectRule, Selection};
use crate::v1::{replace_vars, Template};

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

/// Unchecked v2 sections plus the v1 render pieces.
#[derive(Debug)]
pub(crate) struct Draft {
    pub selections: Vec<Selection>,
    pub indexes: Vec<IndexDraft>,
    pub budgets: Vec<BudgetDraft>,
    pub views: Vec<ViewDraft>,
    pub groups: Vec<GroupDraft>,
    pub plugins: Vec<PluginDecl>,
    pub template: Template,
    pub partials: Vec<(String, String)>,
    pub layouts: Vec<(String, String)>,
}

/// INDEX fields before capability checks.
#[derive(Debug)]
pub(crate) struct IndexDraft {
    pub name: String,
    pub facts: Vec<String>,
    pub parser: Option<String>,
    pub language: String,
    pub dialect: String,
    pub max_bytes: Option<u64>,
}

/// BUDGET fields before the host ceiling is applied. `None` means "use the ceiling".
#[derive(Debug)]
pub(crate) struct BudgetDraft {
    pub name: String,
    pub bytes: Option<u64>,
    pub estimated_tokens: Option<u64>,
    pub hard_estimated_tokens: Option<u64>,
    pub records: Option<u64>,
    pub depth: Option<u32>,
    pub parse_time_ms: Option<u64>,
    pub parse_memory: Option<u64>,
    pub work: Option<u64>,
}

/// VIEW fields before reference checks.
#[derive(Debug)]
pub(crate) struct ViewDraft {
    pub name: String,
    pub mode: Option<String>,
    pub selection: Option<String>,
    pub index: Option<String>,
    pub budget: Option<String>,
    pub render: Option<String>,
    pub layout_label: Option<String>,
}

/// PLUGINS group before network narrowing.
#[derive(Debug)]
pub(crate) struct GroupDraft {
    pub name: String,
    pub network: Option<String>,
    pub enabled: Vec<String>,
    pub order: Vec<String>,
}

/// PLUGIN declaration before catalog checks.
#[derive(Debug)]
pub(crate) struct PluginDecl {
    pub id: String,
    pub version: Option<String>,
    pub digest: Option<String>,
    pub hooks: Vec<String>,
    pub required: bool,
    pub alias: Option<String>,
    pub allow: Vec<String>,
    pub config: Vec<(String, RawValue)>,
    pub before: Vec<String>,
    pub after: Vec<String>,
}

/// A source value that is not yet checked against a manifest field.
#[derive(Debug, Clone)]
pub(crate) enum RawValue {
    Bool(bool),
    Int(u64),
    Str(String),
    List(Vec<String>),
}

/// Parses a v2 document. Call only after [`version_decl`] returned [`VersionDecl::V2`].
pub(crate) fn parse_v2(source: &str) -> Result<Draft, TemplateError> {
    let mut builder = Builder::default();
    for (idx, line) in source.lines().enumerate() {
        let line_no = u32::try_from(idx + 1).unwrap_or(u32::MAX);
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((kind, name)) = header(trimmed) {
            builder.flush()?;
            builder.open(kind, name.trim(), line_no)?;
            continue;
        }
        builder.line(trimmed, line, line_no)?;
    }
    builder.flush()?;
    builder.finish()
}

#[derive(Default)]
struct Builder {
    version_ok: bool,
    selections: Vec<Selection>,
    indexes: Vec<IndexDraft>,
    budgets: Vec<BudgetDraft>,
    views: Vec<ViewDraft>,
    groups: Vec<GroupDraft>,
    plugins: Vec<PluginDecl>,
    variables: HashMap<String, String>,
    partials: Vec<(String, String)>,
    layouts: Vec<(String, String)>,
    current: Current,
    seen: BTreeSet<String>,
}

#[derive(Default)]
enum Current {
    #[default]
    Empty,
    Dts,
    Select(Selection),
    Index(IndexDraft),
    Budget(BudgetDraft),
    View(ViewDraft),
    Plugins(GroupDraft),
    Plugin(PluginDecl),
    Variables,
    Text { name: String, layout: bool, body: String },
}

impl Builder {
    fn open(&mut self, kind: &str, name: &str, line: u32) -> Result<(), TemplateError> {
        self.seen.clear();
        let named = matches!(kind, "SELECT" | "INDEX" | "BUDGET" | "VIEW" | "PLUGINS" | "PLUGIN" | "PARTIAL" | "RENDER");
        if named {
            check_ident(name, line)?;
        } else if !name.is_empty() {
            return Err(TemplateError::InvalidSyntax { line, detail: "unexpected header name".into() });
        }
        self.current = match kind {
            "DTS" => Current::Dts,
            "SELECT" => Current::Select(Selection {
                name: name.to_string(),
                roots: Vec::new(),
                languages: Vec::new(),
                kinds: Vec::new(),
                rules: Vec::new(),
            }),
            "INDEX" => Current::Index(IndexDraft {
                name: name.to_string(),
                facts: Vec::new(),
                parser: None,
                language: String::new(),
                dialect: String::new(),
                max_bytes: None,
            }),
            "BUDGET" => Current::Budget(BudgetDraft {
                name: name.to_string(),
                bytes: None,
                estimated_tokens: None,
                hard_estimated_tokens: None,
                records: None,
                depth: None,
                parse_time_ms: None,
                parse_memory: None,
                work: None,
            }),
            "VIEW" => Current::View(ViewDraft {
                name: name.to_string(),
                mode: None,
                selection: None,
                index: None,
                budget: None,
                render: None,
                layout_label: None,
            }),
            "PLUGINS" => Current::Plugins(GroupDraft {
                name: name.to_string(),
                network: None,
                enabled: Vec::new(),
                order: Vec::new(),
            }),
            "PLUGIN" => Current::Plugin(PluginDecl {
                id: name.to_string(),
                version: None,
                digest: None,
                hooks: Vec::new(),
                required: false,
                alias: None,
                allow: Vec::new(),
                config: Vec::new(),
                before: Vec::new(),
                after: Vec::new(),
            }),
            "VARIABLES" => Current::Variables,
            "PARTIAL" => Current::Text { name: name.to_string(), layout: false, body: String::new() },
            "LAYOUT" => Current::Text { name: "main".to_string(), layout: true, body: String::new() },
            "RENDER" => Current::Text { name: name.to_string(), layout: true, body: String::new() },
            _ => return Err(TemplateError::InvalidSyntax { line, detail: "unknown section".into() }),
        };
        Ok(())
    }

    fn flush(&mut self) -> Result<(), TemplateError> {
        match std::mem::replace(&mut self.current, Current::Empty) {
            Current::Empty | Current::Variables => Ok(()),
            Current::Dts => Ok(()),
            Current::Select(section) => {
                self.selections.push(section);
                Ok(())
            }
            Current::Index(section) => {
                self.indexes.push(section);
                Ok(())
            }
            Current::Budget(section) => {
                self.budgets.push(section);
                Ok(())
            }
            Current::View(section) => {
                self.views.push(section);
                Ok(())
            }
            Current::Plugins(section) => {
                self.groups.push(section);
                Ok(())
            }
            Current::Plugin(section) => {
                self.plugins.push(section);
                Ok(())
            }
            Current::Text { name, layout, body } => {
                let text = body.trim().to_string();
                if layout {
                    self.layouts.push((name, text));
                } else {
                    self.partials.push((name, text));
                }
                Ok(())
            }
        }
    }

    fn line(&mut self, trimmed: &str, raw: &str, line: u32) -> Result<(), TemplateError> {
        let mut current = std::mem::replace(&mut self.current, Current::Empty);
        let result = match &mut current {
            Current::Empty => Err(TemplateError::InvalidSyntax { line, detail: "text outside a section".into() }),
            Current::Text { body, .. } => {
                body.push_str(raw);
                body.push('\n');
                Ok(())
            }
            Current::Variables => {
                let (key, value) = split_key(trimmed, line)?;
                if key.is_empty() {
                    return Err(TemplateError::InvalidSyntax { line, detail: "variable name".into() });
                }
                let value = value.trim().trim_matches('"').replace("\\n", "\n");
                self.variables.insert(key, value);
                Ok(())
            }
            Current::Dts => self.apply_dts(trimmed, line),
            Current::Select(section) => apply_select(section, trimmed, line, &mut self.seen),
            Current::Index(section) => apply_index(section, trimmed, line, &mut self.seen),
            Current::Budget(section) => apply_budget(section, trimmed, line, &mut self.seen),
            Current::View(section) => apply_view(section, trimmed, line, &mut self.seen),
            Current::Plugins(section) => apply_group(section, trimmed, line, &mut self.seen),
            Current::Plugin(section) => apply_plugin(section, trimmed, line, &mut self.seen),
        };
        self.current = current;
        result
    }

    fn apply_dts(&mut self, trimmed: &str, line: u32) -> Result<(), TemplateError> {
        let (key, value) = split_key(trimmed, line)?;
        once(&mut self.seen, &key)?;
        if key != "version" {
            return Err(TemplateError::InvalidSyntax { line, detail: format!("unknown dts field {key}") });
        }
        let version = parse_version(&value, line)?;
        if version != 2 {
            return Err(TemplateError::UnsupportedVersion { found: version });
        }
        self.version_ok = true;
        Ok(())
    }

    fn finish(self) -> Result<Draft, TemplateError> {
        if !self.version_ok {
            return Err(TemplateError::InvalidSyntax { line: 0, detail: "missing dts version".into() });
        }
        let Self { variables, partials, layouts, selections, indexes, budgets, views, groups, plugins, .. } = self;
        let tree = body_named(&partials, "TREE_ENTRY");
        let file = body_named(&partials, "FILE_BLOCK");
        let layout = body_named(&layouts, "main");
        let template = crate::v1::finish(variables, tree, file, layout);
        let resolved_partials = resolve_bodies(&partials, &template);
        let resolved_layouts = resolve_bodies(&layouts, &template);
        Ok(Draft {
            selections,
            indexes,
            budgets,
            views,
            groups,
            plugins,
            template,
            partials: resolved_partials,
            layouts: resolved_layouts,
        })
    }
}

fn body_named(items: &[(String, String)], name: &str) -> String {
    items.iter().find(|(item, _)| item == name).map(|(_, body)| body.clone()).unwrap_or_default()
}

fn resolve_bodies(items: &[(String, String)], template: &Template) -> Vec<(String, String)> {
    items
        .iter()
        .map(|(name, body)| (name.clone(), replace_vars(body, &template.variables)))
        .collect()
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

fn apply_select(section: &mut Selection, trimmed: &str, line: u32, seen: &mut BTreeSet<String>) -> Result<(), TemplateError> {
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

fn apply_index(section: &mut IndexDraft, trimmed: &str, line: u32, seen: &mut BTreeSet<String>) -> Result<(), TemplateError> {
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

fn apply_budget(section: &mut BudgetDraft, trimmed: &str, line: u32, seen: &mut BTreeSet<String>) -> Result<(), TemplateError> {
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

fn apply_view(section: &mut ViewDraft, trimmed: &str, line: u32, seen: &mut BTreeSet<String>) -> Result<(), TemplateError> {
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

fn apply_group(section: &mut GroupDraft, trimmed: &str, line: u32, seen: &mut BTreeSet<String>) -> Result<(), TemplateError> {
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

fn apply_plugin(section: &mut PluginDecl, trimmed: &str, line: u32, seen: &mut BTreeSet<String>) -> Result<(), TemplateError> {
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

fn once(seen: &mut BTreeSet<String>, key: &str) -> Result<(), TemplateError> {
    if seen.insert(key.to_string()) {
        Ok(())
    } else {
        Err(TemplateError::Duplicate { kind: "field", name: key.to_string() })
    }
}

fn split_key(trimmed: &str, line: u32) -> Result<(String, String), TemplateError> {
    let Some((key, value)) = trimmed.split_once('=') else {
        return Err(TemplateError::InvalidSyntax { line, detail: "expected key = value".into() });
    };
    let key = key.trim();
    if key.is_empty() || !key.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' || ch == '-') {
        return Err(TemplateError::InvalidSyntax { line, detail: "field name".into() });
    }
    Ok((key.to_string(), value.trim().to_string()))
}

fn parse_version(value: &str, line: u32) -> Result<u32, TemplateError> {
    let text = value.trim().trim_matches('"');
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(TemplateError::InvalidSyntax { line, detail: "version".into() });
    }
    text.parse::<u32>().map_err(|_| TemplateError::InvalidSyntax { line, detail: "version".into() })
}

fn parse_raw(value: &str, line: u32) -> Result<RawValue, TemplateError> {
    let value = value.trim();
    if let Some(inner) = value.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
        return Ok(RawValue::List(parse_list(inner, line)?));
    }
    if value == "true" {
        return Ok(RawValue::Bool(true));
    }
    if value == "false" {
        return Ok(RawValue::Bool(false));
    }
    if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
        let number = value.parse::<u64>().map_err(|_| TemplateError::InvalidSyntax { line, detail: "integer".into() })?;
        return Ok(RawValue::Int(number));
    }
    Ok(RawValue::Str(unquote(value, line)?))
}

fn parse_list(inner: &str, line: u32) -> Result<Vec<String>, TemplateError> {
    let mut items = Vec::new();
    let mut rest = inner.trim();
    if rest.is_empty() {
        return Ok(items);
    }
    while !rest.is_empty() {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        if !rest.starts_with('"') {
            return Err(TemplateError::InvalidSyntax { line, detail: "list item".into() });
        }
        let mut out = String::new();
        let mut chars = rest[1..].chars();
        let mut closed = false;
        let mut consumed = 1usize;
        while let Some(ch) = chars.next() {
            consumed += ch.len_utf8();
            if ch == '\\' {
                let Some(esc) = chars.next() else {
                    return Err(TemplateError::InvalidSyntax { line, detail: "string escape".into() });
                };
                consumed += esc.len_utf8();
                out.push(match esc {
                    'n' => '\n',
                    '\\' => '\\',
                    '"' => '"',
                    other => other,
                });
                continue;
            }
            if ch == '"' {
                closed = true;
                break;
            }
            out.push(ch);
        }
        if !closed {
            return Err(TemplateError::InvalidSyntax { line, detail: "unterminated string".into() });
        }
        items.push(out);
        rest = rest[consumed..].trim_start();
        if rest.is_empty() {
            break;
        }
        if let Some(next) = rest.strip_prefix(',') {
            rest = next;
            continue;
        }
        return Err(TemplateError::InvalidSyntax { line, detail: "list separator".into() });
    }
    if items.len() > 1024 {
        return Err(TemplateError::Limit { what: "list", limit: 1024 });
    }
    Ok(items)
}

fn unquote(value: &str, line: u32) -> Result<String, TemplateError> {
    if let Some(inner) = value.strip_prefix('"') {
        let Some(inner) = inner.strip_suffix('"') else {
            return Err(TemplateError::InvalidSyntax { line, detail: "unterminated string".into() });
        };
        let mut out = String::new();
        let mut chars = inner.chars();
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                let Some(esc) = chars.next() else {
                    return Err(TemplateError::InvalidSyntax { line, detail: "string escape".into() });
                };
                out.push(match esc {
                    'n' => '\n',
                    '\\' => '\\',
                    '"' => '"',
                    other => other,
                });
            } else {
                out.push(ch);
            }
        }
        return Ok(out);
    }
    if value.chars().any(|ch| ch.is_whitespace()) {
        return Err(TemplateError::InvalidSyntax { line, detail: "string".into() });
    }
    Ok(value.to_string())
}

impl RawValue {
    fn list(self, line: u32) -> Result<Vec<String>, TemplateError> {
        match self {
            Self::List(items) => Ok(items),
            _ => Err(TemplateError::InvalidSyntax { line, detail: "expected a list".into() }),
        }
    }

    fn string(self, line: u32) -> Result<String, TemplateError> {
        match self {
            Self::Str(text) => Ok(text),
            _ => Err(TemplateError::InvalidSyntax { line, detail: "expected a string".into() }),
        }
    }

    fn int(self, line: u32) -> Result<u64, TemplateError> {
        match self {
            Self::Int(number) => Ok(number),
            _ => Err(TemplateError::InvalidSyntax { line, detail: "expected an integer".into() }),
        }
    }

    fn boolean(self, line: u32) -> Result<bool, TemplateError> {
        match self {
            Self::Bool(value) => Ok(value),
            _ => Err(TemplateError::InvalidSyntax { line, detail: "expected a boolean".into() }),
        }
    }
}

fn header(trimmed: &str) -> Option<(&str, &str)> {
    let inner = trimmed.strip_prefix('[')?.strip_suffix(']')?;
    if inner.is_empty() || inner.contains('[') {
        return None;
    }
    let (kind, name) = inner.split_once(':').unwrap_or((inner, ""));
    let known = matches!(
        kind,
        "DTS" | "SELECT" | "INDEX" | "BUDGET" | "VIEW" | "PLUGINS" | "PLUGIN" | "VARIABLES" | "PARTIAL" | "LAYOUT" | "RENDER"
    );
    if known { Some((kind, name)) } else { None }
}

fn check_ident(name: &str, line: u32) -> Result<(), TemplateError> {
    if name.is_empty() || name.len() > 128 {
        return Err(TemplateError::InvalidSyntax { line, detail: "name".into() });
    }
    if name.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '/' | '.')) {
        Ok(())
    } else {
        Err(TemplateError::InvalidSyntax { line, detail: "name".into() })
    }
}
