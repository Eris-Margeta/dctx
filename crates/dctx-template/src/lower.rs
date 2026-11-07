// SPDX-License-Identifier: Apache-2.0
//! Host checks for a parsed template: permissions, budgets, plugins and render bytecode.
//! Implements PRODUCT-CONTRACTS C03 and PLUGINS.md §3.5 (narrow only, cycles refused).
//! Depends on [`crate::source`], [`crate::opcode`] and [`crate::binary`]. Callers use [`compile`].
//! A value above a ceiling is an error. An omitted budget field keeps the ceiling.

use std::collections::BTreeSet;

use crate::binary;
use crate::error::{
    MAX_INSTRUCTIONS, MAX_PLAN_BYTES, MAX_SECTIONS, TemplateError, TemplateValidationError,
};
use crate::model::{
    BudgetPlan, CAP_OPAQUE, CAP_PACK, CAP_PLUGIN_HOST, ContextPlan, HostCeiling, IndexPlan,
    KNOWN_FACTS, KNOWN_KINDS, KNOWN_LANGUAGES, RenderProgram, Selection, ViewPlan,
};
use crate::opcode::{compile_program, include_targets};
use crate::plugin::PluginState;
use crate::source::{
    BudgetDraft, Draft, IndexDraft, VersionDecl, ViewDraft, guard_source, include_names, parse_v2,
};
use crate::v1::{Template, replace_vars};

/// Compiles v1 or v2 source into a sealed [`ContextPlan`].
///
/// # Errors
/// Refuses invalid or oversized source, unresolved references, unsupported features and plugin/grant/budget requests outside the host ceiling.
pub fn compile(source: &str, host: &HostCeiling) -> Result<ContextPlan, TemplateError> {
    guard_source(source)?;
    let mut plan = match version_decl_plan(source, host)? {
        PlanBuild::Ready(plan) => *plan,
        PlanBuild::V1(template) => lower_v1(&template, host)?,
    };
    binary::seal(&mut plan)?;
    let bytes = binary::encode(&plan, true)?;
    let limit = u64::from(host.max_plan_bytes.min(MAX_PLAN_BYTES));
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(TemplateError::Limit {
            what: "plan",
            limit,
        });
    }
    let sections = host.max_sections.min(MAX_SECTIONS);
    if sections < 7 {
        return Err(TemplateError::Limit {
            what: "sections",
            limit: u64::from(sections),
        });
    }
    Ok(plan)
}

enum PlanBuild {
    Ready(Box<ContextPlan>),
    V1(Template),
}

fn version_decl_plan(source: &str, host: &HostCeiling) -> Result<PlanBuild, TemplateError> {
    match crate::source::version_decl(source)? {
        VersionDecl::V1 => Ok(PlanBuild::V1(Template::parse(source))),
        VersionDecl::V2 => Ok(PlanBuild::Ready(Box::new(lower_v2(
            parse_v2(source)?,
            host,
        )?))),
    }
}

