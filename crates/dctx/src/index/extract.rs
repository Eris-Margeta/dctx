//! Structural extractor for the facade's supported source and configuration languages.
//! Implements the strict parser profile: coverage `structural`.
//! Uses `lex_dispatch` and dctx-lex-md. Explicit structural Rust stays line-oriented; the default walk uses syn.
//! The snapshot walk calls [`prepare_structural`]. Answer ids and the structural precision label stay put.

use dctx_map_model::{NodeKind, StableId};

use super::refs::RawRef;
use super::span_index::SpanIndex;
use super::{IndexedNode, TextBlob, TextSpan};

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

/// Duplicate of `dctx-map-parse` `domain::facts::EXTENSIONS`.
///
/// This crate does not depend on `dctx-map-parse`, so the table cannot be
/// shared by a dependency. `tsx` is tagged `typescript` (the JS lexer owns it).
/// Tests lock this copy to the same literal list as map-parse.
const EXTENSIONS: &[(&str, &str)] = &[
    ("rs", "rust"),
    ("js", "javascript"),
    ("mjs", "javascript"),
    ("cjs", "javascript"),
    ("jsx", "javascript"),
    ("ts", "typescript"),
    ("mts", "typescript"),
    ("cts", "typescript"),
    ("tsx", "typescript"),
    ("md", "markdown"),
    ("markdown", "markdown"),
    ("py", "python"),
    ("pyi", "python"),
    ("go", "go"),
    ("java", "java"),
    ("c", "c"),
    ("h", "c"),
    ("cc", "c"),
    ("cpp", "c"),
    ("cxx", "c"),
    ("hpp", "c"),
    ("hh", "c"),
    ("json", "config"),
    ("jsonc", "config"),
    ("toml", "config"),
    ("yaml", "config"),
    ("yml", "config"),
    ("cs", "csharp"),
    ("kt", "kotlin"),
    ("kts", "kotlin"),
    ("rb", "ruby"),
    ("rake", "ruby"),
    ("gemspec", "ruby"),
    ("php", "php"),
    ("phtml", "php"),
    ("inc", "php"),
    ("swift", "swift"),
    ("scala", "scala"),
    ("sc", "scala"),
    ("dart", "dart"),
    ("zig", "zig"),
    ("sol", "solidity"),
    ("gleam", "gleam"),
    ("perl", "perl"),
    ("groovy", "groovy"),
    ("gradle", "groovy"),
    ("txt", "text"),
    ("rst", "rst"),
    ("adoc", "asciidoc"),
    ("asciidoc", "asciidoc"),
    ("org", "org"),
    ("tex", "tex"),
    ("ex", "elixir"),
    ("exs", "elixir"),
    ("lua", "lua"),
    ("ml", "ocaml"),
    ("mli", "ocaml"),
    ("r", "r"),
    ("jl", "julia"),
    ("hs", "haskell"),
    ("tf", "hcl"),
    ("tfvars", "hcl"),
    ("hcl", "hcl"),
    ("sql", "sql"),
    ("sh", "shell"),
    ("bash", "shell"),
    ("zsh", "shell"),
    ("bats", "shell"),
    ("mk", "make"),
    ("proto", "proto"),
    ("pas", "pascal"),
    ("pp", "pascal"),
    ("dpr", "pascal"),
    ("graphql", "graphql"),
    ("gql", "graphql"),
    ("vue", "web"),
    ("svelte", "web"),
    ("astro", "web"),
    ("html", "web"),
    ("htm", "web"),
    ("clj", "clojure"),
    ("cljs", "clojure"),
    ("cljc", "clojure"),
    ("edn", "clojure"),
    ("lisp", "lisp"),
    ("lsp", "lisp"),
    ("cl", "lisp"),
    ("scm", "scheme"),
    ("ss", "scheme"),
    ("el", "elisp"),
    ("nix", "nix"),
];

/// Duplicate of `dctx-map-parse` `domain::facts::RUBY_BASENAMES`.
///
/// `dctx-lex-rb` accepts these final segments case-insensitively, and rejects
/// `Guardfile` and `config.ru`. A basename wins over the extension.
const RUBY_BASENAMES: &[&str] = &["Rakefile", "Gemfile"];

/// Duplicate of `dctx-map-parse` `domain::facts::INFRA_BASENAMES`: files with no extension that are still a known format.
///
/// Compared case-insensitively on the final path segment, like [`RUBY_BASENAMES`].
const INFRA_BASENAMES: &[(&str, &str)] =
    &[("Makefile", "make"), ("GNUmakefile", "make"), ("Dockerfile", "docker"), ("Containerfile", "docker")];

