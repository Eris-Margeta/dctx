// SPDX-License-Identifier: Apache-2.0
//! Canonical generation payload. Walk order and the clock are not part of the digest.
//! Implements decision 077 snapshot identity: own SHA-256 over paths, decisions, metadata and content digests.
//! Depends on [`super`] index rows and [`crate::domain::digest_text`]. The session seals this payload.
//! Equal bytes and equal modification times produce equal digests.
//! Each row group is written into the payload and dropped before the next group is built.

use std::fmt::Write;

use dctx_core::FileHandlingDecision;

use super::{IndexedEdge, IndexedFile, IndexedNode, TextBlob, write_endpoint, write_node_id};
use crate::domain::{DctxError, Resource, digest_text};

/// Canonical UTF-8 payload and its content digest display form.
pub(crate) fn canonical_bytes(
    files: &[IndexedFile],
    nodes: &[IndexedNode],
    edges: &[IndexedEdge],
    text: &TextBlob,
) -> Result<(Vec<u8>, String), DctxError> {
    let mut payload = Vec::new();
    // Bytewise order is E, then F, then N, because every row starts with that letter.
    {
        let (raw, spans) = edge_rows(nodes, edges, text)?;
        append_sorted(&mut payload, &raw, spans);
    }
    {
        let (raw, spans) = file_rows(files)?;
        append_sorted(&mut payload, &raw, spans);
    }
    {
        let (raw, spans) = node_rows(nodes, text)?;
        append_sorted(&mut payload, &raw, spans);
    }
    payload.push(b'\n');
    let digest = digest_text(&payload)?;
    Ok((payload, digest.to_string()))
}

fn edge_rows(
    nodes: &[IndexedNode],
    edges: &[IndexedEdge],
    text: &TextBlob,
) -> Result<(Vec<u8>, Vec<(usize, usize)>), DctxError> {
    let mut raw = Vec::new();
    let mut spans = Vec::with_capacity(edges.len());
    let mut src = String::new();
    let mut dst = String::new();
    for edge in edges {
        src.clear();
        dst.clear();
        write_endpoint(&mut src, nodes, text, edge.src);
        write_endpoint(&mut dst, nodes, text, edge.dst);
        push_row(
            &mut raw,
            &mut spans,
            format_args!(
                "E\t{src}\t{dst}\t{}\t{}",
                text.get(edge.edge),
                text.get(edge.confidence)
            ),
        )?;
    }
    Ok((raw, spans))
}

fn file_rows(files: &[IndexedFile]) -> Result<(Vec<u8>, Vec<(usize, usize)>), DctxError> {
    let mut raw = Vec::new();
    let mut spans = Vec::with_capacity(files.len());
    for file in files {
        match &file.digest {
            Some(digest) => push_row(
                &mut raw,
                &mut spans,
                format_args!(
                    "F\t{}\t{}\t{}\t{}\t{digest}",
                    file.path,
                    decision_letter(file.decision),
                    file.size,
                    file.modified
                ),
            )?,
            None => push_row(
                &mut raw,
                &mut spans,
                format_args!(
                    "F\t{}\t{}\t{}\t{}\t-",
                    file.path,
                    decision_letter(file.decision),
                    file.size,
                    file.modified
                ),
            )?,
        }
    }
    Ok((raw, spans))
}

fn node_rows(
    nodes: &[IndexedNode],
    text: &TextBlob,
) -> Result<(Vec<u8>, Vec<(usize, usize)>), DctxError> {
    let mut raw = Vec::new();
    let mut spans = Vec::with_capacity(nodes.len());
    let mut id = String::new();
    for node in nodes {
        id.clear();
        write_node_id(&mut id, text, node);
        push_row(
            &mut raw,
            &mut spans,
            format_args!(
                "N\t{id}\t{}\t{}\t{}-{}\t{}-{}\t{}\t{}",
                node.kind,
                text.get(node.path),
                node.lines[0],
                node.lines[1],
                node.span[0],
                node.span[1],
                node.summary,
                node.summary_source
            ),
        )?;
    }
    Ok((raw, spans))
}

fn push_row(
    raw: &mut Vec<u8>,
    spans: &mut Vec<(usize, usize)>,
    args: std::fmt::Arguments<'_>,
) -> Result<(), DctxError> {
    let start = raw.len();
    let mut writer = RowWriter { raw };
    writer
        .write_fmt(args)
        .map_err(|_| DctxError::ResourceLimit {
            resource: Resource::Memory,
            limit: u64::MAX,
        })?;
    spans.push((start, raw.len()));
    Ok(())
}

struct RowWriter<'a> {
    raw: &'a mut Vec<u8>,
}

impl Write for RowWriter<'_> {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        self.raw.extend_from_slice(text.as_bytes());
        Ok(())
    }
}

fn append_sorted(out: &mut Vec<u8>, raw: &[u8], mut spans: Vec<(usize, usize)>) {
    if spans.is_empty() {
        return;
    }
    spans.sort_unstable_by(|left, right| row(raw, *left).cmp(row(raw, *right)));
    for (index, span) in spans.iter().copied().enumerate() {
        if index > 0 || !out.is_empty() {
            out.push(b'\n');
        }
        out.extend_from_slice(row(raw, span));
    }
}

fn row(raw: &[u8], span: (usize, usize)) -> &[u8] {
    let end = span.1.min(raw.len());
    let start = span.0.min(end);
    &raw[start..end]
}

