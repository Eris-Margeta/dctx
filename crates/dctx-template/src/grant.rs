//! Capability selectors and typed config checks.
//! Implements plugin-api/dts.md allow table and S-CONFIG / S-CAPABILITY.
//! Depends on [`crate::plugin`] and [`crate::error`]. [`crate::plugin_plan`] applies the results.
//! Selectors narrow a manifest record. They never invent a grant or raise a limit.

use std::collections::BTreeSet;

use crate::error::TemplateError;
use crate::plugin::{
    Capability, ConfigValue, FieldKind, FieldSpec, KnownPlugin, RESERVED_ALIASES,
};

const HOOKS: &[&str] = &[
    "before-pack-output-written",
    "after-pack-output-written",
    "before-index",
    "after-index",
    "cache-gc",
];

/// Closed hook name at payload version 1.
pub(crate) fn known_hook(name: &str) -> bool {
    HOOKS.contains(&name)
}

/// Canonical `publisher/name` id.
pub(crate) fn canonical_id(id: &str) -> bool {
    let Some((publisher, name)) = id.split_once('/') else {
        return false;
    };
    id.matches('/').count() == 1 && component(publisher) && component(name)
}

fn component(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes.len() > 64 || text.ends_with('.') {
        return false;
    }
    let first = bytes[0];
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return false;
    }
    bytes.iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-'))
        && !reserved_device(text)
}

fn reserved_device(text: &str) -> bool {
    let stem = text.split('.').next().unwrap_or(text);
    matches!(stem, "con" | "prn" | "aux" | "nul" | "com1" | "com2" | "com3" | "com4" | "com5" | "com6" | "com7" | "com8" | "com9" | "lpt1" | "lpt2" | "lpt3" | "lpt4" | "lpt5" | "lpt6" | "lpt7" | "lpt8" | "lpt9")
}

/// Exact semver, not a range, URL or branch.
pub(crate) fn require_semver(text: &str) -> Result<(), TemplateError> {
    if semver_ok(text) {
        Ok(())
    } else {
        Err(TemplateError::InvalidSyntax { line: 0, detail: "plugin version".into() })
    }
}

fn semver_ok(text: &str) -> bool {
    if text.len() > 128 || text.contains("://") || text.contains('^') || text.contains('~') || text.contains('*') || text.contains(' ') {
        return false;
    }
    let (body, build) = match text.split_once('+') {
        Some((body, build)) => (body, Some(build)),
        None => (text, None),
    };
    let (core, pre) = match body.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (body, None),
    };
    let mut parts = core.split('.');
    let Some(major) = parts.next() else {
        return false;
    };
    let Some(minor) = parts.next() else {
        return false;
    };
    let Some(patch) = parts.next() else {
        return false;
    };
    if parts.next().is_some() || !numeric(major) || !numeric(minor) || !numeric(patch) {
        return false;
    }
    pre.map(|text| pre_ok(text, true)).unwrap_or(true) && build.map(|text| pre_ok(text, false)).unwrap_or(true)
}

fn numeric(part: &str) -> bool {
    !part.is_empty() && !(part.len() > 1 && part.starts_with('0')) && part.bytes().all(|byte| byte.is_ascii_digit())
}

fn pre_ok(text: &str, reject_leading_zero: bool) -> bool {
    if text.is_empty() {
        return false;
    }
    text.split('.').all(|part| {
        if part.is_empty() {
            return false;
        }
        let ident = part.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-');
        let numeric_part = part.bytes().all(|byte| byte.is_ascii_digit());
        ident && !(reject_leading_zero && numeric_part && part.len() > 1 && part.starts_with('0'))
    })
}

/// Organization token from a PLUGINS header.
pub(crate) fn organization_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-'))
}

/// Alias grammar, including the reserved built-ins.
pub(crate) fn check_alias(alias: &str) -> Result<(), TemplateError> {
    if alias.is_empty() {
        return Ok(());
    }
    let bytes = alias.as_bytes();
    let shape = bytes.len() <= 64
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-');
    if !shape {
        return Err(TemplateError::InvalidSyntax { line: 0, detail: "alias".into() });
    }
    if RESERVED_ALIASES.contains(&alias) {
        return Err(TemplateError::ReservedAlias { alias: alias.to_string() });
    }
    Ok(())
}

/// `sha256:` digest or 64 hex digits.
pub(crate) fn parse_digest(text: &str) -> Result<[u8; 32], TemplateError> {
    let hex = text.strip_prefix("sha256:").unwrap_or(text);
    let bad = || TemplateError::InvalidSyntax { line: 0, detail: "plugin digest".into() };
    if hex.len() != 64 || !hex.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')) {
        return Err(bad());
    }
    let mut out = [0u8; 32];
    for index in 0..32 {
        let byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).map_err(|_| bad())?;
        out[index] = byte;
    }
    Ok(out)
}

