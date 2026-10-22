//! DTS keys, scalar values, lists and section header grammar.
//! Implements ARCHITECTURE-PROPOSAL §2 and PLUGINS.md §3.5.
//! Depends on source raw values and typed template errors; used by the DTS source parser.

use std::collections::BTreeSet;

use crate::error::TemplateError;

use super::draft::RawValue;

pub(super) fn once(seen: &mut BTreeSet<String>, key: &str) -> Result<(), TemplateError> {
    if seen.insert(key.to_string()) {
        Ok(())
    } else {
        Err(TemplateError::Duplicate { kind: "field", name: key.to_string() })
    }
}

pub(super) fn split_key(trimmed: &str, line: u32) -> Result<(String, String), TemplateError> {
    let Some((key, value)) = trimmed.split_once('=') else {
        return Err(TemplateError::InvalidSyntax { line, detail: "expected key = value".into() });
    };
    let key = key.trim();
    if key.is_empty() || !key.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' || ch == '-') {
        return Err(TemplateError::InvalidSyntax { line, detail: "field name".into() });
    }
    Ok((key.to_string(), value.trim().to_string()))
}

pub(super) fn parse_version(value: &str, line: u32) -> Result<u32, TemplateError> {
    let text = value.trim().trim_matches('"');
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(TemplateError::InvalidSyntax { line, detail: "version".into() });
    }
    text.parse::<u32>().map_err(|_| TemplateError::InvalidSyntax { line, detail: "version".into() })
}

pub(super) fn parse_raw(value: &str, line: u32) -> Result<RawValue, TemplateError> {
    let value = value.trim();
    if let Some(inner) = value.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
        return Ok(RawValue::List(parse_list(inner, line)?));
    }
    if value == "true" {
        return Ok(RawValue::Bool(true));
    }
    if value == "false" {
        return Ok(RawValue::Bool(false));
    }
    if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
        let number = value.parse::<u64>().map_err(|_| TemplateError::InvalidSyntax { line, detail: "integer".into() })?;
        return Ok(RawValue::Int(number));
    }
    Ok(RawValue::Str(unquote(value, line)?))
}

fn parse_list(inner: &str, line: u32) -> Result<Vec<String>, TemplateError> {
    let mut items = Vec::new();
    let mut rest = inner.trim();
    if rest.is_empty() {
        return Ok(items);
    }
    while !rest.is_empty() {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        if !rest.starts_with('"') {
            return Err(TemplateError::InvalidSyntax { line, detail: "list item".into() });
        }
        let mut out = String::new();
        let mut chars = rest[1..].chars();
        let mut closed = false;
        let mut consumed = 1usize;
        while let Some(ch) = chars.next() {
            consumed += ch.len_utf8();
            if ch == '\\' {
                let Some(esc) = chars.next() else {
                    return Err(TemplateError::InvalidSyntax { line, detail: "string escape".into() });
                };
                consumed += esc.len_utf8();
                out.push(match esc {
                    'n' => '\n',
                    '\\' => '\\',
                    '"' => '"',
                    other => other,
                });
                continue;
            }
            if ch == '"' {
                closed = true;
                break;
            }
            out.push(ch);
        }
        if !closed {
            return Err(TemplateError::InvalidSyntax { line, detail: "unterminated string".into() });
        }
        items.push(out);
        rest = rest[consumed..].trim_start();
        if rest.is_empty() {
            break;
        }
        if let Some(next) = rest.strip_prefix(',') {
            rest = next;
            continue;
        }
        return Err(TemplateError::InvalidSyntax { line, detail: "list separator".into() });
    }
    if items.len() > 1024 {
        return Err(TemplateError::Limit { what: "list", limit: 1024 });
    }
    Ok(items)
}

fn unquote(value: &str, line: u32) -> Result<String, TemplateError> {
    if let Some(inner) = value.strip_prefix('"') {
        let Some(inner) = inner.strip_suffix('"') else {
            return Err(TemplateError::InvalidSyntax { line, detail: "unterminated string".into() });
        };
        let mut out = String::new();
        let mut chars = inner.chars();
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                let Some(esc) = chars.next() else {
                    return Err(TemplateError::InvalidSyntax { line, detail: "string escape".into() });
                };
                out.push(match esc {
                    'n' => '\n',
                    '\\' => '\\',
                    '"' => '"',
                    other => other,
                });
            } else {
                out.push(ch);
            }
        }
        return Ok(out);
    }
    if value.chars().any(char::is_whitespace) {
        return Err(TemplateError::InvalidSyntax { line, detail: "string".into() });
    }
    Ok(value.to_string())
}

impl RawValue {
    pub(super) fn list(self, line: u32) -> Result<Vec<String>, TemplateError> {
        match self {
            Self::List(items) => Ok(items),
            _ => Err(TemplateError::InvalidSyntax { line, detail: "expected a list".into() }),
        }
    }

    pub(super) fn string(self, line: u32) -> Result<String, TemplateError> {
        match self {
            Self::Str(text) => Ok(text),
            _ => Err(TemplateError::InvalidSyntax { line, detail: "expected a string".into() }),
        }
    }

    pub(super) fn int(self, line: u32) -> Result<u64, TemplateError> {
        match self {
            Self::Int(number) => Ok(number),
            _ => Err(TemplateError::InvalidSyntax { line, detail: "expected an integer".into() }),
        }
    }

    pub(super) fn boolean(self, line: u32) -> Result<bool, TemplateError> {
        match self {
            Self::Bool(value) => Ok(value),
            _ => Err(TemplateError::InvalidSyntax { line, detail: "expected a boolean".into() }),
        }
    }
}

pub(super) fn header(trimmed: &str) -> Option<(&str, &str)> {
    let inner = trimmed.strip_prefix('[')?.strip_suffix(']')?;
    if inner.is_empty() || inner.contains('[') {
        return None;
    }
    let (kind, name) = inner.split_once(':').unwrap_or((inner, ""));
    let known = matches!(
        kind,
        "DTS" | "SELECT" | "INDEX" | "BUDGET" | "VIEW" | "PLUGINS" | "PLUGIN" | "VARIABLES" | "PARTIAL" | "LAYOUT" | "RENDER"
    );
    if known { Some((kind, name)) } else { None }
}

pub(super) fn check_ident(name: &str, line: u32) -> Result<(), TemplateError> {
    if name.is_empty() || name.len() > 128 {
        return Err(TemplateError::InvalidSyntax { line, detail: "name".into() });
    }
    if name.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '/' | '.')) {
        Ok(())
    } else {
        Err(TemplateError::InvalidSyntax { line, detail: "name".into() })
    }
}
