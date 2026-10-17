//! Language dispatch for the pure-Rust parser and the optional tree-sitter oracle.
//! Implements SPEC-MAP-MODE section 6.2 and PARSERS-v1 for Rust, JavaScript,
//! TypeScript and Markdown. Unknown extensions stay module-only.
//! Uses the dctx-lex crates by default; tree-sitter compiles only with syntax-tree-sitter.
mod facts;
mod javascript;
mod markdown;
mod rust;

#[cfg(feature = "syntax-tree-sitter")]
mod javascript_items;
#[cfg(feature = "syntax-tree-sitter")]
mod javascript_references;
#[cfg(feature = "syntax-tree-sitter")]
mod rust_items;
#[cfg(feature = "syntax-tree-sitter")]
mod rust_references;

use crate::{Language, ParseError, ParsedFile, Parser};
use dctx_map_model::{Completion, Coverage, NodeKind, StableId};
use facts::{Facts, file_precision};

/// Pure-Rust language adapters with a module-only fallback.
///
/// Rust partial extraction is refused (`ParseError::Syntax`) so callers can
/// recover by masking. Line-scan facts are not reported as a complete parse.
#[derive(Debug, Clone, Copy, Default)]
pub struct PureParser;

impl Parser for PureParser {
    fn parse(&self, path: &str, bytes: &[u8]) -> Result<ParsedFile, ParseError> {
        let source = prepare(path, bytes)?;
        match Language::for_path(path) {
            Language::Rust => rust::parse_pure(path, source, bytes),
            Language::JavaScript | Language::TypeScript | Language::Tsx => {
                javascript::parse_pure(path, bytes)
            }
            Language::Markdown => markdown::parse_pure(path, source, bytes),
            Language::Unknown => fallback(path, source),
        }
    }
}

/// Tree-sitter language adapters. Compiled only with `syntax-tree-sitter`.
#[cfg(feature = "syntax-tree-sitter")]
#[derive(Debug, Clone, Copy, Default)]
pub struct TreeSitterParser;

#[cfg(feature = "syntax-tree-sitter")]
impl Parser for TreeSitterParser {
    fn parse(&self, path: &str, bytes: &[u8]) -> Result<ParsedFile, ParseError> {
        let source = prepare(path, bytes)?;
        let language = Language::for_path(path);
        if language == Language::Unknown {
            return fallback(path, source);
        }
        let precision = file_precision(Coverage::Syntax, Completion::Complete, "tree-sitter")?;
        let mut facts = Facts::new(path, source, language, precision)?;
        match language {
            Language::Rust => rust::parse(source, &mut facts)?,
            Language::JavaScript | Language::TypeScript | Language::Tsx => {
                javascript::parse(source, &mut facts)?;
            }
            Language::Markdown => markdown::parse(source, &mut facts)?,
            Language::Unknown => {}
        }
        facts.finish()
    }
}

fn prepare<'a>(path: &str, bytes: &'a [u8]) -> Result<&'a str, ParseError> {
    if bytes.len() >= u32::MAX as usize {
        return Err(ParseError::TooLarge);
    }
    let source = std::str::from_utf8(bytes)?;
    let _ = StableId::new(NodeKind::Module, path, None, None)?;
    Ok(source)
}

pub(super) fn fallback(path: &str, source: &str) -> Result<ParsedFile, ParseError> {
    let precision = file_precision(
        Coverage::Opaque,
        Completion::Complete,
        "dctx-map-parse/opaque",
    )?;
    Facts::new(path, source, Language::Unknown, precision)?.finish()
}

#[cfg(feature = "syntax-tree-sitter")]
pub(super) fn tree(
    source: &str,
    language: tree_sitter::Language,
) -> Result<tree_sitter::Tree, ParseError> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&language)
        .map_err(|error| ParseError::Grammar(error.to_string()))?;
    parser.parse(source, None).ok_or(ParseError::NoTree)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dctx_map_model::NodeKind;

    #[test]
    fn unknown_files_are_modules_with_comment_summaries() {
        let parsed = PureParser
            .parse("script.xyz", b"// Script notes. More.\nrun()")
            .unwrap();
        assert_eq!(parsed.nodes.len(), 1);
        assert_eq!(parsed.nodes[0].kind, NodeKind::Module);
        assert_eq!(parsed.nodes[0].summary.text, "Script notes.");
        assert_eq!(parsed.precision.coverage(), Coverage::Opaque);
        assert!(parsed.references.is_empty());
        assert!(PureParser.parse("empty.xyz", b"").is_ok());
    }

    #[test]
    fn invalid_input_is_refused_without_partial_facts() {
        assert!(matches!(
            PureParser.parse("../escape.rs", b""),
            Err(ParseError::InvalidId(_))
        ));
        assert!(matches!(
            PureParser.parse("x.rs", &[255]),
            Err(ParseError::InvalidUtf8(_))
        ));
        assert!(matches!(
            PureParser.parse("x.rs", b"fn broken("),
            Err(ParseError::Syntax { .. })
        ));
        let valid = PureParser.parse("x.rs", b"fn valid() {}").unwrap();
        assert_eq!(valid.precision.coverage(), Coverage::Syntax);
        assert_eq!(valid.precision.completion().as_str(), "complete");
    }
}