/// Language tag from the final path segment. Anything else is `opaque`.
///
/// Extensions are case-insensitive. See [`EXTENSIONS`] and [`RUBY_BASENAMES`].
pub(crate) fn language_of(path: &str) -> &'static str {
    let base = path.rsplit(['/', '\\']).next().unwrap_or(path);
    if RUBY_BASENAMES.iter().any(|name| base.eq_ignore_ascii_case(name)) {
        return "ruby";
    }
    if let Some((_, tag)) = INFRA_BASENAMES.iter().find(|(name, _)| base.eq_ignore_ascii_case(name)) {
        return tag;
    }
    if is_sql_template(base) {
        return "sql";
    }
    let Some(extension) = base.rsplit_once('.').map(|(_, extension)| extension.to_ascii_lowercase()) else {
        return "opaque";
    };
    EXTENSIONS
        .iter()
        .find(|(name, _)| *name == extension)
        .map_or("opaque", |(_, tag)| *tag)
}

/// True for `pgtap.sql.in`: a SQL template that a build step turns into `.sql`. Duplicate of `dctx-map-parse` `facts.rs`.
fn is_sql_template(base: &str) -> bool {
    base.len() > ".sql.in".len() && base.get(base.len() - ".sql.in".len()..).is_some_and(|tail| tail.eq_ignore_ascii_case(".sql.in"))
}

/// Lexer output for one included file and its raw call and import references. Config keeps the lex so the graph can move it once.
pub(crate) fn prepare_structural(path: &str, bytes: &[u8]) -> (Prepared, Vec<RawRef>) {
    if bytes.contains(&0) || std::str::from_utf8(bytes).is_err() {
        return (Prepared::Symbols(Vec::new()), Vec::new());
    }
    if language_of(path) == "config" {
        return match dctx_lex_cfg::CfgLexer::default().extract(path, bytes) {
            Ok(lexed) if lexed.completion == dctx_lex_cfg::Completion::Complete => (Prepared::Config(lexed), Vec::new()),
            _ => (Prepared::Symbols(Vec::new()), Vec::new()),
        };
    }
    let (symbols, refs) = extract_structural_refs(path, bytes);
    (Prepared::Symbols(symbols), refs)
}

/// Projects `prepared` into `nodes` and `edges`. Returns the symbol count charged to the file.
pub(crate) fn index_prepared(
    path: &str,
    bytes: &[u8],
    path_span: TextSpan,
    module_index: u32,
    prepared: Prepared,
    nodes: &mut Vec<IndexedNode>,
    edges: &mut Vec<super::IndexedEdge>,
    text: &mut TextBlob,
) -> usize {
    match prepared {
        Prepared::Symbols(symbols) => {
            let count = symbols.len();
            index_symbols(path, path_span, module_index, symbols, nodes, edges, text);
            count
        }
        Prepared::Config(lexed) => append_cfg(path, bytes, path_span, module_index, lexed, nodes, edges, text),
    }
}

/// Moves a complete config lex into the graph. Duplicate keys never reach a complete lex.
fn append_cfg(
    path: &str,
    bytes: &[u8],
    path_span: TextSpan,
    module_index: u32,
    mut lexed: dctx_lex_cfg::LexedFile,
    nodes: &mut Vec<IndexedNode>,
    edges: &mut Vec<super::IndexedEdge>,
    text: &mut TextBlob,
) -> usize {
    let Ok(body) = std::str::from_utf8(bytes) else {
        return 0;
    };
    let spans = SpanIndex::build(body);
    let facts = std::mem::take(&mut lexed.nodes);
    drop(lexed);
    let edge = text.push("contains");
    let confidence = text.push("exact");
    let path_ok = StableId::new(NodeKind::Module, path, None, None).is_ok();
    nodes.reserve(facts.len());
    edges.reserve(facts.len());
    let mut kept = 0usize;
    for labeled in facts {
        let node = labeled.fact;
        if matches!(node.kind, NodeKind::Module | NodeKind::Place | NodeKind::Package) || node.name.is_empty() {
            continue;
        }
        let graph = match node.kind {
            NodeKind::Doc => NodeKind::Doc,
            NodeKind::Test => NodeKind::Test,
            _ => NodeKind::Symbol,
        };
        if stored_ordinal(graph, path, &node.name, None, path_ok).is_none() {
            continue;
        }
        let Some(dst) = u32::try_from(nodes.len()).ok() else {
            break;
        };
        let kind_label = if graph == NodeKind::Doc {
            "heading"
        } else {
            node.symbol_kind.map_or("fn", dctx_map_model::SymbolKind::as_str)
        };
        let source_label = node.summary.source.as_str();
        let lines = node.lines;
        nodes.push(IndexedNode {
            letter: letter_byte(graph),
            ordinal: 0,
            kind: reuse_label(node.fingerprint, kind_label),
            name: node.name,
            path: path_span,
            lines,
            span: spans.line_span(lines),
            summary: node.summary.text,
            summary_source: reuse_label(node.path, source_label),
            signature: node.signature,
        });
        edges.push(super::IndexedEdge { src: module_index, dst, edge, confidence });
        kept = kept.saturating_add(1);
    }
    kept
}

