// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::domain::Completion;

fn names(file: &LexedFile) -> Vec<&str> {
    file.nodes
        .iter()
        .map(|node| node.fact.name.as_str())
        .collect()
}

fn signature<'a>(file: &'a LexedFile, name: &str) -> &'a str {
    file.nodes
        .iter()
        .find(|node| node.fact.name == name)
        .map_or("", |node| node.fact.signature.as_str())
}

#[test]
fn block_markers_stay_inside_the_scalar() {
    let limits = Limits::default();
    let literal = extract("a.yaml", "msg: |\n  ---\nb: 1\n", &limits).unwrap();
    assert_eq!(
        literal.completion,
        Completion::Complete,
        "{:?}",
        literal.diagnostics
    );
    assert_eq!(names(&literal), ["a.yaml", "msg", "b"]);
    assert_eq!(signature(&literal, "msg"), "---");
    assert_eq!(signature(&literal, "b"), "1");

    let folded = extract("a.yaml", "msg: >\n  ...\nb: 1\n", &limits).unwrap();
    assert_eq!(
        folded.completion,
        Completion::Complete,
        "{:?}",
        folded.diagnostics
    );
    assert_eq!(names(&folded), ["a.yaml", "msg", "b"]);
    assert_eq!(signature(&folded, "msg"), "...");

    let quoted = extract("a.yaml", "a: \"hello\n---\nworld\"\nb: 1\n", &limits).unwrap();
    assert_eq!(
        quoted.completion,
        Completion::Complete,
        "{:?}",
        quoted.diagnostics
    );
    assert_eq!(names(&quoted), ["a.yaml", "a", "b"]);
    assert_eq!(signature(&quoted, "a"), "hello --- world");

    let single = extract("a.yaml", "a: 'hello\n  ...\nworld'\nb: 1\n", &limits).unwrap();
    assert_eq!(
        single.completion,
        Completion::Complete,
        "{:?}",
        single.diagnostics
    );
    assert_eq!(names(&single), ["a.yaml", "a", "b"]);
    assert!(
        signature(&single, "a").contains("..."),
        "{}",
        signature(&single, "a")
    );

    let docs = extract("a.yaml", "---\na: 1\n---\nb: 2\n", &limits).unwrap();
    assert_eq!(
        docs.completion,
        Completion::Complete,
        "{:?}",
        docs.diagnostics
    );
    assert_eq!(names(&docs), ["a.yaml", "0", "0.a", "1", "1.b"]);
    let end = extract("a.yaml", "a: 1\n...\nb: 2\n", &limits).unwrap();
    assert_eq!(names(&end), ["a.yaml", "0", "0.a", "1", "1.b"]);
}

#[test]
fn quoted_scalars_span_lines() {
    let limits = Limits::default();
    let double = extract("a.yaml", "a: \"hello\nworld\"\nb: 1\n", &limits).unwrap();
    assert_eq!(
        double.completion,
        Completion::Complete,
        "{:?}",
        double.diagnostics
    );
    assert_eq!(names(&double), ["a.yaml", "a", "b"]);
    assert_eq!(signature(&double, "a"), "hello world");
    assert_eq!(signature(&double, "b"), "1");

    let single = extract("a.yaml", "a: 'hello\nworld'\nb: 1\n", &limits).unwrap();
    assert_eq!(
        single.completion,
        Completion::Complete,
        "{:?}",
        single.diagnostics
    );
    assert_eq!(names(&single), ["a.yaml", "a", "b"]);
    assert_eq!(signature(&single, "a"), "hello world");

    let escaped = extract("a.yaml", "a: \"hello\\\nworld\"\nb: 1\n", &limits).unwrap();
    assert_eq!(
        escaped.completion,
        Completion::Complete,
        "{:?}",
        escaped.diagnostics
    );
    assert_eq!(signature(&escaped, "a"), "helloworld");
    assert_eq!(names(&escaped), ["a.yaml", "a", "b"]);

    let line = extract("a.yaml", "a: \"hello\"\nb: 1\n", &limits).unwrap();
    assert_eq!(
        line.completion,
        Completion::Complete,
        "{:?}",
        line.diagnostics
    );
    assert_eq!(signature(&line, "a"), "hello");
    assert_eq!(signature(&line, "b"), "1");
}
