// SPDX-License-Identifier: Apache-2.0
//! C and C++ regressions for comparison nesting and dialect suffixes.
//! Spec: docs/reviews/dctx-lexers-a.md findings 3, 10 and 11.
//! Depends on dctx-lex-c and dctx-map-model; used by the crate test suite.
//! Template angles nest; comparisons, shifts and `.c` do not.
#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    use dctx_lex_c::{Completion, Dialect, LexError, LexedFile, Limits, Precision, CLexer};
    use dctx_map_model::{Confidence, EdgeKind, Node};

    fn extract(path: &str, source: &str) -> LexedFile {
        CLexer::default()
            .extract(path, source.as_bytes())
            .unwrap_or_else(|error| panic!("{path}: {error}"))
    }

    fn names(facts: &LexedFile) -> Vec<&str> {
        facts.nodes.iter().map(|node| node.fact.name.as_str()).collect()
    }

    fn node<'a>(facts: &'a LexedFile, name: &str) -> &'a Node {
        &facts
            .nodes
            .iter()
            .find(|node| node.fact.name == name)
            .unwrap_or_else(|| panic!("missing {name} in {:?}", names(facts)))
            .fact
    }

    #[test]
    fn comparison_and_shift_nesting_keeps_the_translation_unit() {
        let source_one = "int a;\nvoid f(void) { if (a < 1) a++; }\n";
        let one = extract("one.c", source_one);
        assert_eq!(one.completion, Completion::Complete, "{:?}", one.diagnostics);
        assert!(names(&one).contains(&"f"), "{:?}", names(&one));

        let mut repeated = String::from("int a;\nvoid f(void) {\n");
        for _ in 0..257 {
            repeated.push_str("  if (a < 1) a++;\n  if (a >> 1) a++;\n");
        }
        repeated.push_str("}\n");
        let many = CLexer::default()
            .extract("many.c", repeated.as_bytes())
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(many.completion, Completion::Complete, "{:?}", many.diagnostics);
        assert!(names(&many).contains(&"f"));

        let tight = CLexer::new(Limits {
            max_depth: 4,
            ..Limits::default()
        });
        let mut expr = String::from("int v = ");
        for index in 0..8 {
            if index > 0 {
                expr.push_str(" && ");
            }
            expr.push_str("(a < 1)");
        }
        expr.push_str(";\nint after = 1;\n");
        let facts = tight
            .extract("expr.c", expr.as_bytes())
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(facts.completion, Completion::Complete, "{:?}", facts.diagnostics);
        let found = names(&facts);
        assert!(found.contains(&"v") && found.contains(&"after"), "{found:?}");

        let declared = "int x = a < b, y;\nint p = a >> 1, q;\nint m = a + b, n;\n";
        let decls = extract("decl.c", declared);
        assert_eq!(decls.completion, Completion::Complete, "{:?}", decls.diagnostics);
        let found = names(&decls);
        for name in ["x", "y", "p", "q", "m", "n"] {
            assert!(found.contains(&name), "missing {name} in {found:?}");
        }

        let within = "template<class A = B<int>> struct S { int value; };\n";
        let held = tight
            .extract("held.hpp", within.as_bytes())
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(names(&held).contains(&"S"), "{:?}", names(&held));
        assert!(names(&held).contains(&"S::value"), "{:?}", names(&held));

        let deep = CLexer::new(Limits {
            max_depth: 3,
            ..Limits::default()
        });
        let shifted = "template<int N = (1 >> 2), class A = B<C<D<int>>>> struct S;\n";
        assert!(matches!(
            deep.extract("shifted.hpp", shifted.as_bytes()),
            Err(LexError::TooDeep { .. })
        ));
        let plain = "template<class A = B<C<D<int>>>> struct S;\n";
        assert!(matches!(
            deep.extract("plain.hpp", plain.as_bytes()),
            Err(LexError::TooDeep { .. })
        ));
    }

    #[test]
    fn cpp_extensions_select_the_cpp_dialect() {
        let source = "int value;\n";
        let c_file = extract("box.c", source);
        assert_eq!(c_file.dialect, Dialect::C);
        assert!(names(&c_file).contains(&"value"));
        assert_eq!(extract("include/box.h", source).dialect, Dialect::C);
        assert_eq!(extract("include/box.H", source).dialect, Dialect::C);

        let exact = extract("box.C", source);
        assert_eq!(exact.dialect, Dialect::Cpp);
        assert!(names(&exact).contains(&"value"));
        for path in ["Widget.hxx", "Widget.HXX", "unit.c++", "unit.C++", "mod.ixx", "part.cppm", "a.CPP"] {
            let facts = extract(path, source);
            assert_eq!(facts.dialect, Dialect::Cpp, "{path}");
            assert!(names(&facts).contains(&"value"), "{path}");
        }
        assert!(matches!(
            CLexer::default().extract("notes.md", source.as_bytes()),
            Err(LexError::UnsupportedDialect)
        ));
    }

    #[test]
    fn damaged_containment_keeps_exact_confidence() {
        // Not a defect: SPEC-MAP-MODE 5.3 permits only Exact for Contains.
        // A span past the first diagnostic is already Precision::Partial.
        let damaged = extract(
            "damaged.c",
            "int ready(void) { return 1; }\nint broken(int x) { return x( }\nint later(void);\n",
        );
        assert_eq!(damaged.completion, Completion::Partial);
        assert_ne!(damaged.diagnostics, []);

        let ready_id = node(&damaged, "ready").id.as_str();
        let ready = damaged
            .edges
            .iter()
            .find(|edge| edge.fact.dst.as_str() == ready_id)
            .unwrap_or_else(|| panic!("missing contains edge for ready"));
        assert_eq!(ready.fact.kind, EdgeKind::Contains);
        assert_eq!(ready.precision, Precision::Structural);
        assert_eq!(ready.fact.conf, Confidence::Exact);

        for name in ["broken", "later"] {
            let id = node(&damaged, name).id.as_str();
            let edge = damaged
                .edges
                .iter()
                .find(|edge| edge.fact.dst.as_str() == id)
                .unwrap_or_else(|| panic!("missing contains edge for {name}"));
            assert_eq!(edge.precision, Precision::Partial, "{name}");
            assert_eq!(edge.fact.conf, Confidence::Exact, "{name}");
            assert!(edge.fact.kind.permits(edge.fact.conf), "{name}");
        }

        let clean = extract(
            "clean.c",
            "int ready(void) { return 1; }\nint broken(int x) { return x; }\nint later(void);\n",
        );
        assert_eq!(clean.completion, Completion::Complete, "{:?}", clean.diagnostics);
        for name in ["ready", "broken", "later"] {
            let id = node(&clean, name).id.as_str();
            let edge = clean
                .edges
                .iter()
                .find(|edge| edge.fact.dst.as_str() == id)
                .unwrap_or_else(|| panic!("missing contains edge for {name}"));
            assert_eq!(edge.precision, Precision::Structural, "{name}");
            assert_eq!(edge.fact.conf, Confidence::Exact, "{name}");
            assert!(edge.fact.kind.permits(edge.fact.conf), "{name}");
        }
    }
}
