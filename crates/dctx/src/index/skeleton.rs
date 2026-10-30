//! The `skeleton` verb: every declaration signature of one file, no bodies, in source order.
//! Implements the Trail-Code `skeleton` parity verb (DCX-15) over the in-memory generation.
//! The file's module record comes first, then each symbol by start line. Nothing reads the source.
//! A missing module is [`Collected::Outcome`]; a non-module id is refused.

use super::query::{missing, record_of, require, Collected};
use super::Generation;
use crate::domain::{AnswerRecord, DctxError, MapRequest};

/// The module of the file named by `request.target`, then its symbols with their signatures.
pub(super) fn skeleton(generation: &Generation, request: &MapRequest) -> Result<Collected, DctxError> {
    let text = &generation.text;
    let Some(root) = require(&generation.nodes, text, &request.target)? else {
        return Ok(missing(&request.target));
    };
    if root.kind != "module" {
        return Err(DctxError::invalid("skeleton takes a file path or an m: module id"));
    }
    let path = text.get(root.path);
    let mut rows = Vec::new();
    for node in &generation.nodes {
        if node.kind == "module" || text.get(node.path) != path {
            continue;
        }
        let mut record = record_of(text, node);
        if !node.signature.is_empty() {
            record.signature = Some(node.signature.clone());
        }
        rows.push(record);
    }
    source_order(&mut rows);
    let mut records = Vec::with_capacity(rows.len() + 1);
    records.push(record_of(text, root));
    records.extend(rows);
    Ok(Collected::Records(records))
}

/// Start line ascending, so an enclosing declaration precedes its members; a longer span first on a tie, then id.
fn source_order(rows: &mut [AnswerRecord]) {
    rows.sort_by(|left, right| {
        left.lines[0]
            .cmp(&right.lines[0])
            .then(right.lines[1].cmp(&left.lines[1]))
            .then(left.id.cmp(&right.id))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, lines: [u32; 2]) -> AnswerRecord {
        AnswerRecord {
            id: id.to_string(),
            kind: "fn".to_string(),
            path: "a.py".to_string(),
            lines,
            summary: String::new(),
            summary_source: String::new(),
            signature: None,
            span: None,
            source_utf8: None,
            edge: None,
            direction: None,
            target: None,
            confidence: None,
            sites: Vec::new(),
            score: 0,
        }
    }

    #[test]
    fn declarations_come_out_in_source_order_with_the_enclosing_one_first() {
        let mut rows = vec![
            row("s:a.py#helper", [8, 10]),
            row("s:a.py#Repo::put", [5, 6]),
            row("s:a.py#Repo", [1, 6]),
            row("s:a.py#Repo::get", [2, 3]),
        ];
        source_order(&mut rows);
        let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["s:a.py#Repo", "s:a.py#Repo::get", "s:a.py#Repo::put", "s:a.py#helper"]);
    }
}
