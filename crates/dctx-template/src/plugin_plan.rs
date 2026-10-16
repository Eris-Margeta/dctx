//! Compile and reload a DTS plugin plan.
//! Implements PLUGINS.md §3.5: locks, ceilings, acyclic order, sticky disable.
//! Depends on [`crate::grant`], [`crate::model`] and [`crate::source`]. [`crate::lower`] calls it.
//! Configuration changes the plugin digest only. Map facts stay on the other sections.

use std::collections::{BTreeMap, BTreeSet};

use crate::error::TemplateError;
use crate::grant::{
    accept_config, canonical_id, capability_within, check_alias, expand_allow, known_hook, organization_name,
    parse_digest, plain_text, require_semver, screen_selectors,
};
use crate::model::HostCeiling;
use crate::plugin::{
    Capability, ConfigEntry, ConfigValue, HookDecl, KnownPlugin, Liveness, NetworkPolicy, PluginBinding,
    PluginGroup, PluginPlan, PluginState, Resolution, ResourceLimits, UnavailableReason,
};
use crate::source::{GroupDraft, PluginDecl, RawValue};

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

fn bind_decl(
    decl: &PluginDecl,
    host: &HostCeiling,
    enabled: Option<&BTreeSet<String>>,
    aliases: &mut BTreeSet<String>,
) -> Result<PluginBinding, TemplateError> {
    if !canonical_id(&decl.id) {
        return Err(TemplateError::InvalidSyntax { line: 0, detail: "plugin id".into() });
    }
    screen_hooks(&decl.hooks)?;
    bounded(&decl.before, "before", 64)?;
    bounded(&decl.after, "after", 64)?;
    no_self(&decl.id, &decl.before)?;
    no_self(&decl.id, &decl.after)?;
    screen_selectors(&decl.allow)?;
    if let Some(version) = &decl.version {
        require_semver(version)?;
    }
    if let Some(alias) = &decl.alias {
        check_alias(alias)?;
    }
    let Some(catalog) = host.plugins.iter().find(|plugin| plugin.id == decl.id) else {
        if decl.required {
            return Err(TemplateError::RequiredPluginMissing { id: decl.id.clone() });
        }
        return Ok(unavailable(&decl.id, UnavailableReason::NotInstalled));
    };
    let version = decl.version.clone().ok_or_else(|| TemplateError::InvalidSyntax {
        line: 0,
        detail: "plugin version".into(),
    })?;
    require_semver(&catalog.version)?;
    if version != catalog.version {
        return Err(TemplateError::InvalidSyntax { line: 0, detail: "plugin version".into() });
    }
    confirm_digest(decl.digest.as_deref(), catalog.digest)?;
    let hooks = hooks_of(&decl.hooks, catalog)?;
    let denied = denied_set(host, &decl.id);
    let allow = expand_allow(&decl.allow, catalog, &denied)?;
    let user = host.user_config.get(&decl.id).map(Vec::as_slice).unwrap_or(&[]);
    let config = merge_config(&decl.id, &catalog.fields, &decl.config, user)?;
    let alias = decl.alias.clone().unwrap_or_else(|| catalog.alias.clone());
    claim_alias(&alias, aliases)?;
    let listed = enabled.map(|set| set.contains(&decl.id)).unwrap_or(true);
    if host.disabled_plugins.contains(&decl.id) {
        if decl.required {
            return Err(TemplateError::UserDisabled { id: decl.id.clone() });
        }
        return Ok(locked(catalog, &version, alias, PluginState::Disabled, decl.required, &decl.before, &decl.after));
    }
    if !listed {
        if decl.required {
            return Err(TemplateError::InvalidSyntax { line: 0, detail: "required plugin is not enabled".into() });
        }
        return Ok(locked(catalog, &version, alias, PluginState::Disabled, decl.required, &decl.before, &decl.after));
    }
    Ok(active(
        catalog,
        &version,
        alias,
        decl.required,
        Effect { hooks, allow, config },
        &decl.before,
        &decl.after,
    ))
}

