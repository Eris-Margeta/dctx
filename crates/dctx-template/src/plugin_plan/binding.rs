// SPDX-License-Identifier: Apache-2.0
//! Binding source declarations and distribution catalog entries.
//! Implements PLUGINS.md §3.5: locks, ceilings and sticky disable.
//! Depends on host catalogs, grant admission and binding validation; used by the plugin plan compiler and loader.

use std::collections::BTreeSet;

use crate::error::TemplateError;
use crate::grant::{canonical_id, check_alias, expand_allow, require_semver, screen_selectors};
use crate::model::HostCeiling;
use crate::plugin::{
    Capability, ConfigEntry, HookDecl, KnownPlugin, PluginBinding, PluginState, Resolution,
    UnavailableReason,
};
use crate::source::PluginDecl;

use super::validation::{
    bounded, claim_alias, confirm_digest, denied_set, granted_selectors, hooks_of, merge_config,
    no_self, screen_hooks,
};

pub(super) fn bind_decl(
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
    let user = host.user_config.get(&decl.id).map_or(&[][..], Vec::as_slice);
    let config = merge_config(&decl.id, &catalog.fields, &decl.config, user)?;
    let alias = decl.alias.clone().unwrap_or_else(|| catalog.alias.clone());
    claim_alias(&alias, aliases)?;
    let listed = enabled.is_none_or(|set| set.contains(&decl.id));
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

pub(super) fn bind_catalog(
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
    let user = host.user_config.get(&catalog.id).map_or(&[][..], Vec::as_slice);
    let config = merge_config(&catalog.id, &catalog.fields, &[], user)?;
    Ok(active(catalog, &catalog.version, catalog.alias.clone(), false, Effect { hooks, allow, config }, &[], &[]))
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

