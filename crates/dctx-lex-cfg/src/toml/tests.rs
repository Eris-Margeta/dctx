// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::domain::Completion;

#[test]
fn dotted_keys_and_duplicate_assignments() {
    let limits = Limits::default();
    let dotted = extract("p.toml", "point.x = 1\npoint.y = 2\n", &limits).unwrap();
    let names: Vec<_> = dotted
        .nodes
        .iter()
        .map(|node| node.fact.name.as_str())
        .collect();
    assert_eq!(names, ["p.toml", "point", "point.x", "point.y"]);
    assert_eq!(dotted.completion, Completion::Complete);
    let again = extract("p.toml", "a = 1\na = 2\nb = 3\n", &limits).unwrap();
    assert_eq!(again.completion, Completion::Partial);
    let kept: Vec<_> = again
        .nodes
        .iter()
        .map(|node| node.fact.name.as_str())
        .collect();
    assert_eq!(kept, ["p.toml", "a", "b"]);
}

fn signature<'a>(file: &'a LexedFile, name: &str) -> &'a str {
    file.nodes
        .iter()
        .find(|node| node.fact.name == name)
        .map_or("", |node| node.fact.signature.as_str())
}

#[test]
fn multiline_backslash_accepts_crlf() {
    let limits = Limits::default();
    let crlf = extract("a.toml", "a = \"\"\"foo\\\r\nbar\"\"\"\n", &limits).unwrap();
    assert_eq!(
        crlf.completion,
        Completion::Complete,
        "{:?}",
        crlf.diagnostics
    );
    assert_eq!(signature(&crlf, "a"), "foobar");
    let indented = extract("a.toml", "a = \"\"\"foo\\\r\n  bar\"\"\"\n", &limits).unwrap();
    assert_eq!(
        indented.completion,
        Completion::Complete,
        "{:?}",
        indented.diagnostics
    );
    assert_eq!(signature(&indented, "a"), "foobar");
    let lf = extract("a.toml", "a = \"\"\"foo\\\nbar\"\"\"\n", &limits).unwrap();
    assert_eq!(lf.completion, Completion::Complete, "{:?}", lf.diagnostics);
    assert_eq!(signature(&lf, "a"), "foobar");
}
