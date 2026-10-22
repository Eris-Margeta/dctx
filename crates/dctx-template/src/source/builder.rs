//! DTS section builder and template body resolution.
//! Implements ARCHITECTURE-PROPOSAL §2 and PLUGINS.md §3.5.
//! Depends on source drafts, field parsers and v1 rendering; used by the DTS source parser.

use std::collections::{BTreeSet, HashMap};

use crate::error::TemplateError;
use crate::model::Selection;
use crate::v1::{replace_vars, Template};

use super::draft::{Draft, IndexDraft, BudgetDraft, ViewDraft, GroupDraft, PluginDecl};
use super::fields::{apply_select, apply_index, apply_budget, apply_view, apply_group, apply_plugin};
use super::value::{check_ident, header, once, parse_version, split_key};

/// Parses a v2 document. Call only after [`super::version_decl`] returned [`super::VersionDecl::V2`].
pub(crate) fn parse_v2(source: &str) -> Result<Draft, TemplateError> {
    let mut builder = Builder::default();
    for (idx, line) in source.lines().enumerate() {
        let line_no = u32::try_from(idx + 1).unwrap_or(u32::MAX);
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((kind, name)) = header(trimmed) {
            builder.flush();
            builder.open(kind, name.trim(), line_no)?;
            continue;
        }
        builder.line(trimmed, line, line_no)?;
    }
    builder.flush();
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

    fn flush(&mut self) {
        match std::mem::replace(&mut self.current, Current::Empty) {
            Current::Empty | Current::Variables | Current::Dts => {},
            Current::Select(section) => {
                self.selections.push(section);
            }
            Current::Index(section) => {
                self.indexes.push(section);
            }
            Current::Budget(section) => {
                self.budgets.push(section);
            }
            Current::View(section) => {
                self.views.push(section);
            }
            Current::Plugins(section) => {
                self.groups.push(section);
            }
            Current::Plugin(section) => {
                self.plugins.push(section);
            }
            Current::Text { name, layout, body } => {
                let text = body.trim().to_string();
                if layout {
                    self.layouts.push((name, text));
                } else {
                    self.partials.push((name, text));
                }
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
        let template = crate::v1::finish(variables, &tree, &file, &layout);
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