fn bind_catalog(
    catalog: &KnownPlugin,
    host: &HostCeiling,
    aliases: &mut BTreeSet<String>,
) -> Result<PluginBinding, TemplateError> {
    if !canonical_id(&catalog.id) {
        return Err(TemplateError::InvalidSyntax { line: 0, detail: "plugin id".into() });
    }
    require_semver(&catalog.version)?;
    check_alias(&catalog.alias)?;
    claim_alias(&catalog.alias, aliases)?;
    if host.disabled_plugins.contains(&catalog.id) {
        return Ok(locked(catalog, &catalog.version, catalog.alias.clone(), PluginState::Disabled, false, &[], &[]));
    }
    let denied = denied_set(host, &catalog.id);
    let selectors = granted_selectors(catalog, &denied);
    let allow = expand_allow(&selectors, catalog, &denied)?;
    let hooks = hooks_of(&catalog.hooks, catalog)?;
    let user = host.user_config.get(&catalog.id).map(Vec::as_slice).unwrap_or(&[]);
    let config = merge_config(&catalog.id, &catalog.fields, &[], user)?;
    Ok(active(catalog, &catalog.version, catalog.alias.clone(), false, Effect { hooks, allow, config }, &[], &[]))
}

fn check_binding(plugin: &PluginBinding, host: &HostCeiling, aliases: &mut BTreeSet<String>) -> Result<(), TemplateError> {
    if plugin.state != PluginState::Unavailable && !plugin.alias.is_empty() {
        check_alias(&plugin.alias)?;
        claim_alias(&plugin.alias, aliases)?;
    }
    match (plugin.state, plugin.resolution) {
        (PluginState::Unavailable, Resolution::Unavailable { .. }) => {
            if plugin.required || plugin.digest.is_some() || plugin.manifest_digest.is_some() || !plugin.allow.is_empty()
                || !plugin.hooks.is_empty() || !plugin.config.is_empty() || !plugin.version.is_empty()
            {
                return Err(TemplateError::GrantExpansion { kind: "plugin" });
            }
            if host.plugins.iter().any(|catalog| catalog.id == plugin.id) {
                return Err(TemplateError::InvalidSyntax { line: 0, detail: "plugin lock".into() });
            }
            Ok(())
        }
        (PluginState::Enabled, Resolution::Available) => {
            if host.disabled_plugins.contains(&plugin.id) {
                return Err(TemplateError::UserDisabled { id: plugin.id.clone() });
            }
            let catalog = catalog_of(host, &plugin.id)?;
            match_lock(plugin, catalog)?;
            let denied = denied_set(host, &plugin.id);
            for cap in &plugin.allow {
                if !capability_within(cap, catalog, &denied) {
                    return Err(TemplateError::GrantExpansion { kind: "plugin" });
                }
            }
            for hook in &plugin.hooks {
                if hook.version != 1 || !known_hook(&hook.name) || !catalog.hooks.iter().any(|name| name == &hook.name) {
                    return Err(TemplateError::UnknownCapability { name: format!("hook:{}", hook.name) });
                }
            }
            for entry in &plugin.config {
                let field = catalog.fields.iter().find(|field| field.name == entry.key).ok_or_else(|| {
                    TemplateError::UnknownField { plugin: plugin.id.clone(), field: entry.key.clone() }
                })?;
                accept_config(field, &entry.value)?;
            }
            Ok(())
        }
        (PluginState::Disabled, Resolution::Available) => {
            if !plugin.allow.is_empty() || !plugin.hooks.is_empty() || !plugin.config.is_empty() {
                return Err(TemplateError::GrantExpansion { kind: "plugin" });
            }
            let catalog = catalog_of(host, &plugin.id)?;
            match_lock(plugin, catalog)
        }
        _ => Err(TemplateError::Corrupt { reason: "plugin state" }),
    }
}

fn catalog_of<'a>(host: &'a HostCeiling, id: &str) -> Result<&'a KnownPlugin, TemplateError> {
    host.plugins.iter().find(|catalog| catalog.id == id).ok_or_else(|| TemplateError::UnresolvedRef {
        kind: "plugin",
        name: id.to_string(),
    })
}