fn lower_v1(template: &Template, host: &HostCeiling) -> Result<ContextPlan, TemplateError> {
    if let Some(err) = template.validate().into_iter().next() {
        return Err(map_v1(err));
    }
    require_cap(host, "mode:pack")?;
    require_cap(host, "parser:opaque")?;
    let plugins = crate::plugin_plan::distribution_plan(host)?;
    let mut capabilities = CAP_PACK | CAP_OPAQUE;
    if plugins
        .plugins
        .iter()
        .any(|plugin| plugin.state == PluginState::Enabled)
    {
        capabilities |= CAP_PLUGIN_HOST;
    }
    let partials = vec![
        (
            "TREE_ENTRY".to_string(),
            replace_vars(&template.partial_tree_entry, &template.variables),
        ),
        (
            "FILE_BLOCK".to_string(),
            replace_vars(&template.partial_file_block, &template.variables),
        ),
    ];
    let layouts = vec![(
        "main".to_string(),
        replace_vars(&template.layout, &template.variables),
    )];
    let render = limited_program(
        host,
        compile_program(&template.variables, &partials, &layouts)?,
    )?;
    Ok(ContextPlan {
        source_version: 1,
        capabilities,
        selections: Vec::new(),
        indexes: vec![IndexPlan {
            name: "pack".to_string(),
            facts: Vec::new(),
            parser: "opaque".to_string(),
            language: String::new(),
            dialect: String::new(),
            max_bytes: host.max_parse_memory,
        }],
        budgets: vec![BudgetPlan {
            name: "pack".to_string(),
            bytes: host.max_bytes,
            estimated_tokens: host
                .max_estimated_tokens
                .min(host.max_hard_estimated_tokens),
            hard_estimated_tokens: host.max_hard_estimated_tokens,
            records: host.max_records,
            depth: host.max_depth,
            parse_time_ms: host.max_parse_time_ms,
            parse_memory: host.max_parse_memory,
            work: host.max_work,
        }],
        views: vec![ViewPlan {
            name: "pack".to_string(),
            mode: "pack".to_string(),
            selection: String::new(),
            index: "pack".to_string(),
            budget: "pack".to_string(),
            render: "main".to_string(),
            layout_label: "SNAPSHOT".to_string(),
        }],
        render,
        plugins,
        selection_digest: [0; 32],
        extraction_digest: [0; 32],
        render_digest: [0; 32],
        plugin_digest: [0; 32],
        plan_digest: [0; 32],
    })
}

fn lower_v2(mut draft: Draft, host: &HostCeiling) -> Result<ContextPlan, TemplateError> {
    let sections = host.max_sections.min(MAX_SECTIONS);
    if sections < 7 {
        return Err(TemplateError::Limit {
            what: "sections",
            limit: u64::from(sections),
        });
    }
    let mut selections = std::mem::take(&mut draft.selections);
    unique(
        selections.iter().map(|item| item.name.as_str()),
        "selection",
    )?;
    for selection in &mut selections {
        check_selection(selection)?;
    }
    selections.sort_by(|left, right| left.name.cmp(&right.name));

    let mut indexes = Vec::new();
    unique(draft.indexes.iter().map(|item| item.name.as_str()), "index")?;
    let mut capabilities = 0u64;
    for index in &draft.indexes {
        indexes.push(check_index(index, host, &mut capabilities)?);
    }
    indexes.sort_by(|left, right| left.name.cmp(&right.name));

    let mut budgets = Vec::new();
    unique(
        draft.budgets.iter().map(|item| item.name.as_str()),
        "budget",
    )?;
    for budget in &draft.budgets {
        budgets.push(check_budget(budget, host)?);
    }
    budgets.sort_by(|left, right| left.name.cmp(&right.name));

    let render = lower_render(&draft, host)?;
    let plugins = crate::plugin_plan::compile_plugins(&draft.groups, &draft.plugins, host)?;
    if plugins
        .plugins
        .iter()
        .any(|plugin| plugin.state == PluginState::Enabled)
    {
        capabilities |= CAP_PLUGIN_HOST;
    }
    let views = lower_views(
        &draft.views,
        &selections,
        &indexes,
        &budgets,
        &render,
        host,
        &mut capabilities,
    )?;

    Ok(ContextPlan {
        source_version: 2,
        capabilities,
        selections,
        indexes,
        budgets,
        views,
        render,
        plugins,
        selection_digest: [0; 32],
        extraction_digest: [0; 32],
        render_digest: [0; 32],
        plugin_digest: [0; 32],
        plan_digest: [0; 32],
    })
}

fn check_selection(selection: &Selection) -> Result<(), TemplateError> {
    if selection.rules.len() > 10_000 || selection.roots.len() > 1024 {
        return Err(TemplateError::Limit {
            what: "rules",
            limit: 10_000,
        });
    }
    for root in &selection.roots {
        check_relative(root, false)?;
    }
    for language in &selection.languages {
        known(language, KNOWN_LANGUAGES, "language")?;
    }
    for kind in &selection.kinds {
        known(kind, KNOWN_KINDS, "kind")?;
    }
    for rule in &selection.rules {
        check_relative(&rule.pattern, true)?;
    }
    Ok(())
}

