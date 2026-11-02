use super::*;
use crate::domain::Completion;

#[test]
fn jsonc_comments_are_summaries_and_json_comments_are_partial() {
    let limits = Limits::default();
    let source = "{\n// Name.\n\"n\": \"./a.rs\"\n}\n";
    let jsonc = extract("a.jsonc", source, true, &limits).unwrap();
    assert_eq!(jsonc.completion, Completion::Complete);
    assert_eq!(jsonc.nodes[1].fact.summary.text, "Name.");
    assert_eq!(jsonc.references[0].fact.name, "./a.rs");
    let json = extract("a.json", source, false, &limits).unwrap();
    assert_eq!(json.completion, Completion::Partial);
    assert!(json.nodes.iter().any(|node| node.fact.name == "n"));
}

fn names(file: &LexedFile) -> Vec<&str> {
    file.nodes.iter().map(|node| node.fact.name.as_str()).collect()
}

fn signature<'a>(file: &'a LexedFile, name: &str) -> &'a str {
    file.nodes
        .iter()
        .find(|node| node.fact.name == name)
        .map_or("", |node| node.fact.signature.as_str())
}

#[test]
fn surrogate_pairs_keep_the_rest_of_the_object() {
    let limits = Limits::default();
    let pair = extract("a.json", r#"{"emoji":"\uD83D\uDE00","keep":1}"#, false, &limits).unwrap();
    assert_eq!(pair.completion, Completion::Complete, "{:?}", pair.diagnostics);
    assert_eq!(names(&pair), ["a.json", "emoji", "keep"]);
    assert_eq!(signature(&pair, "emoji"), "\u{1F600}");
    assert_eq!(signature(&pair, "keep"), "1");

    let literal = extract("a.json", r#"{"a":"\u0041","keep":1}"#, false, &limits).unwrap();
    assert_eq!(literal.completion, Completion::Complete, "{:?}", literal.diagnostics);
    assert_eq!(signature(&literal, "a"), "A");
    assert_eq!(names(&literal), ["a.json", "a", "keep"]);

    let lone = extract("a.json", r#"{"emoji":"\uD800","keep":1}"#, false, &limits).unwrap();
    assert_eq!(lone.completion, Completion::Partial);
    assert!(lone.diagnostics.iter().any(|item| item.reason == DiagnosticReason::InvalidSyntax));
    assert_eq!(names(&lone), ["a.json", "emoji", "keep"]);
    assert_eq!(signature(&lone, "keep"), "1");

    let low = extract("a.json", r#"{"emoji":"A\uDE00B","keep":1}"#, false, &limits).unwrap();
    assert_eq!(low.completion, Completion::Partial);
    assert_eq!(signature(&low, "emoji"), "AB");
    assert!(names(&low).contains(&"keep"));
}

#[test]
fn array_trailing_commas_follow_the_dialect() {
    let limits = Limits::default();
    let strict = extract("a.json", "[1,]", false, &limits).unwrap();
    assert_eq!(strict.completion, Completion::Partial);
    assert!(strict.diagnostics.iter().any(|item| item.reason == DiagnosticReason::InvalidSyntax));
    assert_eq!(signature(&strict, "0"), "1");

    let clean = extract("a.json", "[1]", false, &limits).unwrap();
    assert_eq!(clean.completion, Completion::Complete, "{:?}", clean.diagnostics);
    assert_eq!(signature(&clean, "0"), "1");
    assert_eq!(names(&clean), ["a.json", "0"]);

    let jsonc = extract("a.jsonc", "[1,]", true, &limits).unwrap();
    assert_eq!(jsonc.completion, Completion::Complete, "{:?}", jsonc.diagnostics);
    assert_eq!(signature(&jsonc, "0"), "1");

    let object = extract("a.json", r#"{"editor.tabSize":4,}"#, false, &limits).unwrap();
    assert_eq!(object.completion, Completion::Complete, "{:?}", object.diagnostics);
    assert_eq!(signature(&object, "editor.tabSize"), "4");
}

fn assert_oracle(path: &str, source: &str, jsonc: bool, limits: &Limits) {
    assert_eq!(
        extract(path, source, jsonc, limits),
        extract_oracle(path, source, jsonc, limits),
        "{path} jsonc={jsonc}"
    );
}

#[test]
fn fast_json_matches_the_character_decoder() {
    let limits = Limits::default();
    let fixtures = [
        ("basic.json", include_str!("../../tests/fixtures/basic.json"), false),
        ("settings.json", include_str!("../../tests/fixtures/settings.json"), false),
    ];
    for (path, source, jsonc) in fixtures {
        assert_oracle(path, source, jsonc, &limits);
    }
    let samples = [
        ("empty.json", "", false),
        ("obj.json", "{}", false),
        ("arr.json", "[]", false),
        ("scalar.json", "1", false),
        ("nest.json", r#"{"a":{"b":[1,{"c":"x"},true,null]},"d":"plain"}"#, false),
        ("esc.json", r#"{"q":"a\"b\\c\/d\b\f\n\r\t","u":"\u0041","emoji":"\uD83D\uDE00"}"#, false),
        ("lone.json", r#"{"emoji":"\uD800","keep":1}"#, false),
        ("low.json", r#"{"emoji":"A\uDE00B","keep":1}"#, false),
        ("dot.json", r#"{"a.b":1,"a":{"b":2},"keep":3}"#, false),
        ("dup.json", r#"{"a":1,"a":2}"#, false),
        ("trail.json", r#"{"a":1,}"#, false),
        ("badarr.json", "[1,]", false),
        ("okarr.jsonc", "[1,]", true),
        ("comment.json", "{\n// Name.\n\"n\": \"./a.rs\"\n}\n", false),
        ("comment.jsonc", "{\n// Name.\n\"n\": \"./a.rs\"\n}\n", true),
        ("block.jsonc", "{\"n\": 1 /* tail */}", true),
        ("deps.json", r#"{"dependencies":{"serde":"1.0","local":{"path":"../local"}},"workspaces":["crates/core"]}"#, false),
        ("ws.json", "{\n  \"name\": \"café\",\n  \"gap\": \"a\\nb\"\n}\n", false),
        ("cut.json", r#"{"ready":1,"broken":"#, false),
        ("badesc.json", r#"{"a":"\q","b":1}"#, false),
        ("vt.json", "{\"a\":\"a\u{000b}b\"}", false),
    ];
    for (path, source, jsonc) in samples {
        assert_oracle(path, source, jsonc, &limits);
    }
    let mut long = String::from("{");
    for index in 0..40 {
        if index > 0 {
            long.push(',');
        }
        write!(long, "\"k{index}\":\"{}\"", "z".repeat(200)).expect("writing to a String cannot fail");
    }
    long.push('}');
    assert_oracle("long.json", &long, false, &limits);
    assert_oracle("wide.json", &large_nested(), false, &limits);
    assert_oracle("pretty.json", &large_pretty(), false, &limits);
    let tight = Limits { max_facts: 8, ..Limits::default() };
    assert_oracle("wide.json", &large_nested(), false, &tight);
    let deep = Limits { max_depth: 2, ..Limits::default() };
    assert_oracle("nest.json", r#"{"a":{"b":{"c":1}}}"#, false, &deep);
    assert_oracle("shallow.json", r#"{"a":{"b":1}}"#, false, &deep);
}

fn large_nested() -> String {
    let mut out = String::from("{\"rows\":[");
    for index in 0..400 {
        if index > 0 {
            out.push(',');
        }
        let ok = if index % 2 == 0 { "true" } else { "false" };
        let hash = "ab".repeat(32);
        write!(out,
            r#"{{"id":{index},"name":"item-{index}","path":"./p/{index}.rs","ok":{ok},"note":"line\n{index}","emoji":"\uD83D\uDE00","hash":"{hash}","nested":{{"a.b":1,"a":{{"b":2}},"arr":[1,2,{{"k":"v"}}]}},"deps":{{"serde":"1"}}}}"#
        ).expect("writing to a String cannot fail");
    }
    out.push_str("]}");
    out
}

fn large_pretty() -> String {
    let mut out = String::from("{\n  \"items\": [\n");
    for index in 0..80 {
        if index > 0 {
            out.push_str(",\n");
        }
        write!(out,
            "    {{\"n\": {index}, \"text\": \"word {index}\", \"path\": \"../x/{index}\"}}"
        ).expect("writing to a String cannot fail");
    }
    out.push_str("\n  ]\n}\n");
    out
}
