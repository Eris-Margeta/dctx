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
        ("f90", "fortran"),
        ("f95", "fortran"),
        ("f03", "fortran"),
        ("f08", "fortran"),
        ("f", "fortran"),
        ("for", "fortran"),
        ("pl", "perl"),
        ("pm", "perl"),
        ("t", "perl"),
        ("psgi", "perl"),
        ("erl", "erlang"),
        ("hrl", "erlang"),
        ("groovy", "groovy"),
        ("gradle", "groovy"),
        ("ps1", "powershell"),
        ("psm1", "powershell"),
        ("psd1", "powershell"),
        ("v", "verilog"),
        ("vh", "verilog"),
        ("sv", "verilog"),
        ("svh", "verilog"),
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
