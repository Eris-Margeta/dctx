// SPDX-License-Identifier: Apache-2.0
//! Differential test against the tree-sitter oracle in dctx-map-parse.
//! Implements the PARSERS-v1 gate: per-language, per-family precision and recall,
//! with declarations held to >= 98% precision and >= 85% recall (ARCH §3).
//! tree-sitter is reached only through this dev-dependency.
#![forbid(unsafe_code)]
use dctx_lex_js::{JsLexer, LexedFile};
use dctx_map_model::{EdgeKind, NodeKind, SummarySource};
use dctx_map_parse::{ParsedFile, Parser, TreeSitterParser};
use std::collections::BTreeMap;

const CASES: &[(&str, &str)] = &[
    (
        "module.mjs",
        include_str!("../../dctx-map-parse/tests/fixtures/js/module.mjs"),
    ),
    (
        "module.ts",
        include_str!("../../dctx-map-parse/tests/fixtures/ts/module.ts"),
    ),
    ("corpus/service.js", include_str!("corpus/service.js")),
    ("corpus/view.jsx", include_str!("corpus/view.jsx")),
    ("corpus/unicode.js", include_str!("corpus/unicode.js")),
    ("corpus/store.ts", include_str!("corpus/store.ts")),
    ("corpus/store.test.ts", include_str!("corpus/store.test.ts")),
    ("corpus/panel.tsx", include_str!("corpus/panel.tsx")),
];

const FAMILIES: [&str; 8] = [
    "declarations",
    "spans",
    "docs",
    "imports",
    "calls",
    "tests",
    "uses-type",
    "bodies",
];

type Facts = BTreeMap<&'static str, Vec<String>>;

fn oracle_facts(file: &ParsedFile) -> Facts {
    let nodes = file.nodes.iter().map(|n| (n, n.kind));
    let refs = file
        .references
        .iter()
        .map(|r| (r.src.as_str(), r.kind, r.name.as_str(), r.alias.as_deref(), r.scope.as_str(), r.lines));
    collect(nodes, refs)
}

fn lexer_facts(file: &LexedFile) -> Facts {
    let nodes = file.nodes.iter().map(|n| (&n.fact, n.fact.kind));
    let refs = file.references.iter().map(|r| {
        let r = &r.fact;
        (r.src.as_str(), r.kind, r.name.as_str(), r.alias.as_deref(), r.scope.as_str(), r.lines)
    });
    collect(nodes, refs)
}

type Ref<'a> = (&'a str, EdgeKind, &'a str, Option<&'a str>, &'a str, [u32; 2]);

fn collect<'a>(
    nodes: impl Iterator<Item = (&'a dctx_map_model::Node, NodeKind)>,
    refs: impl Iterator<Item = Ref<'a>>,
) -> Facts {
    let mut facts = Facts::new();
    for (node, kind) in nodes {
        if kind != NodeKind::Module {
            let id = format!("{} {:?}", node.id, node.symbol_kind);
            facts.entry("declarations").or_default().push(id.clone());
            facts
                .entry("spans")
                .or_default()
                .push(format!("{id} {:?}", node.lines));
            facts.entry("bodies").or_default().push(format!(
                "{id} {} {} {}",
                node.signature, node.fingerprint, node.public
            ));
        }
        if node.summary.source == SummarySource::Doc {
            facts
                .entry("docs")
                .or_default()
                .push(format!("{} {}", node.id, node.summary.text));
        }
    }
    for (src, kind, name, alias, scope, lines) in refs {
        let family = match kind {
            EdgeKind::Imports | EdgeKind::Reexports => "imports",
            EdgeKind::Calls => "calls",
            EdgeKind::Tests => "tests",
            EdgeKind::UsesType => "uses-type",
            _ => "other",
        };
        facts.entry(family).or_default().push(format!(
            "{src} {kind:?} {name} {alias:?} {scope} {lines:?}"
        ));
    }
    facts
}

/// (matched, emitted, expected) for multisets of fact keys.
fn score(emitted: &[String], expected: &[String]) -> (usize, usize, usize) {
    let mut pool: BTreeMap<&str, usize> = BTreeMap::new();
    for key in expected {
        *pool.entry(key).or_default() += 1;
    }
    let mut matched = 0;
    for key in emitted {
        if let Some(count) = pool.get_mut(key.as_str())
            && *count > 0
        {
            *count -= 1;
            matched += 1;
        }
    }
    (matched, emitted.len(), expected.len())
}

