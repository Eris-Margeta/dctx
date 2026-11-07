// SPDX-License-Identifier: Apache-2.0
//! Plugin policy, capability, configuration and lock value encodings.
//! Implements contracts/DTS-v2.md plugin section payloads.
//! Depends on plugin domain types and bounded byte I/O; used by the binary plugin section codec.

use crate::error::TemplateError;
use crate::plugin::{
    Capability, ConfigValue, Liveness, NetworkPolicy, PluginOrigin, PluginState, Resolution,
    ResourceLimits, UnavailableReason,
};

use super::io::{Buf, Reader};

pub(super) fn write_limits(buf: &mut Buf, limits: &ResourceLimits) {
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

pub(super) fn read_limits(reader: &mut Reader<'_>) -> Result<ResourceLimits, TemplateError> {
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

pub(super) fn write_liveness(buf: &mut Buf, policy: &Liveness) {
    match policy {
        Liveness::Interactive {
            sample_ms,
            cancel_grace_ms,
        } => {
            buf.u8(1);
            buf.u64(*sample_ms);
            buf.u64(*cancel_grace_ms);
        }
        Liveness::UnattendedFailClosed {
            sample_ms,
            cancel_grace_ms,
        } => {
            buf.u8(2);
            buf.u64(*sample_ms);
            buf.u64(*cancel_grace_ms);
        }
        Liveness::UnattendedBounded {
            sample_ms,
            cancel_grace_ms,
            inactivity_ms,
        } => {
            buf.u8(3);
            buf.u64(*sample_ms);
            buf.u64(*cancel_grace_ms);
            buf.u64(*inactivity_ms);
        }
    }
}

pub(super) fn read_liveness(reader: &mut Reader<'_>) -> Result<Liveness, TemplateError> {
    match reader.u8()? {
        1 => Ok(Liveness::Interactive {
            sample_ms: reader.u64()?,
            cancel_grace_ms: reader.u64()?,
        }),
        2 => Ok(Liveness::UnattendedFailClosed {
            sample_ms: reader.u64()?,
            cancel_grace_ms: reader.u64()?,
        }),
        3 => Ok(Liveness::UnattendedBounded {
            sample_ms: reader.u64()?,
            cancel_grace_ms: reader.u64()?,
            inactivity_ms: reader.u64()?,
        }),
        _ => Err(TemplateError::Corrupt { reason: "liveness" }),
    }
}

pub(super) fn write_capability(buf: &mut Buf, cap: &Capability) -> Result<(), TemplateError> {
    match cap {
        Capability::FileRead { path, max_bytes } => {
            buf.u8(1);
            buf.str(path)?;
            buf.u64(*max_bytes);
        }
        Capability::FileWrite {
            path,
            create,
            max_growth_bytes,
            max_bytes,
        } => {
            buf.u8(2);
            buf.str(path)?;
            buf.u8(u8::from(*create));
            buf.u64(*max_growth_bytes);
            buf.u64(*max_bytes);
        }
        Capability::Program {
            operation,
            resources,
            max_output_bytes,
        } => {
            buf.u8(3);
            buf.str(operation)?;
            buf.strs(resources)?;
            buf.u64(*max_output_bytes);
        }
        Capability::OutputsRead {
            roles,
            operations,
            max_bytes,
        } => {
            buf.u8(4);
            buf.strs(roles)?;
            buf.strs(operations)?;
            buf.u64(*max_bytes);
        }
        Capability::State {
            read,
            write,
            reclaim,
            max_bytes,
            source_content,
        } => {
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

pub(super) fn read_capability(reader: &mut Reader<'_>) -> Result<Capability, TemplateError> {
    match reader.u8()? {
        1 => Ok(Capability::FileRead {
            path: reader.string()?,
            max_bytes: reader.u64()?,
        }),
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
        _ => Err(TemplateError::Corrupt {
            reason: "capability",
        }),
    }
}

pub(super) fn write_config(buf: &mut Buf, value: &ConfigValue) -> Result<(), TemplateError> {
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

pub(super) fn read_config(reader: &mut Reader<'_>) -> Result<ConfigValue, TemplateError> {
    match reader.u8()? {
        1 => Ok(ConfigValue::Bool(flag(reader.u8()?)?)),
        2 => Ok(ConfigValue::Int(reader.u64()?)),
        3 => Ok(ConfigValue::Str(reader.string()?)),
        4 => Ok(ConfigValue::List(reader.string_list()?)),
        5 => Ok(ConfigValue::Enum(reader.string()?)),
        _ => Err(TemplateError::Corrupt { reason: "config" }),
    }
}

pub(super) fn write_digest(buf: &mut Buf, digest: Option<[u8; 32]>) {
    match digest {
        None => buf.u8(0),
        Some(digest) => {
            buf.u8(1);
            buf.raw(&digest);
        }
    }
}

pub(super) fn read_digest(reader: &mut Reader<'_>) -> Result<Option<[u8; 32]>, TemplateError> {
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

pub(super) fn write_resolution(buf: &mut Buf, resolution: Resolution) {
    match resolution {
        Resolution::Available => buf.u8(1),
        Resolution::Unavailable { reason } => {
            buf.u8(2);
            buf.u8(reason_code(reason));
        }
    }
}

pub(super) fn read_resolution(reader: &mut Reader<'_>) -> Result<Resolution, TemplateError> {
    match reader.u8()? {
        1 => Ok(Resolution::Available),
        2 => Ok(Resolution::Unavailable {
            reason: reason_from(reader.u8()?)?,
        }),
        _ => Err(TemplateError::Corrupt {
            reason: "resolution",
        }),
    }
}

pub(super) fn network_code(policy: NetworkPolicy) -> u8 {
    match policy {
        NetworkPolicy::None => 0,
        NetworkPolicy::Loopback => 1,
        NetworkPolicy::External => 2,
    }
}

pub(super) fn network_from(code: u8) -> Result<NetworkPolicy, TemplateError> {
    match code {
        0 => Ok(NetworkPolicy::None),
        1 => Ok(NetworkPolicy::Loopback),
        2 => Ok(NetworkPolicy::External),
        _ => Err(TemplateError::Corrupt { reason: "network" }),
    }
}

pub(super) fn state_code(state: PluginState) -> u8 {
    match state {
        PluginState::Enabled => 1,
        PluginState::Disabled => 2,
        PluginState::Unavailable => 3,
    }
}

pub(super) fn state_from(code: u8) -> Result<PluginState, TemplateError> {
    match code {
        1 => Ok(PluginState::Enabled),
        2 => Ok(PluginState::Disabled),
        3 => Ok(PluginState::Unavailable),
        _ => Err(TemplateError::Corrupt {
            reason: "plugin state",
        }),
    }
}

pub(super) fn origin_code(origin: Option<PluginOrigin>) -> u8 {
    match origin {
        None => 0,
        Some(PluginOrigin::Bundled) => 1,
        Some(PluginOrigin::Installed) => 2,
        Some(PluginOrigin::Override) => 3,
    }
}

pub(super) fn origin_from(code: u8) -> Result<Option<PluginOrigin>, TemplateError> {
    match code {
        0 => Ok(None),
        1 => Ok(Some(PluginOrigin::Bundled)),
        2 => Ok(Some(PluginOrigin::Installed)),
        3 => Ok(Some(PluginOrigin::Override)),
        _ => Err(TemplateError::Corrupt { reason: "origin" }),
    }
}

pub(super) fn reason_code(reason: UnavailableReason) -> u8 {
    match reason {
        UnavailableReason::NotInstalled => 1,
        UnavailableReason::Incompatible => 2,
        UnavailableReason::UnsupportedPlatform => 3,
        UnavailableReason::Quarantined => 4,
    }
}

pub(super) fn reason_from(code: u8) -> Result<UnavailableReason, TemplateError> {
    match code {
        1 => Ok(UnavailableReason::NotInstalled),
        2 => Ok(UnavailableReason::Incompatible),
        3 => Ok(UnavailableReason::UnsupportedPlatform),
        4 => Ok(UnavailableReason::Quarantined),
        _ => Err(TemplateError::Corrupt {
            reason: "unavailable",
        }),
    }
}

pub(super) fn flag(code: u8) -> Result<bool, TemplateError> {
    match code {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(TemplateError::Corrupt { reason: "flag" }),
    }
}
