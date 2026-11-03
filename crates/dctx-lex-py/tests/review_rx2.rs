// SPDX-License-Identifier: Apache-2.0
//! Regression tests for review findings 2 and 11 on the Python lexer.
//! Covers same-quote f-strings and containment confidence after damage.
//! Uses the public extractor and the shared fact invariants.
#![forbid(unsafe_code)]

mod common;

use common::{call_names, check_invariants, extract, names};
use dctx_lex_py::{Completion, Precision};
use dctx_map_model::{Confidence, EdgeKind};

fn assert_complete(source: &str, node_names: &[&str], calls: &[&str]) {
    let facts = extract("m.py", source);
    check_invariants(source, &facts);
    assert_eq!(names(&facts), node_names, "{source}");
    assert_eq!(facts.completion, Completion::Complete, "{source}");
    assert!(facts.diagnostics.is_empty(), "{source} {:?}", facts.diagnostics);
    assert_eq!(call_names(&facts), calls, "{source}");
}

/// Finding 2. A replacement field may reuse the f-string's own quotes.
#[test]
fn same_quote_fstring_keeps_the_following_declaration() {
    // Positive control: the opposite quote already ends the inner literal.
    assert_complete("x = f\"{foo('bar')}\"\ndef real():\n    pass\n", &["m.py", "real"], &["foo"]);
    assert_complete("x = f\"{foo(\"bar\")}\"\ndef real():\n    pass\n", &["m.py", "real"], &["foo"]);
    assert_complete("x = f'{foo('bar')}'\ndef real():\n    pass\n", &["m.py", "real"], &["foo"]);
    assert_complete("x = f\"{b\"bar\"}\"\ndef real():\n    pass\n", &["m.py", "real"], &[]);
    assert_complete(
        "x = (\n    f\"{foo(\"bar\")}\"\n)\ndef real():\n    pass\n",
        &["m.py", "real"],
        &["foo"],
    );
    assert_complete(concat!(r#"f"{"}"}"; FLAG = 1"#, "\n"), &["m.py", "FLAG"], &[]);
}

/// Finding 11. A span past the first diagnostic keeps an exact contains-edge.
/// Not a defect: SPEC-MAP-MODE 5.3 allows only `exact` for `contains`, and
/// `EdgeKind::permits` rejects `name`. Damage is already `Precision::Partial`
/// on that edge; completion stays `Partial`.
#[test]
fn partial_containment_stays_exact_and_labels_precision() {
    let source = "def kept():\n    return 1\nx = \"unterminated\ndef also():\n    return 1\n";
    let facts = extract("p.py", source);
    check_invariants(source, &facts);
    assert_eq!(facts.completion, Completion::Partial);
    assert!(EdgeKind::Contains.permits(Confidence::Exact));
    assert!(!EdgeKind::Contains.permits(Confidence::Name));
    let edges: Vec<_> = facts
        .edges
        .iter()
        .zip(facts.nodes.iter().skip(1))
        .map(|(edge, node)| (node.fact.name.as_str(), edge.fact.conf, edge.precision, node.precision))
        .collect();
    assert_eq!(
        edges,
        [
            ("kept", Confidence::Exact, Precision::Structural, Precision::Structural),
            ("also", Confidence::Exact, Precision::Partial, Precision::Partial),
        ]
    );
}
