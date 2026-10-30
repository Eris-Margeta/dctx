//! Language dispatch for the pure-Rust parser and the optional tree-sitter oracle.
//! Implements SPEC-MAP-MODE section 6.2 and PARSERS-v1 for Rust, JavaScript,
//! TypeScript, Markdown, Python, Go, Java, C/C++, configuration, C#, Kotlin, Ruby, PHP and Swift.
//! Uses the dctx-lex crates by default; tree-sitter compiles only with syntax-tree-sitter.
//! Languages without a tree-sitter grammar, including Swift, use the pure path in the oracle.
mod c;
mod config;
mod csharp;
mod facts;
mod go;
mod java;
mod javascript;
mod kotlin;
mod markdown;
mod php;
mod python;
mod ruby;
mod rust;
mod swift;

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
/// Partial or diagnostic extraction is refused (`ParseError::Syntax`) so
/// callers can recover by masking. Line-scan facts are not a complete parse.
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
            Language::Python => python::parse_pure(path, bytes),
            Language::Go => go::parse_pure(path, bytes),
            Language::Java => java::parse_pure(path, bytes),
            Language::C => c::parse_pure(path, bytes),
            Language::Config => config::parse_pure(path, bytes),
            Language::CSharp => csharp::parse_pure(path, bytes),
            Language::Kotlin => kotlin::parse_pure(path, bytes),
            Language::Ruby => ruby::parse_pure(path, bytes),
            Language::Php => php::parse_pure(path, bytes),
            Language::Swift => swift::parse_pure(path, bytes),
            // Heuristic families are extracted by the facade; here they stay module-only.
            Language::Scala | Language::Dart | Language::Zig | Language::Solidity | Language::Gleam | Language::Groovy | Language::Text => {
                fallback(path, source)
            }
            Language::Elixir | Language::Lua | Language::OCaml | Language::R | Language::Julia | Language::Haskell => {
                fallback(path, source)
            }
            Language::Unknown => fallback(path, source),
        }
    }
}

/// Tree-sitter language adapters. Compiled only with `syntax-tree-sitter`.
///
/// Python, Go, Java, C/C++, configuration, C#, Kotlin, Ruby, PHP and Swift have no grammar here.
/// The oracle falls back to the pure adapters with the same precision and refusals.
#[cfg(feature = "syntax-tree-sitter")]
#[derive(Debug, Clone, Copy, Default)]
pub struct TreeSitterParser;

#[cfg(feature = "syntax-tree-sitter")]
impl Parser for TreeSitterParser {
    fn parse(&self, path: &str, bytes: &[u8]) -> Result<ParsedFile, ParseError> {
        let source = prepare(path, bytes)?;
        let language = Language::for_path(path);
        // No tree-sitter grammar ships for these languages. The oracle uses
        // the pure adapters so the feature still returns structural facts.
        match language {
            Language::Python => return python::parse_pure(path, bytes),
            Language::Go => return go::parse_pure(path, bytes),
            Language::Java => return java::parse_pure(path, bytes),
            Language::C => return c::parse_pure(path, bytes),
            Language::Config => return config::parse_pure(path, bytes),
            Language::CSharp => return csharp::parse_pure(path, bytes),
            Language::Kotlin => return kotlin::parse_pure(path, bytes),
            Language::Ruby => return ruby::parse_pure(path, bytes),
            Language::Php => return php::parse_pure(path, bytes),
            Language::Swift => return swift::parse_pure(path, bytes),
            Language::Scala | Language::Dart | Language::Zig | Language::Solidity | Language::Gleam | Language::Groovy | Language::Text => {
                return fallback(path, source);
            }
            Language::Elixir | Language::Lua | Language::OCaml | Language::R | Language::Julia | Language::Haskell => {
                return fallback(path, source);
            }
            Language::Unknown => return fallback(path, source),
            Language::Rust
            | Language::JavaScript
            | Language::TypeScript
            | Language::Tsx
            | Language::Markdown => {}
        }
        let precision = file_precision(Coverage::Syntax, Completion::Complete, "tree-sitter")?;
        let mut facts = Facts::new(path, source, language, precision)?;
        match language {
            Language::Rust => rust::parse(source, &mut facts)?,
            Language::JavaScript | Language::TypeScript | Language::Tsx => {
                javascript::parse(source, &mut facts)?;
            }
            Language::Markdown => markdown::parse(source, &mut facts)?,
            Language::Python
            | Language::Go
            | Language::Java
            | Language::C
            | Language::Config
            | Language::CSharp
            | Language::Kotlin
            | Language::Ruby
            | Language::Php
            | Language::Swift
            | Language::Scala
            | Language::Dart
            | Language::Zig
            | Language::Solidity
            | Language::Gleam
            | Language::Groovy
            | Language::Text
            | Language::Elixir
            | Language::Lua
            | Language::OCaml
            | Language::R
            | Language::Julia
            | Language::Haskell
            | Language::Unknown => {}
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
        assert_eq!(parsed.references, [] as [crate::RawReference; 0]);
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
