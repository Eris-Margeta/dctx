// SPDX-License-Identifier: Apache-2.0
//! Gap oracles for compound Go types, graph ownership and hostile limits.
//! Implements the P11 rows GO1–GO3 in docs/reviews/dctx-test-gaps.md.
//! Depends on the public Go extractor. Run as the `dctx-lex-go` integration tests.
#![forbid(unsafe_code)]

use std::collections::BTreeSet;

use dctx_lex_go::{Completion, DiagnosticReason, GoLexer, LexError, LexedFile, Limits, Precision};
use dctx_map_model::EdgeKind;

const PATH: &str = "p.go";
const SENTINEL: &str = "SENTINEL_GO3_SOURCE";

#[test]
fn go1_compound_type_dependencies() {
    let source = "package p; type Key int; type Value int; var M map[Key]Value; var G Box[Value]; var V Value\n";
    let facts = extract(PATH, source);
    assert_eq!(facts.completion, Completion::Complete, "{:?}", facts.diagnostics);
    // Direct name, the control for the map-key and type-argument walks.
    assert_eq!(uses(&facts, "V"), ["Value"]);
    assert_eq!(uses(&facts, "M"), ["Key", "Value"]);
    assert_eq!(uses(&facts, "G"), ["Box", "Value"]);
    for scope in ["M", "G", "V"] {
        let id = node_id(&facts, scope);
        assert!(facts.references.iter().filter(|reference| reference.fact.scope == scope).all(
            |reference| {
                reference.fact.src.as_str() == id && reference.precision == Precision::Structural
            }
        ));
    }
}

#[test]
fn go2_generated_owner_identity_is_stable() {
    // Not a defect: ids are qualified names plus duplicate ordinals, and a
    // reference keeps the draft index of its owner. Line shifts never enter an id.
    // The span helper never checked containment or reference endpoints.
    let source = "\
package p

type A int
type B int
type Holder interface {
	N(x Value)
}

func (a A) M() { Keep() }
func (b B) M() { Drop() }
func F() { Left() }
func F() { Right() }
func Δ() { Δ() }
";
    let facts = extract(PATH, source);
    assert_eq!(facts.completion, Completion::Complete, "{:?}", facts.diagnostics);
    let ids = node_ids(&facts);
    assert_eq!(ids.len(), facts.nodes.len(), "duplicate ids in {ids:?}");
    assert_eq!(facts.edges.len(), facts.nodes.len() - 1);

    let module = "m:p.go";
    let destinations: BTreeSet<_> = facts.edges.iter().map(|edge| edge.fact.dst.as_str()).collect();
    let mut expected = ids.clone();
    expected.remove(module);
    assert_eq!(destinations, expected);
    assert!(facts.edges.iter().all(|edge| {
        edge.fact.kind == EdgeKind::Contains
            && ids.contains(edge.fact.src.as_str())
            && edge.fact.owner_path == PATH
    }));
    assert_contains(&facts, module, "k:p.go");
    for name in ["A::M", "B::M", "F~1", "F~2", "Δ", "A", "B", "Holder"] {
        assert_contains(&facts, module, &format!("s:p.go#{name}"));
    }
    assert_contains(&facts, "s:p.go#Holder", "s:p.go#Holder::N");
    assert_ne!(node_id(&facts, "A::M"), node_id(&facts, "B::M"));
    assert!(facts.nodes.iter().find(|node| node.fact.name == "Δ").unwrap().fact.public);
    assert!(facts.nodes.iter().all(|node| node.fact.path == PATH));

    assert_eq!(ref_src(&facts, EdgeKind::Calls, "Keep"), "s:p.go#A::M");
    assert_eq!(ref_src(&facts, EdgeKind::Calls, "Drop"), "s:p.go#B::M");
    assert_eq!(ref_src(&facts, EdgeKind::Calls, "Left"), "s:p.go#F~1");
    assert_eq!(ref_src(&facts, EdgeKind::Calls, "Right"), "s:p.go#F~2");
    assert_eq!(ref_src(&facts, EdgeKind::Calls, "Δ"), "s:p.go#Δ");
    assert_eq!(ref_src(&facts, EdgeKind::UsesType, "Value"), "s:p.go#Holder::N");
    assert!(facts.references.iter().all(|reference| {
        ids.contains(reference.fact.src.as_str()) && reference.fact.owner_path == PATH
    }));

    let shifted = format!("\n// shift\n{source}");
    let shifted_facts = extract(PATH, &shifted);
    assert_eq!(node_ids(&shifted_facts), ids);
    assert_eq!(endpoints(&facts), endpoints(&shifted_facts));
    let interior = source.replace("func F() { Left() }", "func F() {\n\tLeft()\n}");
    let interior_facts = extract(PATH, &interior);
    assert_eq!(node_ids(&interior_facts), ids);
    assert_eq!(endpoints(&facts), endpoints(&interior_facts));
}

