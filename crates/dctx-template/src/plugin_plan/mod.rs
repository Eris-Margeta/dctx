// SPDX-License-Identifier: Apache-2.0
//! Compile and reload a DTS plugin plan.
//! Implements PLUGINS.md §3.5: locks, ceilings, acyclic order, sticky disable.
//! Depends on [`crate::grant`], [`crate::model`] and [`crate::source`]. [`crate::lower`] calls it.
//! Configuration changes the plugin digest only. Map facts stay on the other sections.

use std::collections::BTreeSet;

use crate::error::TemplateError;
use crate::grant::{canonical_id, organization_name};
use crate::model::HostCeiling;
use crate::plugin::{Liveness, NetworkPolicy, PluginGroup, PluginPlan, ResourceLimits};
use crate::source::{GroupDraft, PluginDecl};

mod binding;
mod schedule;
mod validation;

use binding::{bind_catalog, bind_decl};
use schedule::schedule;
use validation::{bounded, check_binding};

const JS_MAX: u64 = 9_007_199_254_740_991;

/// Lowers v2 plugin sections. An empty document yields an empty plan with the host ceiling.
pub(crate) fn compile_plugins(
    groups: &[GroupDraft],
    decls: &[PluginDecl],
    host: &HostCeiling,
) -> Result<PluginPlan, TemplateError> {
    let limits = admit_limits(&host.limits)?;
    let liveness = admit_liveness(&host.liveness)?;
    if groups.len() > 1 {
        return Err(TemplateError::Duplicate { kind: "plugins", name: groups[1].name.clone() });
    }
    if decls.len() > 64 {
        return Err(TemplateError::Limit { what: "plugins", limit: 64 });
    }
    unique_ids(decls)?;
    let mut lowered = Vec::new();
    let mut enabled = None;
    if let Some(group) = groups.first() {
        if !organization_name(&group.name) {
            return Err(TemplateError::InvalidSyntax { line: 0, detail: "organization".into() });
        }
        bounded(&group.enabled, "enabled", 64)?;
        bounded(&group.order, "order", 64)?;
        let declared: BTreeSet<&str> = decls.iter().map(|decl| decl.id.as_str()).collect();
        for id in group.enabled.iter().chain(group.order.iter()) {
            if !declared.contains(id.as_str()) {
                return Err(TemplateError::UnresolvedRef { kind: "plugin", name: id.clone() });
            }
        }
        lowered.push(PluginGroup {
            name: group.name.clone(),
            network: network_of(group.network.as_deref())?,
            enabled: group.enabled.clone(),
            order: group.order.clone(),
        });
        enabled = Some(group.enabled.iter().cloned().collect::<BTreeSet<_>>());
    }
    let mut aliases = BTreeSet::new();
    let mut bindings = Vec::with_capacity(decls.len());
    for decl in decls {
        bindings.push(bind_decl(decl, host, enabled.as_ref(), &mut aliases)?);
    }
    bindings.sort_by(|left, right| left.id.cmp(&right.id));
    let order = schedule(&lowered, &bindings)?;
    let organization = lowered.first().map(|group| group.name.clone()).unwrap_or_default();
    Ok(PluginPlan { organization, groups: lowered, plugins: bindings, order, limits, liveness })
}

/// V1 distribution plan: host catalog filtered by user disable and denied grants.
pub(crate) fn distribution_plan(host: &HostCeiling) -> Result<PluginPlan, TemplateError> {
    let limits = admit_limits(&host.limits)?;
    let liveness = admit_liveness(&host.liveness)?;
    let mut aliases = BTreeSet::new();
    let mut bindings = Vec::with_capacity(host.plugins.len());
    for catalog in &host.plugins {
        bindings.push(bind_catalog(catalog, host, &mut aliases)?);
    }
    bindings.sort_by(|left, right| left.id.cmp(&right.id));
    let order = schedule(&[], &bindings)?;
    Ok(PluginPlan {
        organization: String::new(),
        groups: Vec::new(),
        plugins: bindings,
        order,
        limits,
        liveness,
    })
}