#[allow(clippy::cast_precision_loss, reason = "Precision and recall use only the fixed differential fixture corpus, whose counts are far below the exact f64 integer range.")]
fn ratio(numerator: usize, denominator: usize) -> Option<f64> {
    (denominator > 0).then(|| numerator as f64 / denominator as f64)
}

#[allow(clippy::case_sensitive_file_extension_comparisons, reason = "These are checked-in, case-sensitive oracle fixture paths; matching their literal .ts and .tsx suffixes is intentional.")]
fn language(path: &str) -> &'static str {
    if path.ends_with(".ts") || path.ends_with(".tsx") {
        "typescript"
    } else {
        "javascript"
    }
}

#[test]
#[allow(clippy::many_single_char_names, reason = "Keep m/n/d aligned with the table's matched/emitted/expected counts; e/x name the emitted/expected fact sets.")]
fn precision_and_recall_meet_the_declaration_gate_per_language() {
    let mut totals: BTreeMap<(&str, &str), (usize, usize, usize)> = BTreeMap::new();
    for (path, source) in CASES {
        let oracle = TreeSitterParser
            .parse(path, source.as_bytes())
            .unwrap_or_else(|e| panic!("oracle rejects {path}: {e}"));
        let lexed = JsLexer::default().extract(path, source.as_bytes()).unwrap();
        let (expected, emitted) = (oracle_facts(&oracle), lexer_facts(&lexed));
        for family in FAMILIES {
            let none = Vec::new();
            let e = emitted.get(family).unwrap_or(&none);
            let x = expected.get(family).unwrap_or(&none);
            let (m, n, d) = score(e, x);
            if m != n || m != d {
                let missing: Vec<_> = x.iter().filter(|k| !e.contains(k)).collect();
                let extra: Vec<_> = e.iter().filter(|k| !x.contains(k)).collect();
                println!("{path} {family}: missing {missing:#?} extra {extra:#?}");
            }
            let total = totals.entry((language(path), family)).or_default();
            total.0 += m;
            total.1 += n;
            total.2 += d;
        }
    }
    println!("language    family        precision  recall  (matched/emitted/expected)");
    for ((lang, family), (m, n, d)) in &totals {
        let show = |v: Option<f64>| v.map_or("n/a".to_owned(), |v| format!("{v:.3}"));
        println!(
            "{lang:<11} {family:<13} {:>9}  {:>6}  ({m}/{n}/{d})",
            show(ratio(*m, *n)),
            show(ratio(*m, *d))
        );
    }
    for lang in ["javascript", "typescript"] {
        let (m, n, d) = totals[&(lang, "declarations")];
        let precision = ratio(m, n).unwrap_or(0.0);
        let recall = ratio(m, d).unwrap_or(0.0);
        assert!(precision >= 0.98, "{lang} declaration precision {precision}");
        assert!(recall >= 0.85, "{lang} declaration recall {recall}");
        let (m, n, _) = totals[&(lang, "imports")];
        assert!(ratio(m, n).unwrap_or(0.0) >= 0.98, "{lang} import precision");
    }
}

#[test]
fn oracle_fixtures_are_reproduced_exactly() {
    for (path, source) in &CASES[..2] {
        let oracle = TreeSitterParser.parse(path, source.as_bytes()).unwrap();
        let lexed = JsLexer::default().extract(path, source.as_bytes()).unwrap();
        let nodes: Vec<_> = lexed.nodes.iter().map(|n| n.fact.clone()).collect();
        assert_eq!(nodes, oracle.nodes, "{path} nodes");
        let edges: Vec<_> = lexed.edges.iter().map(|e| e.fact.clone()).collect();
        assert_eq!(edges, oracle.edges, "{path} edges");
        let refs: Vec<_> = lexed
            .references
            .iter()
            .map(|r| {
                let r = &r.fact;
                (r.src.clone(), r.kind, r.name.clone(), r.scope.clone(), r.alias.clone(), r.lines)
            })
            .collect();
        let expected: Vec<_> = oracle
            .references
            .iter()
            .map(|r| (r.src.clone(), r.kind, r.name.clone(), r.scope.clone(), r.alias.clone(), r.lines))
            .collect();
        assert_eq!(refs, expected, "{path} references");
    }
}
