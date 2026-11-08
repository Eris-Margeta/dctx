// SPDX-License-Identifier: Apache-2.0
//! Structural extractor for the facade's supported source and configuration languages.
//! Implements the strict parser profile: coverage `structural`.
//! Uses `lex_dispatch` and dctx-lex-md. Explicit structural Rust stays line-oriented; the default walk uses syn.
//! The snapshot walk calls [`prepare_structural`]. Answer ids and the structural precision label stay put.

use dctx_map_model::{NodeKind, StableId};

use super::IndexedEdge;
use super::refs::RawRef;
use super::span_index::SpanIndex;
use super::{IndexedNode, TextBlob, TextSpan};

mod graph;
mod language;
mod source;

pub(crate) use graph::index_prepared;
#[cfg(test)]
pub(crate) use graph::index_symbols;
#[cfg(test)]
use graph::{letter_byte, stored_ordinal};
pub(crate) use language::language_of;
#[cfg(test)]
pub(super) use language::{EXTENSIONS, INFRA_BASENAMES, RUBY_BASENAMES};
#[cfg(test)]
use source::SIGNATURE_CAP;
pub(crate) use source::signature_text;
use source::{markdown_symbols, rust_symbols};

/// Facts ready to enter the graph. A complete config lex is projected once.
pub(crate) enum Prepared {
    /// Rust, Markdown and other languages, already projected to symbols.
    Symbols(Vec<Symbol>),
    /// Complete configuration lex. Names are unique, so ids have no `~` ordinal.
    Config(dctx_lex_cfg::LexedFile),
}

/// One declaration before stable ids are assigned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Symbol {
    /// Name as it should appear after `#` in the stable id.
    pub name: String,
    /// Wire kind.
    pub kind: String,
    /// Graph category. Doc headings use `d:`; tests use `t:`; other declarations use `s:`.
    pub graph: NodeKind,
    /// Inclusive 1-based lines.
    pub lines: [u32; 2],
    /// Byte span `[start, end)`.
    pub span: [u64; 2],
    /// Summary text.
    pub summary: String,
    /// `doc`, `heading` or `sig`.
    pub summary_source: String,
    /// Signature without the body.
    pub signature: String,
}

/// Lexer output for one included file and its raw call and import references. Config keeps the lex so the graph can move it once.
pub(crate) fn prepare_structural(path: &str, bytes: &[u8]) -> (Prepared, Vec<RawRef>) {
    if bytes.contains(&0) || std::str::from_utf8(bytes).is_err() {
        return (Prepared::Symbols(Vec::new()), Vec::new());
    }
    if language_of(path) == "config" {
        return match dctx_lex_cfg::CfgLexer::default().extract(path, bytes) {
            Ok(lexed) if lexed.completion == dctx_lex_cfg::Completion::Complete => {
                (Prepared::Config(lexed), Vec::new())
            }
            _ => (Prepared::Symbols(Vec::new()), Vec::new()),
        };
    }
    let (symbols, refs) = extract_structural_refs(path, bytes);
    (Prepared::Symbols(symbols), refs)
}

/// Structural facts. NUL bytes yield no symbols; the caller still hashes the file.
#[cfg(test)]
pub(crate) fn extract_structural(path: &str, bytes: &[u8]) -> Vec<Symbol> {
    extract_structural_refs(path, bytes).0
}

/// Structural facts and the lexer's raw call and import references. Rust and Markdown carry none here.
pub(crate) fn extract_structural_refs(path: &str, bytes: &[u8]) -> (Vec<Symbol>, Vec<RawRef>) {
    if bytes.contains(&0) {
        return (Vec::new(), Vec::new());
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return (Vec::new(), Vec::new());
    };
    // A leading UTF-8 BOM is encoding metadata, not a declaration token.
    // Keep spans relative to the authorized physical bytes for symbol reads.
    let source = text.strip_prefix('\u{feff}').unwrap_or(text);
    let prefix = u64::try_from(text.len() - source.len()).unwrap_or(0);
    let (mut symbols, refs) = match language_of(path) {
        "rust" => (rust_symbols(source), Vec::new()),
        "markdown" => (markdown_symbols(source.as_bytes()), Vec::new()),
        document if super::text_docs::handles(document) => {
            (super::text_docs::symbols(document, source), Vec::new())
        }
        language => super::lex_dispatch::lexed(language, path, source),
    };
    for symbol in &mut symbols {
        symbol.span[0] += prefix;
        symbol.span[1] += prefix;
    }
    (symbols, refs)
}

#[cfg(test)]
mod tests;