fn reuse_label(mut slot: String, label: &str) -> String {
    slot.clear();
    slot.push_str(label);
    slot
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
    match language_of(path) {
        "rust" => (rust_symbols(text), Vec::new()),
        "markdown" => (markdown_symbols(bytes), Vec::new()),
        document if super::text_docs::handles(document) => (super::text_docs::symbols(document, text), Vec::new()),
        language => super::lex_dispatch::lexed(language, path, text),
    }
}

/// Assigns stable ids and contains edges for one file's symbols.
///
/// Consumes `symbols` so names, kinds and summaries move into the graph.
/// The id text is not built here. Rows keep the letter, the ordinal and the shared path.
/// `module_index` is the module node already pushed for this file.
pub(crate) fn index_symbols(
    path: &str,
    path_span: TextSpan,
    module_index: u32,
    symbols: Vec<Symbol>,
    nodes: &mut Vec<IndexedNode>,
    edges: &mut Vec<super::IndexedEdge>,
    text: &mut TextBlob,
) {
    let ordinals = duplicate_ordinals(&symbols);
    let path_ok = StableId::new(NodeKind::Module, path, None, None).is_ok();
    let edge = text.push("contains");
    let confidence = text.push("exact");
    nodes.reserve(ordinals.len());
    edges.reserve(ordinals.len());
    for (symbol, ordinal) in symbols.into_iter().zip(ordinals) {
        let Some(ordinal) = stored_ordinal(symbol.graph, path, &symbol.name, ordinal, path_ok) else {
            continue;
        };
        let Some(dst) = u32::try_from(nodes.len()).ok() else {
            break;
        };
        nodes.push(IndexedNode {
            letter: letter_byte(symbol.graph),
            ordinal,
            kind: symbol.kind,
            name: symbol.name,
            path: path_span,
            lines: symbol.lines,
            span: symbol.span,
            summary: symbol.summary,
            summary_source: symbol.summary_source,
            signature: symbol.signature,
        });
        edges.push(super::IndexedEdge { src: module_index, dst, edge, confidence });
    }
}

/// First occurrence keeps an empty ordinal. Later copies use 2, 3, … in source order.
#[allow(clippy::cast_possible_truncation, reason = "The hash is an intentionally native-word table index; truncation keeps its low bits before masking to a power-of-two capacity.")]
fn duplicate_ordinals(symbols: &[Symbol]) -> Vec<Option<u32>> {
    let capacity = table_len(symbols.len());
    let mask = capacity - 1;
    let mut slots = vec![Slot { hash: 0, index: 0, count: 0 }; capacity];
    let mut ordinals = Vec::with_capacity(symbols.len());
    for (index, symbol) in symbols.iter().enumerate() {
        let mut hash = fx_hash(symbol.name.as_bytes());
        if hash == 0 {
            hash = 1;
        }
        let mut slot = (hash as usize) & mask;
        let stored = loop {
            let entry = &mut slots[slot];
            if entry.hash == 0 {
                entry.hash = hash;
                entry.index = u32::try_from(index).unwrap_or(u32::MAX);
                entry.count = 1;
                break None;
            }
            if entry.hash == hash {
                let prior = usize::try_from(entry.index).unwrap_or(usize::MAX);
                if symbols.get(prior).is_some_and(|other| other.name == symbol.name) {
                    entry.count = entry.count.saturating_add(1);
                    break (entry.count > 1).then_some(entry.count);
                }
            }
            slot = (slot + 1) & mask;
        };
        ordinals.push(stored);
    }
    ordinals
}

/// `0` when this is the first copy. [`StableId`] still rejects a name that cannot be stored.
fn stored_ordinal(kind: NodeKind, path: &str, name: &str, ordinal: Option<u32>, path_ok: bool) -> Option<u32> {
    if ordinal == Some(0) {
        return None;
    }
    let stored = ordinal.unwrap_or(0);
    if path_ok && name_is_plain(name) {
        return Some(stored);
    }
    StableId::new(kind, path, Some(name), ordinal).ok()?;
    Some(stored)
}

fn letter_byte(kind: NodeKind) -> u8 {
    match kind {
        NodeKind::Place => b'p',
        NodeKind::Package => b'k',
        NodeKind::Module => b'm',
        NodeKind::Test => b't',
        NodeKind::Doc => b'd',
        NodeKind::Symbol => b's',
    }
}

#[derive(Clone, Copy)]
struct Slot {
    hash: u64,
    index: u32,
    count: u32,
}

