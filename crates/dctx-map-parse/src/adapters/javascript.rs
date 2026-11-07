// SPDX-License-Identifier: Apache-2.0
//! JavaScript and TypeScript facts from dctx-lex-js, plus the optional tree-sitter walker.
//! Implements SPEC-MAP-MODE sections 5 and 6.2 and the PARSERS-v1 JavaScript and TypeScript rows.
//! The lexer already emits model nodes; this adapter copies them and records structural precision.
//! Partial input is refused so invalid files do not replace stored facts. Tree-sitter is the oracle.
use super::facts::file_precision;
use crate::{Language, ParseError, ParsedFile};
use dctx_lex_js::{Completion, JsLexer, LexError};
use dctx_map_model::{Completion as FileCompletion, Coverage};

#[cfg(feature = "syntax-tree-sitter")]
use super::facts::{Facts, text};
#[cfg(feature = "syntax-tree-sitter")]
use super::javascript_items::{Scope, children, item, jsdoc};
#[cfg(feature = "syntax-tree-sitter")]
use super::javascript_references::references;

/// Copies a complete structural extraction. Diagnostics refuse the whole file.
pub(super) fn parse_pure(path: &str, bytes: &[u8]) -> Result<ParsedFile, ParseError> {
    let lexed = JsLexer::default()
        .extract(path, bytes)
        .map_err(|error| map_error(path, &error))?;
    if lexed.completion != Completion::Complete {
        let line = lexed
            .diagnostics
            .first()
            .map_or(1, |diagnostic| diagnostic.line.max(1));
        return Err(ParseError::Syntax {
            path: path.into(),
            line,
        });
    }
    let parser = format!(
        "{}:{}",
        dctx_lex_js::PARSER_ID,
        dctx_lex_js::PARSER_REVISION
    );
    Ok(ParsedFile {
        path: lexed.path,
        language: Language::for_path(path),
        nodes: lexed
            .nodes
            .into_iter()
            .map(|labeled| labeled.fact)
            .collect(),
        edges: lexed
            .edges
            .into_iter()
            .map(|labeled| labeled.fact)
            .collect(),
        references: lexed
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
            .collect(),
        precision: file_precision(Coverage::Structural, FileCompletion::Complete, &parser)?,
    })
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

#[cfg(feature = "syntax-tree-sitter")]
pub(super) fn parse(source: &str, facts: &mut Facts) -> Result<(), ParseError> {
    let language = match facts.file.language {
        Language::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        Language::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        _ => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
    };
    let tree = super::tree(source, language)?;
    let root = tree.root_node();
    if root.has_error() {
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            if node.is_error() || node.is_missing() {
                return Err(ParseError::Syntax {
                    path: facts.file.path.clone(),
                    line: node.start_position().row as u32 + 1,
                });
            }
            pending.extend(children(node).into_iter().rev().filter(|n| n.has_error()));
        }
        return Err(ParseError::Syntax {
            path: facts.file.path.clone(),
            line: 1,
        });
    }
    if let Some(first) = children(root).first()
        && first.kind() == "comment"
        && let Some(summary) = jsdoc(text(*first, source))
    {
        facts.file.nodes[0].summary.text = summary;
    }
    let mut pending = vec![(root, Scope::default())];
    while let Some((node, scope)) = pending.pop() {
        let context = item(node, source, &scope, facts)?.unwrap_or(scope);
        references(node, source, &context, facts);
        if matches!(node.kind(), "import_statement" | "comment") {
            continue;
        }
        pending.extend(
            children(node)
                .into_iter()
                .rev()
                .map(|n| (n, context.clone())),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Parser, PureParser};
    use dctx_map_model::{EdgeKind, SummarySource, SymbolKind};

    #[test]
    fn jsdoc_default_exports_and_class_fields_keep_their_owners() {
        let facts = PureParser
            .parse(
                "x.js",
                b"/** Module docs. */\nexport default function() { run(); }",
            )
            .unwrap();
        assert_eq!(facts.nodes[0].summary.text, "Module docs.");
        assert_eq!(facts.nodes[1].name, "default");
        assert!(facts.nodes[1].public);
        assert_eq!(facts.nodes[1].summary.source, SummarySource::Doc);
        assert_eq!(facts.precision.coverage(), Coverage::Structural);
        let facts = PureParser
            .parse(
                "x.js",
                b"const f = function internal() { work(); }; class C { run = () => f(); }",
            )
            .unwrap();
        assert_eq!(
            facts
                .nodes
                .iter()
                .map(|n| n.name.as_str())
                .collect::<Vec<_>>(),
            ["x.js", "f", "C", "C::run"]
        );
        assert_eq!(facts.nodes[3].symbol_kind, Some(SymbolKind::Method));
        assert_eq!(facts.references[0].src, facts.nodes[1].id);
        assert_eq!(facts.references[1].src, facts.nodes[3].id);
    }

    #[test]
    fn exports_requires_and_test_modifiers_keep_aliases() {
        let facts = PureParser
            .parse(
                "x.js",
                b"export { local as default }; export * as helpers from './helpers';",
            )
            .unwrap();
        let actual = facts
            .references
            .iter()
            .map(|r| (r.kind, r.name.as_str(), r.alias.as_deref()))
            .collect::<Vec<_>>();
        assert_eq!(
            actual,
            [
                (EdgeKind::Reexports, "local", Some("default")),
                (EdgeKind::Reexports, "./helpers", Some("helpers"))
            ]
        );
        let facts = PureParser
            .parse(
                "x.js",
                b"require(name); it.skip('case', () => worker.run());",
            )
            .unwrap();
        assert_eq!(
            facts
                .references
                .iter()
                .map(|r| (r.kind, r.name.as_str()))
                .collect::<Vec<_>>(),
            [
                (EdgeKind::Calls, "require"),
                (EdgeKind::Calls, "worker.run"),
                (EdgeKind::Tests, "worker.run")
            ]
        );
        assert_eq!(facts.references[1].src, facts.nodes[1].id);
    }
}
