// SPDX-License-Identifier: Apache-2.0
//! Binary plugin section of a `ContextPlan`.
//! Implements contracts/DTS-v2.md section payloads for the plugin extension.
//! Depends on [`crate::plugin`] and [`crate::error`]. [`crate::binary`] frames the bytes.
//! Field order is fixed so a reload reproduces the compiled value.

use crate::error::TemplateError;
use crate::plugin::{ConfigEntry, HookDecl, PluginBinding, PluginGroup, PluginPlan};

#[cfg(test)]
use crate::plugin::{Liveness, ResourceLimits};

mod io;
mod values;

use io::{count, Buf, Reader};
use values::{
    flag, network_code, network_from, origin_code, origin_from, read_capability, read_config,
    read_digest, read_limits, read_liveness, read_resolution, state_code, state_from,
    write_capability, write_config, write_digest, write_limits, write_liveness, write_resolution,
};

/// Encodes the plugin section payload.
pub(crate) fn encode(plan: &PluginPlan) -> Result<Vec<u8>, TemplateError> {
    let mut buf = Buf::default();
    buf.str(&plan.organization)?;
    write_limits(&mut buf, &plan.limits);
    write_liveness(&mut buf, &plan.liveness);
    buf.u32(count(plan.groups.len())?);
    for group in &plan.groups {
        buf.str(&group.name)?;
        buf.u8(network_code(group.network));
        buf.strs(&group.enabled)?;
        buf.strs(&group.order)?;
    }
    buf.u32(count(plan.plugins.len())?);
    for plugin in &plan.plugins {
        write_plugin(&mut buf, plugin)?;
    }
    buf.strs(&plan.order)?;
    Ok(buf.0)
}

/// Decodes a plugin section payload.
pub(crate) fn decode(payload: &[u8]) -> Result<PluginPlan, TemplateError> {
    let mut reader = Reader::new(payload);
    let organization = reader.string()?;
    let limits = read_limits(&mut reader)?;
    let liveness = read_liveness(&mut reader)?;
    let group_count = reader.list_len()?;
    let mut groups = Vec::with_capacity(group_count);
    for _ in 0..group_count {
        groups.push(PluginGroup {
            name: reader.string()?,
            network: network_from(reader.u8()?)?,
            enabled: reader.string_list()?,
            order: reader.string_list()?,
        });
    }
    let plugin_count = reader.list_len()?;
    let mut plugins = Vec::with_capacity(plugin_count);
    for _ in 0..plugin_count {
        plugins.push(read_plugin(&mut reader)?);
    }
    let order = reader.string_list()?;
    reader.finish()?;
    Ok(PluginPlan { organization, groups, plugins, order, limits, liveness })
}

fn write_plugin(buf: &mut Buf, plugin: &PluginBinding) -> Result<(), TemplateError> {
    buf.str(&plugin.id)?;
    buf.str(&plugin.version)?;
    buf.str(&plugin.publisher_key)?;
    write_digest(buf, plugin.digest);
    write_digest(buf, plugin.manifest_digest);
    buf.u8(origin_code(plugin.origin));
    write_resolution(buf, plugin.resolution);
    buf.u8(u8::from(plugin.required));
    buf.u8(state_code(plugin.state));
    buf.str(&plugin.alias)?;
    buf.u32(count(plugin.hooks.len())?);
    for hook in &plugin.hooks {
        buf.str(&hook.name)?;
        buf.u16(hook.version);
    }
    buf.u32(count(plugin.allow.len())?);
    for cap in &plugin.allow {
        write_capability(buf, cap)?;
    }
    buf.u32(count(plugin.config.len())?);
    for entry in &plugin.config {
        buf.str(&entry.key)?;
        write_config(buf, &entry.value)?;
    }
    buf.strs(&plugin.before)?;
    buf.strs(&plugin.after)?;
    Ok(())
}

fn read_plugin(reader: &mut Reader<'_>) -> Result<PluginBinding, TemplateError> {
    let id = reader.string()?;
    let version = reader.string()?;
    let publisher_key = reader.string()?;
    let digest = read_digest(reader)?;
    let manifest_digest = read_digest(reader)?;
    let origin = origin_from(reader.u8()?)?;
    let resolution = read_resolution(reader)?;
    let required = flag(reader.u8()?)?;
    let state = state_from(reader.u8()?)?;
    let alias = reader.string()?;
    let hook_count = reader.list_len()?;
    let mut hooks = Vec::with_capacity(hook_count);
    for _ in 0..hook_count {
        hooks.push(HookDecl { name: reader.string()?, version: reader.u16()? });
    }
    let allow_count = reader.list_len()?;
    let mut allow = Vec::with_capacity(allow_count);
    for _ in 0..allow_count {
        allow.push(read_capability(reader)?);
    }
    let config_count = reader.list_len()?;
    let mut config = Vec::with_capacity(config_count);
    for _ in 0..config_count {
        config.push(ConfigEntry { key: reader.string()?, value: read_config(reader)? });
    }
    Ok(PluginBinding {
        id,
        version,
        digest,
        manifest_digest,
        publisher_key,
        origin,
        resolution,
        required,
        state,
        alias,
        hooks,
        allow,
        config,
        before: reader.string_list()?,
        after: reader.string_list()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_plugin_section_roundtrips() {
        let plan = PluginPlan {
            organization: String::new(),
            groups: Vec::new(),
            plugins: Vec::new(),
            order: Vec::new(),
            limits: ResourceLimits::standard(),
            liveness: Liveness::fail_closed(),
        };
        let bytes = encode(&plan).expect("encode");
        assert_eq!(decode(&bytes).expect("decode"), plan);
    }
}