fn table_len(count: usize) -> usize {
    count.saturating_mul(2).max(16).checked_next_power_of_two().unwrap_or(1_usize << (usize::BITS - 1))
}

fn fx_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0;
    let mut offset = 0;
    while offset + 8 <= bytes.len() {
        let mut chunk = [0u8; 8];
        chunk.copy_from_slice(&bytes[offset..offset + 8]);
        hash = (hash.rotate_left(5) ^ u64::from_le_bytes(chunk)).wrapping_mul(0x517c_c1b7_2722_0a95);
        offset += 8;
    }
    if offset < bytes.len() {
        let mut tail: u64 = 0;
        for (shift, byte) in bytes[offset..].iter().copied().enumerate() {
            tail |= u64::from(byte) << (shift * 8);
        }
        hash = (hash.rotate_left(5) ^ tail).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
    hash
}

fn name_is_plain(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut ascii = true;
    for byte in name.bytes() {
        if byte < 0x20 || byte == 0x7f || byte == b'#' || byte == b'~' {
            return false;
        }
        if byte >= 0x80 {
            ascii = false;
        }
    }
    ascii || !name.chars().any(char::is_control)
}

fn rust_symbols(text: &str) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    let mut pending: Option<String> = None;
    let mut offset = 0usize;
    for (line_no, line) in (1u32..).zip(text.split_inclusive('\n')) {
        let body_len = line.trim_end_matches(['\r', '\n']).len();
        let body = &line[..body_len];
        let trimmed = body.trim();
        if trimmed.is_empty() {
            pending = None;
        } else if let Some(stripped) = trimmed.strip_prefix("///") {
            let rest = stripped.strip_prefix(' ').unwrap_or(stripped);
            if pending.is_none() && !rest.is_empty() {
                pending = Some(rest.to_string());
            }
        } else if trimmed.starts_with("#[") || trimmed.starts_with("//!") {
            // An attribute keeps a pending doc. A module doc is not a symbol.
        } else if let Some((kind, name)) = rust_decl(trimmed) {
            let mut symbol = placed(name, kind, NodeKind::Symbol, line_no, offset, body_len);
            symbol.signature = signature_text(trimmed);
            symbol.summary_source = if pending.is_some() { "doc".to_string() } else { "sig".to_string() };
            symbol.summary = pending.take().unwrap_or_else(|| symbol.signature.clone());
            symbols.push(symbol);
        } else {
            pending = None;
        }
        offset += line.len();
    }
    symbols
}

fn rust_decl(line: &str) -> Option<(&'static str, String)> {
    for (keyword, kind) in [("fn", "fn"), ("struct", "struct"), ("enum", "enum"), ("trait", "trait"), ("mod", "mod")] {
        if let Some(name) = keyword_name(line, keyword) {
            return Some((kind, name));
        }
    }
    None
}

fn markdown_symbols(bytes: &[u8]) -> Vec<Symbol> {
    let Ok(document) = dctx_lex_md::extract(bytes) else { return Vec::new() };
    let mut symbols = Vec::new();
    for heading in document.headings {
        if heading.title.is_empty() {
            continue;
        }
        let summary = if heading.summary.is_empty() { heading.title.clone() } else { heading.summary };
        symbols.push(Symbol {
            name: heading.title,
            kind: "heading".to_string(),
            graph: NodeKind::Doc,
            lines: heading.lines,
            span: [
                u64::try_from(heading.span.start).unwrap_or(u64::MAX),
                u64::try_from(heading.span.end).unwrap_or(u64::MAX),
            ],
            summary,
            summary_source: "heading".to_string(),
            signature: String::new(),
        });
    }
    symbols
}

fn keyword_name(line: &str, keyword: &str) -> Option<String> {
    if keyword.is_empty() {
        return ident_of(line);
    }
    let bytes = line.as_bytes();
    let key = keyword.as_bytes();
    let mut index = 0usize;
    while index + key.len() <= bytes.len() {
        let before_ok = index == 0 || !is_ident(bytes[index - 1]);
        let after = index + key.len();
        let after_ok = after == bytes.len() || !is_ident(bytes[after]);
        if before_ok && after_ok && bytes[index..].starts_with(key)
            && let Some(name) = ident_of(line[after..].trim_start())
        {
            return Some(name);
        }
        index += 1;
    }
    None
}

fn ident_of(text: &str) -> Option<String> {
    let mut name = String::new();
    for (index, ch) in text.chars().enumerate() {
        let ok = ch == '_' || ch.is_ascii_alphanumeric();
        let start_ok = index != 0 || ch == '_' || ch.is_ascii_alphabetic();
        if index == 0 && !start_ok {
            return None;
        }
        if !ok {
            break;
        }
        name.push(ch);
    }
    if name.is_empty() { None } else { Some(name) }
}

