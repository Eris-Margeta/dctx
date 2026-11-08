// SPDX-License-Identifier: Apache-2.0
//! Local functions use function kinds and lexical reference ownership.
use dctx_lex_cs::{Completion, CsLexer};
use dctx_map_model::{EdgeKind, SymbolKind};

#[test]
fn local_generics_attributes_and_reference_owners() {
    let source = r#"class Boxed {
    T Outer<T>(T value) {
        [Obsolete]
        U café<U>(U item) {
            U Leaf(U x) => Helper(x);
            return Leaf(item);
        }
        var text = "int Hidden() {}";
        return café(value);
    }
}
"#;
    let lexed = CsLexer::default()
        .extract("sample.cs", source.as_bytes())
        .unwrap();
    assert_eq!(
        lexed.completion,
        Completion::Complete,
        "{:?}",
        lexed.diagnostics
    );
    let names: Vec<_> = lexed
        .nodes
        .iter()
        .skip(1)
        .map(|n| n.fact.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "Boxed",
            "Boxed::Outer",
            "Boxed::Outer::café",
            "Boxed::Outer::café::Leaf"
        ]
    );
    let local = &lexed.nodes[3].fact;
    assert_eq!(local.symbol_kind, Some(SymbolKind::Function));
    assert!(local.signature.starts_with("[Obsolete] U café<U>"));
    assert!(lexed.references.iter().any(|r| {
        r.fact.kind == EdgeKind::Calls
            && r.fact.name == "Helper"
            && r.fact.src.as_str() == "s:sample.cs#Boxed::Outer::café::Leaf"
    }));
    assert!(!lexed.references.iter().any(|r| {
        r.fact.kind == EdgeKind::Calls && r.fact.name == "Helper" && r.fact.scope == "Boxed::Outer"
    }));
}

#[test]
fn deeply_nested_locals_stop_at_the_walker_ceiling() {
    let source = format!(
        "class Boxed {{ void Outer() {{ {}{} }} }}",
        "void Nested() { ".repeat(70),
        "}".repeat(70)
    );
    let lexed = CsLexer::default()
        .extract("deep.cs", source.as_bytes())
        .unwrap();
    assert_eq!(lexed.completion, Completion::Partial);
    assert!(!lexed.diagnostics.is_empty());
}
