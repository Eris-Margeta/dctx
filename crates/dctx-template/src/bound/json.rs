//! JSON output and lookup helpers for bounded answer rendering.

use std::fmt::Write as _;

use dctx_core::FileHandlingDecision;

use crate::model::{Inst, NamedCode, RenderProgram};

use super::Prepared;

pub(super) fn json_document(items: &[Prepared], count: usize, truncated: bool) -> String {
    let mut out = String::from("{\"records\":[");
    for (index, item) in items.iter().take(count).enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"path\":");
        push_json(&mut out, &item.path);
        out.push_str(",\"content\":");
        push_json(&mut out, &json_content(item));
        out.push('}');
    }
    out.push_str("],\"truncated\":");
    out.push_str(if truncated { "true" } else { "false" });
    out.push('}');
    out
}

fn json_content(item: &Prepared) -> String {
    if item.is_dir {
        return String::new();
    }
    match item.decision {
        FileHandlingDecision::Include => String::from_utf8_lossy(&item.content).into_owned(),
        FileHandlingDecision::OmitContent => "[Content omitted by pattern rules]".to_string(),
        FileHandlingDecision::DebugVisible | FileHandlingDecision::Exclude => String::new(),
    }
}

fn push_json(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if (other as u32) < 0x20 => {
                let code = other as u32;
                let _ = write!(out, "\\u{code:04x}");
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

pub(super) fn named<'a>(items: &'a [NamedCode], name: &str) -> Option<&'a [Inst]> {
    items
        .iter()
        .find(|item| item.name == name)
        .map(|item| item.instructions.as_slice())
}

pub(super) fn variable<'a>(program: &'a RenderProgram, key: &str) -> Option<&'a str> {
    program
        .variables
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
}
