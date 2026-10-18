//! Structural lexer dispatch and the facade's shared fact-to-symbol conversion.
//! Implements SPEC-MAP-MODE 5–6.2 using the first-party language lexers.
//! Consumes authorized UTF-8 from extract; never reads source or retains bodies.
//! Rust and Markdown keep their existing specialized extraction paths.

use dctx_map_model::{Node, NodeKind};

use super::extract::Symbol;

/// Converts the selected lexer's facts, abstaining on failed or unsupported input.
pub(super) fn symbols(language: &str, path: &str, text: &str) -> Vec<Symbol> {
    // LexedFile and Completion are crate-local types; the underlying Node is shared.
    macro_rules! complete_symbols {
        ($lexer_crate:ident, $lexer:ident) => {
            match $lexer_crate::$lexer::default().extract(path, text.as_bytes()) {
                Ok(lexed) if lexed.completion == $lexer_crate::Completion::Complete => {
                    from_nodes(text, lexed.nodes.into_iter().map(|labeled| labeled.fact))
                }
                _ => Vec::new(),
            }
        };
    }

    match language {
        // Preserve the existing JS/TS projection, including accepted partial facts.
        "javascript" | "typescript" => match dctx_lex_js::JsLexer::default().extract(path, text.as_bytes()) {
            Ok(lexed) => from_nodes(text, lexed.nodes.into_iter().map(|labeled| labeled.fact)),
            Err(_) => Vec::new(),
        },
        "python" => complete_symbols!(dctx_lex_py, PyLexer),
        "go" => complete_symbols!(dctx_lex_go, GoLexer),
        "java" => complete_symbols!(dctx_lex_java, JavaLexer),
        "c" => complete_symbols!(dctx_lex_c, CLexer),
        "config" => complete_symbols!(dctx_lex_cfg, CfgLexer),
        "csharp" => complete_symbols!(dctx_lex_cs, CsLexer),
        "kotlin" => complete_symbols!(dctx_lex_kt, KotlinLexer),
        "ruby" => complete_symbols!(dctx_lex_rb, RubyLexer),
        "php" => complete_symbols!(dctx_lex_php, PhpLexer),
        "swift" => complete_symbols!(dctx_lex_swift, SwiftLexer),
        _ => Vec::new(),
    }
}

fn from_nodes(text: &str, nodes: impl IntoIterator<Item = Node>) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    for node in nodes {
        if matches!(node.kind, NodeKind::Module | NodeKind::Place | NodeKind::Package) || node.name.is_empty() {
            continue;
        }
        let graph = match node.kind {
            NodeKind::Doc => NodeKind::Doc,
            NodeKind::Test => NodeKind::Test,
            _ => NodeKind::Symbol,
        };
        let kind = if graph == NodeKind::Doc {
            "heading".to_string()
        } else {
            node.symbol_kind.map(|kind| kind.as_str().to_string()).unwrap_or_else(|| "fn".to_string())
        };
        symbols.push(Symbol {
            name: node.name,
            kind,
            graph,
            lines: node.lines,
            span: line_span(text, node.lines),
            summary: node.summary.text,
            summary_source: node.summary.source.as_str().to_string(),
            signature: node.signature,
        });
    }
    symbols
}

