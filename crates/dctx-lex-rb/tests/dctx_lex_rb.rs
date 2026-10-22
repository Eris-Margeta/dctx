//! Golden fixtures, store invariants and deterministic fuzz for the Ruby extractor.
//! Implements PARSERS-v1: partial facts for malformed input, no panic on arbitrary bytes.
//! Every node is owned by the file module and every span lies inside the file.
#![forbid(unsafe_code)]

use dctx_lex_rb::{
    Completion, DiagnosticReason, Dialect, LexedFile, Precision, RubyLexer, PARSER_ID, PARSER_REVISION,
};
use dctx_map_model::{EdgeKind, NodeKind, SummarySource, SymbolKind};
use std::collections::BTreeSet;

const BASIC: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/basic.rb"));
const HEREDOC: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/heredoc.rb"));
const INCOMPLETE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/incomplete.rb"
));
const REAL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/realworld.rb"
));

fn extract(path: &str, source: &str) -> LexedFile {
    RubyLexer::default()
        .extract(path, source.as_bytes())
        .unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn names(facts: &LexedFile) -> Vec<&str> {
    facts.nodes.iter().map(|node| node.fact.name.as_str()).collect()
}

fn node<'a>(facts: &'a LexedFile, name: &str) -> &'a dctx_map_model::Node {
    &facts
        .nodes
        .iter()
        .find(|node| node.fact.name == name)
        .unwrap_or_else(|| panic!("missing {name} in {:?}", names(facts)))
        .fact
}

fn assert_symbol(facts: &LexedFile, name: &str, symbol: SymbolKind, public: bool, doc: &str) {
    let found = node(facts, name);
    assert_eq!(found.kind, NodeKind::Symbol, "{name}");
    assert_eq!(found.symbol_kind, Some(symbol), "{name}");
    assert_eq!(found.public, public, "{name}");
    if doc.is_empty() {
        assert_eq!(found.summary.source, SummarySource::Signature, "{name}");
    } else {
        assert_eq!(found.summary.source, SummarySource::Doc, "{name}");
        assert_eq!(found.summary.text, doc, "{name}");
    }
}

fn node_precision(facts: &LexedFile, name: &str) -> Precision {
    facts
        .nodes
        .iter()
        .find(|node| node.fact.name == name)
        .unwrap_or_else(|| panic!("missing {name}"))
        .precision
}

fn refs(facts: &LexedFile, kind: EdgeKind) -> Vec<&str> {
    facts
        .references
        .iter()
        .filter(|reference| reference.fact.kind == kind)
        .map(|reference| reference.fact.name.as_str())
        .collect()
}

fn file_lines(source: &str) -> u32 {
    u32::try_from(source.bytes().filter(|byte| *byte == b'\n').count()).unwrap_or(u32::MAX) + 1
}

