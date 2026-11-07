// SPDX-License-Identifier: Apache-2.0
//! Named local declarations retain attributes, generics and reference ownership.
use dctx_lex_swift::{Completion, SwiftLexer};
use dctx_map_model::{EdgeKind, SymbolKind};

#[test]
fn local_generics_attributes_and_reference_owners() {
    let source = r#"func outer<T>(_ value: T) -> T {
    @inline(__always)
    func café<U>(_ item: U) -> U {
        func leaf(_ x: U) -> U { return helper(x) }
        return leaf(item)
    }
    let text = "func hidden() {}"
    return café(value)
}
"#;
    let lexed = SwiftLexer::default()
        .extract("sample.swift", source.as_bytes())
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
    assert_eq!(names, ["outer", "outer::café", "outer::café::leaf"]);
    let local = &lexed.nodes[2].fact;
    assert_eq!(local.symbol_kind, Some(SymbolKind::Function));
    assert!(
        local
            .signature
            .starts_with("@inline(__always) func café<U>")
    );
    assert!(lexed.references.iter().any(|r| {
        r.fact.kind == EdgeKind::Calls
            && r.fact.name == "helper"
            && r.fact.src.as_str() == "s:sample.swift#outer::café::leaf"
    }));
    assert!(!lexed.references.iter().any(|r| {
        r.fact.kind == EdgeKind::Calls && r.fact.name == "helper" && r.fact.scope == "outer"
    }));
}

#[test]
fn deeply_nested_locals_stop_at_the_walker_ceiling() {
    let source = format!("{}{}", "func nested() { ".repeat(70), "}".repeat(70));
    let lexed = SwiftLexer::default()
        .extract("deep.swift", source.as_bytes())
        .unwrap();
    assert_eq!(lexed.completion, Completion::Partial);
    assert!(!lexed.diagnostics.is_empty());
}
