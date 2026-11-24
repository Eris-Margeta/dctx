// SPDX-License-Identifier: Apache-2.0
//! Structural extractor for the facade's supported source and configuration languages.
//! Implements the strict parser profile: coverage `structural`.
//! Uses `lex_dispatch` and dctx-lex-md. Explicit structural Rust stays line-oriented; the default walk uses syn.
//! The snapshot walk calls [`prepare_structural`]. Answer ids and the structural precision label stay put.

use dctx_map_model::{NodeKind, StableId};

use super::IndexedEdge;
use super::refs::{RawRef, RefForm};
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
pub(crate) fn language_of(path: &str) -> &'static str {
    if path.rsplit('/').next() == Some("go.mod") {
        "config"
    } else {
        language::language_of(path)
    }
}
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
    if path.rsplit('/').next() == Some("go.mod") {
        let manifest = std::str::from_utf8(bytes)
            .ok()
            .and_then(super::refs::go::module_name)
            .map(|module| RawRef::fact(RefForm::GoModule, module, ""))
            .into_iter()
            .collect();
        return (Prepared::Symbols(Vec::new()), manifest);
    }
    if language_of(path) == "config" {
        return match dctx_lex_cfg::CfgLexer::default().extract(path, bytes) {
            Ok(lexed) if lexed.completion == dctx_lex_cfg::Completion::Complete => {
                let refs = super::refs::config::references(path, &lexed, bytes);
                (Prepared::Config(lexed), refs)
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
    let language = language_of(path);
    macro_rules! inventory {
        ($module:ident, $lexer:ident, $partial:expr $(,$typed:ident,$equals:ident)?) => {{
            match $module::$lexer::default().extract(path, source.as_bytes()) {
                Ok(facts) if $partial || facts.completion == $module::Completion::Complete => {
                    let spans = SpanIndex::build(source);
                    let projection = ReferenceProjection::new(language, source, &spans);
                    let mut refs = facts
                        .references
                        .into_iter()
                        .filter_map(|item| {
                            let type_only = false $(|| item.$typed)?;
                            let import_equals = false $(|| item.$equals)?;
                            let fact = item.fact;
                            let mut reference = RawRef::new(
                                fact.src.as_str(),
                                fact.kind,
                                &fact.name,
                                &fact.scope,
                                fact.alias.as_deref(),
                            )?;
                            if type_only {
                                reference.form = RefForm::TypeOnly;
                            }
                            if import_equals {
                                reference.form = RefForm::ImportEquals;
                            }
                            projection.project(
                                reference,
                                fact.lines,
                                item.precision == $module::Precision::Convention,
                            )
                        })
                        .collect();
                    let symbols = project_inventory(
                        &spans,
                        facts.nodes.into_iter().map(|n| n.fact),
                        &mut refs,
                        language,
                    );
                    (symbols, refs)
                }
                _ => (Vec::new(), Vec::new()),
            }
        }};
    }
    let (mut symbols, mut refs) = match language {
        "javascript" | "typescript" => {
            inventory!(dctx_lex_js, JsLexer, true, type_only, import_equals)
        }
        "php" => inventory!(dctx_lex_php, PhpLexer, false),
        "ruby" => inventory!(dctx_lex_rb, RubyLexer, false),
        "c" => inventory!(dctx_lex_c, CLexer, false),
        "go" => inventory!(dctx_lex_go, GoLexer, false),
        "java" => inventory!(dctx_lex_java, JavaLexer, false),
        "kotlin" => inventory!(dctx_lex_kt, KotlinLexer, false),
        "rust" => (rust_symbols(source), Vec::new()),
        "markdown" => markdown_symbols(path, source.as_bytes()),
        document if super::text_docs::handles(document) => {
            (super::text_docs::symbols(document, source), Vec::new())
        }
        language => super::lex_dispatch::lexed(language, path, source),
    };
    if path.rsplit('/').next() == Some("Package.swift") {
        for (target, deps) in dctx_lex_swift::SwiftLexer::default().package_targets(source) {
            refs.push(RawRef::fact(RefForm::Package, &target, ""));
            refs.extend(
                deps.into_iter()
                    .map(|dep| RawRef::fact(RefForm::SwiftDependency, &dep, &target)),
            );
        }
    }
    if language == "python" {
        let lexer = dctx_lex_py::PyLexer::default();
        if let Some(names) = super::export_list::names(language, source) {
            refs.extend(
                names
                    .into_iter()
                    .map(|name| RawRef::fact(RefForm::PythonAll, &name, "")),
            );
        }
        for symbol in symbols
            .iter()
            .filter(|s| matches!(s.kind.as_str(), "fn" | "method"))
        {
            refs.extend(
                lexer
                    .parameter_names(&symbol.signature)
                    .iter()
                    .map(|param| RawRef::fact(RefForm::Parameter, param, &symbol.name)),
            );
        }
    }
    for symbol in &mut symbols {
        symbol.span[0] += prefix;
        symbol.span[1] += prefix;
    }
    (symbols, refs)
}

/// Only languages with form-specific raw facts enter this projection.
struct ReferenceProjection<'a> {
    language: &'a str,
    source: &'a str,
    spans: &'a SpanIndex,
    php_kinds: std::collections::HashMap<(u32, String), &'static str>,
}

impl<'a> ReferenceProjection<'a> {
    fn new(language: &'a str, source: &'a str, spans: &'a SpanIndex) -> Self {
        let php_kinds = if language == "php" {
            dctx_lex_php::PhpLexer::default().use_kinds(source)
        } else {
            Default::default()
        };
        Self {
            language,
            source,
            spans,
            php_kinds,
        }
    }

    fn project(&self, mut reference: RawRef, lines: [u32; 2], convention: bool) -> Option<RawRef> {
        if reference.kind == dctx_map_model::EdgeKind::Reexports
            && convention
            && reference.alias.as_deref() == Some("export=")
        {
            reference.form = RefForm::ExportAssignment;
        }
        if reference.kind != dctx_map_model::EdgeKind::Imports {
            return Some(reference);
        }
        let [start, end] = self.spans.line_span(lines);
        let site = self
            .source
            .get(usize::try_from(start).ok()?..usize::try_from(end).ok()?)?;
        reference.form = match self.language {
            "javascript" | "typescript" if convention => RefForm::Require,
            "c" => match dctx_lex_c::CLexer::include_delimiter(site, &reference.name)? {
                '"' => RefForm::QuotedHeader,
                '<' => RefForm::AngleHeader,
                _ => return None,
            },
            "ruby" => match dctx_lex_rb::RubyLexer::literal_loader(site, &reference.name)? {
                "require_relative" => RefForm::RelativeLoad,
                "load" => RefForm::Load,
                "require" => RefForm::Require,
                _ => return None,
            },
            "php" => match self
                .php_kinds
                .get(&(lines[0], reference.name.clone()))
                .copied()
            {
                Some("function") => RefForm::PhpFunction,
                Some("const") => RefForm::PhpConst,
                Some("class") => RefForm::Namespace,
                Some("file")
                    if reference.name.starts_with("./") || reference.name.starts_with("../") =>
                {
                    RefForm::PhpFile
                }
                Some("directory") => {
                    let literal = reference.name.strip_prefix("__DIR__ . ")?;
                    let operand = literal
                        .get(1..literal.len().checked_sub(1)?)?
                        .strip_prefix('/')?;
                    reference.name = operand.to_owned();
                    RefForm::PhpDirectory
                }
                _ => return None,
            },
            _ => reference.form,
        };
        Some(reference)
    }
}

/// Keep declaration and import evidence before reducing lexer nodes to facade symbols.
fn project_inventory(
    spans: &SpanIndex,
    nodes: impl Iterator<Item = dctx_map_model::Node>,
    refs: &mut Vec<RawRef>,
    language: &str,
) -> Vec<Symbol> {
    nodes
        .filter_map(|node| {
            if node.kind == NodeKind::Package
                || (node.kind == NodeKind::Module && matches!(language, "java" | "kotlin"))
            {
                let name = if node.kind == NodeKind::Package {
                    &node.name
                } else {
                    &node.signature
                };
                refs.push(RawRef::fact(RefForm::Package, name, ""));
            }
            if matches!(
                node.kind,
                NodeKind::Module | NodeKind::Place | NodeKind::Package
            ) || node.name.is_empty()
            {
                return None;
            }
            if node.public {
                refs.push(RawRef::fact(
                    RefForm::Public,
                    &node.name,
                    node.symbol_kind
                        .map_or("", dctx_map_model::SymbolKind::as_str),
                ));
            }
            let graph = match node.kind {
                NodeKind::Doc | NodeKind::Test => node.kind,
                _ => NodeKind::Symbol,
            };
            let kind = if graph == NodeKind::Doc {
                "heading"
            } else {
                node.symbol_kind
                    .map_or("fn", dctx_map_model::SymbolKind::as_str)
            };
            Some(Symbol {
                name: node.name,
                kind: kind.to_owned(),
                graph,
                lines: node.lines,
                span: spans.line_span(node.lines),
                summary: node.summary.text,
                summary_source: node.summary.source.as_str().to_owned(),
                signature: node.signature,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