#[test]
fn go3_hostile_width_and_exact_refusals() {
    // Not a defect: byte, depth and fact ceilings already return typed errors,
    // and finish keeps at most MAX_DIAGNOSTICS. Errors do not copy source bytes.
    // This rejects a repeated error string that is not the expected refusal.
    assert_import_width();
    assert_declaration_width();
    assert_fact_width();
    assert_depth_limit();
    assert_malformed_brackets();
}

fn assert_import_width() {
    let head = "package p\nimport \"";
    let tail = "\"\nfunc F() {}\n";
    let budget = 128;
    let body_len = budget - head.len() - tail.len();
    let body = format!("{SENTINEL}{}", "a".repeat(body_len - SENTINEL.len()));
    let source = format!("{head}{body}{tail}");
    assert_eq!(source.len(), budget);
    let facts = extract_with(&source, budget, 8, 32);
    assert_eq!(facts.completion, Completion::Complete, "{:?}", facts.diagnostics);
    assert_eq!(import_names(&facts), [body.as_str()]);
    assert!(facts.nodes.iter().any(|node| node.fact.name == "F"));
    let refused = extract_err(&source, budget - 1, 8, 32);
    assert_exact(
        refused,
        LexError::TooLarge {
            bytes: budget,
            limit: budget - 1,
        },
        &format!("file of {budget} bytes exceeds the {}-byte limit", budget - 1),
    );
}

fn assert_declaration_width() {
    let head = "package p\nfunc ";
    let tail = "() {}\n";
    let budget = 96;
    let name = format!("F{}", "a".repeat(budget - head.len() - tail.len() - 1));
    let source = format!("{head}{name}{tail}");
    assert_eq!(source.len(), budget);
    let facts = extract_with(&source, budget, 8, 32);
    assert_eq!(facts.completion, Completion::Complete, "{:?}", facts.diagnostics);
    assert_eq!(node_id(&facts, &name), format!("s:p.go#{name}"));
    let refused = extract_err(&source, budget - 1, 8, 32);
    assert_exact(
        refused,
        LexError::TooLarge {
            bytes: budget,
            limit: budget - 1,
        },
        &format!("file of {budget} bytes exceeds the {}-byte limit", budget - 1),
    );
}

fn assert_fact_width() {
    // Module, package, two imports and one function fill five facts.
    let source = format!("package p\nimport \"a\"\nimport \"b\"\nfunc F() {{}}\n// {SENTINEL}\n");
    let facts = extract_with(&source, 1024, 8, 5);
    assert_eq!(facts.completion, Completion::Complete, "{:?}", facts.diagnostics);
    assert_eq!(import_names(&facts), ["a", "b"]);
    assert!(facts.nodes.iter().any(|node| node.fact.name == "F"));
    let refused = extract_err(&source, 1024, 8, 4);
    assert_exact(
        refused,
        LexError::TooManyFacts { limit: 4 },
        "file yields more than 4 facts",
    );
}

fn assert_depth_limit() {
    let shallow = format!("package p\nfunc f() {{}} // {SENTINEL}\n");
    let deep = format!("package p\nfunc f() {{{{{{}}}}}} // {SENTINEL}\n");
    let facts = extract_with(&shallow, 1024, 2, 32);
    assert_eq!(facts.completion, Completion::Complete, "{:?}", facts.diagnostics);
    assert!(facts.nodes.iter().any(|node| node.fact.name == "f"));
    let refused = extract_err(&deep, 1024, 2, 32);
    assert_exact(
        refused,
        LexError::TooDeep { line: 2, limit: 2 },
        "nesting deeper than 2 at line 2",
    );
}

