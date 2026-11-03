// SPDX-License-Identifier: Apache-2.0
//! Conservative recovery through the strict parser's public API.
//! Implements DX-FIX1 pending the syn extractor: retain complete Rust item
//! prefixes and suffixes, never infer facts from the rejected region.
//! Original line coordinates survive masking; recovered facts are labelled.
use crate::ParseDiagnostic;
use dctx_map_model::NodeKind;
use dctx_map_parse::{ParsedFile, Parser, TreeSitterParser};

const MAX_BOUNDARIES: usize = 64;

pub(super) fn recover(
    path: &str,
    bytes: &[u8],
    diagnostic: &ParseDiagnostic,
) -> Option<ParsedFile> {
    let source = std::str::from_utf8(bytes).ok()?;
    let mut offset = 0;
    let lines: Vec<_> = source
        .split_inclusive('\n')
        .map(|line| {
            let start = offset;
            offset += line.len();
            (start, line)
        })
        .collect();
    let error = diagnostic.lines[0].saturating_sub(1) as usize;
    // Only try unindented item boundaries. Each retained side must independently
    // pass the strict parser, so a function body is never treated as a file.
    let boundaries: Vec<_> = lines
        .iter()
        .enumerate()
        .filter(|(_, (_, line))| item_boundary(line))
        .map(|(line, (byte, _))| (line, *byte))
        .collect();
    let prefix = boundaries
        .iter()
        .rev()
        .filter(|(line, _)| *line <= error)
        .take(MAX_BOUNDARIES)
        .find_map(|(_, byte)| {
            TreeSitterParser
                .parse(path, &bytes[..*byte])
                .ok()
                .map(|_| *byte)
        })
        .unwrap_or(0);
    let suffix = boundaries
        .iter()
        .filter(|(line, _)| *line > error)
        .take(MAX_BOUNDARIES)
        .find_map(|(_, byte)| {
            TreeSitterParser
                .parse(path, &bytes[*byte..])
                .ok()
                .map(|_| *byte)
        })
        .unwrap_or(bytes.len());
    let mut masked = bytes.to_vec();
    for byte in &mut masked[prefix..suffix] {
        if !matches!(*byte, b'\n' | b'\r') {
            *byte = b' ';
        }
    }
    let mut parsed = TreeSitterParser.parse(path, &masked).ok()?;
    if !parsed
        .nodes
        .iter()
        .any(|node| node.kind != NodeKind::Module)
    {
        return None;
    }
    // Reference extraction may depend on imports or scopes in the omitted region.
    parsed.references.clear();
    for node in &mut parsed.nodes {
        node.summary.text = format!(
            "low-precision: partial Rust facts; {}. {}",
            diagnostic.error, node.summary.text
        );
        // Fingerprints of masked bodies must not suggest exact symbol moves.
        node.fingerprint.clear();
    }
    Some(parsed)
}

fn item_boundary(line: &str) -> bool {
    [
        "pub ",
        "fn ",
        "async ",
        "unsafe ",
        "const ",
        "static ",
        "struct ",
        "enum ",
        "trait ",
        "impl",
        "type ",
        "mod ",
        "use ",
        "extern ",
        "macro_rules!",
        "#",
        "//",
    ]
    .iter()
    .any(|prefix| line.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_items_survive_with_original_lines_and_no_inferred_references() {
        let source = b"pub fn before() {}\npub fn unsupported() {\n    let value = ;\n}\npub fn after() { before(); }\n";
        let diagnostic = ParseDiagnostic {
            error: "invalid syntax in sample.rs at line 3".into(),
            lines: [3, 3],
            syntax: "let value = ;".into(),
        };
        let parsed = recover("sample.rs", source, &diagnostic).unwrap();
        assert_eq!(parsed.nodes.len(), 3);
        assert_eq!(parsed.nodes[1].name, "before");
        assert_eq!(parsed.nodes[1].lines, [1, 1]);
        assert_eq!(parsed.nodes[2].name, "after");
        assert_eq!(parsed.nodes[2].lines, [5, 5]);
        assert_eq!(parsed.references, [] as [dctx_map_parse::RawReference; 0]);
        assert!(
            parsed
                .edges
                .iter()
                .all(|edge| edge.conf == dctx_map_model::Confidence::Exact)
        );
        assert!(
            parsed
                .nodes
                .iter()
                .all(|node| node.summary.text.starts_with("low-precision:"))
        );
        assert!(recover("sample.rs", b"pub fn broken(", &diagnostic).is_none());
    }
}
