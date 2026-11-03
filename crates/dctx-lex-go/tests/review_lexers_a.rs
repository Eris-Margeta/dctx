// SPDX-License-Identifier: Apache-2.0
//! Go regression tests for trailing floats and containment confidence.
//! Spec: docs/reviews/dctx-lexers-a.md findings 1 and 11; SPEC-MAP-MODE §5.3.
//! Depends on dctx-lex-go and dctx-map-model; used by the crate test suite.
//! Trailing decimal dots belong to the number so newline semicolons survive.
#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    use dctx_lex_go::{Completion, GoLexer, Labeled, LexedFile, Precision};
    use dctx_map_model::{Confidence, Edge, EdgeKind};

    fn extract(source: &str) -> LexedFile {
        GoLexer::default()
            .extract("p.go", source.as_bytes())
            .unwrap_or_else(|error| panic!("{error}"))
    }

    fn names(facts: &LexedFile) -> Vec<&str> {
        facts.nodes.iter().map(|node| node.fact.name.as_str()).collect()
    }

    fn calls(facts: &LexedFile) -> Vec<&str> {
        facts
            .references
            .iter()
            .filter(|reference| reference.fact.kind == EdgeKind::Calls)
            .map(|reference| reference.fact.name.as_str())
            .collect()
    }

    fn edge_to<'a>(facts: &'a LexedFile, name: &str) -> &'a Labeled<Edge> {
        let id = facts
            .nodes
            .iter()
            .find(|node| node.fact.name == name)
            .unwrap_or_else(|| panic!("missing {name} in {:?}", names(facts)))
            .fact
            .id
            .as_str();
        facts
            .edges
            .iter()
            .find(|edge| edge.fact.dst.as_str() == id)
            .unwrap_or_else(|| panic!("missing contains edge for {name}"))
    }

    #[test]
    fn trailing_float_dot_does_not_swallow_the_next_declaration() {
        // Positive control: a fraction digit already keeps the dot on the number.
        let control = extract("package p\nconst A = 1.0\nfunc F() { G(1.0) }\n");
        assert_eq!(control.completion, Completion::Complete, "{:?}", control.diagnostics);
        assert!(names(&control).contains(&"A") && names(&control).contains(&"F"));
        assert_eq!(calls(&control), ["G"]);

        let bodies = [
            "const A = 1.\nfunc F() {}\n",
            "var A = 1.\nfunc F() {}\n",
            "const (\n\tA = 1.\n\tB = 2\n)\nfunc F() {}\n",
            "var (\n\tA = 1.\n\tB = 2\n)\nfunc F() {}\n",
            "const A = 1.e2\nfunc F() {}\n",
            "var A = 1.e2\nfunc F() {}\n",
            "const (\n\tA = 1.e2\n\tB = 2\n)\nfunc F() {}\n",
            "var (\n\tA = 1.E2\n\tB = 2\n)\nfunc F() {}\n",
        ];
        for body in bodies {
            let facts = extract(&format!("package p\n{body}"));
            assert_eq!(facts.completion, Completion::Complete, "{body}{:?}", facts.diagnostics);
            let got = names(&facts);
            assert!(got.contains(&"A") && got.contains(&"F"), "{body}{got:?}");
            if body.contains("B =") {
                assert!(got.contains(&"B"), "{body}{got:?}");
            }
        }

        // `1.e2` / `1.E2` are one number, so the exponent is not a callee.
        for literal in ["1.e2", "1.E2"] {
            let facts = extract(&format!("package p\nconst A = {literal}(x)\nfunc F() {{}}\n"));
            assert_eq!(facts.completion, Completion::Complete, "{literal} {:?}", facts.diagnostics);
            assert!(names(&facts).contains(&"F"), "{literal} {:?}", names(&facts));
            assert!(calls(&facts).is_empty(), "{literal} {:?}", calls(&facts));
        }
    }

    #[test]
    fn damaged_containment_keeps_exact_confidence() {
        // Not a defect: SPEC-MAP-MODE §5.3 and `EdgeKind::Contains` permit only
        // `Confidence::Exact`. There is no non-exact contains variant. A span that
        // runs past the first diagnostic is already `Precision::Partial` on that
        // edge; PARSERS-v1 layers partial completion over the syntax label.
        let damaged = extract("package p\nfunc Ready() {}\nfunc Broken() {\n\treturn len(s\n}\nconst Later = 1\n");
        assert_eq!(damaged.completion, Completion::Partial);
        assert_ne!(damaged.diagnostics, []);

        let ready = edge_to(&damaged, "Ready");
        assert_eq!(ready.fact.kind, EdgeKind::Contains);
        assert_eq!(ready.precision, Precision::Structural);
        assert_eq!(ready.fact.conf, Confidence::Exact);

        for name in ["Broken", "Later"] {
            let edge = edge_to(&damaged, name);
            assert_eq!(edge.precision, Precision::Partial, "{name}");
            assert_eq!(edge.fact.conf, Confidence::Exact, "{name}");
            assert!(edge.fact.kind.permits(edge.fact.conf), "{name}");
        }

        let clean = extract("package p\nfunc Ready() {}\nfunc Broken() {}\nconst Later = 1\n");
        assert_eq!(clean.completion, Completion::Complete, "{:?}", clean.diagnostics);
        for name in ["p", "Ready", "Broken", "Later"] {
            let edge = edge_to(&clean, name);
            assert_eq!(edge.precision, Precision::Structural, "{name}");
            assert_eq!(edge.fact.conf, Confidence::Exact, "{name}");
            assert!(edge.fact.kind.permits(edge.fact.conf), "{name}");
        }
    }
}