fn check_index(
    index: &IndexDraft,
    host: &HostCeiling,
    caps: &mut u64,
) -> Result<IndexPlan, TemplateError> {
    let parser = index
        .parser
        .clone()
        .ok_or_else(|| TemplateError::InvalidSyntax {
            line: 0,
            detail: "index parser".into(),
        })?;
    let (name, bit) = parser_cap(&parser)?;
    require_cap(host, name)?;
    *caps |= bit;
    for fact in &index.facts {
        known(fact, KNOWN_FACTS, "fact")?;
    }
    if !index.language.is_empty() {
        known(&index.language, KNOWN_LANGUAGES, "language")?;
    }
    let max_bytes = match index.max_bytes {
        None => host.max_parse_memory,
        Some(value) => ceiling_u64(value, host.max_parse_memory, "index.max_bytes")?,
    };
    Ok(IndexPlan {
        name: index.name.clone(),
        facts: index.facts.clone(),
        parser,
        language: index.language.clone(),
        dialect: index.dialect.clone(),
        max_bytes,
    })
}

fn check_budget(budget: &BudgetDraft, host: &HostCeiling) -> Result<BudgetPlan, TemplateError> {
    let hard = optional_u64(
        budget.hard_estimated_tokens,
        host.max_hard_estimated_tokens,
        "hard_estimated_tokens",
    )?;
    let estimated_ceiling = host.max_estimated_tokens.min(hard);
    let estimated = optional_u64(
        budget.estimated_tokens,
        estimated_ceiling,
        "estimated_tokens",
    )?;
    let depth = match budget.depth {
        None => host.max_depth,
        Some(value) if value > host.max_depth => {
            return Err(TemplateError::BudgetAboveCeiling {
                field: "depth",
                requested: u64::from(value),
                ceiling: u64::from(host.max_depth),
            });
        }
        Some(value) => value,
    };
    Ok(BudgetPlan {
        name: budget.name.clone(),
        bytes: optional_u64(budget.bytes, host.max_bytes, "bytes")?,
        estimated_tokens: estimated,
        hard_estimated_tokens: hard,
        records: optional_u64(budget.records, host.max_records, "records")?,
        depth,
        parse_time_ms: optional_u64(
            budget.parse_time_ms,
            host.max_parse_time_ms,
            "parse_time_ms",
        )?,
        parse_memory: optional_u64(budget.parse_memory, host.max_parse_memory, "parse_memory")?,
        work: optional_u64(budget.work, host.max_work, "work")?,
    })
}

fn lower_render(draft: &Draft, host: &HostCeiling) -> Result<RenderProgram, TemplateError> {
    let includes = include_names(&draft.template, &draft.partials, &draft.layouts);
    let mut errors = draft.template.validate();
    errors.retain(|err| match err {
        TemplateValidationError::EmptySection(_) => false,
        TemplateValidationError::UnresolvedVariable(name) => !includes.contains(name),
        _ => true,
    });
    if let Some(err) = errors.into_iter().next() {
        return Err(map_v1(err));
    }
    let names: BTreeSet<&str> = draft
        .partials
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    for (_, body) in draft.partials.iter().chain(draft.layouts.iter()) {
        for target in include_targets(body) {
            if !names.contains(target.as_str()) {
                return Err(TemplateError::UnresolvedRef {
                    kind: "partial",
                    name: target,
                });
            }
        }
    }
    partial_cycles(&draft.partials)?;
    let program = compile_program(&draft.template.variables, &draft.partials, &draft.layouts)?;
    limited_program(host, program)
}

