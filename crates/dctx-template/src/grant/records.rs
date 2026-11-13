// SPDX-License-Identifier: Apache-2.0
//! Manifest capability records and ceiling checks.
//! Implements plugin-api/dts.md allow table and S-CONFIG / S-CAPABILITY.
//! Depends on plugin catalog types and selector role validation; used by plugin plan admission.

use std::collections::BTreeSet;

use crate::error::TemplateError;
use crate::plugin::{Capability, KnownPlugin};

use super::selectors::role_ok;

/// The compiled grant is inside the manifest and the deny set, limits included.
pub(crate) fn capability_within(
    cap: &Capability,
    catalog: &KnownPlugin,
    denied: &BTreeSet<&str>,
) -> bool {
    match cap {
        Capability::FileRead { path, max_bytes } => {
            admitted(&format!("file.read:{path}"), catalog, denied)
                && *max_bytes >= 1
                && *max_bytes <= ceiling_read(catalog, path)
        }
        Capability::FileWrite {
            path,
            create,
            max_growth_bytes,
            max_bytes,
        } => {
            admitted(&format!("file.write:{path}"), catalog, denied)
                && *max_bytes >= 1
                && write_within(catalog, path, *create, *max_growth_bytes, *max_bytes)
        }
        Capability::Program {
            operation,
            resources,
            max_output_bytes,
        } => {
            operation == "git.add-ignore"
                && resources.len() == 1
                && resources
                    .first()
                    .is_some_and(|item| item == "git-admin:stage-ignore")
                && admitted("program:git.add-ignore", catalog, denied)
                && admitted("resource:git-admin:stage-ignore", catalog, denied)
                && *max_output_bytes >= 1
                && *max_output_bytes <= ceiling_program(catalog)
                && *max_output_bytes <= 65_536
        }
        Capability::OutputsRead {
            roles,
            operations,
            max_bytes,
        } => {
            !roles.is_empty()
                && !operations.is_empty()
                && roles.iter().all(|role| role_ok(role))
                && operations
                    .iter()
                    .all(|op| matches!(op.as_str(), "pack" | "index"))
                && roles.iter().all(|role| {
                    operations
                        .iter()
                        .all(|op| admitted(&format!("outputs.read:{role}:{op}"), catalog, denied))
                })
                && *max_bytes >= 1
                && *max_bytes <= ceiling_output(catalog)
        }
        Capability::State {
            read,
            write,
            reclaim,
            max_bytes,
            source_content,
        } => {
            (*read || *write || *reclaim)
                && (!*read || admitted("state:read", catalog, denied))
                && (!*write || admitted("state:write", catalog, denied))
                && (!*reclaim || admitted("state:reclaim", catalog, denied))
                && *max_bytes >= 1
                && *max_bytes <= ceiling_state(catalog)
                && (!*source_content || catalog_state_content(catalog))
        }
    }
}
fn admitted(selector: &str, catalog: &KnownPlugin, denied: &BTreeSet<&str>) -> bool {
    !denied.contains(selector) && catalog.capabilities.iter().any(|known| known == selector)
}

pub(super) fn file_read(catalog: &KnownPlugin, path: &str) -> Result<Capability, TemplateError> {
    if let Some(Capability::FileRead { max_bytes, .. }) = catalog
        .records
        .iter()
        .find(|cap| matches!(cap, Capability::FileRead { path: found, .. } if found == path))
    {
        return Ok(Capability::FileRead {
            path: path.to_string(),
            max_bytes: *max_bytes,
        });
    }
    if catalog.records.is_empty() {
        return Ok(Capability::FileRead {
            path: path.to_string(),
            max_bytes: 1_048_576,
        });
    }
    Err(TemplateError::GrantExpansion { kind: "plugin" })
}

pub(super) fn file_write(catalog: &KnownPlugin, path: &str) -> Result<Capability, TemplateError> {
    if let Some(Capability::FileWrite {
        create,
        max_growth_bytes,
        max_bytes,
        ..
    }) = catalog
        .records
        .iter()
        .find(|cap| matches!(cap, Capability::FileWrite { path: found, .. } if found == path))
    {
        return Ok(Capability::FileWrite {
            path: path.to_string(),
            create: *create,
            max_growth_bytes: *max_growth_bytes,
            max_bytes: *max_bytes,
        });
    }
    if catalog.records.is_empty() {
        return Ok(Capability::FileWrite {
            path: path.to_string(),
            create: false,
            max_growth_bytes: 4096,
            max_bytes: 1_052_672,
        });
    }
    Err(TemplateError::GrantExpansion { kind: "plugin" })
}