fn line_span(text: &str, lines: [u32; 2]) -> [u64; 2] {
    let start_line = lines[0].max(1);
    let end_line = lines[1].max(start_line);
    let mut offset = 0usize;
    let mut start = 0usize;
    let mut end = text.len();
    let mut seen = false;
    for (index, line) in text.split_inclusive('\n').enumerate() {
        let line_no = u32::try_from(index.saturating_add(1)).unwrap_or(u32::MAX);
        if line_no == start_line {
            start = offset;
            seen = true;
        }
        if line_no == end_line {
            end = offset.saturating_add(line.trim_end_matches(['\r', '\n']).len());
            break;
        }
        offset = offset.saturating_add(line.len());
    }
    if !seen {
        return [0, 0];
    }
    [u64::try_from(start).unwrap_or(u64::MAX), u64::try_from(end).unwrap_or(u64::MAX)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::extract::extract_structural;

    #[test]
    fn partial_facts_are_not_exposed_for_new_languages() {
        let source = "def Ready():\n    pass\n";
        assert_eq!(symbols("python", "a.py", source)[0].name, "Ready");
        let partial = format!("{source}\n(");
        let lexed = dctx_lex_py::PyLexer::default().extract("a.py", partial.as_bytes()).expect("partial");
        assert_eq!(lexed.completion, dctx_lex_py::Completion::Partial);
        assert!(lexed.nodes.iter().any(|node| node.fact.name == "Ready"));
        assert!(symbols("python", "a.py", &partial).is_empty());

        assert_eq!(symbols("config", "a.json", "{\"Ready\": 1}")[0].name, "Ready");
        let partial = "{\"Ready\": 1, \"broken\":";
        let lexed = dctx_lex_cfg::CfgLexer::default().extract("a.json", partial.as_bytes()).expect("partial");
        assert_eq!(lexed.completion, dctx_lex_cfg::Completion::Partial);
        assert!(lexed.nodes.iter().any(|node| node.fact.name == "Ready"));
        assert!(symbols("config", "a.json", partial).is_empty());
    }

    #[test]
    fn failed_lexes_and_non_text_sources_have_no_symbols() {
        let source = format!("def Ready():\n    pass\n{}", "(".repeat(300));
        assert!(matches!(
            dctx_lex_py::PyLexer::default().extract("a.py", source.as_bytes()),
            Err(dctx_lex_py::LexError::TooDeep { .. })
        ));
        assert!(symbols("python", "a.py", &source).is_empty());
        for extension in ["py", "go", "java", "c", "json", "cs", "kt", "rb", "php", "swift"] {
            let path = format!("a.{extension}");
            assert!(extract_structural(&path, &[0xff]).is_empty(), "invalid UTF-8: {path}");
            assert!(extract_structural(&path, b"\0").is_empty(), "NUL: {path}");
        }
    }

    #[test]
    fn ruby_paths_the_lexer_accepts_keep_the_rb_symbol() {
        let source = "def real\nend\n";
        let control = symbols("ruby", "app.rb", source);
        assert_eq!(control.len(), 1);
        assert_eq!(control[0].name, "real");
        assert_eq!(control[0].kind, "fn");
        assert_eq!(control[0].lines, [1, 2]);
        assert_eq!(control[0].signature, "def real");
        for path in ["Rakefile", "lib/Gemfile", "app.RB", "task.RAKE", "demo.GEMSPEC"] {
            let got = symbols("ruby", path, source);
            assert_eq!(got.len(), 1, "{path}");
            assert_eq!(got[0].name, control[0].name);
            assert_eq!(got[0].kind, control[0].kind);
            assert_eq!(got[0].lines, control[0].lines);
            assert_eq!(got[0].signature, control[0].signature);
        }
        // Not a defect: dctx-lex-rb returns UnsupportedDialect for these.
        assert!(symbols("ruby", "Guardfile", source).is_empty());
        assert!(symbols("ruby", "config.ru", source).is_empty());
    }

    #[test]
    fn scripts_keep_the_existing_partial_fact_projection() {
        for (language, path) in [("javascript", "a.js"), ("typescript", "a.ts")] {
            let source = "function Ready() {}\n";
            let complete = symbols(language, path, source);
            assert_eq!(complete.len(), 1);
            let partial = format!("{source}(");
            let lexed = dctx_lex_js::JsLexer::default().extract(path, partial.as_bytes()).expect("partial");
            assert_eq!(lexed.completion, dctx_lex_js::Completion::Partial);
            assert_eq!(symbols(language, path, &partial), complete);
        }
    }
}
