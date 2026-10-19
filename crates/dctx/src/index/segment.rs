//! Canonical generation payload. Walk order and the clock are not part of the digest.
//! Implements decision 077 snapshot identity: own SHA-256 over paths, decisions, metadata and content digests.
//! Depends on [`super`] index rows and [`crate::domain::digest_text`]. The session seals this payload.
//! Equal bytes and equal modification times produce equal digests.
//! Each row group is written into the payload and dropped before the next group is built.

use std::fmt::Write;

use dctx_core::FileHandlingDecision;

use super::{IndexedEdge, IndexedFile, IndexedNode};
use crate::domain::{digest_text, DctxError, Resource};

/// Canonical UTF-8 payload and its content digest display form.
pub(crate) fn canonical_bytes(
    files: &[IndexedFile],
    nodes: &[IndexedNode],
    edges: &[IndexedEdge],
) -> Result<(Vec<u8>, String), DctxError> {
    let mut payload = Vec::new();
    // Bytewise order is E, then F, then N, because every row starts with that letter.
    {
        let (raw, spans) = edge_rows(edges)?;
        append_sorted(&mut payload, &raw, spans);
    }
    {
        let (raw, spans) = file_rows(files)?;
        append_sorted(&mut payload, &raw, spans);
    }
    {
        let (raw, spans) = node_rows(nodes)?;
        append_sorted(&mut payload, &raw, spans);
    }
    payload.push(b'\n');
    let digest = digest_text(&payload)?;
    Ok((payload, digest.to_string()))
}

fn edge_rows(edges: &[IndexedEdge]) -> Result<(Vec<u8>, Vec<(usize, usize)>), DctxError> {
    let mut raw = Vec::new();
    let mut spans = Vec::with_capacity(edges.len());
    for edge in edges {
        push_row(
            &mut raw,
            &mut spans,
            format_args!("E\t{}\t{}\t{}\t{}", edge.src, edge.dst, edge.edge, edge.confidence),
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

fn node_rows(nodes: &[IndexedNode]) -> Result<(Vec<u8>, Vec<(usize, usize)>), DctxError> {
    let mut raw = Vec::new();
    let mut spans = Vec::with_capacity(nodes.len());
    for node in nodes {
        push_row(
            &mut raw,
            &mut spans,
            format_args!(
                "N\t{}\t{}\t{}\t{}-{}\t{}-{}\t{}\t{}",
                node.id,
                node.kind,
                node.path,
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

fn push_row(raw: &mut Vec<u8>, spans: &mut Vec<(usize, usize)>, args: std::fmt::Arguments<'_>) -> Result<(), DctxError> {
    let start = raw.len();
    let mut writer = RowWriter { raw };
    writer.write_fmt(args).map_err(|_| DctxError::ResourceLimit { resource: Resource::Memory, limit: u64::MAX })?;
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
        };
        let other = IndexedFile { path: "README.md".to_string(), ..file.clone() };
        let left = canonical_bytes(&[file.clone(), other.clone()], &[], &[]).expect("left");
        let right = canonical_bytes(&[other, file], &[], &[]).expect("right");
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
            },
            IndexedFile {
                path: "README.md".to_string(),
                decision: FileHandlingDecision::OmitContent,
                digest: None,
                size: 8,
                modified: 1,
            },
        ];
        let nodes = vec![
            IndexedNode {
                id: "s:src/b.rs#beta".to_string(),
                kind: "fn".to_string(),
                name: "beta".to_string(),
                path: "src/b.rs".to_string(),
                lines: [2, 3],
                span: [4, 9],
                summary: "line\nwith\ttab".to_string(),
                summary_source: "sig".to_string(),
                signature: "fn beta()".to_string(),
            },
            IndexedNode {
                id: "s:src/a.rs#alpha".to_string(),
                kind: "fn".to_string(),
                name: "alpha".to_string(),
                path: "src/a.rs".to_string(),
                lines: [1, 1],
                span: [0, 3],
                summary: "alpha".to_string(),
                summary_source: "doc".to_string(),
                signature: "fn alpha()".to_string(),
            },
        ];
        let edges = vec![
            IndexedEdge {
                src: "m:src/b.rs".to_string(),
                dst: "s:src/b.rs#beta".to_string(),
                edge: "contains".to_string(),
                confidence: "exact".to_string(),
            },
            IndexedEdge {
                src: "m:src/a.rs".to_string(),
                dst: "s:src/a.rs#alpha".to_string(),
                edge: "contains".to_string(),
                confidence: "name".to_string(),
            },
        ];
        let (payload, digest) = canonical_bytes(&files, &nodes, &edges).expect("canonical");
        assert_eq!(payload, joined_rows(&files, &nodes, &edges));
        assert!(digest.starts_with("v1:sha256:"), "{digest}");
        let empty = canonical_bytes(&[], &[], &[]).expect("empty");
        assert_eq!(empty.0, b"\n");
    }

    fn joined_rows(files: &[IndexedFile], nodes: &[IndexedNode], edges: &[IndexedEdge]) -> Vec<u8> {
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
            lines.push(format!(
                "N\t{}\t{}\t{}\t{}-{}\t{}-{}\t{}\t{}",
                node.id,
                node.kind,
                node.path,
                node.lines[0],
                node.lines[1],
                node.span[0],
                node.span[1],
                node.summary,
                node.summary_source
            ));
        }
        for edge in edges {
            lines.push(format!("E\t{}\t{}\t{}\t{}", edge.src, edge.dst, edge.edge, edge.confidence));
        }
        lines.sort();
        let mut text = lines.join("\n");
        text.push('\n');
        text.into_bytes()
    }
}
