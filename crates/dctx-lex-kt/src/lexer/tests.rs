// SPDX-License-Identifier: Apache-2.0
use super::*;

fn idents(source: &str) -> Vec<String> {
    let lexed = lex(source, &Limits::default()).expect("lex");
    lexed
        .tokens
        .iter()
        .filter(|token| token.kind == Kind::Ident)
        .map(|token| source[token.start as usize..token.end as usize].to_string())
        .collect()
}

#[test]
fn strings_hide_declarations_and_templates_keep_code() {
    let source =
        "val s = \"class B { }\"\nval t = \"\"\"class C { }\"\"\"\nval u = \"hi ${foo.bar()}\"\n";
    let lexed = lex(source, &Limits::default()).expect("lex");
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let names = idents(source);
    assert!(names.contains(&"foo".to_string()));
    assert!(names.contains(&"bar".to_string()));
    assert!(!names.iter().any(|name| name == "B" || name == "C"));
    let nested = lex("val x = \"${\"${y}\"}\"\n", &Limits::default()).expect("nest");
    assert!(nested.diagnostics.is_empty(), "{:?}", nested.diagnostics);
}

#[test]
fn partners_recover_and_depth_refuses() {
    let source = "fun f() {\n    return len(s\n}\n";
    let lexed = lex(source, &Limits::default()).expect("lex");
    let open = lexed
        .tokens
        .iter()
        .position(|token| slice(source, token.start, token.end) == "{")
        .expect("brace");
    assert_ne!(lexed.partner[open], NONE);
    assert!(
        lexed
            .diagnostics
            .iter()
            .any(|item| item.reason == DiagnosticReason::UnbalancedBracket)
    );
    let limits = Limits {
        max_depth: 2,
        ..Limits::default()
    };
    assert!(matches!(
        lex("{{{", &limits),
        Err(LexError::TooDeep { limit: 2, .. })
    ));
    assert!(lex("{{}}", &limits).is_ok());
    assert!(keyword("fun") && !keyword("public") && !keyword("import"));
    assert!(KEYWORDS.windows(2).all(|pair| pair[0] < pair[1]));
}
