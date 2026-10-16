//! Differential test of the syn extractor against the tree-sitter Rust adapter (oracle).
//! Checks ARCHITECTURE-PROPOSAL §3 gates on crates/dctx-map-parse/tests/fixtures/rust and
//! a valid snippet corpus: syn ≥99% declaration precision/recall, line scan ≥98%/≥85%.
//! Uses dctx-map-parse only as a dev-dependency; no filesystem writes.
#![forbid(unsafe_code)]
use dctx_lex_rust::{
    Completion, DeclarationKind, Extraction, ImportKind, Limits, Precision, Refusal, extract,
};
use dctx_map_model::{EdgeKind, NodeKind, SymbolKind};
use dctx_map_parse::{ParsedFile, Parser, TreeSitterParser};
use std::collections::BTreeSet;

const FIXTURES: &[(&str, &str)] = &[
    (
        "src/lib.rs",
        include_str!("../../dctx-map-parse/tests/fixtures/rust/src/lib.rs"),
    ),
    (
        "src/support.rs",
        include_str!("../../dctx-map-parse/tests/fixtures/rust/src/support.rs"),
    ),
    ("src/corpus.rs", include_str!("corpus/valid.rs")),
];

type Decl = (String, Option<SymbolKind>, bool, [u32; 2]);

fn symbol_kind(kind: DeclarationKind) -> Option<SymbolKind> {
    use DeclarationKind as K;
    Some(match kind {
        K::Module => return None,
        K::Struct | K::Union => SymbolKind::Struct,
        K::Enum => SymbolKind::Enum,
        K::Trait | K::TraitAlias => SymbolKind::Trait,
        K::Impl => SymbolKind::Impl,
        K::Function => SymbolKind::Function,
        K::Method => SymbolKind::Method,
        K::Const | K::Static => SymbolKind::Const,
        K::TypeAlias => SymbolKind::Type,
        K::Macro => SymbolKind::Macro,
    })
}

fn oracle(path: &str, source: &str) -> ParsedFile {
    TreeSitterParser
        .parse(path, source.as_bytes())
        .unwrap_or_else(|e| panic!("oracle refused {path}: {e}"))
}

fn ours(source: &str) -> Extraction {
    extract(source.as_bytes(), &Limits::default()).unwrap_or_else(|e| panic!("{e}"))
}

fn oracle_decls(file: &ParsedFile) -> BTreeSet<Decl> {
    let nodes = file.nodes.iter().skip(1);
    nodes
        .map(|n| (n.name.clone(), n.symbol_kind, n.kind == NodeKind::Test, n.lines))
        .collect()
}

fn our_decls(facts: &Extraction, with_lines: bool) -> BTreeSet<Decl> {
    let decls = facts.declarations.iter();
    decls
        .map(|d| {
            let lines = if with_lines { d.span.lines } else { [0, 0] };
            (d.name.clone(), symbol_kind(d.kind), d.test, lines)
        })
        .collect()
}

fn refs(file: &ParsedFile, kind: EdgeKind) -> BTreeSet<(String, Option<String>)> {
    let refs = file.references.iter().filter(|r| r.kind == kind);
    refs.map(|r| (r.name.clone(), r.alias.clone())).collect()
}

/// (precision, recall) of `emitted` against `expected`; empty denominators count as 1.
fn score<T: Ord>(emitted: &BTreeSet<T>, expected: &BTreeSet<T>) -> (f64, f64) {
    let correct = emitted.intersection(expected).count() as f64;
    let ratio = |n: usize| if n == 0 { 1.0 } else { correct / n as f64 };
    (ratio(emitted.len()), ratio(expected.len()))
}

#[test]
fn syn_declarations_match_the_oracle() {
    for (path, source) in FIXTURES {
        let expected = oracle_decls(&oracle(path, source));
        let facts = ours(source);
        assert_eq!(facts.completion, Completion::Complete, "{path}");
        let emitted = our_decls(&facts, true);
        let (precision, recall) = score(&emitted, &expected);
        assert!(
            precision >= 0.99 && recall >= 0.99,
            "{path}: precision {precision}, recall {recall}\nonly ours: {:?}\nonly oracle: {:?}",
            emitted.difference(&expected).collect::<Vec<_>>(),
            expected.difference(&emitted).collect::<Vec<_>>(),
        );
        assert!(facts.declarations.iter().all(|d| d.precision == Precision::Syntax));
    }
}

#[test]
fn syn_imports_reexports_and_impls_match_the_oracle() {
    for (path, source) in FIXTURES {
        let file = oracle(path, source);
        let facts = ours(source);
        let imports = facts
            .imports
            .iter()
            .filter(|i| i.kind != ImportKind::ExternCrate);
        let ours: BTreeSet<_> = imports.map(|i| (i.path.clone(), i.alias.clone())).collect();
        assert_eq!(ours, refs(&file, EdgeKind::Imports), "{path} imports");
        let exported = facts
            .imports
            .iter()
            .filter(|i| i.public && i.kind == ImportKind::Use);
        let exported: BTreeSet<_> = exported.map(|i| (i.path.clone(), i.alias.clone())).collect();
        assert_eq!(exported, refs(&file, EdgeKind::Reexports), "{path} reexports");
        let traits = facts.declarations.iter().filter_map(|d| {
            let path = d.implementation.as_ref()?.trait_path.clone()?;
            Some((path, None))
        });
        let traits: BTreeSet<_> = traits.collect();
        assert_eq!(traits, refs(&file, EdgeKind::Implements), "{path} impls");
    }
}

#[test]
fn syn_calls_match_the_oracle_sites() {
    for (path, source) in FIXTURES {
        let file = oracle(path, source);
        let calls = file.references.iter().filter(|r| r.kind == EdgeKind::Calls);
        let expected: Vec<_> = calls.map(|r| (r.name.clone(), r.lines)).collect();
        let facts = ours(source);
        let emitted: Vec<_> = facts
            .references
            .iter()
            .map(|r| (r.target.clone(), r.span.lines))
            .collect();
        let sorted = |mut v: Vec<(String, [u32; 2])>| {
            v.sort();
            v
        };
        assert_eq!(sorted(emitted), sorted(expected), "{path} calls");
        let tests = file.references.iter().filter(|r| r.kind == EdgeKind::Tests);
        let in_test = facts.references.iter().filter(|r| r.in_test).count();
        assert_eq!(in_test, tests.count(), "{path} test calls");
    }
}

#[test]
fn line_scan_meets_the_structural_gate_on_broken_fixtures() {
    for (path, source) in FIXTURES {
        let expected = oracle_decls(&oracle(path, source));
        let expected: BTreeSet<Decl> = expected
            .into_iter()
            .map(|(name, kind, test, _)| (name, kind, test, [0, 0]))
            .chain([("broken".into(), Some(SymbolKind::Function), false, [0, 0])])
            .collect();
        let broken = format!("{source}\nfn broken(\n");
        let facts = ours(&broken);
        assert!(
            matches!(facts.completion, Completion::Partial(Refusal::Syntax { .. } | Refusal::Lex { .. })),
            "{path}: {:?}",
            facts.completion
        );
        assert!(facts.declarations.iter().all(|d| d.precision == Precision::LineScan));
        let emitted = our_decls(&facts, false);
        let (precision, recall) = score(&emitted, &expected);
        assert!(
            precision >= 0.98 && recall >= 0.85,
            "{path}: precision {precision}, recall {recall}\nonly ours: {:?}\nonly oracle: {:?}",
            emitted.difference(&expected).collect::<Vec<_>>(),
            expected.difference(&emitted).collect::<Vec<_>>(),
        );
    }
}
