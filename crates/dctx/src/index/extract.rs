//! Structural extractor for the facade's supported source and configuration languages.
//! Implements the strict parser profile: coverage `structural`.
//! Uses lex_dispatch and dctx-lex-md. Explicit structural Rust stays line-oriented; the default walk uses syn.
//! The snapshot walk calls [`extract_structural`]. Answer ids and the structural precision label stay put.

use dctx_map_model::NodeKind;

use super::IndexedNode;

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
];

/// Duplicate of `dctx-map-parse` `domain::facts::RUBY_BASENAMES`.
///
/// `dctx-lex-rb` accepts these final segments case-insensitively, and rejects
/// `Guardfile` and `config.ru`. A basename wins over the extension.
const RUBY_BASENAMES: &[&str] = &["Rakefile", "Gemfile"];

/// Language tag from the final path segment. Anything else is `opaque`.
///
/// Extensions are case-insensitive. See [`EXTENSIONS`] and [`RUBY_BASENAMES`].
pub(crate) fn language_of(path: &str) -> &'static str {
    let base = path.rsplit(['/', '\\']).next().unwrap_or(path);
    if RUBY_BASENAMES.iter().any(|name| base.eq_ignore_ascii_case(name)) {
        return "ruby";
    }
    let Some(extension) = base.rsplit_once('.').map(|(_, extension)| extension.to_ascii_lowercase()) else {
        return "opaque";
    };
    EXTENSIONS
        .iter()
        .find(|(name, _)| *name == extension)
        .map(|(_, tag)| *tag)
        .unwrap_or("opaque")
}

/// Structural facts. NUL bytes yield no symbols; the caller still hashes the file.
pub(crate) fn extract_structural(path: &str, bytes: &[u8]) -> Vec<Symbol> {
    if bytes.contains(&0) {
        return Vec::new();
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Vec::new();
    };
    match language_of(path) {
        "rust" => rust_symbols(text),
        "markdown" => markdown_symbols(bytes),
        language => super::lex_dispatch::symbols(language, path, text),
    }
}

/// Assigns stable ids and contains edges for one file's symbols.
pub(crate) fn index_symbols(
    path: &str,
    module_id: &str,
    symbols: &[Symbol],
    nodes: &mut Vec<IndexedNode>,
    edges: &mut Vec<super::IndexedEdge>,
) {
    use std::collections::BTreeMap;

    use dctx_map_model::StableId;

    let mut seen: BTreeMap<String, u32> = BTreeMap::new();
    for symbol in symbols {
        let count = seen.entry(symbol.name.clone()).or_insert(0);
        *count += 1;
        let ordinal = if *count == 1 { None } else { Some(*count) };
        let Ok(id) = StableId::new(symbol.graph, path, Some(&symbol.name), ordinal) else {
            continue;
        };
        let id = id.as_str().to_string();
        nodes.push(IndexedNode {
            id: id.clone(),
            kind: symbol.kind.clone(),
            name: symbol.name.clone(),
            path: path.to_string(),
            lines: symbol.lines,
            span: symbol.span,
            summary: symbol.summary.clone(),
            summary_source: symbol.summary_source.clone(),
            signature: symbol.signature.clone(),
        });
        edges.push(super::IndexedEdge {
            src: module_id.to_string(),
            dst: id,
            edge: "contains".to_string(),
            confidence: "exact".to_string(),
        });
    }
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
    let document = match dctx_lex_md::extract(bytes) {
        Ok(document) => document,
        Err(_) => return Vec::new(),
    };
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
        let ok = ch == '_' || ch.is_ascii_alphanumeric() || (index == 0 && ch == '_');
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

/// Signature ending at the matching parenthesis, or the text before a body brace.
pub(crate) fn signature_text(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(start) = trimmed.find('(')
        && let Some(end) = matching_paren(trimmed, start)
    {
        return trimmed[..=end].trim().to_string();
    }
    let cut = trimmed.find('{').unwrap_or(trimmed.len());
    trimmed[..cut].trim().trim_end_matches(';').trim().to_string()
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
        assert!(extract_structural("blob.bin", b"fn ping() {}\0").is_empty());
        assert!(extract_structural("notes.txt", b"# Fixture\n").is_empty());
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
        ];
        const LITERAL_RUBY_BASENAMES: &[&str] = &["Rakefile", "Gemfile"];
        assert_eq!(EXTENSIONS, LITERAL_EXTENSIONS);
        assert_eq!(RUBY_BASENAMES, LITERAL_RUBY_BASENAMES);
        for (extension, tag) in LITERAL_EXTENSIONS {
            assert_eq!(language_of(&format!("src/app.{extension}")), *tag);
            assert_eq!(language_of(&format!("dir.dot/app.{}", extension.to_ascii_uppercase())), *tag);
        }
        for basename in LITERAL_RUBY_BASENAMES {
            assert_eq!(language_of(basename), "ruby");
            assert_eq!(language_of(&format!("lib/{}", basename.to_ascii_lowercase())), "ruby");
            assert_eq!(language_of(&format!("lib\\{basename}")), "ruby");
        }
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
        assert!(extract_structural("Guardfile", source).is_empty());
        assert!(extract_structural("config.ru", source).is_empty());
        assert_eq!(extract_structural("A.RS", b"fn RustMain() {}\n")[0].name, "RustMain");
        assert_eq!(extract_structural("README.MD", b"# MdMain\n")[0].name, "MdMain");
    }
}