fn match_lock(plugin: &PluginBinding, catalog: &KnownPlugin) -> Result<(), TemplateError> {
    require_semver(&plugin.version)?;
    if plugin.version != catalog.version {
        return Err(TemplateError::InvalidSyntax { line: 0, detail: "plugin version".into() });
    }
    if plugin.digest != catalog.digest || plugin.manifest_digest != catalog.manifest_digest {
        return Err(TemplateError::InvalidSyntax { line: 0, detail: "plugin digest".into() });
    }
    if !catalog.publisher_key.is_empty() && plugin.publisher_key != catalog.publisher_key {
        return Err(TemplateError::GrantExpansion { kind: "plugin" });
    }
    if plugin.origin != Some(catalog.origin) {
        return Err(TemplateError::GrantExpansion { kind: "plugin" });
    }
    Ok(())
}

fn confirm_digest(source: Option<&str>, locked: Option<[u8; 32]>) -> Result<(), TemplateError> {
    match (source, locked) {
        (None, _) => Ok(()),
        (Some(text), Some(pinned)) => {
            if parse_digest(text)? != pinned {
                Err(TemplateError::InvalidSyntax { line: 0, detail: "plugin digest mismatch".into() })
            } else {
                Ok(())
            }
        }
        (Some(_), None) => Err(TemplateError::InvalidSyntax { line: 0, detail: "plugin digest".into() }),
    }
}

fn merge_config(
    id: &str,
    fields: &[crate::plugin::FieldSpec],
    template: &[(String, RawValue)],
    user: &[ConfigEntry],
) -> Result<Vec<ConfigEntry>, TemplateError> {
    for (key, _) in template {
        if !fields.iter().any(|field| field.name == *key) {
            return Err(TemplateError::UnknownField { plugin: id.to_string(), field: key.clone() });
        }
    }
    for entry in user {
        if !fields.iter().any(|field| field.name == entry.key) {
            return Err(TemplateError::UnknownField { plugin: id.to_string(), field: entry.key.clone() });
        }
    }
    let mut ordered: Vec<&crate::plugin::FieldSpec> = fields.iter().collect();
    ordered.sort_by(|left, right| left.name.cmp(&right.name));
    let mut config = Vec::new();
    for field in ordered {
        let chosen = if let Some((_, raw)) = template.iter().find(|(key, _)| key == &field.name) {
            Some(accept_config(field, &raw_config(raw)?)?)
        } else if let Some(entry) = user.iter().find(|entry| entry.key == field.name) {
            Some(accept_config(field, &entry.value)?)
        } else if let Some(default) = &field.default_value {
            Some(accept_config(field, default)?)
        } else {
            None
        };
        if let Some(value) = chosen {
            config.push(ConfigEntry { key: field.name.clone(), value });
        }
    }
    Ok(config)
}

fn raw_config(raw: &RawValue) -> Result<ConfigValue, TemplateError> {
    match raw {
        RawValue::Bool(value) => Ok(ConfigValue::Bool(*value)),
        RawValue::Int(value) => Ok(ConfigValue::Int(*value)),
        RawValue::Str(text) => {
            plain_text(text)?;
            Ok(ConfigValue::Str(text.clone()))
        }
        RawValue::List(items) => {
            for item in items {
                plain_text(item)?;
            }
            Ok(ConfigValue::List(items.clone()))
        }
    }
}

fn hooks_of(names: &[String], catalog: &KnownPlugin) -> Result<Vec<HookDecl>, TemplateError> {
    let mut hooks = Vec::with_capacity(names.len());
    for name in names {
        if !catalog.hooks.iter().any(|hook| hook == name) {
            return Err(TemplateError::UnknownCapability { name: format!("hook:{name}") });
        }
        hooks.push(HookDecl { name: name.clone(), version: 1 });
    }
    Ok(hooks)
}

fn screen_hooks(names: &[String]) -> Result<(), TemplateError> {
    if names.len() > 5 {
        return Err(TemplateError::Limit { what: "hooks", limit: 5 });
    }
    let mut seen = BTreeSet::new();
    for name in names {
        if !seen.insert(name.as_str()) {
            return Err(TemplateError::Duplicate { kind: "hook", name: name.clone() });
        }
        plain_text(name)?;
        if !known_hook(name) {
            return Err(TemplateError::UnknownCapability { name: format!("hook:{name}") });
        }
    }
    Ok(())
}

