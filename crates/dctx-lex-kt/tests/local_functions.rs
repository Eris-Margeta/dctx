// SPDX-License-Identifier: Apache-2.0
//! Named local declarations retain their scope; literals and anonymous functions do not invent names.
use dctx_lex_kt::{Completion, KotlinLexer};
use dctx_map_model::{EdgeKind, SymbolKind};

#[test]
fn local_generics_attributes_and_reference_owners() {
    let source = r#"fun <T> outer(value: T): T {
    @Suppress("fixture")
    fun <U> café(item: U): U {
        fun leaf(x: U): U = helper(x)
        return leaf(item)
    }
    val text = "fun hidden() {}"
    val anon = fun(x: T): T { return x }
    println(value); fun after(x: T): T = x
    return café(value)
}
"#;
    let lexed = KotlinLexer::default()
        .extract("sample.kt", source.as_bytes())
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
        ["outer", "outer::café", "outer::café::leaf", "outer::after"]
    );
    assert_eq!(lexed.nodes[4].fact.signature, "fun after(x: T): T");
    let local = &lexed.nodes[2].fact;
    assert_eq!(local.symbol_kind, Some(SymbolKind::Function));
    assert!(!local.public);
    assert!(
        local
            .signature
            .starts_with("@Suppress(\"fixture\") fun <U> café")
    );
    assert!(lexed.references.iter().any(|r| {
        r.fact.kind == EdgeKind::Calls
            && r.fact.name == "helper"
            && r.fact.src.as_str() == "s:sample.kt#outer::café::leaf"
    }));
    assert!(!lexed.references.iter().any(|r| {
        r.fact.kind == EdgeKind::Calls && r.fact.name == "helper" && r.fact.scope == "outer"
    }));
}

#[test]
fn deeply_nested_locals_stop_at_the_walker_ceiling() {
    let source = format!("{}{}", "fun nested() { ".repeat(70), "}".repeat(70));
    let lexed = KotlinLexer::default()
        .extract("deep.kt", source.as_bytes())
        .unwrap();
    assert_eq!(lexed.completion, Completion::Partial);
    assert!(!lexed.diagnostics.is_empty());
}
