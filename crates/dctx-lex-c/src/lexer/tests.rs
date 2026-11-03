// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn splices_strings_and_directives_do_not_leak_tokens() {
    let source = "int spl\\\nice(int v);\n#define NAME \\\nint\n\"int hid\\\nden()\"\n";
    let lexed = lex(source, &Limits::default()).unwrap();
    assert!(lexed.logical.contains("splice"));
    assert!(!lexed.logical.contains('\\'));
    let words: Vec<&str> = lexed
        .tokens
        .iter()
        .map(|token| slice(&lexed.logical, token.start, token.end))
        .collect();
    assert!(words.contains(&"splice"), "{words:?}");
    assert!(!words.contains(&"hidden"), "{words:?}");
    assert!(words.iter().any(|word| word.starts_with("#define")));
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let raw = lex("const char* s = R\"c(int Hidden() { fake(); })c\";\n", &Limits::default()).unwrap();
    let raw_words: Vec<&str> = raw
        .tokens
        .iter()
        .map(|token| slice(&raw.logical, token.start, token.end))
        .collect();
    assert!(!raw_words.contains(&"Hidden"), "{raw_words:?}");
    assert!(!raw_words.contains(&"fake"), "{raw_words:?}");
}