/// Rejects `${...}` and `{{...}}` in a plugin scalar.
pub(crate) fn plain_text(text: &str) -> Result<(), TemplateError> {
    if text.contains("${") || text.contains("{{") || text.contains('\0') {
        return Err(TemplateError::InvalidSyntax { line: 0, detail: "interpolation".into() });
    }
    if text.contains("://") || text.starts_with('/') || text.starts_with('\\') {
        return Err(TemplateError::InvalidSyntax { line: 0, detail: "executable path".into() });
    }
    Ok(())
}

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

/// The compiled grant is inside the manifest and the deny set, limits included.
pub(crate) fn capability_within(cap: &Capability, catalog: &KnownPlugin, denied: &BTreeSet<&str>) -> bool {
    match cap {
        Capability::FileRead { path, max_bytes } => {
            admitted(&format!("file.read:{path}"), catalog, denied)
                && *max_bytes >= 1
                && *max_bytes <= ceiling_read(catalog, path)
        }
        Capability::FileWrite { path, create, max_growth_bytes, max_bytes } => {
            admitted(&format!("file.write:{path}"), catalog, denied)
                && *max_bytes >= 1
                && write_within(catalog, path, *create, *max_growth_bytes, *max_bytes)
        }
        Capability::Program { operation, resources, max_output_bytes } => {
            operation == "git.add-ignore"
                && resources.len() == 1
                && resources.first().is_some_and(|item| item == "git-admin:stage-ignore")
                && admitted("program:git.add-ignore", catalog, denied)
                && admitted("resource:git-admin:stage-ignore", catalog, denied)
                && *max_output_bytes >= 1
                && *max_output_bytes <= ceiling_program(catalog)
                && *max_output_bytes <= 65_536
        }
        Capability::OutputsRead { roles, operations, max_bytes } => {
            !roles.is_empty()
                && !operations.is_empty()
                && roles.iter().all(|role| role_ok(role))
                && operations.iter().all(|op| matches!(op.as_str(), "pack" | "index"))
                && roles.iter().all(|role| {
                    operations.iter().all(|op| admitted(&format!("outputs.read:{role}:{op}"), catalog, denied))
                })
                && *max_bytes >= 1
                && *max_bytes <= ceiling_output(catalog)
        }
        Capability::State { read, write, reclaim, max_bytes, source_content } => {
            (*read || *write || *reclaim)
                && (!*read || admitted("state:read", catalog, denied))
                && (!*write || admitted("state:write", catalog, denied))
                && (!*reclaim || admitted("state:reclaim", catalog, denied))
                && *max_bytes >= 1
                && *max_bytes <= ceiling_state(catalog)
                && !*source_content || catalog_state_content(catalog)
        }
    }
}

