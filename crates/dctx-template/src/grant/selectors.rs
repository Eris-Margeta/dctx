// SPDX-License-Identifier: Apache-2.0
//! Selector grammar, screening and expansion into catalog records.
//! Implements plugin-api/dts.md allow table and S-CONFIG / S-CAPABILITY.
//! Depends on grant scalar screening and manifest records; used by plugin plan admission.

use std::collections::BTreeSet;

use crate::error::TemplateError;
use crate::plugin::{Capability, KnownPlugin};

use super::plain_text;
use super::records::{file_read, file_write, outputs, program_cap, state_cap};

/// Selector syntax, uniqueness and the program/resource pair. No catalog lookup.
pub(crate) fn screen_selectors(allow: &[String]) -> Result<(), TemplateError> {
    if allow.len() > 64 {
        return Err(TemplateError::Limit { what: "allow", limit: 64 });
    }
    let mut seen = BTreeSet::new();
    let mut program = false;
    let mut resource = false;
    for selector in allow {
        if !seen.insert(selector.as_str()) {
            return Err(TemplateError::Duplicate { kind: "allow", name: selector.clone() });
        }
        plain_text(selector)?;
        match classify(selector)? {
            Sel::FileRead(path) | Sel::FileWrite(path) => {
                if !literal_path(&path) {
                    return Err(TemplateError::GrantExpansion { kind: "plugin" });
                }
            }
            Sel::Program(op) => {
                if op != "git.add-ignore" {
                    return Err(TemplateError::GrantExpansion { kind: "plugin" });
                }
                program = true;
            }
            Sel::Resource(name) => {
                if name != "git-admin:stage-ignore" {
                    return Err(TemplateError::GrantExpansion { kind: "plugin" });
                }
                resource = true;
            }
            Sel::Output(role, op) => {
                if !role_ok(&role) || !matches!(op.as_str(), "pack" | "index") {
                    return Err(TemplateError::GrantExpansion { kind: "plugin" });
                }
            }
            Sel::State(op) => {
                if !matches!(op.as_str(), "read" | "write" | "reclaim") {
                    return Err(TemplateError::GrantExpansion { kind: "plugin" });
                }
            }
        }
    }
    if program && !resource {
        return Err(TemplateError::GrantExpansion { kind: "plugin" });
    }
    Ok(())
}

/// Expands screened selectors into manifest records, narrowed by `denied`.
pub(crate) fn expand_allow(
    allow: &[String],
    catalog: &KnownPlugin,
    denied: &BTreeSet<&str>,
) -> Result<Vec<Capability>, TemplateError> {
    screen_selectors(allow)?;
    let mut caps = Vec::new();
    let mut program = false;
    let mut state_read = false;
    let mut state_write = false;
    let mut state_reclaim = false;
    let mut state_seen = false;
    let mut roles = BTreeSet::new();
    let mut operations = BTreeSet::new();
    for selector in allow {
        if !catalog.capabilities.iter().any(|known| known == selector) || denied.contains(selector.as_str()) {
            return Err(TemplateError::GrantExpansion { kind: "plugin" });
        }
        match classify(selector)? {
            Sel::FileRead(path) => caps.push(file_read(catalog, &path)?),
            Sel::FileWrite(path) => caps.push(file_write(catalog, &path)?),
            Sel::Program(_) => program = true,
            Sel::Resource(_) => {}
            Sel::Output(role, op) => {
                roles.insert(role);
                operations.insert(op);
            }
            Sel::State(op) => {
                state_seen = true;
                match op.as_str() {
                    "read" => state_read = true,
                    "write" => state_write = true,
                    _ => state_reclaim = true,
                }
            }
        }
    }
    if !roles.is_empty() {
        caps.push(outputs(catalog, &roles, &operations)?);
    }
    if program {
        caps.push(program_cap(catalog)?);
    }
    if state_seen {
        caps.push(state_cap(catalog, state_read, state_write, state_reclaim)?);
    }
    Ok(caps)
}

enum Sel {
    FileRead(String),
    FileWrite(String),
    Program(String),
    Resource(String),
    Output(String, String),
    State(String),
}

fn classify(selector: &str) -> Result<Sel, TemplateError> {
    if let Some(path) = selector.strip_prefix("file.read:") {
        return Ok(Sel::FileRead(path.to_string()));
    }
    if let Some(path) = selector.strip_prefix("file.write:") {
        return Ok(Sel::FileWrite(path.to_string()));
    }
    if let Some(op) = selector.strip_prefix("program:") {
        return Ok(Sel::Program(op.to_string()));
    }
    if let Some(name) = selector.strip_prefix("resource:") {
        return Ok(Sel::Resource(name.to_string()));
    }
    if let Some(rest) = selector.strip_prefix("outputs.read:") {
        let Some((role, op)) = rest.split_once(':') else {
            return Err(TemplateError::GrantExpansion { kind: "plugin" });
        };
        return Ok(Sel::Output(role.to_string(), op.to_string()));
    }
    if let Some(op) = selector.strip_prefix("state:") {
        return Ok(Sel::State(op.to_string()));
    }
    Err(TemplateError::GrantExpansion { kind: "plugin" })
}

fn literal_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 256
        && !path.starts_with('/')
        && !path.ends_with('/')
        && !path.contains("//")
        && !path.contains('\\')
        && !path.contains('*')
        && path.split('/').all(|segment| !segment.is_empty() && segment != "." && segment != "..")
        && path.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'/' | b'-'))
}

pub(super) fn role_ok(role: &str) -> bool {
    let bytes = role.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

