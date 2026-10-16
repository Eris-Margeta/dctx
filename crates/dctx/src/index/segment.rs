//! Canonical generation payload. Walk order and the clock are not part of the digest.
//! Implements decision 077 snapshot identity: own SHA-256 over paths, decisions, metadata and content digests.
//! Depends on [`super`] index rows and [`crate::domain::digest_text`]. The session seals this payload.
//! Equal bytes and equal modification times produce equal digests.

use dctx_core::FileHandlingDecision;

use super::{IndexedEdge, IndexedFile, IndexedNode};
use crate::domain::{digest_text, DctxError};

/// Canonical UTF-8 payload and its content digest display form.
pub(crate) fn canonical_bytes(
    files: &[IndexedFile],
    nodes: &[IndexedNode],
    edges: &[IndexedEdge],
) -> Result<(Vec<u8>, String), DctxError> {
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
    let digest = digest_text(text.as_bytes())?;
    Ok((text.into_bytes(), digest.to_string()))
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
    }
}