/// Reloads locks, aliases, order and ceilings. A byte blob is not authority by itself.
pub(crate) fn revalidate(plan: &crate::model::ContextPlan, host: &HostCeiling) -> Result<(), TemplateError> {
    let plugins = &plan.plugins;
    plugins.limits.within(&admit_limits(&host.limits)?)?;
    if plugins.liveness != admit_liveness(&host.liveness)? {
        return Err(TemplateError::GrantExpansion { kind: "liveness" });
    }
    if plugins.groups.len() > 1 {
        return Err(TemplateError::Duplicate { kind: "plugins", name: plugins.groups[1].name.clone() });
    }
    if !plugins.organization.is_empty() && !organization_name(&plugins.organization) {
        return Err(TemplateError::InvalidSyntax { line: 0, detail: "organization".into() });
    }
    for group in &plugins.groups {
        if group.network != NetworkPolicy::None {
            return Err(TemplateError::GrantExpansion { kind: "network" });
        }
    }
    let mut aliases = BTreeSet::new();
    for plugin in &plugins.plugins {
        if !canonical_id(&plugin.id) {
            return Err(TemplateError::InvalidSyntax { line: 0, detail: "plugin id".into() });
        }
        check_binding(plugin, host, &mut aliases)?;
    }
    let order = schedule(&plugins.groups, &plugins.plugins)?;
    if order != plugins.order {
        return Err(TemplateError::Corrupt { reason: "plugin order" });
    }
    Ok(())
}

fn network_of(text: Option<&str>) -> Result<NetworkPolicy, TemplateError> {
    match text {
        None | Some("none") => Ok(NetworkPolicy::None),
        Some(_) => Err(TemplateError::GrantExpansion { kind: "network" }),
    }
}

fn unique_ids(decls: &[PluginDecl]) -> Result<(), TemplateError> {
    let mut seen = BTreeSet::new();
    for decl in decls {
        if !seen.insert(decl.id.as_str()) {
            return Err(TemplateError::Duplicate { kind: "plugin", name: decl.id.clone() });
        }
    }
    Ok(())
}

fn admit_limits(host: &ResourceLimits) -> Result<ResourceLimits, TemplateError> {
    Ok(ResourceLimits {
        frame_bytes: take(host.frame_bytes, 1, 1_048_576, "frame_bytes")?,
        read_bytes: take(host.read_bytes, 0, JS_MAX, "read_bytes")?,
        actions: take(host.actions, 0, JS_MAX, "actions")?,
        outstanding_requests: take(host.outstanding_requests, 1, 16, "outstanding_requests")?,
        diagnostics_bytes: take(host.diagnostics_bytes, 0, 65_536, "diagnostics_bytes")?,
        chunk_bytes: take(host.chunk_bytes, 1, 65_536, "chunk_bytes")?,
        state_bytes: take(host.state_bytes, 0, JS_MAX, "state_bytes")?,
        upload_bytes: take(host.upload_bytes, 0, JS_MAX, "upload_bytes")?,
        queue_bytes: take(host.queue_bytes, 1, JS_MAX, "queue_bytes")?,
        memory_bytes: take(host.memory_bytes, 1, JS_MAX, "memory_bytes")?,
        work_units: take(host.work_units, 1, JS_MAX, "work_units")?,
    })
}

fn take(host: u64, min_value: u64, schema_max: u64, field: &'static str) -> Result<u64, TemplateError> {
    if host < min_value {
        return Err(TemplateError::BudgetAboveCeiling { field, requested: min_value, ceiling: host });
    }
    Ok(host.min(schema_max))
}

fn admit_liveness(policy: &Liveness) -> Result<Liveness, TemplateError> {
    let (sample, grace, extra) = match policy {
        Liveness::Interactive { sample_ms, cancel_grace_ms }
        | Liveness::UnattendedFailClosed { sample_ms, cancel_grace_ms } => (*sample_ms, *cancel_grace_ms, None),
        Liveness::UnattendedBounded { sample_ms, cancel_grace_ms, inactivity_ms } => {
            (*sample_ms, *cancel_grace_ms, Some(*inactivity_ms))
        }
    };
    if sample == 0 || grace == 0 || extra == Some(0) || sample > JS_MAX || grace > JS_MAX || extra.unwrap_or(1) > JS_MAX {
        return Err(TemplateError::InvalidSyntax { line: 0, detail: "liveness".into() });
    }
    Ok(policy.clone())
}