fn is_ident(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
}

fn placed(name: String, kind: &str, graph: NodeKind, line_no: u32, offset: usize, body_len: usize) -> Symbol {
    Symbol {
        name,
        kind: kind.to_string(),
        graph,
        lines: [line_no, line_no],
        span: [u64::try_from(offset).unwrap_or(u64::MAX), u64::try_from(offset + body_len).unwrap_or(u64::MAX)],
        summary: String::new(),
        summary_source: String::new(),
        signature: String::new(),
    }
}

/// Longest signature kept, in characters. A signature is a label for a record page, never the body.
const SIGNATURE_CAP: usize = 160;

/// Signature ending at the matching parenthesis, or the text before a body brace. The result is one line of at most
/// [`SIGNATURE_CAP`] characters cut on a char boundary; a call-style declaration `test('name', () => ...)` stops after its first argument.
pub(crate) fn signature_text(text: &str) -> String {
    let trimmed = text.trim();
    let whole = if let Some(head) = call_head(trimmed) {
        head
    } else if let Some(start) = trimmed.find('(')
        && let Some(end) = matching_paren(trimmed, start)
    {
        trimmed[..=end].trim().to_string()
    } else {
        let cut = trimmed.find('{').unwrap_or(trimmed.len());
        trimmed[..cut].trim().trim_end_matches(';').trim().to_string()
    };
    let line = whole.lines().next().unwrap_or_default().trim_end();
    match line.char_indices().nth(SIGNATURE_CAP) {
        Some((cut, _)) => line[..cut].trim_end().to_string(),
        None => line.to_string(),
    }
}

/// `callee('literal', ...)` for a call whose first argument is a string literal followed by more arguments, else `None`.
fn call_head(text: &str) -> Option<String> {
    let open = text.find('(')?;
    let callee = &text[..open];
    if callee.is_empty() || !callee.bytes().all(|byte| is_ident(byte) || byte == b'.') {
        return None;
    }
    let rest = &text[open + 1..];
    let quote = rest.chars().next().filter(|ch| matches!(ch, '\'' | '"' | '`'))?;
    let mut escaped = false;
    let mut close = None;
    for (index, ch) in rest.char_indices().skip(1) {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == quote {
            close = Some(index);
            break;
        }
    }
    let literal_end = close? + quote.len_utf8();
    rest[literal_end..].trim_start().starts_with(',').then(|| format!("{callee}({}, ...)", &rest[..literal_end]))
}