/// Value matches the manifest field.
pub(crate) fn accept_config(field: &FieldSpec, value: &ConfigValue) -> Result<ConfigValue, TemplateError> {
    let bad = || TemplateError::InvalidSyntax { line: 0, detail: format!("config {}", field.name) };
    match (&field.kind, value) {
        (FieldKind::Bool, ConfigValue::Bool(value)) => Ok(ConfigValue::Bool(*value)),
        (FieldKind::Int { min, max }, ConfigValue::Int(value)) if *value >= *min && *value <= *max => {
            Ok(ConfigValue::Int(*value))
        }
        (FieldKind::Str { max_bytes }, ConfigValue::Str(value)) if value.len() <= *max_bytes => {
            plain_text(value)?;
            Ok(ConfigValue::Str(value.clone()))
        }
        (FieldKind::Enum { options }, other) if options.iter().any(|item| item == other) => match other {
            ConfigValue::Str(text) => {
                plain_text(text)?;
                Ok(other.clone())
            }
            ConfigValue::Bool(_) | ConfigValue::Int(_) => Ok(other.clone()),
            ConfigValue::List(_) | ConfigValue::Enum(_) => Err(bad()),
        },
        (FieldKind::List { max_items, item_max_bytes }, ConfigValue::List(items))
            if items.len() <= *max_items && items.iter().all(|item| item.len() <= *item_max_bytes) =>
        {
            for item in items {
                plain_text(item)?;
            }
            Ok(ConfigValue::List(items.clone()))
        }
        _ => Err(bad()),
    }
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

fn role_ok(role: &str) -> bool {
    let bytes = role.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

fn admitted(selector: &str, catalog: &KnownPlugin, denied: &BTreeSet<&str>) -> bool {
    !denied.contains(selector) && catalog.capabilities.iter().any(|known| known == selector)
}

fn file_read(catalog: &KnownPlugin, path: &str) -> Result<Capability, TemplateError> {
    if let Some(Capability::FileRead { max_bytes, .. }) = catalog.records.iter().find(|cap| {
        matches!(cap, Capability::FileRead { path: found, .. } if found == path)
    }) {
        return Ok(Capability::FileRead { path: path.to_string(), max_bytes: *max_bytes });
    }
    if catalog.records.is_empty() {
        return Ok(Capability::FileRead { path: path.to_string(), max_bytes: 1_048_576 });
    }
    Err(TemplateError::GrantExpansion { kind: "plugin" })
}

fn file_write(catalog: &KnownPlugin, path: &str) -> Result<Capability, TemplateError> {
    if let Some(Capability::FileWrite { create, max_growth_bytes, max_bytes, .. }) = catalog.records.iter().find(|cap| {
        matches!(cap, Capability::FileWrite { path: found, .. } if found == path)
    }) {
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

fn program_cap(catalog: &KnownPlugin) -> Result<Capability, TemplateError> {
    if let Some(Capability::Program { max_output_bytes, .. }) =
        catalog.records.iter().find(|cap| matches!(cap, Capability::Program { .. }))
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

fn outputs(
    catalog: &KnownPlugin,
    roles: &BTreeSet<String>,
    operations: &BTreeSet<String>,
) -> Result<Capability, TemplateError> {
    let max_bytes = match catalog.records.iter().find(|cap| matches!(cap, Capability::OutputsRead { .. })) {
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

fn state_cap(catalog: &KnownPlugin, read: bool, write: bool, reclaim: bool) -> Result<Capability, TemplateError> {
    let (max_bytes, source_content) = match catalog.records.iter().find(|cap| matches!(cap, Capability::State { .. })) {
        Some(Capability::State { max_bytes, source_content, read: allow_read, write: allow_write, reclaim: allow_reclaim, .. }) => {
            if (read && !allow_read) || (write && !allow_write) || (reclaim && !allow_reclaim) {
                return Err(TemplateError::GrantExpansion { kind: "plugin" });
            }
            (*max_bytes, *source_content)
        }
        _ if catalog.records.is_empty() => (65_536, false),
        _ => return Err(TemplateError::GrantExpansion { kind: "plugin" }),
    };
    Ok(Capability::State { read, write, reclaim, max_bytes, source_content })
}

fn ceiling_read(catalog: &KnownPlugin, path: &str) -> u64 {
    catalog
        .records
        .iter()
        .find_map(|cap| match cap {
            Capability::FileRead { path: found, max_bytes } if found == path => Some(*max_bytes),
            _ => None,
        })
        .unwrap_or(1_048_576)
}

fn write_within(catalog: &KnownPlugin, path: &str, create: bool, growth: u64, max_bytes: u64) -> bool {
    match catalog.records.iter().find(|cap| matches!(cap, Capability::FileWrite { path: found, .. } if found == path)) {
        Some(Capability::FileWrite { create: ceiling_create, max_growth_bytes, max_bytes: ceiling, .. }) => {
            create == *ceiling_create && growth <= *max_growth_bytes && max_bytes <= *ceiling
        }
        _ => !create && growth <= 4096 && max_bytes <= 1_052_672,
    }
}

fn ceiling_program(catalog: &KnownPlugin) -> u64 {
    catalog
        .records
        .iter()
        .find_map(|cap| match cap {
            Capability::Program { max_output_bytes, .. } => Some(*max_output_bytes),
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
    catalog.records.iter().any(|cap| matches!(cap, Capability::State { source_content: true, .. }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::bundled_git_plugin;

    #[test]
    fn semver_accepts_a_release_and_refuses_a_range() {
        assert!(require_semver("1.0.0").is_ok());
        assert!(require_semver("1.0.0-rc.1").is_ok());
        assert!(require_semver("1").is_err());
        assert!(require_semver("^1.0.0").is_err());
        assert!(require_semver("1.0.0-01").is_err());
    }

    #[test]
    fn program_without_the_resource_is_refused_and_the_pair_expands() {
        let git = bundled_git_plugin();
        let denied = BTreeSet::new();
        let lone = vec!["program:git.add-ignore".to_string()];
        assert!(matches!(screen_selectors(&lone), Err(TemplateError::GrantExpansion { kind: "plugin" })));
        let pair = vec![
            "file.read:.gitignore".to_string(),
            "program:git.add-ignore".to_string(),
            "resource:git-admin:stage-ignore".to_string(),
        ];
        assert!(screen_selectors(&pair).is_ok());
        let caps = expand_allow(&pair, &git, &denied).expect("expand");
        assert_eq!(caps.len(), 2);
        assert!(caps.iter().any(|cap| matches!(cap, Capability::FileRead { .. })));
        assert!(caps.iter().any(|cap| matches!(cap, Capability::Program { .. })));
        let mut denied_write = BTreeSet::new();
        denied_write.insert("file.write:.gitignore");
        let write = vec!["file.write:.gitignore".to_string()];
        assert!(matches!(expand_allow(&write, &git, &denied_write), Err(TemplateError::GrantExpansion { .. })));
    }
}