fn granted_selectors(catalog: &KnownPlugin, denied: &BTreeSet<&str>) -> Vec<String> {
    let mut items: Vec<String> =
        catalog.capabilities.iter().filter(|item| !denied.contains(item.as_str())).cloned().collect();
    let program = items.iter().any(|item| item == "program:git.add-ignore");
    let resource = items.iter().any(|item| item == "resource:git-admin:stage-ignore");
    if program && !resource {
        items.retain(|item| item != "program:git.add-ignore");
    }
    if resource && !program {
        items.retain(|item| item != "resource:git-admin:stage-ignore");
    }
    items
}

fn denied_set<'a>(host: &'a HostCeiling, id: &str) -> BTreeSet<&'a str> {
    let mut denied = BTreeSet::new();
    if let Some(items) = host.denied_capabilities.get(id) {
        denied.extend(items.iter().map(String::as_str));
    }
    if let Some(items) = host.denied_capabilities.get("*") {
        denied.extend(items.iter().map(String::as_str));
    }
    denied
}

fn claim_alias(alias: &str, aliases: &mut BTreeSet<String>) -> Result<(), TemplateError> {
    check_alias(alias)?;
    if !alias.is_empty() && !aliases.insert(alias.to_string()) {
        return Err(TemplateError::Duplicate { kind: "alias", name: alias.to_string() });
    }
    Ok(())
}

fn unavailable(id: &str, reason: UnavailableReason) -> PluginBinding {
    PluginBinding {
        id: id.to_string(),
        version: String::new(),
        digest: None,
        manifest_digest: None,
        publisher_key: String::new(),
        origin: None,
        resolution: Resolution::Unavailable { reason },
        required: false,
        state: PluginState::Unavailable,
        alias: String::new(),
        hooks: Vec::new(),
        allow: Vec::new(),
        config: Vec::new(),
        before: Vec::new(),
        after: Vec::new(),
    }
}

fn locked(
    catalog: &KnownPlugin,
    version: &str,
    alias: String,
    state: PluginState,
    required: bool,
    before: &[String],
    after: &[String],
) -> PluginBinding {
    PluginBinding {
        id: catalog.id.clone(),
        version: version.to_string(),
        digest: catalog.digest,
        manifest_digest: catalog.manifest_digest,
        publisher_key: catalog.publisher_key.clone(),
        origin: Some(catalog.origin),
        resolution: Resolution::Available,
        required,
        state,
        alias,
        hooks: Vec::new(),
        allow: Vec::new(),
        config: Vec::new(),
        before: before.to_vec(),
        after: after.to_vec(),
    }
}

struct Effect {
    hooks: Vec<HookDecl>,
    allow: Vec<Capability>,
    config: Vec<ConfigEntry>,
}

fn active(
    catalog: &KnownPlugin,
    version: &str,
    alias: String,
    required: bool,
    effect: Effect,
    before: &[String],
    after: &[String],
) -> PluginBinding {
    let mut binding = locked(catalog, version, alias, PluginState::Enabled, required, before, after);
    binding.hooks = effect.hooks;
    binding.allow = effect.allow;
    binding.config = effect.config;
    binding
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

fn bounded(items: &[String], kind: &'static str, limit: u64) -> Result<(), TemplateError> {
    let len = u64::try_from(items.len()).unwrap_or(u64::MAX);
    if len > limit {
        return Err(TemplateError::Limit { what: kind, limit });
    }
    let mut seen = BTreeSet::new();
    for item in items {
        if !seen.insert(item.as_str()) {
            return Err(TemplateError::Duplicate { kind, name: item.clone() });
        }
    }
    Ok(())
}

fn no_self(id: &str, items: &[String]) -> Result<(), TemplateError> {
    if items.iter().any(|item| item == id) {
        Err(TemplateError::Cycle { kind: "plugin", name: id.to_string() })
    } else {
        Ok(())
    }
}

fn schedule(groups: &[PluginGroup], plugins: &[PluginBinding]) -> Result<Vec<String>, TemplateError> {
    let ids: Vec<String> =
        plugins.iter().filter(|plugin| plugin.state == PluginState::Enabled).map(|plugin| plugin.id.clone()).collect();
    let mut edges = Vec::new();
    for group in groups {
        for pair in group.order.windows(2) {
            if let (Some(before), Some(after)) = (participant(plugins, &pair[0])?, participant(plugins, &pair[1])?) {
                edges.push((before.to_string(), after.to_string()));
            }
        }
    }
    for plugin in plugins.iter().filter(|plugin| plugin.state == PluginState::Enabled) {
        for before in &plugin.before {
            if let Some(id) = participant(plugins, before)? {
                edges.push((id.to_string(), plugin.id.clone()));
            }
        }
        for after in &plugin.after {
            if let Some(id) = participant(plugins, after)? {
                edges.push((plugin.id.clone(), id.to_string()));
            }
        }
    }
    topo(&ids, &edges)
}

fn participant<'a>(plugins: &'a [PluginBinding], id: &str) -> Result<Option<&'a str>, TemplateError> {
    let Some(plugin) = plugins.iter().find(|plugin| plugin.id == id) else {
        return Err(TemplateError::UnresolvedRef { kind: "plugin", name: id.to_string() });
    };
    if plugin.state == PluginState::Enabled {
        Ok(Some(plugin.id.as_str()))
    } else {
        Ok(None)
    }
}

