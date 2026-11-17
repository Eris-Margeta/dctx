// SPDX-License-Identifier: Apache-2.0
//! The dctx.pack/1 JSON envelope and exact source fragments selected by the text page.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::{ContextReport, ContextRequest};
use crate::domain::quote_json;
use crate::{AnswerRecord, DctxError};

pub(super) fn render(
    report: &ContextReport,
    request: &ContextRequest,
    snapshot: &str,
    selections: &[(String, usize)],
    sources: &BTreeMap<String, AnswerRecord>,
) -> Result<String, DctxError> {
    let mut out = String::from(
        "{\"schema\":\"dctx.pack/1\",\"schema_version\":\"1.0\",\"v\":2,\"verb\":\"context\",\"text\":",
    );
    quote_json(&mut out, &report.text);
    let _ = write!(
        out,
        ",\"hits\":{},\"terms\":{},\"budget\":{},\"snapshot\":",
        report.hits, report.terms, request.budget
    );
    quote_json(&mut out, snapshot);
    out.push_str(",\"commit\":null,\"fragments\":[");
    let mut comma = false;
    for (id, lines) in selections {
        let record = sources.get(id).ok_or(DctxError::CorruptIndex {
            reason: "missing pack source",
        })?;
        let content: String = record
            .source_utf8
            .as_deref()
            .unwrap_or_default()
            .split_inclusive('\n')
            .take(*lines)
            .collect();
        if content.is_empty() {
            continue;
        }
        let start = record.span.ok_or(DctxError::CorruptIndex {
            reason: "missing pack span",
        })?[0];
        let end = start + content.len() as u64;
        let line_end = u64::from(record.lines[0])
            + content.as_bytes()[..content.len() - 1]
                .iter()
                .filter(|&&byte| byte == b'\n')
                .count() as u64;
        let digest = dctx_hash::Sha256::digest(content.as_bytes())
            .map_err(|_| DctxError::invalid("pack source too large"))?;
        if comma {
            out.push(',');
        }
        comma = true;
        out.push_str("{\"target\":");
        quote_json(&mut out, &record.id);
        out.push_str(",\"path\":");
        quote_json(&mut out, &record.path);
        let _ = write!(
            out,
            ",\"byte_start\":{start},\"byte_end\":{end},\"line_start\":{},\"line_end\":{line_end},\"sha256\":\"",
            record.lines[0]
        );
        for byte in digest.as_bytes() {
            let _ = write!(out, "{byte:02x}");
        }
        out.push_str("\",\"content\":");
        quote_json(&mut out, &content);
        out.push('}');
    }
    out.push_str("]}\n");
    Ok(out)
}