fn matching_paren(text: &str, open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    for (index, byte) in bytes.iter().enumerate().skip(open) {
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_rust_fn_keeps_doc_signature_and_span() {
        let bytes = b"/// Respond.\npub fn ping() {}\n";
        let symbols = extract_structural("src/lib.rs", bytes);
        assert_eq!(symbols.len(), 1);
        let symbol = &symbols[0];
        assert_eq!(symbol.name, "ping");
        assert_eq!(symbol.kind, "fn");
        assert_eq!(symbol.lines, [2, 2]);
        assert_eq!(symbol.span, [13, 29]);
        assert_eq!(symbol.summary, "Respond.");
        assert_eq!(symbol.summary_source, "doc");
        assert_eq!(symbol.signature, "pub fn ping()");
        assert_eq!(&bytes[13..29], b"pub fn ping() {}");
    }

    #[test]
    fn signature_text_is_one_short_line_and_call_style_stops_at_the_first_argument() {
        let long = format!("pub fn big({}) {{}}", "a".repeat(400));
        let clipped = signature_text(&long);
        assert_eq!(clipped.chars().count(), SIGNATURE_CAP);
        assert!(clipped.starts_with("pub fn big(aaa"));
        let wide = format!("fn w(\n    x: u8,\n) {{}}{}", "\u{e9}".repeat(300));
        assert_eq!(signature_text(&wide), "fn w(");
        let accents = format!("fn {}() {{}}", "\u{e9}".repeat(300));
        assert_eq!(signature_text(&accents).chars().count(), SIGNATURE_CAP);
        let call = format!("test(\"case 1 {}\", () => {{\n  target();\n}});", "x".repeat(2000));
        let head = signature_text(&call);
        assert!(head.starts_with("test(\"case 1 xxx") && head.chars().count() <= SIGNATURE_CAP);
        assert_eq!(signature_text("it('works', () => { run(); });"), "it('works', ...)");
        assert_eq!(signature_text("it('lonely')"), "it('lonely')");
        assert_eq!(signature_text("pub fn ping() {}"), "pub fn ping()");
    }

    #[test]
    fn duplicate_names_and_headings_are_distinct() {
        let symbols = extract_structural("a.rs", b"fn ping() {}\nfn ping() {}\n");
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].name, symbols[1].name);
        let headings = extract_structural("README.md", b"# Fixture\n");
        assert_eq!(headings[0].name, "Fixture");
        assert_eq!(headings[0].kind, "heading");
        assert_eq!(headings[0].summary, "Fixture");
        assert_eq!(headings[0].summary_source, "heading");
        let script = extract_structural("src/app.js", b"function ping() {}\n");
        assert_eq!(script.len(), 1);
        assert_eq!(script[0].name, "ping");
        assert_eq!(script[0].kind, "fn");
        assert_eq!(script[0].summary_source, "sig");
        assert_eq!(extract_structural("blob.bin", b"fn ping() {}\0"), [] as [crate::index::extract::Symbol; 0]);
        assert_eq!(extract_structural("notes.log", b"# Fixture\n"), [] as [crate::index::extract::Symbol; 0]);
    }

    #[test]
    fn dispatch_table_matches_the_shared_literal_and_ruby_basenames() {
        // Same list as `dctx-map-parse` `facts.rs`, `lexers_extensions.rs` and
        // `tests/map_languages.rs`. Facade tag for `tsx` is `typescript`.
        const LITERAL_EXTENSIONS: &[(&str, &str)] = &[
            ("rs", "rust"),
            ("js", "javascript"),
            ("mjs", "javascript"),
            ("cjs", "javascript"),
            ("jsx", "javascript"),
            ("ts", "typescript"),
            ("mts", "typescript"),
            ("cts", "typescript"),
            ("tsx", "typescript"),
            ("md", "markdown"),
            ("markdown", "markdown"),
            ("py", "python"),
            ("pyi", "python"),
            ("go", "go"),
            ("java", "java"),
            ("c", "c"),
            ("h", "c"),
            ("cc", "c"),
            ("cpp", "c"),
            ("cxx", "c"),
            ("hpp", "c"),
            ("hh", "c"),
            ("json", "config"),
            ("jsonc", "config"),
            ("toml", "config"),
            ("yaml", "config"),
            ("yml", "config"),
            ("cs", "csharp"),
            ("kt", "kotlin"),
            ("kts", "kotlin"),
            ("rb", "ruby"),
            ("rake", "ruby"),
            ("gemspec", "ruby"),
            ("php", "php"),
            ("phtml", "php"),
            ("inc", "php"),
            ("swift", "swift"),
            ("scala", "scala"),
            ("sc", "scala"),
            ("dart", "dart"),
            ("zig", "zig"),
            ("sol", "solidity"),
            ("gleam", "gleam"),
            ("perl", "perl"),
            ("groovy", "groovy"),
            ("gradle", "groovy"),
            ("txt", "text"),
            ("rst", "rst"),
            ("adoc", "asciidoc"),
            ("asciidoc", "asciidoc"),
            ("org", "org"),
            ("tex", "tex"),
            ("ex", "elixir"),
            ("exs", "elixir"),
            ("lua", "lua"),
            ("ml", "ocaml"),
            ("mli", "ocaml"),
            ("r", "r"),
            ("jl", "julia"),
            ("hs", "haskell"),
            ("tf", "hcl"),
            ("tfvars", "hcl"),
            ("hcl", "hcl"),
            ("sql", "sql"),
            ("sh", "shell"),
            ("bash", "shell"),
            ("zsh", "shell"),
            ("bats", "shell"),
            ("mk", "make"),
            ("proto", "proto"),
            ("pas", "pascal"),
            ("pp", "pascal"),
            ("dpr", "pascal"),
            ("graphql", "graphql"),
            ("gql", "graphql"),
            ("vue", "web"),
            ("svelte", "web"),
            ("astro", "web"),
            ("html", "web"),
            ("htm", "web"),
            ("clj", "clojure"),
            ("cljs", "clojure"),
            ("cljc", "clojure"),
            ("edn", "clojure"),
            ("lisp", "lisp"),
            ("lsp", "lisp"),
            ("cl", "lisp"),
            ("scm", "scheme"),
            ("ss", "scheme"),
            ("el", "elisp"),
            ("nix", "nix"),
        ];
        const LITERAL_RUBY_BASENAMES: &[&str] = &["Rakefile", "Gemfile"];
        assert_eq!(EXTENSIONS, LITERAL_EXTENSIONS);
        assert_eq!(RUBY_BASENAMES, LITERAL_RUBY_BASENAMES);
        for (extension, tag) in LITERAL_EXTENSIONS {
            assert_eq!(language_of(&format!("src/app.{extension}")), *tag);
            assert_eq!(language_of(&format!("dir.dot/app.{}", extension.to_ascii_uppercase())), *tag);
        }
        assert_eq!(language_of("sql/pgtap.sql.in"), "sql");
        assert_eq!(language_of("sql/PGTAP.SQL.IN"), "sql");
        assert_eq!(language_of("Makefile.in"), "opaque");
        assert_eq!(language_of(".sql.in"), "opaque");
        for basename in LITERAL_RUBY_BASENAMES {
            assert_eq!(language_of(basename), "ruby");
            assert_eq!(language_of(&format!("lib/{}", basename.to_ascii_lowercase())), "ruby");
            assert_eq!(language_of(&format!("lib\\{basename}")), "ruby");
        }
        for (basename, tag) in [("Makefile", "make"), ("GNUmakefile", "make"), ("Dockerfile", "docker"), ("Containerfile", "docker")] {
            assert_eq!(INFRA_BASENAMES.iter().find(|(name, _)| *name == basename).map(|(_, found)| *found), Some(tag));
            assert_eq!(language_of(basename), tag);
            assert_eq!(language_of(&basename.to_ascii_lowercase()), tag);
            assert_eq!(language_of(&format!("ci/{basename}")), tag);
            assert_eq!(language_of(&format!("ci\\{basename}")), tag);
        }
        assert_eq!(INFRA_BASENAMES.len(), 4);
        assert_eq!(language_of("Makefile.bak"), "opaque");
        assert_eq!(language_of("php"), "opaque");
        assert_eq!(language_of("Gemfile.lock"), "opaque");
        // Not a defect: the Ruby lexer rejects these paths.
        assert_eq!(language_of("Guardfile"), "opaque");
        assert_eq!(language_of("config.ru"), "opaque");

        let source = b"def real\nend\n";
        let control = extract_structural("src/app.rb", source);
        assert_eq!(control.len(), 1);
        assert_eq!(control[0].name, "real");
        assert_eq!(control[0].kind, "fn");
        assert_eq!(control[0].lines, [1, 2]);
        assert_eq!(control[0].signature, "def real");
        assert_eq!(control[0].summary, "def real");
        assert_eq!(control[0].summary_source, "sig");
        for path in ["src/app.RB", "tasks/app.RAKE", "demo.GEMSPEC", "Rakefile", "lib/Gemfile", "pkg/RAKEFILE"] {
            let symbols = extract_structural(path, source);
            assert_eq!(symbols.len(), 1, "{path}");
            assert_eq!(symbols[0].name, control[0].name);
            assert_eq!(symbols[0].kind, control[0].kind);
            assert_eq!(symbols[0].lines, control[0].lines);
            assert_eq!(symbols[0].signature, control[0].signature);
            assert_eq!(symbols[0].summary, control[0].summary);
            assert_eq!(symbols[0].summary_source, control[0].summary_source);
        }
        assert_eq!(extract_structural("Guardfile", source), [] as [crate::index::extract::Symbol; 0]);
        assert_eq!(extract_structural("config.ru", source), [] as [crate::index::extract::Symbol; 0]);
        assert_eq!(extract_structural("A.RS", b"fn RustMain() {}\n")[0].name, "RustMain");
        assert_eq!(extract_structural("README.MD", b"# MdMain\n")[0].name, "MdMain");
    }

    #[test]
    fn plain_ids_match_stable_id_and_rejected_names_stay_rejected() {
        let path = "src/lib.rs";
        let path_ok = dctx_map_model::StableId::new(NodeKind::Module, path, None, None).is_ok();
        assert!(path_ok);
        let mut text = super::TextBlob::default();
        let path_span = text.push(path);
        let names = ["ping", "Chain::append", "名前", "k0.events.0", "a~2", "a~01", "a#b", "has\nline", ""];
        for name in names {
            for ordinal in [None, Some(2)] {
                for kind in [NodeKind::Symbol, NodeKind::Test] {
                    let got = super::stored_ordinal(kind, path, name, ordinal, path_ok);
                    let old = dctx_map_model::StableId::new(kind, path, Some(name), ordinal).ok();
                    let formatted = got.map(|stored| {
                        let mut out = String::new();
                        super::super::write_stable_id(&mut out, super::letter_byte(kind), path, name, stored);
                        out
                    });
                    assert_eq!(formatted.as_deref(), old.as_ref().map(dctx_map_model::StableId::as_str), "{kind:?} {name:?} {ordinal:?}");
                    if let Some(id) = &formatted {
                        let node = node_of(super::letter_byte(kind), got.unwrap_or(0), name, path_span);
                        assert!(super::super::id_matches(&text, &node, id), "{id}");
                        assert!(!super::super::id_matches(&text, &node, &format!("{id}~02")));
                    }
                }
            }
        }
        let bad = "../x";
        assert!(dctx_map_model::StableId::new(NodeKind::Module, bad, None, None).is_err());
        assert!(super::stored_ordinal(NodeKind::Symbol, bad, "ping", None, false).is_none());
    }

    #[test]
    #[allow(clippy::cast_possible_truncation, reason = "The hash is an intentionally native-word table index; truncation keeps its low bits before masking to a power-of-two capacity.")]
    fn duplicate_ordinals_match_the_pre_change_index() {
        let symbols = vec![
            sample("ping"),
            sample("pong"),
            sample("ping"),
            sample("a#b"),
            sample("ping"),
            sample("名前"),
        ];
        let mut text = super::TextBlob::default();
        let path_span = text.push("p.rs");
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        super::index_symbols("p.rs", path_span, 0, symbols.clone(), &mut nodes, &mut edges, &mut text);
        assert_eq!(shown(&text, &nodes, &edges), index_oracle("p.rs", "m:p.rs", &symbols));
        assert_eq!(
            nodes.iter().map(|node| node_text(&text, node)).collect::<Vec<_>>(),
            ["s:p.rs#ping", "s:p.rs#pong", "s:p.rs#ping~2", "s:p.rs#ping~3", "s:p.rs#名前"]
        );
    }

    #[test]
    fn large_index_matches_the_pre_change_index() {
        let mut symbols = Vec::with_capacity(5_000);
        for index in 0..5_000 {
            let name = match index % 500 {
                0 => "a#b".to_string(),
                1 => "a~2".to_string(),
                2 => "ping".to_string(),
                _ => format!("n{index}"),
            };
            symbols.push(sample(&name));
        }
        symbols.push(sample("has\nline"));
        let mut text = super::TextBlob::default();
        let path_span = text.push("p.rs");
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        super::index_symbols("p.rs", path_span, 0, symbols.clone(), &mut nodes, &mut edges, &mut text);
        assert!(nodes.len() > 4_000);
        assert_eq!(shown(&text, &nodes, &edges), index_oracle("p.rs", "m:p.rs", &symbols));
        assert!(nodes.iter().any(|node| node_text(&text, node) == "s:p.rs#ping~2"));
        assert!(!nodes.iter().any(|node| node.name == "a#b"));
    }

    fn sample(name: &str) -> super::Symbol {
        super::Symbol {
            name: name.to_string(),
            kind: "fn".to_string(),
            graph: NodeKind::Symbol,
            lines: [1, 4],
            span: [0, 10],
            summary: format!("sum {name}"),
            summary_source: "sig".to_string(),
            signature: format!("sig {name}"),
        }
    }

    fn node_of(letter: u8, ordinal: u32, name: &str, path: super::TextSpan) -> crate::index::IndexedNode {
        crate::index::IndexedNode {
            letter,
            ordinal,
            kind: "fn".to_string(),
            name: name.to_string(),
            path,
            lines: [1, 1],
            span: [0, 0],
            summary: String::new(),
            summary_source: String::new(),
            signature: String::new(),
        }
    }

    fn node_text(text: &super::TextBlob, node: &crate::index::IndexedNode) -> String {
        let mut out = String::new();
        super::super::write_node_id(&mut out, text, node);
        out
    }

    fn shown(text: &super::TextBlob, nodes: &[crate::index::IndexedNode], edges: &[crate::index::IndexedEdge]) -> Vec<String> {
        nodes
            .iter()
            .zip(edges)
            .map(|(node, edge)| {
                let id = node_text(text, node);
                format!(
                    "{id}\t{}\t{}\t{}\t{:?}\t{:?}\t{}\t{}\t{}\tm:p.rs\t{id}\t{}\t{}",
                    node.kind,
                    node.name,
                    text.get(node.path),
                    node.lines,
                    node.span,
                    node.summary,
                    node.summary_source,
                    node.signature,
                    text.get(edge.edge),
                    text.get(edge.confidence)
                )
            })
            .collect()
    }

    fn index_oracle(path: &str, module_id: &str, symbols: &[super::Symbol]) -> Vec<String> {
        use std::collections::BTreeMap;
        let mut rows = Vec::new();
        let mut seen: BTreeMap<String, u32> = BTreeMap::new();
        for symbol in symbols {
            let count = seen.entry(symbol.name.clone()).or_insert(0);
            *count += 1;
            let ordinal = if *count == 1 { None } else { Some(*count) };
            let Ok(id) = dctx_map_model::StableId::new(symbol.graph, path, Some(&symbol.name), ordinal) else {
                continue;
            };
            rows.push(format!(
                "{}\t{}\t{}\t{}\t{:?}\t{:?}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                id.as_str(),
                symbol.kind,
                symbol.name,
                path,
                symbol.lines,
                symbol.span,
                symbol.summary,
                symbol.summary_source,
                symbol.signature,
                module_id,
                id.as_str(),
                "contains",
                "exact"
            ));
        }
        rows
    }
}