fn topo(ids: &[String], edges: &[(String, String)]) -> Result<Vec<String>, TemplateError> {
    let mut indeg: BTreeMap<String, usize> = ids.iter().map(|id| (id.clone(), 0)).collect();
    let mut outgoing: BTreeMap<String, BTreeSet<String>> = ids.iter().map(|id| (id.clone(), BTreeSet::new())).collect();
    for (before, after) in edges {
        if before == after {
            return Err(TemplateError::Cycle { kind: "plugin", name: before.clone() });
        }
        let Some(next) = outgoing.get_mut(before) else {
            return Err(TemplateError::UnresolvedRef { kind: "plugin", name: before.clone() });
        };
        if next.insert(after.clone()) {
            let Some(degree) = indeg.get_mut(after) else {
                return Err(TemplateError::UnresolvedRef { kind: "plugin", name: after.clone() });
            };
            *degree += 1;
        }
    }
    let mut ready: BTreeSet<String> = indeg.iter().filter(|(_, degree)| **degree == 0).map(|(id, _)| id.clone()).collect();
    let mut order = Vec::new();
    while let Some(id) = ready.iter().next().cloned() {
        ready.remove(&id);
        order.push(id.clone());
        let nexts = outgoing.get(&id).cloned().unwrap_or_default();
        for next in nexts {
            if let Some(degree) = indeg.get_mut(&next) {
                *degree = degree.saturating_sub(1);
                if *degree == 0 {
                    ready.insert(next);
                }
            }
        }
    }
    if order.len() != ids.len() {
        let name = ids.iter().find(|id| !order.contains(*id)).cloned().unwrap_or_default();
        return Err(TemplateError::Cycle { kind: "plugin", name });
    }
    Ok(order)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::PluginOrigin;

    fn enabled(id: &str) -> PluginBinding {
        PluginBinding {
            id: id.to_string(),
            version: "1.0.0".to_string(),
            digest: None,
            manifest_digest: None,
            publisher_key: String::new(),
            origin: Some(PluginOrigin::Installed),
            resolution: Resolution::Available,
            required: false,
            state: PluginState::Enabled,
            alias: String::new(),
            hooks: Vec::new(),
            allow: Vec::new(),
            config: Vec::new(),
            before: Vec::new(),
            after: Vec::new(),
        }
    }

    #[test]
    fn tie_break_uses_canonical_id_and_a_cycle_is_refused() {
        let mut first = enabled("ex/b");
        let second = enabled("ex/a");
        let order = schedule(&[], &[first.clone(), second.clone()]).expect("tie");
        assert_eq!(order, vec!["ex/a".to_string(), "ex/b".to_string()]);
        first.after = vec!["ex/a".to_string()];
        let forced = schedule(&[], &[first.clone(), second]).expect("after");
        assert_eq!(forced, vec!["ex/b".to_string(), "ex/a".to_string()]);
        let mut looped = enabled("ex/a");
        looped.before = vec!["ex/a".to_string()];
        assert!(matches!(schedule(&[], &[looped]), Err(TemplateError::Cycle { kind: "plugin", .. })));
    }
}
