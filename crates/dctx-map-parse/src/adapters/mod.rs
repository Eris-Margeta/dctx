//! Tree-sitter language dispatch and unsupported-file fallback.
//! Implements SPEC-MAP-MODE section 6.2 for Rust and Markdown.
//! Uses pinned grammars and domain facts; implements the Parser port.
mod facts;
mod markdown;
mod rust;
mod rust_items;
mod rust_references;

use crate::{Language, ParseError, ParsedFile, Parser};
use facts::Facts;

/// Rust and Markdown tree-sitter adapters with a module-only fallback.
#[derive(Debug, Clone, Copy, Default)]
pub struct TreeSitterParser;

impl Parser for TreeSitterParser {
    fn parse(&self, path: &str, bytes: &[u8]) -> Result<ParsedFile, ParseError> {
        if bytes.len() >= u32::MAX as usize {
            return Err(ParseError::TooLarge);
        }
        let source = std::str::from_utf8(bytes)?;
        let language = Language::for_path(path);
        let mut facts = Facts::new(path, source, language)?;
        match language {
            Language::Rust => rust::parse(source, &mut facts)?,
            Language::Markdown => markdown::parse(source, &mut facts)?,
            Language::Unknown => {}
        }
        facts.finish()
    }
}

pub(super) fn tree(
    source: &str,
    language: tree_sitter::Language,
) -> Result<tree_sitter::Tree, ParseError> {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&language)?;
    parser.parse(source, None).ok_or(ParseError::NoTree)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dctx_map_model::NodeKind;

    #[test]
    fn unknown_files_are_modules_with_comment_summaries() {
        let parsed = TreeSitterParser
            .parse("script.xyz", b"// Script notes. More.\nrun()")
            .unwrap();
        assert_eq!(parsed.nodes.len(), 1);
        assert_eq!(parsed.nodes[0].kind, NodeKind::Module);
        assert_eq!(parsed.nodes[0].summary.text, "Script notes.");
        assert!(parsed.references.is_empty());
        assert!(TreeSitterParser.parse("empty.xyz", b"").is_ok());
    }

    #[test]
    fn invalid_input_is_refused_without_partial_facts() {
        assert!(matches!(
            TreeSitterParser.parse("../escape.rs", b""),
            Err(ParseError::InvalidId(_))
        ));
        assert!(matches!(
            TreeSitterParser.parse("x.rs", &[255]),
            Err(ParseError::InvalidUtf8(_))
        ));
        assert!(matches!(
            TreeSitterParser.parse("x.rs", b"fn broken("),
            Err(ParseError::Syntax { .. })
        ));
        assert!(TreeSitterParser.parse("x.rs", b"fn valid() {}").is_ok());
    }
}