fn lower_views(
    drafts: &[ViewDraft],
    selections: &[Selection],
    indexes: &[IndexPlan],
    budgets: &[BudgetPlan],
    render: &RenderProgram,
    host: &HostCeiling,
    caps: &mut u64,
) -> Result<Vec<ViewPlan>, TemplateError> {
    unique(drafts.iter().map(|view| view.name.as_str()), "view")?;
    let mut views = Vec::new();
    for view in drafts {
        let mode = match &view.mode {
            Some(mode) => mode.clone(),
            None if view.name == "pack" || view.name == "map" => view.name.clone(),
            None => {
                return Err(TemplateError::InvalidSyntax {
                    line: 0,
                    detail: "view mode".into(),
                });
            }
        };
        let (cap_name, bit) = mode_cap(&mode);
        require_cap(host, &cap_name)?;
        *caps |= bit;
        let selection = view
            .selection
            .clone()
            .ok_or_else(|| TemplateError::UnresolvedRef {
                kind: "selection",
                name: view.name.clone(),
            })?;
        if !selections.iter().any(|item| item.name == selection) {
            return Err(TemplateError::UnresolvedRef {
                kind: "selection",
                name: selection,
            });
        }
        let budget = view
            .budget
            .clone()
            .ok_or_else(|| TemplateError::UnresolvedRef {
                kind: "budget",
                name: view.name.clone(),
            })?;
        if !budgets.iter().any(|item| item.name == budget) {
            return Err(TemplateError::UnresolvedRef {
                kind: "budget",
                name: budget,
            });
        }
        let index = match &view.index {
            Some(name) => name.clone(),
            None if indexes.iter().any(|item| item.name == selection) => selection.clone(),
            None if indexes.len() == 1 => indexes[0].name.clone(),
            None => String::new(),
        };
        if !index.is_empty() && !indexes.iter().any(|item| item.name == index) {
            return Err(TemplateError::UnresolvedRef {
                kind: "index",
                name: index,
            });
        }
        let render_name = match &view.render {
            Some(name) => name.clone(),
            None if render.layouts.iter().any(|layout| layout.name == "main") => "main".to_string(),
            None => String::new(),
        };
        if !render_name.is_empty()
            && !render
                .layouts
                .iter()
                .any(|layout| layout.name == render_name)
        {
            return Err(TemplateError::UnresolvedRef {
                kind: "render",
                name: render_name,
            });
        }
        let layout_label = view.layout_label.clone().unwrap_or_else(|| {
            if render_name.is_empty() {
                "SNAPSHOT".to_string()
            } else {
                render_name.clone()
            }
        });
        views.push(ViewPlan {
            name: view.name.clone(),
            mode,
            selection,
            index,
            budget,
            render: render_name,
            layout_label,
        });
    }
    views.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(views)
}

fn partial_cycles(partials: &[(String, String)]) -> Result<(), TemplateError> {
    let edges: Vec<(String, Vec<String>)> = partials
        .iter()
        .map(|(name, body)| (name.clone(), include_targets(body)))
        .collect();
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for (name, _) in &edges {
        if let Some(name) = walk_partial(name, &edges, &mut visiting, &mut visited) {
            return Err(TemplateError::Cycle {
                kind: "partial",
                name,
            });
        }
    }
    Ok(())
}

fn walk_partial(
    name: &str,
    edges: &[(String, Vec<String>)],
    visiting: &mut BTreeSet<String>,
    visited: &mut BTreeSet<String>,
) -> Option<String> {
    if visited.contains(name) {
        return None;
    }
    if !visiting.insert(name.to_string()) {
        return Some(name.to_string());
    }
    if let Some((_, deps)) = edges.iter().find(|(item, _)| item == name) {
        for dep in deps {
            if let Some(found) = walk_partial(dep, edges, visiting, visited) {
                return Some(found);
            }
        }
    }
    visiting.remove(name);
    visited.insert(name.to_string());
    None
}