fn assert_malformed_brackets() {
    let clean = "package p\nfunc Kept() {}\n";
    let good = extract(PATH, clean);
    assert_eq!(good.completion, Completion::Complete);
    assert_eq!(good.diagnostics, []);
    assert!(good.nodes.iter().any(|node| node.fact.name == "Kept"));

    let mut hostile = format!("package p\nfunc Kept() {{}}\n// {SENTINEL}\n");
    hostile.push_str(&")".repeat(80));
    let facts = extract(PATH, &hostile);
    assert_eq!(facts.diagnostics.len(), LexedFile::MAX_DIAGNOSTICS);
    assert_eq!(facts.completion, Completion::Partial);
    assert!(facts.nodes.iter().any(|node| node.fact.name == "Kept"));
    assert!(facts.diagnostics.windows(2).all(|pair| pair[0].byte <= pair[1].byte));
    assert!(facts.diagnostics.iter().any(|diagnostic| diagnostic.reason == DiagnosticReason::UnbalancedBracket));
    assert!(!format!("{:?}", facts.diagnostics).contains(SENTINEL));
}

#[allow(clippy::needless_pass_by_value, reason = "This test helper owns the two throwaway typed errors for a single equality and redaction check.")]
fn assert_exact(error: LexError, expected: LexError, display: &str) {
    assert_eq!(error, expected);
    assert_eq!(error.to_string(), display);
    assert!(!error.to_string().contains(SENTINEL));
    assert!(!format!("{error:?}").contains(SENTINEL));
}

fn extract(path: &str, source: &str) -> LexedFile {
    GoLexer::default()
        .extract(path, source.as_bytes())
        .unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn extract_with(source: &str, max_bytes: usize, max_depth: u32, max_facts: usize) -> LexedFile {
    lexer(max_bytes, max_depth, max_facts)
        .extract(PATH, source.as_bytes())
        .unwrap_or_else(|error| panic!("unexpected refusal {error}"))
}

fn extract_err(source: &str, max_bytes: usize, max_depth: u32, max_facts: usize) -> LexError {
    lexer(max_bytes, max_depth, max_facts)
        .extract(PATH, source.as_bytes())
        .expect_err("valid-looking control must be the sibling that succeeds")
}

fn lexer(max_bytes: usize, max_depth: u32, max_facts: usize) -> GoLexer {
    GoLexer::new(Limits { max_bytes, max_depth, max_facts })
}

fn uses<'a>(facts: &'a LexedFile, scope: &str) -> Vec<&'a str> {
    facts.references.iter().filter(|reference| reference.fact.kind == EdgeKind::UsesType && reference.fact.scope == scope)
        .map(|reference| reference.fact.name.as_str()).collect()
}

fn node_id<'a>(facts: &'a LexedFile, name: &str) -> &'a str {
    facts.nodes.iter().find(|node| node.fact.name == name).unwrap_or_else(|| panic!("missing {name}")).fact.id.as_str()
}

fn node_ids(facts: &LexedFile) -> BTreeSet<&str> {
    facts.nodes.iter().map(|node| node.fact.id.as_str()).collect()
}

fn assert_contains(facts: &LexedFile, parent: &str, child: &str) {
    assert!(
        facts.edges.iter().any(|edge| edge.fact.src.as_str() == parent && edge.fact.dst.as_str() == child),
        "missing {parent} -> {child}"
    );
}

fn ref_src<'a>(facts: &'a LexedFile, kind: EdgeKind, name: &str) -> &'a str {
    facts.references.iter().find(|reference| reference.fact.kind == kind && reference.fact.name == name)
        .unwrap_or_else(|| panic!("missing {kind:?} {name}")).fact.src.as_str()
}

fn endpoints(facts: &LexedFile) -> (Vec<(String, String)>, Vec<(String, String)>) {
    let edges = facts.edges.iter().map(|edge| (edge.fact.src.to_string(), edge.fact.dst.to_string())).collect();
    let references = facts.references.iter().map(|reference| (reference.fact.src.to_string(), reference.fact.name.clone())).collect();
    (edges, references)
}

fn import_names(facts: &LexedFile) -> Vec<&str> {
    facts.references.iter().filter(|reference| reference.fact.kind == EdgeKind::Imports)
        .map(|reference| reference.fact.name.as_str()).collect()
}