fn check_invariants(path: &str, source: &str, facts: &LexedFile) {
    let lines = file_lines(source);
    assert!(!facts.nodes.is_empty(), "{path}");
    assert_eq!(facts.nodes[0].fact.kind, NodeKind::Module);
    assert_eq!(facts.nodes[0].fact.name, path);
    assert_eq!(facts.nodes[0].precision, Precision::Structural);
    assert_eq!(facts.edges.len(), facts.nodes.len() - 1);
    assert!(facts.diagnostics.len() <= LexedFile::MAX_DIAGNOSTICS);
    if facts.diagnostics.is_empty() {
        assert_eq!(facts.completion, Completion::Complete);
    } else {
        assert_eq!(facts.completion, Completion::Partial);
    }
    let mut ids = BTreeSet::new();
    for node in &facts.nodes {
        assert_eq!(node.fact.path, path, "{}", node.fact.name);
        assert!(
            node.fact.id.as_str().starts_with(node.fact.kind.letter()),
            "{}",
            node.fact.id
        );
        let identity = node.fact.id.as_str().split_once(':').map(|(_, rest)| match rest.split_once('#') {
            Some((file, _)) => file,
            None => rest,
        });
        assert_eq!(identity, Some(path), "{}", node.fact.id);
        let [start, end] = node.fact.lines;
        assert!(
            start >= 1 && start <= end && end <= lines,
            "{} {start}..{end} of {lines}",
            node.fact.name
        );
        assert_eq!(node.fact.fingerprint.len(), 64, "{}", node.fact.name);
        assert!(ids.insert(node.fact.id.as_str().to_owned()), "duplicate {}", node.fact.id);
        assert!(!matches!(node.fact.kind, NodeKind::Place | NodeKind::Package));
    }
    let mut owned = BTreeSet::from([facts.nodes[0].fact.id.as_str()]);
    loop {
        let before = owned.len();
        for edge in &facts.edges {
            assert_eq!(edge.fact.owner_path, path);
            assert_eq!(edge.fact.kind, EdgeKind::Contains);
            assert!(edge.fact.kind.permits(edge.fact.conf));
            assert!(ids.contains(edge.fact.src.as_str()), "{}", edge.fact.src);
            assert!(ids.contains(edge.fact.dst.as_str()), "{}", edge.fact.dst);
            if owned.contains(edge.fact.src.as_str()) {
                owned.insert(edge.fact.dst.as_str());
            }
        }
        if owned.len() == before {
            break;
        }
    }
    for node in &facts.nodes {
        assert!(owned.contains(node.fact.id.as_str()), "{} is not owned by the file", node.fact.name);
    }
    for reference in &facts.references {
        let [start, end] = reference.fact.lines;
        assert!(
            start >= 1 && start <= end && end <= lines,
            "{} {start}..{end} of {lines}",
            reference.fact.name
        );
        assert_eq!(reference.fact.owner_path, path);
        assert!(ids.contains(reference.fact.src.as_str()), "{}", reference.fact.src);
        assert_ne!(reference.fact.kind, EdgeKind::Contains);
        assert_ne!(reference.fact.name, "");
    }
}

fn assert_stable(path: &str, bytes: &[u8]) {
    let lexer = RubyLexer::default();
    let first = lexer.extract(path, bytes);
    let second = lexer.extract(path, bytes);
    match (&first, &second) {
        (Ok(left), Ok(right)) => {
            if let Ok(source) = std::str::from_utf8(bytes) {
                check_invariants(path, source, left);
            }
            assert_eq!(left, right);
        }
        (Err(left), Err(right)) => assert_eq!(left.to_string(), right.to_string()),
        _ => panic!("non-deterministic outcome for {path}"),
    }
}

