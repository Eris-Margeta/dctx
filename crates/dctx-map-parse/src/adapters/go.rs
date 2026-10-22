//! Go facts from dctx-lex-go for `PureParser` and the tree-sitter fallback.
//! Implements SPEC-MAP-MODE sections 5 and 6.2 and the PARSERS-v1 Go row.
//! The lexer emits model nodes; this adapter copies nodes, edges and references.
//! A partial or diagnostic result is `ParseError::Syntax`. `LexError` maps to `ParseError`.
use super::facts::file_precision;
use crate::{Language, ParseError, ParsedFile};
use dctx_lex_go::{Completion, GoLexer, LexError};
use dctx_map_model::{Completion as FileCompletion, Coverage, Edge, Node, NodeKind, StableId};
use std::collections::{BTreeMap, BTreeSet};

/// Copies a complete structural extraction. Diagnostics refuse the whole file.
pub(super) fn parse_pure(path: &str, bytes: &[u8]) -> Result<ParsedFile, ParseError> {
    let lexed = GoLexer::default()
        .extract(path, bytes)
        .map_err(|error| map_error(path, &error))?;
    if lexed.completion != Completion::Complete || !lexed.diagnostics.is_empty() {
        let line = lexed
            .diagnostics
            .first()
            .map_or(1, |diagnostic| diagnostic.line.max(1));
        return Err(ParseError::Syntax {
            path: path.into(),
            line,
        });
    }
    let parser = format!("{}:{}", dctx_lex_go::PARSER_ID, dctx_lex_go::PARSER_REVISION);
    let nodes = lexed.nodes.into_iter().map(|labeled| labeled.fact).collect();
    let edges = lexed.edges.into_iter().map(|labeled| labeled.fact).collect();
    let references = lexed
        .references
        .into_iter()
        .map(|labeled| crate::RawReference {
            src: labeled.fact.src,
            kind: labeled.fact.kind,
            name: labeled.fact.name,
            scope: labeled.fact.scope,
            alias: labeled.fact.alias,
            lines: labeled.fact.lines,
            owner_path: labeled.fact.owner_path,
        })
        .collect();
    let owned = own_file(nodes, edges, references)?;
    Ok(ParsedFile {
        path: lexed.path,
        language: Language::for_path(path),
        nodes: owned.nodes,
        edges: owned.edges,
        references: owned.references,
        precision: file_precision(Coverage::Structural, FileCompletion::Complete, &parser)?,
    })
}

struct OwnedFile {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    references: Vec<crate::RawReference>,
}

/// `NodeKind::Package` is a structural index node. The query indexer inserts
/// every package node again, so a package clause collides with itself in the
/// store. Keep the clause as a file-owned symbol with a unique id.
fn own_file(
    mut nodes: Vec<Node>,
    mut edges: Vec<Edge>,
    mut references: Vec<crate::RawReference>,
) -> Result<OwnedFile, ParseError> {
    let mut used = BTreeSet::new();
    for node in &nodes {
        if node.kind != NodeKind::Package {
            used.insert(node.id.as_str().to_owned());
        }
    }
    let mut remap = BTreeMap::new();
    for node in &mut nodes {
        if node.kind != NodeKind::Package {
            continue;
        }
        let id = file_symbol_id(&node.path, &node.name, &mut used)?;
        remap.insert(node.id.as_str().to_owned(), id.clone());
        node.kind = NodeKind::Symbol;
        node.id = id;
    }
    for edge in &mut edges {
        rewrite(&remap, &mut edge.src);
        rewrite(&remap, &mut edge.dst);
    }
    for reference in &mut references {
        rewrite(&remap, &mut reference.src);
    }
    Ok(OwnedFile {
        nodes,
        edges,
        references,
    })
}

fn rewrite(remap: &BTreeMap<String, StableId>, id: &mut StableId) {
    if let Some(next) = remap.get(id.as_str()) {
        *id = next.clone();
    }
}

fn file_symbol_id(
    path: &str,
    name: &str,
    used: &mut BTreeSet<String>,
) -> Result<StableId, ParseError> {
    let mut ordinal = None;
    for _ in 0..=used.len() {
        let id = StableId::new(NodeKind::Symbol, path, Some(name), ordinal)?;
        if used.insert(id.as_str().to_owned()) {
            return Ok(id);
        }
        ordinal = Some(ordinal.unwrap_or(0).saturating_add(1).max(1));
    }
    Err(ParseError::TooLarge)
}

fn map_error(path: &str, error: &LexError) -> ParseError {
    match error {
        LexError::InvalidId(error) => ParseError::InvalidId(*error),
        LexError::InvalidUtf8(error) => ParseError::InvalidUtf8(*error),
        LexError::TooLarge { .. } | LexError::TooManyFacts { .. } => ParseError::TooLarge,
        LexError::TooDeep { line, .. } => ParseError::Syntax {
            path: path.into(),
            line: (*line).max(1),
        },
        LexError::UnsupportedDialect => ParseError::Syntax {
            path: path.into(),
            line: 1,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Parser, PureParser};

    #[test]
    fn complete_function_is_structural_and_a_broken_header_is_refused() {
        let facts = PureParser
            .parse("x.go", b"package p\nfunc Ok() {}\n")
            .unwrap();
        assert_eq!(facts.language, Language::Go);
        assert_eq!(facts.precision.coverage(), Coverage::Structural);
        assert_eq!(facts.precision.completion().as_str(), "complete");
        assert!(facts.nodes.iter().all(|node| node.kind != NodeKind::Package));
        assert_eq!(facts.nodes[1].name, "p");
        assert_eq!(facts.nodes[1].kind, NodeKind::Symbol);
        assert_eq!(facts.nodes[1].id.as_str(), "s:x.go#p");
        assert!(matches!(
            PureParser.parse("x.go", b"package p\nfunc Broken("),
            Err(ParseError::Syntax { .. })
        ));
    }
}
