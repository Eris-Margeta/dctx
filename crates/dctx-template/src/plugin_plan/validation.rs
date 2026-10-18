//! Plugin locks, hooks, configuration and deny validation.
//! Implements PLUGINS.md §3.5: locks, ceilings and sticky disable.
//! Depends on grant rules, host catalogs and source values; used by the plugin plan compiler and loader.

use std::collections::BTreeSet;

use crate::error::TemplateError;
use crate::grant::{
    accept_config, capability_within, check_alias, known_hook, parse_digest, plain_text,
    require_semver,
};
use crate::model::HostCeiling;
use crate::plugin::{ConfigEntry, ConfigValue, HookDecl, KnownPlugin, PluginBinding, PluginState, Resolution};
use crate::source::RawValue;

pub(super) fn check_binding(plugin: &PluginBinding, host: &HostCeiling, aliases: &mut BTreeSet<String>) -> Result<(), TemplateError> {
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

pub(super) fn catalog_of<'a>(host: &'a HostCeiling, id: &str) -> Result<&'a KnownPlugin, TemplateError> {
    host.plugins.iter().find(|catalog| catalog.id == id).ok_or_else(|| TemplateError::UnresolvedRef {
        kind: "plugin",
        name: id.to_string(),
    })
}

pub(super) fn match_lock(plugin: &PluginBinding, catalog: &KnownPlugin) -> Result<(), TemplateError> {
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

pub(super) fn confirm_digest(source: Option<&str>, locked: Option<[u8; 32]>) -> Result<(), TemplateError> {
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

pub(super) fn merge_config(
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

pub(super) fn raw_config(raw: &RawValue) -> Result<ConfigValue, TemplateError> {
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

pub(super) fn hooks_of(names: &[String], catalog: &KnownPlugin) -> Result<Vec<HookDecl>, TemplateError> {
    let mut hooks = Vec::with_capacity(names.len());
    for name in names {
        if !catalog.hooks.iter().any(|hook| hook == name) {
            return Err(TemplateError::UnknownCapability { name: format!("hook:{name}") });
        }
        hooks.push(HookDecl { name: name.clone(), version: 1 });
    }
    Ok(hooks)
}

pub(super) fn screen_hooks(names: &[String]) -> Result<(), TemplateError> {
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

pub(super) fn granted_selectors(catalog: &KnownPlugin, denied: &BTreeSet<&str>) -> Vec<String> {
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

pub(super) fn denied_set<'a>(host: &'a HostCeiling, id: &str) -> BTreeSet<&'a str> {
    let mut denied = BTreeSet::new();
    if let Some(items) = host.denied_capabilities.get(id) {
        denied.extend(items.iter().map(String::as_str));
    }
    if let Some(items) = host.denied_capabilities.get("*") {
        denied.extend(items.iter().map(String::as_str));
    }
    denied
}

pub(super) fn claim_alias(alias: &str, aliases: &mut BTreeSet<String>) -> Result<(), TemplateError> {
    check_alias(alias)?;
    if !alias.is_empty() && !aliases.insert(alias.to_string()) {
        return Err(TemplateError::Duplicate { kind: "alias", name: alias.to_string() });
    }
    Ok(())
}

pub(super) fn bounded(items: &[String], kind: &'static str, limit: u64) -> Result<(), TemplateError> {
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

pub(super) fn no_self(id: &str, items: &[String]) -> Result<(), TemplateError> {
    if items.iter().any(|item| item == id) {
        Err(TemplateError::Cycle { kind: "plugin", name: id.to_string() })
    } else {
        Ok(())
    }
}