fn limited_program(
    host: &HostCeiling,
    program: RenderProgram,
) -> Result<RenderProgram, TemplateError> {
    let limit = u64::from(host.max_instructions.min(MAX_INSTRUCTIONS));
    if u64::try_from(program.instruction_count()).unwrap_or(u64::MAX) > limit {
        return Err(TemplateError::Limit {
            what: "instructions",
            limit,
        });
    }
    Ok(program)
}

fn optional_u64(
    value: Option<u64>,
    ceiling: u64,
    field: &'static str,
) -> Result<u64, TemplateError> {
    match value {
        None => Ok(ceiling),
        Some(value) => ceiling_u64(value, ceiling, field),
    }
}

fn ceiling_u64(value: u64, ceiling: u64, field: &'static str) -> Result<u64, TemplateError> {
    if value > ceiling {
        Err(TemplateError::BudgetAboveCeiling {
            field,
            requested: value,
            ceiling,
        })
    } else {
        Ok(value)
    }
}

fn require_cap(host: &HostCeiling, name: &str) -> Result<(), TemplateError> {
    if host.capabilities.contains(name) {
        Ok(())
    } else {
        Err(TemplateError::UnknownCapability {
            name: name.to_string(),
        })
    }
}

fn known(value: &str, allowed: &[&str], kind: &str) -> Result<(), TemplateError> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(TemplateError::UnknownCapability {
            name: format!("{kind}:{value}"),
        })
    }
}

fn parser_cap(parser: &str) -> Result<(&'static str, u64), TemplateError> {
    match parser {
        "syn" => Ok(("parser:syn", crate::model::CAP_SYN)),
        "structural" => Ok(("parser:structural", crate::model::CAP_STRUCTURAL)),
        "syntax" => Ok(("parser:syntax", crate::model::CAP_SYNTAX)),
        "opaque" => Ok(("parser:opaque", CAP_OPAQUE)),
        "tree-sitter" => Ok(("parser:tree-sitter", crate::model::CAP_TREE_SITTER)),
        other => Err(TemplateError::UnknownCapability {
            name: format!("parser:{other}"),
        }),
    }
}

fn mode_cap(mode: &str) -> (String, u64) {
    match mode {
        "pack" => ("mode:pack".to_string(), CAP_PACK),
        "map" => ("mode:map".to_string(), crate::model::CAP_MAP),
        other => (format!("mode:{other}"), 0),
    }
}

fn unique<'a>(
    names: impl Iterator<Item = &'a str>,
    kind: &'static str,
) -> Result<(), TemplateError> {
    let mut seen = BTreeSet::new();
    for name in names {
        if !seen.insert(name) {
            return Err(TemplateError::Duplicate {
                kind,
                name: name.to_string(),
            });
        }
    }
    Ok(())
}

fn check_relative(path: &str, allow_glob: bool) -> Result<(), TemplateError> {
    let bad = || TemplateError::InvalidSyntax {
        line: 0,
        detail: "path".into(),
    };
    if path.is_empty()
        || path.len() > 4096
        || path.contains('\0')
        || path.contains('\\')
        || path.starts_with('/')
    {
        return Err(bad());
    }
    if !allow_glob && (path.contains('*') || path.contains('?')) {
        return Err(TemplateError::InvalidSyntax {
            line: 0,
            detail: "path glob".into(),
        });
    }
    if path
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(bad());
    }
    Ok(())
}

fn map_v1(err: TemplateValidationError) -> TemplateError {
    match err {
        TemplateValidationError::MissingSection(name) => TemplateError::MissingSection { name },
        TemplateValidationError::EmptySection(name) => TemplateError::EmptySection { name },
        TemplateValidationError::UnresolvedVariable(name) => {
            TemplateError::UnresolvedVariable { name }
        }
        TemplateValidationError::InvalidSyntax(detail) => {
            TemplateError::InvalidSyntax { line: 0, detail }
        }
        TemplateValidationError::Cycle(name) => TemplateError::Cycle {
            kind: "variable",
            name,
        },
    }
}
