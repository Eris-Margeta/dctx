//! Java facts from dctx-lex-java for PureParser and the tree-sitter fallback.
//! Implements SPEC-MAP-MODE sections 5 and 6.2 and the PARSERS-v1 Java row.
//! The lexer emits model nodes; this adapter copies nodes, edges and references.
//! A partial or diagnostic result is ParseError::Syntax. LexError maps to ParseError.
use super::facts::file_precision;
use crate::{Language, ParseError, ParsedFile};
use dctx_lex_java::{Completion, JavaLexer, LexError};
use dctx_map_model::{Completion as FileCompletion, Coverage};

/// Copies a complete structural extraction. Diagnostics refuse the whole file.
pub(super) fn parse_pure(path: &str, bytes: &[u8]) -> Result<ParsedFile, ParseError> {
    let lexed = JavaLexer::default()
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
    let parser = format!("{}:{}", dctx_lex_java::PARSER_ID, dctx_lex_java::PARSER_REVISION);
    Ok(ParsedFile {
        path: lexed.path,
        language: Language::for_path(path),
        nodes: lexed.nodes.into_iter().map(|labeled| labeled.fact).collect(),
        edges: lexed.edges.into_iter().map(|labeled| labeled.fact).collect(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Parser, PureParser};

    #[test]
    fn complete_method_is_structural_and_an_open_brace_is_refused() {
        let facts = PureParser
            .parse("A.java", b"class A { void run() {} }\n")
            .unwrap();
        assert_eq!(facts.language, Language::Java);
        assert_eq!(facts.precision.coverage(), Coverage::Structural);
        assert_eq!(facts.precision.completion().as_str(), "complete");
        assert!(matches!(
            PureParser.parse("A.java", b"class A { void m() {"),
            Err(ParseError::Syntax { .. })
        ));
    }
}
