//! configuration facts from dctx-lex-cfg for PureParser and the tree-sitter fallback.
//! Implements SPEC-MAP-MODE sections 5 and 6.2 and the PARSERS-v1 configuration row.
//! The lexer emits model nodes; this adapter copies nodes, edges and references.
//! A partial or diagnostic result is ParseError::Syntax. LexError maps to ParseError.
use super::facts::file_precision;
use crate::{Language, ParseError, ParsedFile};
use dctx_lex_cfg::{CfgLexer, Completion, LexError};
use dctx_map_model::{Completion as FileCompletion, Coverage};

/// Copies a complete structural extraction. Diagnostics refuse the whole file.
pub(super) fn parse_pure(path: &str, bytes: &[u8]) -> Result<ParsedFile, ParseError> {
    let lexed = CfgLexer::default()
        .extract(path, bytes)
        .map_err(|error| map_error(path, error))?;
    if lexed.completion != Completion::Complete || !lexed.diagnostics.is_empty() {
        let line = lexed
            .diagnostics
            .first()
            .map(|diagnostic| diagnostic.line.max(1))
            .unwrap_or(1);
        return Err(ParseError::Syntax {
            path: path.into(),
            line,
        });
    }
    let parser = format!(
        "{}:{}",
        dctx_lex_cfg::PARSER_ID,
        dctx_lex_cfg::PARSER_REVISION
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

fn map_error(path: &str, error: LexError) -> ParseError {
    match error {
        LexError::InvalidId(error) => ParseError::InvalidId(error),
        LexError::InvalidUtf8(error) => ParseError::InvalidUtf8(error),
        LexError::TooLarge { .. } | LexError::TooManyFacts { .. } => ParseError::TooLarge,
        LexError::TooDeep { line, .. } => ParseError::Syntax {
            path: path.into(),
            line: line.max(1),
        },
        LexError::UnsupportedDialect => ParseError::Syntax {
            path: path.into(),
            line: 1,
        },
    }
}