fn decision_letter(decision: FileHandlingDecision) -> char {
    match decision {
        FileHandlingDecision::Include => 'I',
        FileHandlingDecision::Exclude => 'E',
        FileHandlingDecision::OmitContent => 'O',
        FileHandlingDecision::DebugVisible => 'D',
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dctx_hash::ContentDigest;

    #[test]
    fn order_and_clock_do_not_change_the_digest() {
        let digest = ContentDigest::sha256([1; 32], 3);
        let file = IndexedFile {
            path: "src/lib.rs".to_string(),
            decision: FileHandlingDecision::Include,
            digest: Some(digest),
            size: 3,
            modified: 7,
            stamp: None,
        };
        let other = IndexedFile {
            path: "README.md".to_string(),
            ..file.clone()
        };
        let text = TextBlob::default();
        let left = canonical_bytes(&[file.clone(), other.clone()], &[], &[], &text).expect("left");
        let right = canonical_bytes(&[other, file], &[], &[], &text).expect("right");
        assert_eq!(left.1, right.1);
        assert_eq!(left.0, right.0);
    }

    #[test]
    fn grouped_rows_match_one_sorted_join() {
        let digest = ContentDigest::sha256([9; 32], 4);
        let files = vec![
            IndexedFile {
                path: "src/b.rs".to_string(),
                decision: FileHandlingDecision::Include,
                digest: Some(digest),
                size: 4,
                modified: 2,
                stamp: None,
            },
            IndexedFile {
                path: "README.md".to_string(),
                decision: FileHandlingDecision::OmitContent,
                digest: None,
                size: 8,
                modified: 1,
                stamp: None,
            },
        ];
        let mut text = TextBlob::default();
        let beta_path = text.push("src/b.rs");
        let alpha_path = text.push("src/a.rs");
        let contains = text.push("contains");
        let exact = text.push("exact");
        let name = text.push("name");
        let nodes = vec![
            IndexedNode {
                letter: b'm',
                ordinal: 0,
                kind: "module".to_string(),
                name: String::new(),
                path: beta_path,
                lines: [1, 3],
                span: [0, 9],
                summary: String::new(),
                summary_source: String::new(),
                signature: String::new(),
            },
            IndexedNode {
                letter: b's',
                ordinal: 0,
                kind: "fn".to_string(),
                name: "beta".to_string(),
                path: beta_path,
                lines: [2, 3],
                span: [4, 9],
                summary: "line\nwith\ttab".to_string(),
                summary_source: "sig".to_string(),
                signature: "fn beta()".to_string(),
            },
            IndexedNode {
                letter: b'm',
                ordinal: 0,
                kind: "module".to_string(),
                name: String::new(),
                path: alpha_path,
                lines: [1, 1],
                span: [0, 3],
                summary: String::new(),
                summary_source: String::new(),
                signature: String::new(),
            },
            IndexedNode {
                letter: b's',
                ordinal: 0,
                kind: "fn".to_string(),
                name: "alpha".to_string(),
                path: alpha_path,
                lines: [1, 1],
                span: [0, 3],
                summary: "alpha".to_string(),
                summary_source: "doc".to_string(),
                signature: "fn alpha()".to_string(),
            },
        ];
        let edges = vec![
            IndexedEdge {
                src: 0,
                dst: 1,
                edge: contains,
                confidence: exact,
            },
            IndexedEdge {
                src: 2,
                dst: 3,
                edge: contains,
                confidence: name,
            },
        ];
        let (payload, digest) = canonical_bytes(&files, &nodes, &edges, &text).expect("canonical");
        assert_eq!(payload, joined_rows(&files, &nodes, &edges, &text));
        assert!(digest.starts_with("v1:sha256:"), "{digest}");
        let empty = canonical_bytes(&[], &[], &[], &TextBlob::default()).expect("empty");
        assert_eq!(empty.0, b"\n");
    }

    fn joined_rows(
        files: &[IndexedFile],
        nodes: &[IndexedNode],
        edges: &[IndexedEdge],
        text: &TextBlob,
    ) -> Vec<u8> {
        let mut lines = Vec::new();
        for file in files {
            let digest = match &file.digest {
                Some(digest) => digest.to_string(),
                None => "-".to_string(),
            };
            lines.push(format!(
                "F\t{}\t{}\t{}\t{}\t{digest}",
                file.path,
                decision_letter(file.decision),
                file.size,
                file.modified
            ));
        }
        for node in nodes {
            let mut id = String::new();
            write_node_id(&mut id, text, node);
            lines.push(format!(
                "N\t{id}\t{}\t{}\t{}-{}\t{}-{}\t{}\t{}",
                node.kind,
                text.get(node.path),
                node.lines[0],
                node.lines[1],
                node.span[0],
                node.span[1],
                node.summary,
                node.summary_source
            ));
        }
        for edge in edges {
            let mut src = String::new();
            let mut dst = String::new();
            write_endpoint(&mut src, nodes, text, edge.src);
            write_endpoint(&mut dst, nodes, text, edge.dst);
            lines.push(format!(
                "E\t{src}\t{dst}\t{}\t{}",
                text.get(edge.edge),
                text.get(edge.confidence)
            ));
        }
        lines.sort();
        let mut text = lines.join("\n");
        text.push('\n');
        text.into_bytes()
    }
}