fn xorshift(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

#[test]
fn golden_basic_extracts_structure_and_abstains_from_metaprogramming() {
    assert_eq!(PARSER_ID, "dctx-lex-rb");
    assert_eq!(PARSER_REVISION, 1);
    let facts = extract("basic.rb", BASIC);
    assert_eq!(facts.dialect, Dialect::Ruby);
    assert!(facts.diagnostics.is_empty(), "{:?}", facts.diagnostics);
    check_invariants("basic.rb", BASIC, &facts);
    assert_eq!(
        names(&facts),
        [
            "basic.rb",
            "Demo",
            "Demo::MAX",
            "Demo::Box",
            "Demo::Box::width",
            "Demo::Box::width=",
            "Demo::Box::height",
            "Demo::Box::title=",
            "Demo::Box::area",
            "Demo::Box::build",
            "Demo::Box::<<self",
            "Demo::Box::register",
            "Demo::Box::measure",
            "Demo::Box::size",
            "Demo::Box::hidden",
            "top",
        ]
    );
    assert_eq!(node(&facts, "basic.rb").summary.text, "File demo is a sample.");
    assert_eq!(node(&facts, "basic.rb").summary.source, SummarySource::Doc);
    assert_symbol(&facts, "Demo", SymbolKind::Type, true, "");
    assert_symbol(&facts, "Demo::MAX", SymbolKind::Const, true, "");
    assert_symbol(&facts, "Demo::Box", SymbolKind::Class, true, "A boxed value.");
    assert_eq!(node_precision(&facts, "Demo::Box::width"), Precision::Convention);
    assert_symbol(&facts, "Demo::Box::width", SymbolKind::Method, true, "");
    assert_symbol(&facts, "Demo::Box::width=", SymbolKind::Method, true, "");
    assert_symbol(&facts, "Demo::Box::area", SymbolKind::Method, true, "Measures the box.");
    assert_eq!(node_precision(&facts, "Demo::Box::area"), Precision::Structural);
    assert!(node(&facts, "Demo::Box::area").signature.contains("def area"));
    assert_symbol(&facts, "Demo::Box::build", SymbolKind::Method, true, "");
    assert_symbol(&facts, "Demo::Box::<<self", SymbolKind::Class, true, "");
    assert_eq!(node_precision(&facts, "Demo::Box::<<self"), Precision::Structural);
    assert_symbol(&facts, "Demo::Box::register", SymbolKind::Method, true, "");
    assert_symbol(&facts, "Demo::Box::measure", SymbolKind::Method, true, "");
    assert_eq!(node_precision(&facts, "Demo::Box::measure"), Precision::Convention);
    assert_symbol(&facts, "Demo::Box::size", SymbolKind::Method, true, "");
    assert_symbol(&facts, "Demo::Box::hidden", SymbolKind::Method, false, "");
    assert_symbol(&facts, "top", SymbolKind::Function, true, "");
    assert!(!names(&facts).iter().any(|name| {
        matches!(*name, "nope" | "NOPE" | "injected" | "Demo::Box::NOPE" | "dynamic")
    }));
    assert_eq!(refs(&facts, EdgeKind::Imports), ["json", "helper", "extra.rb"]);
    assert_eq!(
        refs(&facts, EdgeKind::Calls),
        [
            "helper",
            "self.width",
            "Box.new",
            "store.add",
            "secret",
            "define_method",
            "hidden_meta",
            "const_set",
            "meta_call",
            "a.plus",
        ]
    );
    assert_eq!(
        refs(&facts, EdgeKind::UsesType),
        ["Shape", "self.width", "Box.new", "store.add", "a.plus"]
    );
    assert_stable("basic.rb", BASIC.as_bytes());
}

#[test]
fn golden_heredoc_keeps_literals_and_end_data_out_of_the_graph() {
    let facts = extract("heredoc.rb", HEREDOC);
    assert!(facts.diagnostics.is_empty(), "{:?}", facts.diagnostics);
    check_invariants("heredoc.rb", HEREDOC, &facts);
    assert_eq!(names(&facts), ["heredoc.rb", "Demo", "Demo::show", "real"]);
    assert_eq!(
        node(&facts, "heredoc.rb").summary.text,
        "Heredocs and literals stay out of the graph."
    );
    assert_symbol(&facts, "Demo::show", SymbolKind::Method, true, "");
    assert_symbol(&facts, "real", SymbolKind::Function, true, "");
    assert_eq!(
        refs(&facts, EdgeKind::Calls),
        ["is_called", "nested", "token", "joined.tail", "kept"]
    );
    assert_eq!(refs(&facts, EdgeKind::UsesType), ["joined.tail"]);
    assert!(!names(&facts).iter().any(|name| {
        matches!(
            *name,
            "alpha" | "beta" | "not_called" | "hidden_q" | "gamma" | "commented" | "data_only" | "ghost"
        )
    }));
    assert!(!refs(&facts, EdgeKind::Calls).iter().any(|name| {
        matches!(*name, "alpha" | "beta" | "not_called" | "hidden_q" | "gamma" | "commented" | "ghost")
    }));
}

#[test]
fn realworld_constructs_parse_completely() {
    let facts = extract("realworld.rb", REAL);
    assert!(facts.diagnostics.is_empty(), "{:?}", facts.diagnostics);
    assert_eq!(facts.completion, Completion::Complete);
    check_invariants("realworld.rb", REAL, &facts);
    for name in [
        "Certifi",
        "Certifi::<<self",
        "Certifi::metaclass_hook",
        "Certifi::pythons",
        "Certifi::install",
        "square",
        "tagged",
        "anchor",
        "show",
        "spin",
        "wrapped",
        "wait_loop",
        "classify",
        "touch",
        "punct",
    ] {
        assert!(names(&facts).contains(&name), "missing {name} in {:?}", names(&facts));
    }
    assert_symbol(&facts, "Certifi", SymbolKind::Class, true, "");
    assert_symbol(&facts, "Certifi::pythons", SymbolKind::Method, true, "");
    assert_symbol(&facts, "square", SymbolKind::Function, true, "");
    assert_symbol(&facts, "tagged", SymbolKind::Function, true, "");
    assert_symbol(&facts, "anchor", SymbolKind::Function, true, "");
    let calls = refs(&facts, EdgeKind::Calls);
    assert!(calls.contains(&"dep.to_formula"), "{calls:?}");
    assert!(calls.contains(&"joined.tail"), "{calls:?}");
    assert!(calls.contains(&"value.class"), "{calls:?}");
    assert!(calls.contains(&"obj&.save"), "{calls:?}");
    assert!(!calls.contains(&"alpha"), "{calls:?}");
    assert!(!calls.contains(&"beta"), "{calls:?}");
    assert!(refs(&facts, EdgeKind::UsesType).contains(&"Formula"), "{:?}", facts.references);
}

#[test]
fn incomplete_fixture_keeps_partial_facts() {
    let facts = extract("incomplete.rb", INCOMPLETE);
    check_invariants("incomplete.rb", INCOMPLETE, &facts);
    assert_eq!(facts.completion, Completion::Partial);
    assert!(facts.diagnostics.iter().any(|item| item.reason == DiagnosticReason::UnterminatedString));
    assert!(facts.diagnostics.iter().any(|item| item.reason == DiagnosticReason::UnterminatedBlock));
    assert_eq!(names(&facts), ["incomplete.rb", "ready", "Open", "Open::inner"]);
    assert_eq!(node(&facts, "ready").summary.text, "Ready is complete.");
    assert_eq!(node_precision(&facts, "ready"), Precision::Structural);
    assert_eq!(node(&facts, "ready").symbol_kind, Some(SymbolKind::Function));
    assert_eq!(node_precision(&facts, "Open::inner"), Precision::Structural);
    assert_eq!(node(&facts, "Open").symbol_kind, Some(SymbolKind::Class));
    assert_eq!(node_precision(&facts, "Open"), Precision::Partial);
    assert_eq!(refs(&facts, EdgeKind::Calls), ["helper", "inner_call"]);
}

#[test]
#[allow(clippy::cast_possible_truncation, reason = "The fuzz length deliberately uses the PRNG's native low word before applying the 301-byte bound.")]
fn arbitrary_bytes_and_truncated_fixtures_stay_bounded() {
    let mut state = 0x1234_5678_9abc_def0_u64;
    for _ in 0..200 {
        let len = (xorshift(&mut state) as usize) % 301;
        let mut bytes = Vec::with_capacity(len);
        for _ in 0..len {
            bytes.push(xorshift(&mut state).to_le_bytes()[0]);
        }
        assert_stable("f.rb", &bytes);
        assert_stable("f.rs", &bytes);
    }
    for (path, source) in [("basic.rb", BASIC), ("heredoc.rb", HEREDOC), ("incomplete.rb", INCOMPLETE)] {
        for len in 0..=source.len() {
            let prefix = &source.as_bytes()[..len];
            let facts = RubyLexer::default()
                .extract(path, prefix)
                .unwrap_or_else(|error| panic!("{path} prefix {len}: {error}"));
            check_invariants(path, &source[..len], &facts);
            assert_stable(path, prefix);
        }
    }
}
