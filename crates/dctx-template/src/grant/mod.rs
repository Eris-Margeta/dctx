// SPDX-License-Identifier: Apache-2.0
//! Capability selectors and typed config checks.
//! Implements plugin-api/dts.md allow table and S-CONFIG / S-CAPABILITY.
//! Depends on [`crate::plugin`] and [`crate::error`]. [`crate::plugin_plan`] applies the results.
//! Selectors narrow a manifest record. They never invent a grant or raise a limit.

use crate::error::TemplateError;
use crate::plugin::{ConfigValue, FieldKind, FieldSpec, RESERVED_ALIASES};

#[cfg(test)]
use std::collections::BTreeSet;
#[cfg(test)]
use crate::plugin::Capability;

mod records;
mod selectors;

pub(crate) use records::capability_within;
pub(crate) use selectors::{expand_allow, screen_selectors};

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
    pre.is_none_or(|text| pre_ok(text, true)) && build.is_none_or(|text| pre_ok(text, false))
}

fn numeric(part: &str) -> bool {
    !part.is_empty() && (part.len() <= 1 || !part.starts_with('0')) && part.bytes().all(|byte| byte.is_ascii_digit())
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
