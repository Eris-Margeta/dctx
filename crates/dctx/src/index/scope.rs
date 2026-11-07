// SPDX-License-Identifier: Apache-2.0
//! Request-local generation projection shared by snapshot and direct queries.
//! Only containment edges contribute to these file-local operations and their identity.

use super::{Generation, IdIndex, TextBlob, canonical_bytes};
use crate::domain::{DctxError, FreshnessMode, STRUCTURAL_PARSER};

pub(crate) fn inside(path: &str, scope: &str) -> bool {
    path == scope
        || path
            .strip_prefix(scope)
            .is_some_and(|tail| tail.starts_with('/'))
}

/// Projects the requested address space without borrowing unrelated rows or graph facts.
pub(crate) fn project(generation: &Generation, scope: &str) -> Result<Generation, DctxError> {
    let files: Vec<_> = generation
        .files
        .iter()
        .filter(|file| inside(&file.path, scope))
        .cloned()
        .collect();
    let mut text = TextBlob::default();
    text.partial_dirs = generation
        .text
        .partial_dirs
        .iter()
        .filter(|dir| inside(dir, scope))
        .cloned()
        .collect();
    let mut nodes = Vec::new();
    let mut positions = vec![None; generation.nodes.len()];
    for (old, node) in generation.nodes.iter().enumerate() {
        let path = generation.text.get(node.path);
        if inside(path, scope) {
            let mut node = node.clone();
            node.path = text.push(path);
            positions[old] = Some(
                u32::try_from(nodes.len()).map_err(|_| DctxError::invalid("scope node count"))?,
            );
            nodes.push(node);
        }
    }
    let mut edges = Vec::new();
    for edge in &generation.edges {
        if generation.text.get(edge.edge) != "contains" {
            continue;
        }
        if let (Some(Some(src)), Some(Some(dst))) = (
            positions.get(edge.src as usize),
            positions.get(edge.dst as usize),
        ) {
            let mut edge = edge.clone();
            edge.src = *src;
            edge.dst = *dst;
            edge.edge = text.push("contains");
            edge.confidence = text.push(generation.text.get(edge.confidence));
            edges.push(edge);
        }
    }
    let metadata = generation.freshness == FreshnessMode::Metadata;
    let syntax = files.iter().any(|file| file.syntax);
    let other = files
        .iter()
        .any(|file| file.digest.is_some() && !file.syntax);
    let (coverage, parser) = if generation.coverage == "opaque" {
        ("opaque", "opaque/1")
    } else if !metadata && syntax && !other {
        ("syntax", "dctx-lex-rust/syn-3.0.9")
    } else {
        ("structural", STRUCTURAL_PARSER)
    };
    let completion = if metadata {
        "metadata"
    } else if files.iter().any(|file| file.partial) || !text.partial_dirs.is_empty() {
        "partial"
    } else {
        "complete"
    };
    let (_, digest) = canonical_bytes(&files, &nodes, &edges, &text)?;
    // A direct invocation has no persistent generation counter. Bind its cursor to
    // content so an edit between processes cannot resume against different spans.
    let identity = digest
        .split(':')
        .nth(2)
        .and_then(|hex| hex.get(..16))
        .and_then(|hex| u64::from_str_radix(hex, 16).ok())
        .unwrap_or(1)
        .max(1);
    Ok(Generation {
        id: identity,
        digest,
        encoded_len: 0,
        files,
        nodes,
        edges,
        text,
        freshness: generation.freshness,
        taken_at: generation.taken_at,
        epoch: generation.epoch,
        coverage: coverage.to_string(),
        completion: completion.to_string(),
        parser: parser.to_string(),
        ids: IdIndex::default(),
    })
}
