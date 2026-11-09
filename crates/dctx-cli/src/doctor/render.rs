// SPDX-License-Identifier: Apache-2.0
//! Escaped text and JSON for local installation diagnostics.

use std::fmt::Write as _;

use super::Binary;

pub(super) fn report(binaries: &[Binary], json: bool) -> String {
    if json {
        let rows: Vec<_> = binaries.iter().map(json_row).collect();
        format!(
            "{{\"schema_version\":1,\"binaries\":[{}]}}\n",
            rows.join(",")
        )
    } else {
        let mut out = String::new();
        for binary in binaries {
            let _ = writeln!(out, "{}\n  version: {}\n  commit: {}\n  scope: {}\n  running: {}\n  cache_root: {}\n  cache_size_bytes: {}\n  sha256: {}",
                display(&binary.path.to_string_lossy()),
                binary.version.as_deref().unwrap_or("unavailable"),
                binary.commit.as_deref().unwrap_or("unavailable"),
                binary.scope.as_str(), binary.running,
                binary.cache_root.as_ref().map_or_else(|| "unavailable".into(), |path| display(&path.to_string_lossy())),
                binary.cache_size_bytes.map_or_else(|| "unavailable".into(), |bytes| bytes.to_string()),
                binary.sha256.as_deref().unwrap_or("unavailable"));
            for error in &binary.errors {
                let _ = writeln!(out, "  error: {}", display(error));
            }
        }
        out
    }
}

fn json_row(binary: &Binary) -> String {
    let errors: Vec<_> = binary.errors.iter().map(|error| quote(error)).collect();
    format!(
        "{{\"path\":{},\"real_path\":{},\"version\":{},\"commit\":{},\"scope\":{},\"running\":{},\"cache_root\":{},\"cache_size_bytes\":{},\"sha256\":{},\"errors\":[{}]}}",
        quote(&binary.path.to_string_lossy()),
        optional(binary.real_path.as_ref().map(|path| path.to_string_lossy().into_owned()).as_deref()),
        optional(binary.version.as_deref()), optional(binary.commit.as_deref()), quote(binary.scope.as_str()), binary.running,
        optional(binary.cache_root.as_ref().map(|path| path.to_string_lossy().into_owned()).as_deref()),
        binary.cache_size_bytes.map_or_else(|| "null".into(), |bytes| bytes.to_string()),
        optional(binary.sha256.as_deref()), errors.join(","),
    )
}

fn optional(value: Option<&str>) -> String {
    value.map_or_else(|| "null".into(), quote)
}

fn quote(value: &str) -> String {
    let mut out = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if other.is_control() => {
                let _ = write!(out, "\\u{:04x}", u32::from(other));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

pub(super) fn display(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| {
            if character.is_control() {
                character.escape_default().collect::<Vec<_>>()
            } else {
                vec![character]
            }
        })
        .collect()
}