pub(super) fn program_cap(catalog: &KnownPlugin) -> Result<Capability, TemplateError> {
    if let Some(Capability::Program {
        max_output_bytes, ..
    }) = catalog
        .records
        .iter()
        .find(|cap| matches!(cap, Capability::Program { .. }))
    {
        return Ok(Capability::Program {
            operation: "git.add-ignore".to_string(),
            resources: vec!["git-admin:stage-ignore".to_string()],
            max_output_bytes: *max_output_bytes,
        });
    }
    if catalog.records.is_empty() {
        return Ok(Capability::Program {
            operation: "git.add-ignore".to_string(),
            resources: vec!["git-admin:stage-ignore".to_string()],
            max_output_bytes: 65_536,
        });
    }
    Err(TemplateError::GrantExpansion { kind: "plugin" })
}

pub(super) fn outputs(
    catalog: &KnownPlugin,
    roles: &BTreeSet<String>,
    operations: &BTreeSet<String>,
) -> Result<Capability, TemplateError> {
    let max_bytes = match catalog
        .records
        .iter()
        .find(|cap| matches!(cap, Capability::OutputsRead { .. }))
    {
        Some(Capability::OutputsRead { max_bytes, .. }) => *max_bytes,
        _ if catalog.records.is_empty() => 65_536,
        _ => return Err(TemplateError::GrantExpansion { kind: "plugin" }),
    };
    Ok(Capability::OutputsRead {
        roles: roles.iter().cloned().collect(),
        operations: operations.iter().cloned().collect(),
        max_bytes,
    })
}

pub(super) fn state_cap(
    catalog: &KnownPlugin,
    read: bool,
    write: bool,
    reclaim: bool,
) -> Result<Capability, TemplateError> {
    let (max_bytes, source_content) = match catalog
        .records
        .iter()
        .find(|cap| matches!(cap, Capability::State { .. }))
    {
        Some(Capability::State {
            max_bytes,
            source_content,
            read: allow_read,
            write: allow_write,
            reclaim: allow_reclaim,
            ..
        }) => {
            if (read && !allow_read) || (write && !allow_write) || (reclaim && !allow_reclaim) {
                return Err(TemplateError::GrantExpansion { kind: "plugin" });
            }
            (*max_bytes, *source_content)
        }
        _ if catalog.records.is_empty() => (65_536, false),
        _ => return Err(TemplateError::GrantExpansion { kind: "plugin" }),
    };
    Ok(Capability::State {
        read,
        write,
        reclaim,
        max_bytes,
        source_content,
    })
}

fn ceiling_read(catalog: &KnownPlugin, path: &str) -> u64 {
    catalog
        .records
        .iter()
        .find_map(|cap| match cap {
            Capability::FileRead {
                path: found,
                max_bytes,
            } if found == path => Some(*max_bytes),
            _ => None,
        })
        .unwrap_or(1_048_576)
}

fn write_within(
    catalog: &KnownPlugin,
    path: &str,
    create: bool,
    growth: u64,
    max_bytes: u64,
) -> bool {
    match catalog
        .records
        .iter()
        .find(|cap| matches!(cap, Capability::FileWrite { path: found, .. } if found == path))
    {
        Some(Capability::FileWrite {
            create: ceiling_create,
            max_growth_bytes,
            max_bytes: ceiling,
            ..
        }) => create == *ceiling_create && growth <= *max_growth_bytes && max_bytes <= *ceiling,
        _ => !create && growth <= 4096 && max_bytes <= 1_052_672,
    }
}

fn ceiling_program(catalog: &KnownPlugin) -> u64 {
    catalog
        .records
        .iter()
        .find_map(|cap| match cap {
            Capability::Program {
                max_output_bytes, ..
            } => Some(*max_output_bytes),
            _ => None,
        })
        .unwrap_or(65_536)
}

fn ceiling_output(catalog: &KnownPlugin) -> u64 {
    catalog
        .records
        .iter()
        .find_map(|cap| match cap {
            Capability::OutputsRead { max_bytes, .. } => Some(*max_bytes),
            _ => None,
        })
        .unwrap_or(65_536)
}

fn ceiling_state(catalog: &KnownPlugin) -> u64 {
    catalog
        .records
        .iter()
        .find_map(|cap| match cap {
            Capability::State { max_bytes, .. } => Some(*max_bytes),
            _ => None,
        })
        .unwrap_or(65_536)
}

fn catalog_state_content(catalog: &KnownPlugin) -> bool {
    catalog.records.iter().any(|cap| {
        matches!(
            cap,
            Capability::State {
                source_content: true,
                ..
            }
        )
    })
}
