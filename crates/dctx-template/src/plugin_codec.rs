//! Binary plugin section of a ContextPlan.
//! Implements contracts/DTS-v2.md section payloads for the plugin extension.
//! Depends on [`crate::plugin`] and [`crate::error`]. [`crate::binary`] frames the bytes.
//! Field order is fixed so a reload reproduces the compiled value.

use crate::error::{TemplateError, MAX_STRING_BYTES};
use crate::plugin::{
    Capability, ConfigEntry, ConfigValue, HookDecl, Liveness, NetworkPolicy, PluginBinding, PluginGroup, PluginOrigin,
    PluginPlan, PluginState, Resolution, ResourceLimits, UnavailableReason,
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

fn write_limits(buf: &mut Buf, limits: &ResourceLimits) {
    buf.u64(limits.frame_bytes);
    buf.u64(limits.read_bytes);
    buf.u64(limits.actions);
    buf.u64(limits.outstanding_requests);
    buf.u64(limits.diagnostics_bytes);
    buf.u64(limits.chunk_bytes);
    buf.u64(limits.state_bytes);
    buf.u64(limits.upload_bytes);
    buf.u64(limits.queue_bytes);
    buf.u64(limits.memory_bytes);
    buf.u64(limits.work_units);
}

fn read_limits(reader: &mut Reader<'_>) -> Result<ResourceLimits, TemplateError> {
    Ok(ResourceLimits {
        frame_bytes: reader.u64()?,
        read_bytes: reader.u64()?,
        actions: reader.u64()?,
        outstanding_requests: reader.u64()?,
        diagnostics_bytes: reader.u64()?,
        chunk_bytes: reader.u64()?,
        state_bytes: reader.u64()?,
        upload_bytes: reader.u64()?,
        queue_bytes: reader.u64()?,
        memory_bytes: reader.u64()?,
        work_units: reader.u64()?,
    })
}

fn write_liveness(buf: &mut Buf, policy: &Liveness) {
    match policy {
        Liveness::Interactive { sample_ms, cancel_grace_ms } => {
            buf.u8(1);
            buf.u64(*sample_ms);
            buf.u64(*cancel_grace_ms);
        }
        Liveness::UnattendedFailClosed { sample_ms, cancel_grace_ms } => {
            buf.u8(2);
            buf.u64(*sample_ms);
            buf.u64(*cancel_grace_ms);
        }
        Liveness::UnattendedBounded { sample_ms, cancel_grace_ms, inactivity_ms } => {
            buf.u8(3);
            buf.u64(*sample_ms);
            buf.u64(*cancel_grace_ms);
            buf.u64(*inactivity_ms);
        }
    }
}

fn read_liveness(reader: &mut Reader<'_>) -> Result<Liveness, TemplateError> {
    match reader.u8()? {
        1 => Ok(Liveness::Interactive { sample_ms: reader.u64()?, cancel_grace_ms: reader.u64()? }),
        2 => Ok(Liveness::UnattendedFailClosed { sample_ms: reader.u64()?, cancel_grace_ms: reader.u64()? }),
        3 => Ok(Liveness::UnattendedBounded {
            sample_ms: reader.u64()?,
            cancel_grace_ms: reader.u64()?,
            inactivity_ms: reader.u64()?,
        }),
        _ => Err(TemplateError::Corrupt { reason: "liveness" }),
    }
}

fn write_capability(buf: &mut Buf, cap: &Capability) -> Result<(), TemplateError> {
    match cap {
        Capability::FileRead { path, max_bytes } => {
            buf.u8(1);
            buf.str(path)?;
            buf.u64(*max_bytes);
        }
        Capability::FileWrite { path, create, max_growth_bytes, max_bytes } => {
            buf.u8(2);
            buf.str(path)?;
            buf.u8(u8::from(*create));
            buf.u64(*max_growth_bytes);
            buf.u64(*max_bytes);
        }
        Capability::Program { operation, resources, max_output_bytes } => {
            buf.u8(3);
            buf.str(operation)?;
            buf.strs(resources)?;
            buf.u64(*max_output_bytes);
        }
        Capability::OutputsRead { roles, operations, max_bytes } => {
            buf.u8(4);
            buf.strs(roles)?;
            buf.strs(operations)?;
            buf.u64(*max_bytes);
        }
        Capability::State { read, write, reclaim, max_bytes, source_content } => {
            buf.u8(5);
            buf.u8(u8::from(*read));
            buf.u8(u8::from(*write));
            buf.u8(u8::from(*reclaim));
            buf.u64(*max_bytes);
            buf.u8(u8::from(*source_content));
        }
    }
    Ok(())
}

fn read_capability(reader: &mut Reader<'_>) -> Result<Capability, TemplateError> {
    match reader.u8()? {
        1 => Ok(Capability::FileRead { path: reader.string()?, max_bytes: reader.u64()? }),
        2 => Ok(Capability::FileWrite {
            path: reader.string()?,
            create: flag(reader.u8()?)?,
            max_growth_bytes: reader.u64()?,
            max_bytes: reader.u64()?,
        }),
        3 => Ok(Capability::Program {
            operation: reader.string()?,
            resources: reader.string_list()?,
            max_output_bytes: reader.u64()?,
        }),
        4 => Ok(Capability::OutputsRead {
            roles: reader.string_list()?,
            operations: reader.string_list()?,
            max_bytes: reader.u64()?,
        }),
        5 => Ok(Capability::State {
            read: flag(reader.u8()?)?,
            write: flag(reader.u8()?)?,
            reclaim: flag(reader.u8()?)?,
            max_bytes: reader.u64()?,
            source_content: flag(reader.u8()?)?,
        }),
        _ => Err(TemplateError::Corrupt { reason: "capability" }),
    }
}

fn write_config(buf: &mut Buf, value: &ConfigValue) -> Result<(), TemplateError> {
    match value {
        ConfigValue::Bool(value) => {
            buf.u8(1);
            buf.u8(u8::from(*value));
        }
        ConfigValue::Int(value) => {
            buf.u8(2);
            buf.u64(*value);
        }
        ConfigValue::Str(value) => {
            buf.u8(3);
            buf.str(value)?;
        }
        ConfigValue::List(items) => {
            buf.u8(4);
            buf.strs(items)?;
        }
        ConfigValue::Enum(value) => {
            buf.u8(5);
            buf.str(value)?;
        }
    }
    Ok(())
}

fn read_config(reader: &mut Reader<'_>) -> Result<ConfigValue, TemplateError> {
    match reader.u8()? {
        1 => Ok(ConfigValue::Bool(flag(reader.u8()?)?)),
        2 => Ok(ConfigValue::Int(reader.u64()?)),
        3 => Ok(ConfigValue::Str(reader.string()?)),
        4 => Ok(ConfigValue::List(reader.string_list()?)),
        5 => Ok(ConfigValue::Enum(reader.string()?)),
        _ => Err(TemplateError::Corrupt { reason: "config" }),
    }
}

fn write_digest(buf: &mut Buf, digest: Option<[u8; 32]>) {
    match digest {
        None => buf.u8(0),
        Some(digest) => {
            buf.u8(1);
            buf.raw(&digest);
        }
    }
}

fn read_digest(reader: &mut Reader<'_>) -> Result<Option<[u8; 32]>, TemplateError> {
    match reader.u8()? {
        0 => Ok(None),
        1 => {
            let bytes = reader.take(32)?;
            let mut digest = [0u8; 32];
            digest.copy_from_slice(bytes);
            Ok(Some(digest))
        }
        _ => Err(TemplateError::Corrupt { reason: "digest" }),
    }
}

fn write_resolution(buf: &mut Buf, resolution: Resolution) {
    match resolution {
        Resolution::Available => buf.u8(1),
        Resolution::Unavailable { reason } => {
            buf.u8(2);
            buf.u8(reason_code(reason));
        }
    }
}

fn read_resolution(reader: &mut Reader<'_>) -> Result<Resolution, TemplateError> {
    match reader.u8()? {
        1 => Ok(Resolution::Available),
        2 => Ok(Resolution::Unavailable { reason: reason_from(reader.u8()?)? }),
        _ => Err(TemplateError::Corrupt { reason: "resolution" }),
    }
}

fn network_code(policy: NetworkPolicy) -> u8 {
    match policy {
        NetworkPolicy::None => 0,
        NetworkPolicy::Loopback => 1,
        NetworkPolicy::External => 2,
    }
}

fn network_from(code: u8) -> Result<NetworkPolicy, TemplateError> {
    match code {
        0 => Ok(NetworkPolicy::None),
        1 => Ok(NetworkPolicy::Loopback),
        2 => Ok(NetworkPolicy::External),
        _ => Err(TemplateError::Corrupt { reason: "network" }),
    }
}

fn state_code(state: PluginState) -> u8 {
    match state {
        PluginState::Enabled => 1,
        PluginState::Disabled => 2,
        PluginState::Unavailable => 3,
    }
}

fn state_from(code: u8) -> Result<PluginState, TemplateError> {
    match code {
        1 => Ok(PluginState::Enabled),
        2 => Ok(PluginState::Disabled),
        3 => Ok(PluginState::Unavailable),
        _ => Err(TemplateError::Corrupt { reason: "plugin state" }),
    }
}

fn origin_code(origin: Option<PluginOrigin>) -> u8 {
    match origin {
        None => 0,
        Some(PluginOrigin::Bundled) => 1,
        Some(PluginOrigin::Installed) => 2,
        Some(PluginOrigin::Override) => 3,
    }
}

fn origin_from(code: u8) -> Result<Option<PluginOrigin>, TemplateError> {
    match code {
        0 => Ok(None),
        1 => Ok(Some(PluginOrigin::Bundled)),
        2 => Ok(Some(PluginOrigin::Installed)),
        3 => Ok(Some(PluginOrigin::Override)),
        _ => Err(TemplateError::Corrupt { reason: "origin" }),
    }
}

fn reason_code(reason: UnavailableReason) -> u8 {
    match reason {
        UnavailableReason::NotInstalled => 1,
        UnavailableReason::Incompatible => 2,
        UnavailableReason::UnsupportedPlatform => 3,
        UnavailableReason::Quarantined => 4,
    }
}

fn reason_from(code: u8) -> Result<UnavailableReason, TemplateError> {
    match code {
        1 => Ok(UnavailableReason::NotInstalled),
        2 => Ok(UnavailableReason::Incompatible),
        3 => Ok(UnavailableReason::UnsupportedPlatform),
        4 => Ok(UnavailableReason::Quarantined),
        _ => Err(TemplateError::Corrupt { reason: "unavailable" }),
    }
}

fn flag(code: u8) -> Result<bool, TemplateError> {
    match code {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(TemplateError::Corrupt { reason: "flag" }),
    }
}

fn count(len: usize) -> Result<u32, TemplateError> {
    u32::try_from(len).map_err(|_| TemplateError::Limit { what: "count", limit: 64 })
}

#[derive(Default)]
struct Buf(Vec<u8>);

impl Buf {
    fn u8(&mut self, value: u8) {
        self.0.push(value);
    }

    fn u16(&mut self, value: u16) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn raw(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }

    fn str(&mut self, text: &str) -> Result<(), TemplateError> {
        if text.len() > MAX_STRING_BYTES {
            return Err(TemplateError::Limit { what: "string", limit: MAX_STRING_BYTES as u64 });
        }
        self.u32(u32::try_from(text.len()).map_err(|_| TemplateError::Limit {
            what: "string",
            limit: MAX_STRING_BYTES as u64,
        })?);
        self.raw(text.as_bytes());
        Ok(())
    }

    fn strs(&mut self, items: &[String]) -> Result<(), TemplateError> {
        self.u32(count(items.len())?);
        for item in items {
            self.str(item)?;
        }
        Ok(())
    }
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, at: 0 }
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], TemplateError> {
        let end = self.at.checked_add(len).ok_or(TemplateError::Corrupt { reason: "truncated" })?;
        if end > self.data.len() {
            return Err(TemplateError::Corrupt { reason: "truncated" });
        }
        let out = &self.data[self.at..end];
        self.at = end;
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8, TemplateError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, TemplateError> {
        let bytes = self.take(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&mut self) -> Result<u32, TemplateError> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn u64(&mut self) -> Result<u64, TemplateError> {
        let bytes = self.take(8)?;
        Ok(u64::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7]]))
    }

    fn string(&mut self) -> Result<String, TemplateError> {
        let len = usize::try_from(self.u32()?).unwrap_or(usize::MAX);
        if len > MAX_STRING_BYTES {
            return Err(TemplateError::Limit { what: "string", limit: MAX_STRING_BYTES as u64 });
        }
        let bytes = self.take(len)?;
        std::str::from_utf8(bytes).map(str::to_string).map_err(|_| TemplateError::Corrupt { reason: "utf8" })
    }

    fn list_len(&mut self) -> Result<usize, TemplateError> {
        let len = usize::try_from(self.u32()?).unwrap_or(usize::MAX);
        if len > 64 {
            return Err(TemplateError::Limit { what: "list", limit: 64 });
        }
        Ok(len)
    }

    fn string_list(&mut self) -> Result<Vec<String>, TemplateError> {
        let len = self.list_len()?;
        let mut items = Vec::with_capacity(len);
        for _ in 0..len {
            items.push(self.string()?);
        }
        Ok(items)
    }

    fn finish(&self) -> Result<(), TemplateError> {
        if self.at == self.data.len() {
            Ok(())
        } else {
            Err(TemplateError::Corrupt { reason: "payload" })
        }
    }
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
