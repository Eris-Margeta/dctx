//! Structural lexer dispatch and the facade's shared fact-to-symbol conversion.
//! Implements SPEC-MAP-MODE 5–6.2 using the first-party language lexers.
//! Consumes authorized UTF-8 from extract; never reads source or retains bodies.
//! Rust and Markdown keep their existing specialized extraction paths.

use dctx_map_model::{Node, NodeKind};

use super::extract::Symbol;
use super::refs::RawRef;
use super::span_index::SpanIndex;

/// Converts the selected lexer's facts and keeps its call and import references, abstaining on failed or unsupported input.
pub(super) fn lexed(language: &str, path: &str, text: &str) -> (Vec<Symbol>, Vec<RawRef>) {
    // LexedFile and Completion are crate-local types; the underlying Node is shared.
    macro_rules! raw_refs {
        ($lexed:expr) => {
            $lexed
                .references
                .iter()
                .filter_map(|labeled| {
                    let fact = &labeled.fact;
                    RawRef::new(fact.src.as_str(), fact.kind, &fact.name, &fact.scope, fact.alias.as_deref())
                })
                .collect()
        };
    }
    macro_rules! complete_symbols {
        ($lexer_crate:ident, $lexer:ident) => {
            match $lexer_crate::$lexer::default().extract(path, text.as_bytes()) {
                Ok(lexed) if lexed.completion == $lexer_crate::Completion::Complete => {
                    let refs = raw_refs!(lexed);
                    (from_nodes(text, lexed.nodes.into_iter().map(|labeled| labeled.fact)), refs)
                }
                _ => (Vec::new(), Vec::new()),
            }
        };
    }

    match language {
        // Preserve the existing JS/TS projection, including accepted partial facts.
        "javascript" | "typescript" => match dctx_lex_js::JsLexer::default().extract(path, text.as_bytes()) {
            Ok(lexed) => {
                let refs = raw_refs!(lexed);
                (from_nodes(text, lexed.nodes.into_iter().map(|labeled| labeled.fact)), refs)
            }
            Err(_) => (Vec::new(), Vec::new()),
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
        // Heuristic families without a first-party lexer: records carry confidence `guess`, no references.
        guessed if super::generic::handles(guessed) => (super::generic::symbols(guessed, text), Vec::new()),
        _ => (Vec::new(), Vec::new()),
    }
}

/// The symbols of [`lexed`] alone.
#[cfg(test)]
pub(super) fn symbols(language: &str, path: &str, text: &str) -> Vec<Symbol> {
    lexed(language, path, text).0
}

fn from_nodes(text: &str, nodes: impl IntoIterator<Item = Node>) -> Vec<Symbol> {
    let spans = SpanIndex::build(text);
    let nodes = nodes.into_iter();
    let (lower, upper) = nodes.size_hint();
    let mut symbols = Vec::with_capacity(upper.unwrap_or(lower));
    for node in nodes {
        if matches!(node.kind, NodeKind::Module | NodeKind::Place | NodeKind::Package) || node.name.is_empty() {
            continue;
        }
        let graph = match node.kind {
            NodeKind::Doc => NodeKind::Doc,
            NodeKind::Test => NodeKind::Test,
            _ => NodeKind::Symbol,
        };
        // Kind and summary source are static labels. Reuse buffers the graph does not keep.
        let kind_label = if graph == NodeKind::Doc {
            "heading"
        } else {
            node.symbol_kind.map_or("fn", dctx_map_model::SymbolKind::as_str)
        };
        let source_label = node.summary.source.as_str();
        let lines = node.lines;
        symbols.push(Symbol {
            name: node.name,
            kind: stamp(node.fingerprint, kind_label),
            graph,
            lines,
            span: spans.line_span(lines),
            summary: node.summary.text,
            summary_source: stamp(node.path, source_label),
            signature: node.signature,
        });
    }
    symbols
}

/// Writes `text` into `slot`'s existing allocation.
fn stamp(mut slot: String, text: &str) -> String {
    slot.clear();
    slot.push_str(text);
    slot
}

/// Pre-index span scan. Tests compare [`super::span_index::SpanIndex`] with this oracle.
#[cfg(test)]
pub(crate) fn line_span_oracle(text: &str, lines: [u32; 2]) -> [u64; 2] {
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
        assert_eq!(symbols("python", "a.py", &partial), [] as [crate::index::extract::Symbol; 0]);

        assert_eq!(symbols("config", "a.json", "{\"Ready\": 1}")[0].name, "Ready");
        let partial = "{\"Ready\": 1, \"broken\":";
        let lexed = dctx_lex_cfg::CfgLexer::default().extract("a.json", partial.as_bytes()).expect("partial");
        assert_eq!(lexed.completion, dctx_lex_cfg::Completion::Partial);
        assert!(lexed.nodes.iter().any(|node| node.fact.name == "Ready"));
        assert_eq!(symbols("config", "a.json", partial), [] as [crate::index::extract::Symbol; 0]);
    }

    #[test]
    fn failed_lexes_and_non_text_sources_have_no_symbols() {
        let source = format!("def Ready():\n    pass\n{}", "(".repeat(300));
        assert!(matches!(
            dctx_lex_py::PyLexer::default().extract("a.py", source.as_bytes()),
            Err(dctx_lex_py::LexError::TooDeep { .. })
        ));
        assert_eq!(symbols("python", "a.py", &source), [] as [crate::index::extract::Symbol; 0]);
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
        assert_eq!(symbols("ruby", "Guardfile", source), [] as [crate::index::extract::Symbol; 0]);
        assert_eq!(symbols("ruby", "config.ru", source), [] as [crate::index::extract::Symbol; 0]);
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

    #[test]
    fn symbol_spans_match_the_pre_index_scan() {
        let fixtures = [
            ("compact.json", "{\"Alpha\":1,\"Beta\":{\"Gamma\":2},\"Delta\":[3]}"),
            ("pretty.json", "{\n  \"Alpha\": 1,\n\n  \"Beta\": {\n    \"Gamma\": 2\n  }\n}\n"),
            ("crlf.json", "{\r\n  \"Alpha\": 1,\r\n\r\n  \"Beta\": 2\r\n}\r\n"),
            ("multi.json", "{\n  \"名前\": \"café\",\n  \"Key\": \"🔑\"\n}\n"),
            ("tail.json", "{\n  \"Alpha\": 1,\n\n  \"Tail\": \"é\"}"),
        ];
        for (path, text) in fixtures {
            let found = symbols("config", path, text);
            assert!(!found.is_empty(), "{path}");
            for symbol in &found {
                assert_eq!(symbol.span, line_span_oracle(text, symbol.lines), "{path} {}", symbol.name);
                let start = usize::try_from(symbol.span[0]).unwrap_or(usize::MAX);
                let end = usize::try_from(symbol.span[1]).unwrap_or(usize::MAX);
                assert!(text.is_char_boundary(start) && text.is_char_boundary(end), "{path} {}", symbol.name);
                assert!(start <= end && end <= text.len(), "{path} {}", symbol.name);
            }
        }
    }

    #[test]
    fn from_nodes_matches_the_pre_change_projection() {
        use dctx_map_model::{SummarySource, SymbolKind};
        let mut large = String::new();
        write_nested(&mut large, 3, 6, &mut 0);
        let fixtures = [
            ("compact.json", "{\"Alpha\":1,\"Beta\":{\"Gamma\":2},\"Delta\":[3]}".to_string()),
            ("pretty.json", "{\n  \"Alpha\": 1,\n\n  \"Beta\": {\n    \"Gamma\": 2\n  }\n}\n".to_string()),
            ("crlf.json", "{\r\n  \"Alpha\": 1,\r\n\r\n  \"Beta\": 2\r\n}\r\n".to_string()),
            ("multi.json", "{\n  \"名前\": \"café\",\n  \"Key\": \"🔑\"\n}\n".to_string()),
            ("tail.json", "{\n  \"Alpha\": 1,\n\n  \"Tail\": \"é\"}".to_string()),
            ("dup.json", "{\"Alpha\":1,\"Alpha\":2,\"Beta\":{\"Alpha\":3}}".to_string()),
            ("nested.json", large),
        ];
        for (path, text) in &fixtures {
            let lexed = dctx_lex_cfg::CfgLexer::default().extract(path, text.as_bytes()).expect(path);
            if lexed.completion != dctx_lex_cfg::Completion::Complete {
                assert!(symbols("config", path, text).is_empty(), "{path}");
                let nodes: Vec<_> = lexed.nodes.into_iter().map(|labeled| labeled.fact).collect();
                assert_eq!(from_nodes(text, nodes.clone()), from_nodes_oracle(text, nodes), "{path}");
                continue;
            }
            let got = symbols("config", path, text);
            let old = from_nodes_oracle(text, lexed.nodes.into_iter().map(|labeled| labeled.fact));
            assert_eq!(got, old, "{path}");
            assert!(got.len() > 1, "{path}");
            let mut blob = super::super::TextBlob::default();
            let path_span = blob.push(path);
            let mut nodes = Vec::new();
            let mut edges = Vec::new();
            super::super::extract::index_symbols(path, path_span, 0, got.clone(), &mut nodes, &mut edges, &mut blob);
            let (old_nodes, old_edges) = index_oracle(path, &got);
            assert_eq!(nodes, old_nodes, "{path}");
            assert_eq!(edges, old_edges, "{path}");
            let again = dctx_lex_cfg::CfgLexer::default().extract(path, text.as_bytes()).expect(path);
            let mut direct_blob = super::super::TextBlob::default();
            let direct_path = direct_blob.push(path);
            let mut direct_nodes = Vec::new();
            let mut direct_edges = Vec::new();
            super::super::extract::index_prepared(
                path,
                text.as_bytes(),
                direct_path,
                0,
                super::super::extract::Prepared::Config(again),
                &mut direct_nodes,
                &mut direct_edges,
                &mut direct_blob,
            );
            assert_eq!(direct_nodes, nodes, "direct {path}");
            assert_eq!(direct_edges, edges, "direct {path}");
        }

        let text = "{\n  \"Alpha\": 1,\n  \"Beta\": 2\n}\n";
        let hand = vec![
            fact(NodeKind::Module, None, "", [1, 3], "", SummarySource::Doc),
            fact(NodeKind::Symbol, Some(SymbolKind::Struct), "Alpha", [2, 2], "Alpha: 1", SummarySource::Signature),
            fact(NodeKind::Symbol, None, "", [2, 2], "skip", SummarySource::Signature),
            fact(NodeKind::Doc, None, "Title", [1, 1], "Title", SummarySource::Heading),
            fact(NodeKind::Test, Some(SymbolKind::Function), "ping", [3, 3], "ping()", SummarySource::Doc),
            fact(NodeKind::Place, None, "root", [1, 1], "", SummarySource::Doc),
            fact(NodeKind::Package, None, "pkg", [1, 1], "", SummarySource::Doc),
        ];
        assert_eq!(from_nodes(text, hand.clone()), from_nodes_oracle(text, hand));
    }

    fn write_nested(out: &mut String, depth: usize, branch: usize, next: &mut u32) {
        out.push('{');
        for index in 0..branch {
            if index > 0 {
                out.push(',');
            }
            let id = *next;
            *next += 1;
            out.push('"');
            out.push('k');
            out.push_str(&id.to_string());
            out.push('"');
            out.push(':');
            if depth == 0 {
                out.push_str(&id.to_string());
            } else {
                write_nested(out, depth - 1, branch, next);
            }
        }
        out.push('}');
    }

    fn fact(
        kind: NodeKind,
        symbol_kind: Option<dctx_map_model::SymbolKind>,
        name: &str,
        lines: [u32; 2],
        summary: &str,
        source: dctx_map_model::SummarySource,
    ) -> Node {
        // Place, package and module ids have no name. Symbol, test and doc ids require one.
        let named = !name.is_empty() && matches!(kind, NodeKind::Symbol | NodeKind::Test | NodeKind::Doc);
        let id_kind = if named || matches!(kind, NodeKind::Place | NodeKind::Package | NodeKind::Module) {
            kind
        } else {
            NodeKind::Module
        };
        let id = dctx_map_model::StableId::new(id_kind, "hand.json", if named { Some(name) } else { None }, None)
            .expect("id");
        Node {
            id,
            kind,
            symbol_kind,
            path: "hand.json".into(),
            lines,
            name: name.into(),
            signature: format!("sig:{name}"),
            fingerprint: "ab".repeat(32),
            public: false,
            summary: dctx_map_model::Summary { text: summary.into(), source },
        }
    }

    /// Pre-change `from_nodes`. The stored strings are the oracle for the buffer reuse.
    fn from_nodes_oracle(text: &str, nodes: impl IntoIterator<Item = Node>) -> Vec<super::super::extract::Symbol> {
        let spans = SpanIndex::build(text);
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
                node.symbol_kind.map_or_else(|| "fn".to_string(), |kind| kind.as_str().to_string())
            };
            symbols.push(super::super::extract::Symbol {
                name: node.name,
                kind,
                graph,
                lines: node.lines,
                span: spans.line_span(node.lines),
                summary: node.summary.text,
                summary_source: node.summary.source.as_str().to_string(),
                signature: node.signature,
            });
        }
        symbols
    }

    fn index_oracle(
        path: &str,
        symbols: &[super::super::extract::Symbol],
    ) -> (Vec<super::super::IndexedNode>, Vec<super::super::IndexedEdge>) {
        use std::collections::BTreeMap;
        let mut text = super::super::TextBlob::default();
        let path_span = text.push(path);
        let edge = text.push("contains");
        let confidence = text.push("exact");
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        let mut seen: BTreeMap<String, u32> = BTreeMap::new();
        for symbol in symbols {
            let count = seen.entry(symbol.name.clone()).or_insert(0);
            *count += 1;
            let ordinal = if *count == 1 { None } else { Some(*count) };
            if dctx_map_model::StableId::new(symbol.graph, path, Some(&symbol.name), ordinal).is_err() {
                continue;
            }
            let Some(dst) = u32::try_from(nodes.len()).ok() else {
                break;
            };
            nodes.push(super::super::IndexedNode {
                letter: match symbol.graph {
                    dctx_map_model::NodeKind::Test => b't',
                    dctx_map_model::NodeKind::Doc => b'd',
                    _ => b's',
                },
                ordinal: ordinal.unwrap_or(0),
                kind: symbol.kind.clone(),
                name: symbol.name.clone(),
                path: path_span,
                lines: symbol.lines,
                span: symbol.span,
                summary: symbol.summary.clone(),
                summary_source: symbol.summary_source.clone(),
                signature: symbol.signature.clone(),
            });
            edges.push(super::super::IndexedEdge { src: 0, dst, edge, confidence });
        }
        (nodes, edges)
    }
}
