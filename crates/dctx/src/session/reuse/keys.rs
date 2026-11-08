// SPDX-License-Identifier: Apache-2.0
use std::collections::HashMap;

use super::SYNTAX_PARSER;
use crate::domain::{DctxError, ParserChoice, RuleSources, STRUCTURAL_PARSER, ScanLimits};
use crate::index::IndexedFile;

pub(super) fn scoped_source_key(source_key: &str, rules: Option<&RuleSources>) -> String {
    let Some(rules) = rules else {
        return source_key.to_string();
    };
    format!("{source_key}\0{}", rule_stamp(rules))
}

fn rule_stamp(rules: &RuleSources) -> String {
    let mut bytes = Vec::new();
    push_rule(&mut bytes, rules.global_ignore.as_deref());
    push_rule(&mut bytes, rules.global_omit.as_deref());
    push_rule(&mut bytes, rules.local_ignore.as_deref());
    push_rule(&mut bytes, rules.local_omit.as_deref());
    match crate::domain::digest_text(&bytes) {
        Ok(digest) => digest.to_string(),
        Err(_) => format!("rules-unavailable:{}", bytes.len()),
    }
}

fn push_rule(out: &mut Vec<u8>, text: Option<&str>) {
    match text {
        None => out.push(0),
        Some(text) => {
            out.push(1);
            out.extend_from_slice(text.as_bytes());
            out.push(0xff);
        }
    }
}

pub(super) fn retryable(error: &DctxError) -> bool {
    match error {
        DctxError::Busy => true,
        DctxError::InvalidRequest { reason, .. } if *reason == "stale ledger" => true,
        _ => false,
    }
}

pub(super) fn parser_compatible(stored: &str, choice: ParserChoice) -> bool {
    match choice {
        ParserChoice::Opaque => stored == "opaque/1",
        ParserChoice::Structural => stored == STRUCTURAL_PARSER,
        ParserChoice::Syn => stored == STRUCTURAL_PARSER || stored == SYNTAX_PARSER,
        ParserChoice::TreeSitter => false,
    }
}

pub(super) fn limits_match(stored: &[u64; 4], live: &ScanLimits) -> bool {
    stored[0] == live.entries
        && stored[1] == live.depth
        && stored[2] == live.file_bytes
        && stored[3] == live.source_bytes
}

pub(super) fn file_inventory(files: &[IndexedFile]) -> Option<HashMap<&str, &IndexedFile>> {
    let mut map = HashMap::with_capacity(files.len());
    for file in files {
        if map.insert(file.path.as_str(), file).is_some() {
            return None;
        }
    }
    Some(map)
}
